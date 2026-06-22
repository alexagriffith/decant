use crate::model::*;
use serde_json::Value;

const KNOWN_META: &[&str] = &[
    "summary",
    "ai-title",
    "last-prompt",
    "permission-mode",
    "attachment",
    "file-history-snapshot",
    "queue-operation",
];

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

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                issues.push(Issue {
                    line_no: i + 1,
                    error: e.to_string(),
                    raw_line: line.to_string(),
                });
                continue;
            }
        };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if started_at.is_none() {
                started_at = Some(ts.to_string());
            }
            ended_at = Some(ts.to_string());
        }
        if cwd.is_none() {
            cwd = v.get("cwd").and_then(Value::as_str).map(String::from);
        }
        if git_branch.is_none() {
            git_branch = v.get("gitBranch").and_then(Value::as_str).map(String::from);
        }
        if cli_version.is_none() {
            cli_version = v.get("version").and_then(Value::as_str).map(String::from);
        }

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
                    if let Some(s) = v
                        .get("summary")
                        .and_then(Value::as_str)
                        .or_else(|| v.get("title").and_then(Value::as_str))
                    {
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
        model: primary_model(&messages),
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
    msg.blocks
        .iter()
        .find(|b| b.block_type == BlockType::Text)
        .and_then(|b| b.text.clone())
}

/// The session's primary model for display and cost: the most-frequently-used
/// model we can price, falling back to the most-frequent model overall, then
/// None. Preferring a priceable model skips synthetic/auxiliary markers
/// (`<synthetic>`, `exa-research-*`) that would otherwise mask the real model
/// and zero out the session's estimated cost. Cost applies one price to the
/// session's aggregate tokens, so the dominant model is the best single choice.
fn primary_model(messages: &[NormalizedMessage]) -> Option<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for m in messages {
        if let Some(model) = m.model.as_deref() {
            *counts.entry(model).or_default() += 1;
        }
    }
    counts
        .iter()
        .max_by(|a, b| {
            // Priceable wins; then higher frequency; then name for determinism.
            crate::cost::is_priceable(a.0)
                .cmp(&crate::cost::is_priceable(b.0))
                .then(a.1.cmp(b.1))
                .then_with(|| b.0.cmp(a.0))
        })
        .map(|(model, _)| model.to_string())
}

fn simple_message(v: &Value, role: Role, seq: i64) -> NormalizedMessage {
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(String::from),
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
    let mut has_text = false;
    let mut has_tool_result = false;
    let content = v.get("message").and_then(|m| m.get("content"));
    match content {
        Some(Value::String(s)) => {
            has_text = true;
            blocks.push(text_block(0, s));
        }
        Some(Value::Array(items)) => {
            for (ord, item) in items.iter().enumerate() {
                let ord = ord as i64;
                let bt = item.get("type").and_then(Value::as_str).unwrap_or("");
                match bt {
                    "text" => {
                        has_text = true;
                        blocks.push(text_block(
                            ord,
                            item.get("text").and_then(Value::as_str).unwrap_or(""),
                        ));
                    }
                    "tool_result" => {
                        has_tool_result = true;
                        blocks.push(NormalizedBlock {
                            ordinal: ord,
                            block_type: BlockType::ToolResult,
                            text: None,
                            tool_name: None,
                            tool_use_id: item
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .map(String::from),
                            tool_input: None,
                            tool_result: Some(stringify_content(item.get("content"))),
                            is_error: item.get("is_error").and_then(Value::as_bool),
                        });
                    }
                    _ => blocks.push(other_block(ord, item)),
                }
            }
        }
        _ => {}
    }
    // Role is Tool only when the turn is purely tool results (no human text).
    let role = if has_tool_result && !has_text {
        Role::Tool
    } else {
        Role::User
    };
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(String::from),
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
    let model = m
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
        .map(String::from);
    let stop_reason = m
        .and_then(|m| m.get("stop_reason"))
        .and_then(Value::as_str)
        .map(String::from);
    let usage = m.and_then(|m| m.get("usage")).map(|u| {
        let g = |k: &str| u.get(k).and_then(Value::as_i64).unwrap_or(0);
        TokenUsage {
            input: g("input_tokens"),
            output: g("output_tokens"),
            cache_read: g("cache_read_input_tokens"),
            cache_creation: g("cache_creation_input_tokens"),
            // Claude redacts its thinking text and reports no reasoning count.
            reasoning: 0,
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
                "text" => blocks.push(text_block(
                    ord as i64,
                    item.get("text").and_then(Value::as_str).unwrap_or(""),
                )),
                "thinking" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64,
                    block_type: BlockType::Thinking,
                    text: item
                        .get("thinking")
                        .and_then(Value::as_str)
                        .map(String::from),
                    tool_name: None,
                    tool_use_id: None,
                    tool_input: None,
                    tool_result: None,
                    is_error: None,
                }),
                "tool_use" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64,
                    block_type: BlockType::ToolUse,
                    text: None,
                    tool_name: item.get("name").and_then(Value::as_str).map(String::from),
                    tool_use_id: item.get("id").and_then(Value::as_str).map(String::from),
                    tool_input: item.get("input").cloned(),
                    tool_result: None,
                    is_error: None,
                }),
                _ => blocks.push(other_block(ord as i64, item)),
            }
        }
    }
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(String::from),
        role: Role::Assistant,
        model,
        stop_reason,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage,
        raw: v.clone(),
        blocks,
    }
}

fn text_block(ordinal: i64, text: &str) -> NormalizedBlock {
    NormalizedBlock {
        ordinal,
        block_type: BlockType::Text,
        text: Some(text.to_string()),
        tool_name: None,
        tool_use_id: None,
        tool_input: None,
        tool_result: None,
        is_error: None,
    }
}

fn other_block(ordinal: i64, item: &Value) -> NormalizedBlock {
    NormalizedBlock {
        ordinal,
        block_type: BlockType::Other,
        text: Some(item.to_string()),
        tool_name: None,
        tool_use_id: None,
        tool_input: None,
        tool_result: None,
        is_error: None,
    }
}

fn stringify_content(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|it| match it.get("text").and_then(Value::as_str) {
                Some(t) => t.to_string(),
                None => it.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sample.jsonl"
        ))
        .unwrap()
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
        assert_eq!(
            kinds,
            vec![BlockType::Thinking, BlockType::Text, BlockType::ToolUse]
        );
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

    #[test]
    fn malformed_lines_become_issues_and_blank_lines_are_skipped() {
        let content = "\n   \n{not valid json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n";
        let parsed = parse_session("s", content);
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].line_no, 3);
        assert!(!parsed.issues[0].error.is_empty());
        assert_eq!(parsed.session.messages.len(), 1);
        assert_eq!(parsed.session.title.as_deref(), Some("hello"));
    }

    #[test]
    fn meta_summary_sets_title_and_system_and_unknown_types_become_messages() {
        let content = concat!(
            "{\"type\":\"summary\",\"summary\":\"Synthesized Title\"}\n",
            "{\"type\":\"system\",\"uuid\":\"s1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"system\",\"content\":\"boot\"}}\n",
            "{\"type\":\"weird-thing\",\"uuid\":\"w1\",\"parentUuid\":\"s1\"}\n",
        );
        let parsed = parse_session("s", content);
        let s = &parsed.session;
        // Title is synthesized from the `summary` meta record.
        assert_eq!(s.title.as_deref(), Some("Synthesized Title"));
        // system -> System role; unknown top-level type -> Other role.
        let roles: Vec<_> = s.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, vec![Role::System, Role::Other]);
        assert_eq!(s.messages[1].source_uuid.as_deref(), Some("w1"));
        assert!(s.messages[1].blocks.is_empty());
    }

    #[test]
    fn meta_title_field_is_used_when_summary_absent() {
        let content = "{\"type\":\"ai-title\",\"title\":\"From Title Field\"}\n";
        let parsed = parse_session("s", content);
        assert_eq!(parsed.session.title.as_deref(), Some("From Title Field"));
    }

    #[test]
    fn unknown_blocks_and_tool_result_arrays_parse() {
        let content = concat!(
            // Assistant turn with an unrecognized content block -> Other block.
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"},{\"type\":\"mystery\",\"foo\":1}]}}\n",
            // Pure tool-result turn: array content (mixed text + non-text) -> Tool role.
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-05-01T10:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"is_error\":true,\"content\":[{\"type\":\"text\",\"text\":\"line one\"},{\"blob\":42}]},{\"type\":\"mystery\",\"z\":2}]}}\n",
            // content that is neither string nor array -> no blocks, User role.
            "{\"type\":\"user\",\"uuid\":\"u2\",\"timestamp\":\"2026-05-01T10:00:06.000Z\",\"message\":{\"role\":\"user\",\"content\":7}}\n",
        );
        let parsed = parse_session("s", content);
        let m = &parsed.session.messages;
        assert_eq!(m[0].role, Role::Assistant);
        assert_eq!(
            m[0].blocks.iter().map(|b| b.block_type).collect::<Vec<_>>(),
            vec![BlockType::Text, BlockType::Other]
        );
        // Tool-result-only turn collapses to the Tool role.
        assert_eq!(m[1].role, Role::Tool);
        let tr = m[1]
            .blocks
            .iter()
            .find(|b| b.block_type == BlockType::ToolResult)
            .unwrap();
        // Array content is stringified, joining text with the raw JSON of non-text items.
        let body = tr.tool_result.clone().unwrap();
        assert!(body.contains("line one"));
        assert!(body.contains("blob"));
        assert_eq!(tr.is_error, Some(true));
        // The trailing unknown block in the same turn becomes an Other block.
        assert!(m[1].blocks.iter().any(|b| b.block_type == BlockType::Other));
        // Non-string/array content yields a User message with no blocks.
        assert_eq!(m[2].role, Role::User);
        assert!(m[2].blocks.is_empty());
    }

    #[test]
    fn meta_without_title_and_already_titled_meta_are_noops() {
        // First a real user turn sets the title; a later meta record with no
        // summary/title field is a no-op (exercises both `if title.is_none()`
        // = false and, on a title-less meta, the `if let Some(s)` = false arm).
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"real prompt\"}}\n",
            // Meta record while title is already set -> `if title.is_none()` false.
            "{\"type\":\"attachment\",\"summary\":\"ignored\"}\n",
            // Meta record with neither summary nor title -> inner `if let` false.
            "{\"type\":\"file-history-snapshot\"}\n",
        );
        let parsed = parse_session("s", content);
        assert_eq!(parsed.session.title.as_deref(), Some("real prompt"));
    }

    #[test]
    fn meta_first_then_no_summary_meta() {
        // A KNOWN_META record that carries neither summary nor title while the
        // title is still unset -> the inner `if let Some(s)` false arm.
        let content = "{\"type\":\"attachment\"}\n";
        let parsed = parse_session("s", content);
        assert!(parsed.session.title.is_none());
    }

    #[test]
    fn assistant_with_string_content_has_no_blocks() {
        // An assistant message whose `content` is a plain string (not an array)
        // skips the block loop entirely -> the `if let Some(Value::Array)` else.
        let content = "{\"type\":\"assistant\",\"uuid\":\"a1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":\"just text\"}}\n";
        let parsed = parse_session("s", content);
        assert_eq!(parsed.session.messages.len(), 1);
        assert!(parsed.session.messages[0].blocks.is_empty());
    }

    #[test]
    fn tool_result_scalar_and_missing_content_stringify() {
        // A tool_result whose `content` is a bare scalar (neither string nor
        // array) -> the `Some(other) => other.to_string()` arm. A second with no
        // `content` field at all -> the `None => String::new()` arm.
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":99}]}}\n",
            "{\"type\":\"user\",\"uuid\":\"u2\",\"timestamp\":\"2026-05-01T10:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t2\"}]}}\n",
        );
        let parsed = parse_session("s", content);
        let m = &parsed.session.messages;
        let tr0 = m[0]
            .blocks
            .iter()
            .find(|b| b.block_type == BlockType::ToolResult)
            .unwrap();
        assert_eq!(tr0.tool_result.as_deref(), Some("99"));
        let tr1 = m[1]
            .blocks
            .iter()
            .find(|b| b.block_type == BlockType::ToolResult)
            .unwrap();
        assert_eq!(tr1.tool_result.as_deref(), Some(""));
    }

    #[test]
    fn primary_model_prefers_priceable_then_frequency() {
        // Two distinct models force the comparator to run. The unpriceable
        // `<synthetic>` marker must not win even when it appears more often.
        let content = concat!(
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"timestamp\":\"2026-05-01T10:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"<synthetic>\",\"content\":[]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a2\",\"timestamp\":\"2026-05-01T10:00:01.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"<synthetic>\",\"content\":[]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a3\",\"timestamp\":\"2026-05-01T10:00:02.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[]}}\n",
        );
        let parsed = parse_session("s", content);
        assert_eq!(parsed.session.model.as_deref(), Some("claude-opus-4-7"));
    }
}
