//! Heuristic session classification: outcome and work type. Deterministic
//! functions of the parsed session — versioned by code, recomputed on
//! re-ingest, never by an LLM. Labels are coarse on purpose: they exist to
//! make the archive filterable and to power recommendation signals, not to be
//! ground truth.

use crate::enrich::{FileRef, Operation};
use crate::model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed,
    Abandoned,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Completed => "completed",
            Outcome::Failed => "failed",
            Outcome::Abandoned => "abandoned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkType {
    Debugging,
    Feature,
    Refactor,
    Research,
    Ops,
}

impl WorkType {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkType::Debugging => "debugging",
            WorkType::Feature => "feature",
            WorkType::Refactor => "refactor",
            WorkType::Research => "research",
            WorkType::Ops => "ops",
        }
    }
}

/// How did the session end? Rules, in priority order:
/// 1. `Abandoned` — an interruption lands in the last 3 messages, the session
///    ends on a real user turn (a question nobody answered), or the final
///    assistant message stopped at `tool_use` (mid-flight when the file ends).
/// 2. `Failed` — the session's last tool result is an error and no assistant
///    text follows it.
/// 3. `Completed` — ends with assistant output.
///
/// None for empty/contentless sessions — including **subagent transcripts**:
/// Claude Code writes each subagent run as its own session file with every
/// message marked sidechain, so there is no main thread to judge. On the real
/// archive that is ~63% of session files; over primary sessions the labels
/// split ~71% completed / 27% abandoned / 2% failed (validated 2026-06-10).
pub fn outcome(s: &NormalizedSession) -> Option<Outcome> {
    // Sidechain (subagent) messages tail many Claude sessions; the *main*
    // thread's shape is what reflects how the session ended.
    let main: Vec<&NormalizedMessage> = s
        .messages
        .iter()
        .filter(|m| {
            m.raw
                .get("isSidechain")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        })
        .collect();
    if main.is_empty() {
        return None;
    }

    let tail = main.iter().rev().take(3);
    for m in tail {
        for b in &m.blocks {
            if b.block_type == BlockType::Text
                && b.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("[Request interrupted by user"))
            {
                return Some(Outcome::Abandoned);
            }
        }
    }

    // Last message with any conversational substance (skip meta/other rows).
    let last = main
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User | Role::Assistant | Role::Tool))?;

    match last.role {
        // Ends on a user message — a prompt nobody answered (or a bare
        // mid-flight tool payload): abandoned either way.
        Role::User => Some(Outcome::Abandoned),
        Role::Tool => {
            // Session ends on a tool result the assistant never consumed.
            // Only a trailing *error* result marks failure; a successful tail
            // after recovered errors is mid-flight abandonment.
            let is_err = last
                .blocks
                .iter()
                .any(|b| b.block_type == BlockType::ToolResult && b.is_error == Some(true));
            if is_err {
                Some(Outcome::Failed)
            } else {
                Some(Outcome::Abandoned)
            }
        }
        Role::Assistant => {
            if last.stop_reason.as_deref() == Some("tool_use") {
                Some(Outcome::Abandoned)
            } else {
                Some(Outcome::Completed)
            }
        }
        _ => None,
    }
}

/// What kind of work was this? The user's first prompt wins when it states
/// intent; otherwise fall back to what the session *did* (tool mix) — also
/// when no qualifying prompt exists at all.
pub fn work_type(s: &NormalizedSession, refs: &[FileRef]) -> Option<WorkType> {
    if let Some(prompt) = first_user_prompt(s) {
        if let Some(wt) = keyword_work_type(&prompt) {
            return Some(wt);
        }
    }
    tool_mix_work_type(s, refs)
}

fn first_user_prompt(s: &NormalizedSession) -> Option<String> {
    s.messages
        .iter()
        .filter(|m| m.role == Role::User)
        .flat_map(|m| m.blocks.iter())
        .find(|b| {
            b.block_type == BlockType::Text
                && b.text.as_deref().is_some_and(|t| {
                    !t.trim().is_empty()
                        && !t.starts_with("[Request interrupted by user")
                        && !t.contains("<command-name>")
                })
        })
        .and_then(|b| b.text.clone())
        .map(|t| t.to_lowercase())
}

fn keyword_work_type(prompt: &str) -> Option<WorkType> {
    // First 400 chars: intent lives up front; long pasted context below
    // shouldn't vote.
    let head: String = prompt.chars().take(400).collect();
    // Whole-word matching ("fix" must not hit "prefix"/"fixture"), so single
    // keywords list their common inflections explicitly. Multi-word phrases
    // match as substrings — the embedded space already bounds them.
    let words: std::collections::HashSet<&str> = head
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |keys: &[&str]| {
        keys.iter().any(|k| {
            if k.contains(' ') {
                head.contains(k)
            } else {
                words.contains(k)
            }
        })
    };

    // Priority: an explicit "fix the bug" beats an incidental "build".
    if has(&[
        "fix",
        "fixes",
        "fixing",
        "fixed",
        "bug",
        "bugs",
        "fail",
        "fails",
        "failing",
        "failed",
        "failure",
        "error",
        "errors",
        "broken",
        "crash",
        "crashes",
        "crashing",
        "regression",
        "debug",
        "debugging",
    ]) {
        return Some(WorkType::Debugging);
    }
    if has(&[
        "refactor",
        "refactoring",
        "rename",
        "renaming",
        "simplify",
        "clean up",
        "cleanup",
        "reorganize",
        "extract",
    ]) {
        return Some(WorkType::Refactor);
    }
    if has(&[
        "research",
        "investigate",
        "compare",
        "should we",
        "look into",
        "evaluate",
        "explore whether",
    ]) {
        return Some(WorkType::Research);
    }
    if has(&[
        "deploy",
        "deploying",
        "install",
        "installing",
        "configure",
        "configuring",
        "release",
        "ci",
        "pipeline",
        "provision",
        "upgrade",
        "upgrading",
    ]) {
        return Some(WorkType::Ops);
    }
    if has(&[
        "implement",
        "implementing",
        "add",
        "adding",
        "build",
        "building",
        "create",
        "creating",
        "write a",
        "new feature",
        "support for",
    ]) {
        return Some(WorkType::Feature);
    }
    None
}

fn tool_mix_work_type(s: &NormalizedSession, refs: &[FileRef]) -> Option<WorkType> {
    let reads = refs
        .iter()
        .filter(|r| r.operation == Operation::Read)
        .count();
    let mutations = refs
        .iter()
        .filter(|r| {
            matches!(
                r.operation,
                Operation::Edit | Operation::Write | Operation::Delete
            )
        })
        .count();
    let web_searches = s
        .messages
        .iter()
        .flat_map(|m| m.blocks.iter())
        .filter(|b| {
            b.block_type == BlockType::WebSearch
                || (b.block_type == BlockType::ToolUse
                    && matches!(b.tool_name.as_deref(), Some("WebSearch") | Some("WebFetch")))
        })
        .count();

    // Looked around (or at the web) without changing anything → research.
    if mutations == 0 && (reads > 0 || web_searches > 0) {
        return Some(WorkType::Research);
    }
    // Wrote brand-new files more than it edited existing ones → feature work.
    let writes = refs
        .iter()
        .filter(|r| r.operation == Operation::Write)
        .count();
    let edits = refs
        .iter()
        .filter(|r| r.operation == Operation::Edit)
        .count();
    if writes > edits && writes > 0 {
        return Some(WorkType::Feature);
    }
    if edits > 0 {
        return Some(WorkType::Feature);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: Role, blocks: Vec<NormalizedBlock>) -> NormalizedMessage {
        NormalizedMessage {
            seq: 0,
            source_uuid: None,
            parent_source_uuid: None,
            role,
            model: None,
            stop_reason: None,
            timestamp: None,
            usage: None,
            raw: json!({}),
            blocks,
        }
    }

    fn text(t: &str) -> NormalizedBlock {
        NormalizedBlock {
            ordinal: 0,
            block_type: BlockType::Text,
            text: Some(t.to_string()),
            tool_name: None,
            tool_use_id: None,
            tool_input: None,
            tool_result: None,
            is_error: None,
        }
    }

    fn tool_result(err: bool) -> NormalizedBlock {
        NormalizedBlock {
            ordinal: 0,
            block_type: BlockType::ToolResult,
            text: None,
            tool_name: None,
            tool_use_id: Some("t1".into()),
            tool_input: None,
            tool_result: Some("out".into()),
            is_error: Some(err),
        }
    }

    fn session(messages: Vec<NormalizedMessage>) -> NormalizedSession {
        NormalizedSession {
            tool: Tool::ClaudeCode,
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
            est_reasoning_tokens: 0,
            reasoning_source: ReasoningSource::None,
            messages,
        }
    }

    fn fref(op: Operation) -> FileRef {
        FileRef {
            message_idx: 0,
            path: "x.rs".into(),
            rel_path: Some("x.rs".into()),
            ext: Some("rs".into()),
            operation: op,
            timestamp: None,
        }
    }

    #[test]
    fn outcome_completed_when_assistant_ends_with_text() {
        let mut a = msg(Role::Assistant, vec![text("Done.")]);
        a.stop_reason = Some("end_turn".into());
        let s = session(vec![msg(Role::User, vec![text("do it")]), a]);
        assert_eq!(outcome(&s), Some(Outcome::Completed));
    }

    #[test]
    fn outcome_abandoned_when_session_ends_on_user_text() {
        let s = session(vec![
            msg(Role::Assistant, vec![text("Done.")]),
            msg(Role::User, vec![text("and one more thing")]),
        ]);
        assert_eq!(outcome(&s), Some(Outcome::Abandoned));
    }

    #[test]
    fn outcome_abandoned_when_assistant_stops_mid_tool_use() {
        let mut a = msg(Role::Assistant, vec![text("Reading…")]);
        a.stop_reason = Some("tool_use".into());
        let s = session(vec![msg(Role::User, vec![text("go")]), a]);
        assert_eq!(outcome(&s), Some(Outcome::Abandoned));
    }

    #[test]
    fn outcome_abandoned_on_recent_interruption() {
        let s = session(vec![
            msg(Role::User, vec![text("go")]),
            msg(Role::Assistant, vec![text("working")]),
            msg(Role::User, vec![text("[Request interrupted by user]")]),
        ]);
        assert_eq!(outcome(&s), Some(Outcome::Abandoned));
    }

    #[test]
    fn outcome_failed_when_trailing_tool_error_unconsumed() {
        let s = session(vec![
            msg(Role::User, vec![text("go")]),
            msg(Role::Tool, vec![tool_result(true)]),
        ]);
        assert_eq!(outcome(&s), Some(Outcome::Failed));
    }

    #[test]
    fn outcome_ignores_sidechain_tail() {
        let mut a = msg(Role::Assistant, vec![text("Done.")]);
        a.stop_reason = Some("end_turn".into());
        let mut sc = msg(Role::User, vec![text("subagent prompt")]);
        sc.raw = json!({"isSidechain": true});
        let s = session(vec![msg(Role::User, vec![text("go")]), a, sc]);
        assert_eq!(
            outcome(&s),
            Some(Outcome::Completed),
            "sidechain tail must not flip a completed session to abandoned"
        );
    }

    #[test]
    fn outcome_none_for_empty_session() {
        assert_eq!(outcome(&session(vec![])), None);
    }

    #[test]
    fn work_type_tool_mix_fallback_runs_without_any_prompt() {
        // No qualifying user text at all (tool-result-only session): the
        // tool-mix fallback must still classify from what the session did.
        let s = session(vec![msg(Role::Tool, vec![tool_result(false)])]);
        assert_eq!(
            work_type(&s, &[fref(Operation::Read), fref(Operation::Edit)]),
            Some(WorkType::Feature),
            "missing prompt must not bypass the tool-mix fallback"
        );
    }

    #[test]
    fn outcome_abandoned_when_trailing_result_ok_despite_earlier_error() {
        // An earlier tool error was recovered; the session then ends mid-flight
        // on a SUCCESSFUL result. That is abandoned, not failed.
        let s = session(vec![
            msg(Role::User, vec![text("go")]),
            msg(Role::Tool, vec![tool_result(true)]),
            msg(Role::Assistant, vec![text("retrying with a fix")]),
            msg(Role::Tool, vec![tool_result(false)]),
        ]);
        assert_eq!(outcome(&s), Some(Outcome::Abandoned));
    }

    #[test]
    fn keywords_match_whole_words_not_substrings() {
        // "fixture" must not trigger the "fix" debugging keyword.
        let s = session(vec![msg(
            Role::User,
            vec![text("Update the fixture data for tests")],
        )]);
        assert_eq!(work_type(&s, &[]), None, "fixture is not fix");
        // "prefix" must not trigger it either.
        let s2 = session(vec![msg(
            Role::User,
            vec![text("Document the URL prefix handling")],
        )]);
        assert_eq!(work_type(&s2, &[]), None, "prefix is not fix");
        // Inflections still hit: "fixing" counts as debugging intent.
        let s3 = session(vec![msg(
            Role::User,
            vec![text("Fixing the flaky login spec")],
        )]);
        assert_eq!(work_type(&s3, &[]), Some(WorkType::Debugging));
    }

    #[test]
    fn work_type_keywords_take_priority() {
        let cases = [
            ("Fix the failing auth test", WorkType::Debugging),
            (
                "Implement cursor pagination for /sessions",
                WorkType::Feature,
            ),
            (
                "Refactor the parser module into two files",
                WorkType::Refactor,
            ),
            ("Research whether turso fits this app", WorkType::Research),
            ("Configure the release pipeline", WorkType::Ops),
        ];
        for (prompt, want) in cases {
            let s = session(vec![msg(Role::User, vec![text(prompt)])]);
            assert_eq!(work_type(&s, &[]), Some(want), "prompt: {prompt}");
        }
    }

    #[test]
    fn work_type_falls_back_to_tool_mix() {
        // Reads but zero mutations → research.
        let s = session(vec![msg(Role::User, vec![text("what does this repo do")])]);
        assert_eq!(
            work_type(&s, &[fref(Operation::Read)]),
            Some(WorkType::Research)
        );
        // Edits present → feature-ish work.
        let s2 = session(vec![msg(Role::User, vec![text("ok let's go then")])]);
        assert_eq!(
            work_type(&s2, &[fref(Operation::Read), fref(Operation::Edit)]),
            Some(WorkType::Feature)
        );
    }

    #[test]
    fn work_type_none_without_signal() {
        let s = session(vec![msg(Role::User, vec![text("hello there")])]);
        assert_eq!(work_type(&s, &[]), None);
    }

    #[test]
    fn outcome_and_work_type_as_str_round_trip() {
        assert_eq!(Outcome::Completed.as_str(), "completed");
        assert_eq!(Outcome::Failed.as_str(), "failed");
        assert_eq!(Outcome::Abandoned.as_str(), "abandoned");
        assert_eq!(WorkType::Debugging.as_str(), "debugging");
        assert_eq!(WorkType::Feature.as_str(), "feature");
        assert_eq!(WorkType::Refactor.as_str(), "refactor");
        assert_eq!(WorkType::Research.as_str(), "research");
        assert_eq!(WorkType::Ops.as_str(), "ops");
    }

    #[test]
    fn outcome_none_when_main_thread_has_no_conversational_role() {
        // The only non-sidechain message is a System/Other row (no User/
        // Assistant/Tool turn), so `last` finds nothing to judge.
        let s = session(vec![msg(Role::System, vec![text("boot")])]);
        assert_eq!(outcome(&s), None);
    }

    #[test]
    fn work_type_writes_outweigh_edits_is_feature() {
        // No qualifying prompt; tool mix has more writes than edits → feature
        // via the writes>edits branch (not the trailing edits>0 branch).
        let s = session(vec![msg(Role::Tool, vec![tool_result(false)])]);
        let refs = vec![
            fref(Operation::Write),
            fref(Operation::Write),
            fref(Operation::Edit),
        ];
        assert_eq!(work_type(&s, &refs), Some(WorkType::Feature));
    }
}
