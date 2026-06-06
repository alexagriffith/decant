use crate::model::*;
use serde_json::Value;

/// Parse one Claude Code session file's contents (one session per file).
pub fn parse_session(source_session_id: &str, content: &str) -> ParsedSession {
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut title: Option<String> = None;
    let mut totals = TokenUsage::default();
    let mut seq: i64 = 0;

    const KNOWN_META: &[&str] = &[
        "summary", "ai-title", "last-prompt", "permission-mode",
        "attachment", "file-history-snapshot", "queue-operation",
    ];

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                issues.push(Issue { line_no: i + 1, error: e.to_string(), raw_line: line.to_string() });
                continue;
            }
        };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if started_at.is_none() { started_at = Some(ts.to_string()); }
            ended_at = Some(ts.to_string());
        }
        if cwd.is_none() { cwd = v.get("cwd").and_then(Value::as_str).map(String::from); }
        if git_branch.is_none() { git_branch = v.get("gitBranch").and_then(Value::as_str).map(String::from); }
        if cli_version.is_none() { cli_version = v.get("version").and_then(Value::as_str).map(String::from); }

        match typ {
            "user" => {
                let msg = parse_user(&v, seq);
                if title.is_none() && msg.role == Role::User {
                    title = first_text(&msg).map(|t| truncate(&t, 120));
                }
                messages.push(msg);
                seq += 1;
            }
            "assistant" => {
                let msg = parse_assistant(&v, seq, &mut totals);
                messages.push(msg);
                seq += 1;
            }
            "system" => {
                messages.push(simple_message(&v, Role::System, seq));
                seq += 1;
            }
            t if KNOWN_META.contains(&t) => {
                if title.is_none() {
                    if let Some(s) = v.get("summary").and_then(Value::as_str)
                        .or_else(|| v.get("title").and_then(Value::as_str)) {
                        title = Some(truncate(s, 120));
                    }
                }
            }
            _ => {
                messages.push(simple_message(&v, Role::Other, seq));
                seq += 1;
            }
        }
    }

    let session = NormalizedSession {
        tool: Tool::ClaudeCode,
        source_session_id: source_session_id.to_string(),
        project_path: cwd.clone(),
        title,
        cwd,
        git_branch,
        model: dominant_model(&messages),
        cli_version,
        started_at,
        ended_at,
        is_archived: false,
        raw_meta: Value::Null,
        totals,
        messages,
    };
    ParsedSession { session, issues }
}

fn truncate(s: &str, max: usize) -> String {
    crate::tools::preview(s.trim(), max)
}

fn first_text(msg: &NormalizedMessage) -> Option<String> {
    msg.blocks.iter().find(|b| b.block_type == BlockType::Text).and_then(|b| b.text.clone())
}

fn dominant_model(messages: &[NormalizedMessage]) -> Option<String> {
    messages.iter().filter_map(|m| m.model.clone()).next()
}

fn simple_message(v: &Value, role: Role, seq: i64) -> NormalizedMessage {
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role,
        model: None,
        stop_reason: None,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage: None,
        raw: v.clone(),
        blocks: Vec::new(),
    }
}

fn parse_user(v: &Value, seq: i64) -> NormalizedMessage {
    let mut blocks = Vec::new();
    let mut role = Role::User;
    let content = v.get("message").and_then(|m| m.get("content"));
    match content {
        Some(Value::String(s)) => {
            blocks.push(text_block(0, s));
        }
        Some(Value::Array(items)) => {
            for (ord, item) in items.iter().enumerate() {
                let bt = item.get("type").and_then(Value::as_str).unwrap_or("");
                match bt {
                    "text" => blocks.push(text_block(ord as i64, item.get("text").and_then(Value::as_str).unwrap_or(""))),
                    "tool_result" => {
                        role = Role::Tool;
                        blocks.push(NormalizedBlock {
                            ordinal: ord as i64,
                            block_type: BlockType::ToolResult,
                            text: None,
                            tool_name: None,
                            tool_use_id: item.get("tool_use_id").and_then(Value::as_str).map(String::from),
                            tool_input: None,
                            tool_result: Some(stringify_content(item.get("content"))),
                            is_error: item.get("is_error").and_then(Value::as_bool),
                        });
                    }
                    _ => blocks.push(other_block(ord as i64, item)),
                }
            }
        }
        _ => {}
    }
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role,
        model: None,
        stop_reason: None,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage: None,
        raw: v.clone(),
        blocks,
    }
}

fn parse_assistant(v: &Value, seq: i64, totals: &mut TokenUsage) -> NormalizedMessage {
    let m = v.get("message");
    let model = m.and_then(|m| m.get("model")).and_then(Value::as_str).map(String::from);
    let stop_reason = m.and_then(|m| m.get("stop_reason")).and_then(Value::as_str).map(String::from);
    let usage = m.and_then(|m| m.get("usage")).map(|u| {
        let g = |k: &str| u.get(k).and_then(Value::as_i64).unwrap_or(0);
        TokenUsage {
            input: g("input_tokens"),
            output: g("output_tokens"),
            cache_read: g("cache_read_input_tokens"),
            cache_creation: g("cache_creation_input_tokens"),
        }
    });
    if let Some(u) = &usage {
        totals.input += u.input;
        totals.output += u.output;
        totals.cache_read += u.cache_read;
        totals.cache_creation += u.cache_creation;
    }
    let mut blocks = Vec::new();
    if let Some(Value::Array(items)) = m.and_then(|m| m.get("content")) {
        for (ord, item) in items.iter().enumerate() {
            let bt = item.get("type").and_then(Value::as_str).unwrap_or("");
            match bt {
                "text" => blocks.push(text_block(ord as i64, item.get("text").and_then(Value::as_str).unwrap_or(""))),
                "thinking" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64, block_type: BlockType::Thinking,
                    text: item.get("thinking").and_then(Value::as_str).map(String::from),
                    tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
                }),
                "tool_use" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64, block_type: BlockType::ToolUse,
                    text: None,
                    tool_name: item.get("name").and_then(Value::as_str).map(String::from),
                    tool_use_id: item.get("id").and_then(Value::as_str).map(String::from),
                    tool_input: item.get("input").cloned(),
                    tool_result: None, is_error: None,
                }),
                _ => blocks.push(other_block(ord as i64, item)),
            }
        }
    }
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role: Role::Assistant,
        model, stop_reason,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage,
        raw: v.clone(),
        blocks,
    }
}

fn text_block(ordinal: i64, text: &str) -> NormalizedBlock {
    NormalizedBlock {
        ordinal, block_type: BlockType::Text, text: Some(text.to_string()),
        tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
    }
}

fn other_block(ordinal: i64, item: &Value) -> NormalizedBlock {
    NormalizedBlock {
        ordinal, block_type: BlockType::Other,
        text: Some(item.to_string()),
        tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
    }
}

fn stringify_content(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap()
    }

    #[test]
    fn parses_messages_blocks_and_roles() {
        let parsed = parse_session("sess-claude-1", &fixture());
        let s = &parsed.session;
        assert!(parsed.issues.is_empty());
        assert_eq!(s.tool, Tool::ClaudeCode);
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[0].role, Role::User);
        assert_eq!(s.messages[1].role, Role::Assistant);
        assert_eq!(s.messages[2].role, Role::Tool);
        let kinds: Vec<_> = s.messages[1].blocks.iter().map(|b| b.block_type).collect();
        assert_eq!(kinds, vec![BlockType::Thinking, BlockType::Text, BlockType::ToolUse]);
    }

    #[test]
    fn aggregates_tokens_and_picks_model_and_title() {
        let parsed = parse_session("sess-claude-1", &fixture());
        let s = &parsed.session;
        assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(s.title.as_deref(), Some("Fix the failing auth test"));
        assert_eq!(s.totals.input, 1200 + 1500);
        assert_eq!(s.totals.output, 340 + 120);
        assert_eq!(s.started_at.as_deref(), Some("2026-05-01T10:00:00.000Z"));
        assert_eq!(s.ended_at.as_deref(), Some("2026-05-01T10:00:10.000Z"));
    }
}
