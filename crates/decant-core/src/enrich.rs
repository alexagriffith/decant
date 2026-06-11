//! Deterministic enrichment: file references and session facets extracted from
//! a normalized session. Pure functions of the parsed data — no I/O, no LLM —
//! so the whole tier is recomputable by re-ingesting source files.

use crate::model::*;
use serde_json::Value;

/// One file-level operation evidenced by a tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRef {
    /// Index into `session.messages`; ingest maps it to the message row id.
    pub message_idx: usize,
    /// Path exactly as the tool recorded it.
    pub path: String,
    /// Project-relative aggregation key (cwd stripped); None when underivable.
    pub rel_path: Option<String>,
    /// Lowercased final extension without the dot.
    pub ext: Option<String>,
    pub operation: Operation,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Read,
    Edit,
    Write,
    Delete,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Read => "read",
            Operation::Edit => "edit",
            Operation::Write => "write",
            Operation::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Operation> {
        match s {
            "read" => Some(Operation::Read),
            "edit" => Some(Operation::Edit),
            "write" => Some(Operation::Write),
            "delete" => Some(Operation::Delete),
            _ => None,
        }
    }
}

/// Per-session counters stored on the `session` row (schema v4).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Facets {
    pub turn_count: i64,
    pub error_count: i64,
    pub interruption_count: i64,
    pub compaction_count: i64,
    pub sidechain_message_count: i64,
    pub agent_spawn_count: i64,
    pub skill_count: i64,
    pub command_count: i64,
    pub thinking_block_count: i64,
    pub thinking_chars: i64,
    pub active_seconds: i64,
}

/// Gaps longer than this count as idle when summing active wallclock. Naive
/// span (ended - started) inflates "time worked" badly on sessions left open.
const ACTIVE_GAP_CAP_SECS: i64 = 300;

const INTERRUPTION_MARKER: &str = "[Request interrupted by user";
const COMMAND_MARKER: &str = "<command-name>";

/// Extract file-level operations. Claude: builtin Read/Edit/Write/NotebookEdit
/// inputs carry explicit paths (~100% coverage measured on a real archive).
/// Codex: `apply_patch` input is the raw patch text whose `*** … File:` headers
/// name every touched file (100% parseable on real data). Shell commands are
/// deliberately not mined — that would be guesswork, not evidence.
pub fn file_refs(s: &NormalizedSession) -> Vec<FileRef> {
    let cwd = s.cwd.as_deref();
    let mut out = Vec::new();
    for (idx, m) in s.messages.iter().enumerate() {
        for b in &m.blocks {
            if b.block_type != BlockType::ToolUse {
                continue;
            }
            match s.tool {
                Tool::ClaudeCode => claude_ref(idx, m, b, cwd, &mut out),
                Tool::Codex => codex_refs(idx, m, b, cwd, &mut out),
            }
        }
    }
    out
}

fn claude_ref(
    idx: usize,
    m: &NormalizedMessage,
    b: &NormalizedBlock,
    cwd: Option<&str>,
    out: &mut Vec<FileRef>,
) {
    let (key, op) = match b.tool_name.as_deref() {
        Some("Read") => ("file_path", Operation::Read),
        Some("Edit") => ("file_path", Operation::Edit),
        Some("Write") => ("file_path", Operation::Write),
        Some("NotebookEdit") => ("notebook_path", Operation::Edit),
        _ => return,
    };
    let Some(path) = b
        .tool_input
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
    else {
        return;
    };
    out.push(make_ref(idx, m, path, op, cwd));
}

fn codex_refs(
    idx: usize,
    m: &NormalizedMessage,
    b: &NormalizedBlock,
    cwd: Option<&str>,
    out: &mut Vec<FileRef>,
) {
    if b.tool_name.as_deref() != Some("apply_patch") {
        return;
    }
    // Codex tool inputs are JSON-encoded *strings*; apply_patch's is the raw
    // patch text itself, not nested JSON.
    let Some(Value::String(patch)) = b.tool_input.as_ref() else {
        return;
    };
    for line in patch.lines() {
        let (marker, op) = if let Some(rest) = line.strip_prefix("*** Add File: ") {
            (rest, Operation::Write)
        } else if let Some(rest) = line.strip_prefix("*** Update File: ") {
            (rest, Operation::Edit)
        } else if let Some(rest) = line.strip_prefix("*** Delete File: ") {
            (rest, Operation::Delete)
        } else {
            continue;
        };
        let path = marker.trim();
        if !path.is_empty() {
            out.push(make_ref(idx, m, path, op, cwd));
        }
    }
}

fn make_ref(
    idx: usize,
    m: &NormalizedMessage,
    path: &str,
    op: Operation,
    cwd: Option<&str>,
) -> FileRef {
    FileRef {
        message_idx: idx,
        path: path.to_string(),
        rel_path: relativize(path, cwd),
        ext: extension(path),
        operation: op,
        timestamp: m.timestamp.clone(),
    }
}

/// Project-relative form of `path`: strip the cwd prefix from absolute paths;
/// relative paths (Codex patches) are already project-relative. Absolute paths
/// outside the project keep no rel_path (aggregating them under a project key
/// would be wrong).
fn relativize(path: &str, cwd: Option<&str>) -> Option<String> {
    if !path.starts_with('/') {
        return Some(path.to_string());
    }
    let cwd = cwd?.trim_end_matches('/');
    path.strip_prefix(cwd)
        .and_then(|r| r.strip_prefix('/'))
        .filter(|r| !r.is_empty())
        .map(String::from)
}

/// Lowercased extension of the path's leaf, without the dot.
fn extension(path: &str) -> Option<String> {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = leaf.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Compute the session's facet counters in one pass over messages and blocks.
pub fn facets(s: &NormalizedSession) -> Facets {
    let mut f = Facets::default();
    let mut prev_ts: Option<i64> = None;

    for m in &s.messages {
        let sidechain = m
            .raw
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if sidechain {
            f.sidechain_message_count += 1;
        }
        if m.raw.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
            || m.raw.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            f.compaction_count += 1;
        }

        let mut has_real_text = false;
        for b in &m.blocks {
            match b.block_type {
                BlockType::Text => {
                    let text = b.text.as_deref().unwrap_or("");
                    if text.starts_with(INTERRUPTION_MARKER) {
                        f.interruption_count += 1;
                    } else if text.contains(COMMAND_MARKER) {
                        f.command_count += 1;
                    } else if !text.trim().is_empty() {
                        has_real_text = true;
                    }
                }
                BlockType::Thinking => {
                    f.thinking_block_count += 1;
                    f.thinking_chars += b.text.as_deref().map(|t| t.len() as i64).unwrap_or(0);
                }
                BlockType::ToolUse => match b.tool_name.as_deref() {
                    Some("Agent") | Some("Task") => f.agent_spawn_count += 1,
                    Some("Skill") => f.skill_count += 1,
                    _ => {}
                },
                BlockType::ToolResult if b.is_error == Some(true) => f.error_count += 1,
                _ => {}
            }
        }
        if m.role == Role::User && has_real_text && !sidechain {
            f.turn_count += 1;
        }

        if let Some(ts) = m.timestamp.as_deref().and_then(epoch_secs) {
            if let Some(prev) = prev_ts {
                let gap = ts - prev;
                if gap > 0 {
                    f.active_seconds += gap.min(ACTIVE_GAP_CAP_SECS);
                }
            }
            prev_ts = Some(ts);
        }
    }
    f
}

/// Parse an RFC 3339 UTC timestamp ("2026-05-01T10:00:05.123Z") to epoch
/// seconds. Session files always use Z time; anything else returns None and
/// the message is skipped for duration purposes.
fn epoch_secs(ts: &str) -> Option<i64> {
    let ts = ts.strip_suffix('Z')?;
    let (date, time) = ts.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    Some(days_from_civil(y, mo, day) * 86_400 + h * 3_600 + mi * 60 + sec)
}

/// Howard Hinnant's days-from-civil: days since 1970-01-01 for a proleptic
/// Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn claude_session() -> NormalizedSession {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/enriched.jsonl"
        ))
        .unwrap();
        crate::sources::claude::parse_session("sess-claude-enr", &content).session
    }

    fn codex_session() -> NormalizedSession {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/enriched.jsonl"
        ))
        .unwrap();
        crate::sources::codex::parse_session("fallback", &content, &HashMap::new()).session
    }

    fn brief(r: &FileRef) -> (Option<&str>, &'static str, Option<&str>) {
        (
            r.rel_path.as_deref(),
            r.operation.as_str(),
            r.ext.as_deref(),
        )
    }

    #[test]
    fn active_seconds_ignores_nonpositive_gaps_and_missing_timestamps() {
        // Three turns: the second is *earlier* than the first (gap <= 0, so the
        // `if gap > 0` body is skipped), and the third has no timestamp at all
        // (the `if let Some(ts)` else path). active_seconds stays 0.
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-05-01T10:00:10.000Z\",\"message\":{\"role\":\"user\",\"content\":\"a\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"timestamp\":\"2026-05-01T10:00:05.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[{\"type\":\"text\",\"text\":\"b\"}]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a2\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[{\"type\":\"text\",\"text\":\"c\"}]}}\n",
        );
        let s = crate::sources::claude::parse_session("s", content).session;
        let f = facets(&s);
        assert_eq!(f.active_seconds, 0);
    }

    #[test]
    fn claude_file_refs_extracted_with_rel_paths_and_exts() {
        let refs = file_refs(&claude_session());
        let got: Vec<_> = refs.iter().map(brief).collect();
        assert_eq!(
            got,
            vec![
                (Some("src/main.rs"), "read", Some("rs")),
                (Some("src/main.rs"), "edit", Some("rs")),
                (Some("README.md"), "write", Some("md")),
                (Some("nb.ipynb"), "edit", Some("ipynb")),
            ]
        );
        assert_eq!(refs[0].path, "/Users/dev/proj/src/main.rs");
        assert_eq!(
            refs[0].timestamp.as_deref(),
            Some("2026-05-03T10:01:00.000Z")
        );
    }

    #[test]
    fn codex_apply_patch_headers_become_refs_and_exec_command_is_ignored() {
        let refs = file_refs(&codex_session());
        let got: Vec<_> = refs.iter().map(brief).collect();
        assert_eq!(
            got,
            vec![
                (Some("docs/new.md"), "write", Some("md")),
                (Some("src/lib.rs"), "edit", Some("rs")),
                (Some("old.txt"), "delete", Some("txt")),
            ]
        );
    }

    #[test]
    fn claude_facets_count_all_markers() {
        let f = facets(&claude_session());
        assert_eq!(f.turn_count, 1, "only u1 is a real user turn");
        assert_eq!(f.error_count, 1);
        assert_eq!(f.interruption_count, 1);
        assert_eq!(f.command_count, 1);
        assert_eq!(f.compaction_count, 1);
        assert_eq!(f.sidechain_message_count, 2);
        assert_eq!(f.agent_spawn_count, 1);
        assert_eq!(f.skill_count, 1);
        assert_eq!(f.thinking_block_count, 1);
        assert_eq!(
            f.thinking_chars,
            "Plan the refactor carefully.".len() as i64
        );
        // gaps: 60+30+30+30+min(600,300)+10+10+10+10
        assert_eq!(f.active_seconds, 490);
    }

    #[test]
    fn codex_facets_thinking_and_active_duration() {
        let f = facets(&codex_session());
        assert_eq!(f.turn_count, 1);
        assert_eq!(f.thinking_block_count, 1);
        assert!(f.thinking_chars > 0);
        // response_item gaps: 1+2+1+1+1+2 (meta/context/event lines are not messages)
        assert_eq!(f.active_seconds, 8);
        assert_eq!(f.error_count, 0);
        assert_eq!(f.sidechain_message_count, 0);
    }

    #[test]
    fn relativize_and_extension_edge_cases() {
        assert_eq!(
            relativize("/a/b/c.rs", Some("/a/b")),
            Some("c.rs".to_string())
        );
        assert_eq!(relativize("/a/b/c.rs", Some("/a/b/")), Some("c.rs".into()));
        assert_eq!(relativize("/elsewhere/c.rs", Some("/a/b")), None);
        assert_eq!(relativize("/a/b/c.rs", None), None);
        assert_eq!(relativize("docs/x.md", None), Some("docs/x.md".into()));
        assert_eq!(extension("a/b/c.RS"), Some("rs".into()));
        assert_eq!(extension("Makefile"), None);
        assert_eq!(extension(".env"), None, "dotfile leaf is not an extension");
        assert_eq!(extension("a.tar.gz"), Some("gz".into()));
    }

    fn tool_use(tool: &str, input: Option<serde_json::Value>) -> NormalizedBlock {
        NormalizedBlock {
            ordinal: 0,
            block_type: BlockType::ToolUse,
            text: None,
            tool_name: Some(tool.to_string()),
            tool_use_id: Some("t1".into()),
            tool_input: input,
            tool_result: None,
            is_error: None,
        }
    }

    fn one_block_session(tool: Tool, block: NormalizedBlock) -> NormalizedSession {
        NormalizedSession {
            tool,
            source_session_id: "s".into(),
            project_path: None,
            title: None,
            cwd: None,
            git_branch: None,
            model: None,
            cli_version: None,
            started_at: None,
            ended_at: None,
            is_archived: false,
            raw_meta: serde_json::Value::Null,
            totals: TokenUsage::default(),
            messages: vec![NormalizedMessage {
                seq: 0,
                source_uuid: None,
                parent_source_uuid: None,
                role: Role::Assistant,
                model: None,
                stop_reason: None,
                timestamp: None,
                usage: None,
                raw: serde_json::Value::Null,
                blocks: vec![block],
            }],
        }
    }

    #[test]
    fn claude_ref_without_path_input_is_skipped() {
        // A Read tool_use whose input lacks the `file_path` key yields no ref.
        let s = one_block_session(
            Tool::ClaudeCode,
            tool_use("Read", Some(serde_json::json!({"something_else": 1}))),
        );
        assert!(file_refs(&s).is_empty());
        // ...and an unrecognized tool name also yields nothing (the `_ => return`).
        let s2 = one_block_session(
            Tool::ClaudeCode,
            tool_use("Bash", Some(serde_json::json!({"command": "ls"}))),
        );
        assert!(file_refs(&s2).is_empty());
    }

    #[test]
    fn codex_apply_patch_with_non_string_input_is_skipped() {
        // apply_patch's input should be a raw JSON string; an object form is
        // rejected (no refs) by the `Value::String` guard.
        let s = one_block_session(
            Tool::Codex,
            tool_use("apply_patch", Some(serde_json::json!({"patch": "x"}))),
        );
        assert!(file_refs(&s).is_empty());
        // A non-apply_patch Codex tool is ignored entirely.
        let s2 = one_block_session(
            Tool::Codex,
            tool_use("shell", Some(serde_json::json!("ls -la"))),
        );
        assert!(file_refs(&s2).is_empty());
    }

    #[test]
    fn epoch_secs_parses_session_timestamps() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(epoch_secs("1970-01-02T00:00:01Z"), Some(86_401));
        assert_eq!(epoch_secs("not a time"), None);
        // 2026-05-03T10:00:00Z vs 10:01:00Z differ by exactly 60s.
        let a = epoch_secs("2026-05-03T10:00:00.000Z").unwrap();
        let b = epoch_secs("2026-05-03T10:01:00.000Z").unwrap();
        assert_eq!(b - a, 60);
    }
}
