use crate::model::*;
use serde_json::Value;
use std::collections::HashMap;

/// Parse one Codex rollout file (one session per file). `titles` maps session id ->
/// thread name (from ~/.codex/session_index.jsonl); used as the preferred title.
pub fn parse_session(fallback_id: &str, content: &str, titles: &HashMap<String, String>) -> ParsedSession {
    let mut issues = Vec::new();
    let mut id = fallback_id.to_string();
    let mut cwd: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut model: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut title: Option<String> = None;
    let mut totals = TokenUsage::default();
    let mut raw_meta = Value::Null;
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut seq: i64 = 0;

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() { continue; }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => { issues.push(Issue { line_no: i + 1, error: e.to_string(), raw_line: line.to_string() }); continue; }
        };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if started_at.is_none() { started_at = Some(ts.to_string()); }
            ended_at = Some(ts.to_string());
        }
        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
        match typ {
            "session_meta" => {
                if let Some(s) = payload.get("id").and_then(Value::as_str) { id = s.to_string(); }
                cwd = payload.get("cwd").and_then(Value::as_str).map(String::from);
                cli_version = payload.get("cli_version").and_then(Value::as_str).map(String::from);
                raw_meta = payload.clone();
            }
            "turn_context" => {
                if let Some(m) = payload.get("model").and_then(Value::as_str) { model = Some(m.to_string()); }
                if cwd.is_none() { cwd = payload.get("cwd").and_then(Value::as_str).map(String::from); }
            }
            "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                let g = |k: &str| payload.get(k).and_then(Value::as_i64).unwrap_or(0);
                // token_count is cumulative; keep the latest seen.
                totals = TokenUsage {
                    input: g("input_tokens"),
                    output: g("output_tokens"),
                    cache_read: g("cached_input_tokens"),
                    cache_creation: 0,
                };
            }
            "response_item" => {
                if let Some(msg) = parse_item(&v, &payload, seq, &mut title) {
                    messages.push(msg);
                    seq += 1;
                }
            }
            _ => {}
        }
    }

    if let Some(t) = titles.get(&id) { title = Some(t.clone()); }

    let session = NormalizedSession {
        tool: Tool::Codex,
        source_session_id: id,
        project_path: cwd.clone(),
        title,
        cwd,
        git_branch: None,
        model,
        cli_version,
        started_at,
        ended_at,
        is_archived: false,
        raw_meta,
        totals,
        messages,
    };
    ParsedSession { session, issues }
}

fn parse_item(line: &Value, payload: &Value, seq: i64, title: &mut Option<String>) -> Option<NormalizedMessage> {
    let ptyp = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = line.get("timestamp").and_then(Value::as_str).map(String::from);
    let mk = |role: Role, block: NormalizedBlock| NormalizedMessage {
        seq, source_uuid: None, parent_source_uuid: None, role,
        model: None, stop_reason: None, timestamp: ts.clone(), usage: None,
        raw: line.clone(), blocks: vec![block],
    };
    match ptyp {
        "message" => {
            let role = match payload.get("role").and_then(Value::as_str) {
                Some("assistant") => Role::Assistant,
                Some("system") => Role::System,
                _ => Role::User,
            };
            let text = collect_text(payload.get("content"));
            if role == Role::User && title.is_none() && !text.is_empty() {
                *title = Some(crate::tools::preview(text.trim(), 120));
            }
            Some(mk(role, NormalizedBlock {
                ordinal: 0, block_type: BlockType::Text, text: Some(text),
                tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
            }))
        }
        "reasoning" => {
            let text = collect_text(payload.get("summary")).trim().to_string();
            let text = if text.is_empty() { collect_text(payload.get("content")) } else { text };
            Some(mk(Role::Assistant, NormalizedBlock {
                ordinal: 0, block_type: BlockType::Thinking, text: Some(text),
                tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
            }))
        }
        "function_call" | "custom_tool_call" | "tool_search_call" | "mcp_tool_call" => {
            let name = payload.get("name").and_then(Value::as_str).map(String::from);
            let args = payload.get("arguments").cloned()
                .or_else(|| payload.get("input").cloned());
            Some(mk(Role::Assistant, NormalizedBlock {
                ordinal: 0, block_type: BlockType::ToolUse, text: None,
                tool_name: name,
                tool_use_id: payload.get("call_id").and_then(Value::as_str).map(String::from),
                tool_input: args, tool_result: None, is_error: None,
            }))
        }
        "function_call_output" | "custom_tool_call_output" | "tool_search_output" => {
            Some(mk(Role::Tool, NormalizedBlock {
                ordinal: 0, block_type: BlockType::ToolResult, text: None,
                tool_name: None,
                tool_use_id: payload.get("call_id").and_then(Value::as_str).map(String::from),
                tool_input: None,
                tool_result: Some(stringify(payload.get("output"))),
                is_error: None,
            }))
        }
        "web_search_call" => Some(mk(Role::Assistant, NormalizedBlock {
            ordinal: 0, block_type: BlockType::WebSearch, text: None,
            tool_name: Some("web_search".to_string()),
            tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
        })),
        _ => Some(mk(Role::Other, NormalizedBlock {
            ordinal: 0, block_type: BlockType::Other, text: Some(payload.to_string()),
            tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
        })),
    }
}

fn collect_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|it| it.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn stringify(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/codex/sample.jsonl")).unwrap()
    }

    #[test]
    fn parses_meta_model_and_conversation() {
        let titles = HashMap::new();
        let parsed = parse_session("fallback", &fixture(), &titles);
        let s = &parsed.session;
        assert!(parsed.issues.is_empty());
        assert_eq!(s.tool, Tool::Codex);
        assert_eq!(s.source_session_id, "sess-codex-1");
        assert_eq!(s.cwd.as_deref(), Some("/Users/dev/proj"));
        assert_eq!(s.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[0].role, Role::User);
        assert_eq!(s.messages[1].blocks[0].block_type, BlockType::ToolUse);
        assert_eq!(s.messages[2].role, Role::Tool);
        assert_eq!(s.title.as_deref(), Some("List the open TODOs"));
    }

    #[test]
    fn cumulative_token_count_becomes_session_totals() {
        let parsed = parse_session("fallback", &fixture(), &HashMap::new());
        assert_eq!(parsed.session.totals.input, 900);
        assert_eq!(parsed.session.totals.output, 150);
        assert_eq!(parsed.session.totals.cache_read, 400);
    }

    #[test]
    fn session_index_title_overrides() {
        let mut titles = HashMap::new();
        titles.insert("sess-codex-1".to_string(), "TODO audit".to_string());
        let parsed = parse_session("fallback", &fixture(), &titles);
        assert_eq!(parsed.session.title.as_deref(), Some("TODO audit"));
    }
}
