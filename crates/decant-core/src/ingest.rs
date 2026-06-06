use crate::cost::{default_pricing, estimate_cost};
use crate::model::*;
use crate::tools::classify_tool;
use crate::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Insert (or replace) one parsed session.
///
/// The caller MUST run this inside a transaction (e.g. `conn.unchecked_transaction()`
/// or a per-file `Transaction`) so the delete-then-insert and all child writes are
/// atomic — a mid-write failure must roll back the whole session, never leave partial
/// rows. Deletes any prior rows for the same (tool, source_session_id) first, so
/// re-ingest is idempotent. FTS is maintained by triggers.
pub fn upsert_session(conn: &Connection, parsed: &ParsedSession, source_path: &str, mtime: i64, size: i64, hash: &str) -> Result<i64> {
    let s = &parsed.session;

    // Project (by cwd path).
    let project_id: Option<i64> = if let Some(path) = &s.project_path {
        conn.execute(
            "INSERT INTO project(path, name, first_seen_at, last_seen_at)
             VALUES (?1, ?2, datetime('now'), datetime('now'))
             ON CONFLICT(path) DO UPDATE SET last_seen_at = datetime('now')",
            params![path, basename(path)],
        )?;
        Some(conn.query_row("SELECT id FROM project WHERE path = ?1", params![path], |r| r.get(0))?)
    } else {
        None
    };

    // Remove any existing session with the same identity (cascades to children).
    conn.execute(
        "DELETE FROM session WHERE tool = ?1 AND source_session_id = ?2",
        params![s.tool.as_str(), s.source_session_id],
    )?;

    let cost = estimate_cost(s.model.as_deref(), &s.totals, &default_pricing());
    conn.execute(
        "INSERT INTO session(
            tool, source_session_id, project_id, title, cwd, git_branch, model, cli_version,
            started_at, ended_at, message_count,
            total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_creation_tokens,
            estimated_cost_usd, is_archived, source_path, raw_meta,
            ingested_at, source_mtime, source_size, source_hash)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,datetime('now'),?20,?21,?22)",
        params![
            s.tool.as_str(), s.source_session_id, project_id, s.title, s.cwd, s.git_branch, s.model, s.cli_version,
            s.started_at, s.ended_at, s.messages.len() as i64,
            s.totals.input, s.totals.output, s.totals.cache_read, s.totals.cache_creation,
            cost, s.is_archived as i64, source_path, s.raw_meta.to_string(),
            mtime, size, hash,
        ],
    )?;
    let session_id = conn.last_insert_rowid();

    let mut results: HashMap<String, i64> = HashMap::new();
    let mut result_errors: HashMap<String, Option<bool>> = HashMap::new();
    // (message_id, call_block_id, message_timestamp, block)
    let mut tool_use_blocks: Vec<(i64, i64, Option<String>, NormalizedBlock)> = Vec::new();

    for m in &s.messages {
        conn.execute(
            "INSERT INTO message(session_id, seq, source_uuid, parent_source_uuid, role, model, stop_reason,
                                 timestamp, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, raw)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                session_id, m.seq, m.source_uuid, m.parent_source_uuid, m.role.as_str(), m.model, m.stop_reason,
                m.timestamp,
                m.usage.as_ref().map(|u| u.input),
                m.usage.as_ref().map(|u| u.output),
                m.usage.as_ref().map(|u| u.cache_read),
                m.usage.as_ref().map(|u| u.cache_creation),
                m.raw.to_string(),
            ],
        )?;
        let message_id = conn.last_insert_rowid();

        for b in &m.blocks {
            conn.execute(
                "INSERT INTO block(message_id, session_id, ordinal, type, text, tool_name, tool_use_id, tool_input, tool_result)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    message_id, session_id, b.ordinal, b.block_type.as_str(), b.text, b.tool_name,
                    b.tool_use_id, b.tool_input.as_ref().map(|v| v.to_string()), b.tool_result,
                ],
            )?;
            let block_id = conn.last_insert_rowid();
            match b.block_type {
                BlockType::ToolUse => {
                    tool_use_blocks.push((message_id, block_id, m.timestamp.clone(), b.clone()));
                }
                BlockType::ToolResult => {
                    if let Some(id) = &b.tool_use_id {
                        results.insert(id.clone(), block_id);
                        result_errors.insert(id.clone(), b.is_error);
                    }
                }
                _ => {}
            }
        }
    }

    // Build tool_call rows from tool_use blocks, pairing results by tool_use_id.
    for (message_id, call_block_id, ts, b) in &tool_use_blocks {
        let name = b.tool_name.clone().unwrap_or_default();
        let (kind, server, base) = classify_tool(&name);
        let result_block_id = b.tool_use_id.as_ref().and_then(|id| results.get(id)).copied();
        let is_error: Option<bool> = b
            .tool_use_id
            .as_ref()
            .and_then(|id| result_errors.get(id))
            .copied()
            .flatten();
        conn.execute(
            "INSERT INTO tool_call(session_id, message_id, call_block_id, result_block_id, tool_kind, tool_name,
                                   mcp_server, tool_base_name, tool_use_id, input, is_error, ordinal, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                session_id, message_id, call_block_id, result_block_id, kind.as_str(), name,
                server, base, b.tool_use_id,
                b.tool_input.as_ref().map(|v| v.to_string()),
                is_error.map(|e| e as i64),
                b.ordinal, ts,
            ],
        )?;
    }

    Ok(session_id)
}

fn basename(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema, sources};

    fn claude_fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap()
    }

    #[test]
    fn writes_session_messages_blocks_and_tool_calls() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &claude_fixture());
        // upsert_session must run inside a transaction (atomic session write).
        let tx = conn.unchecked_transaction().unwrap();
        let sid = upsert_session(&tx, &parsed, "/x/sample.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();
        assert!(sid > 0);

        let (msgs, blocks, calls, mcount, tin): (i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM message),
                        (SELECT COUNT(*) FROM block),
                        (SELECT COUNT(*) FROM tool_call),
                        (SELECT message_count FROM session),
                        (SELECT total_input_tokens FROM session)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(msgs, 4);
        assert_eq!(mcount, 4);
        assert_eq!(blocks, 6);
        assert_eq!(calls, 1);
        assert_eq!(tin, 2700);

        let (kind, base): (String, String) = conn
            .query_row("SELECT tool_kind, tool_base_name FROM tool_call", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(kind, "builtin");
        assert_eq!(base, "Read");

        let msg_id_nulls: i64 = conn
            .query_row("SELECT COUNT(*) FROM tool_call WHERE message_id IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(msg_id_nulls, 0, "tool_call.message_id must be populated");

        let fts: i64 = conn
            .query_row("SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH 'auth'", [], |r| r.get(0))
            .unwrap();
        assert!(fts >= 1, "FTS should find 'auth'");
    }
}
