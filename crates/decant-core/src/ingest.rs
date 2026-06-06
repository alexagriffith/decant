use crate::config::Config;
use crate::cost::{default_pricing, estimate_cost};
use crate::model::*;
use crate::sources;
use crate::tools::classify_tool;
use crate::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default)]
pub struct SyncReport {
    pub scanned: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub issues: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub tool: Tool,
    pub path: PathBuf,
    pub archived: bool,
}

/// Find all Claude and Codex session files under the configured roots.
pub fn discover(config: &Config) -> Vec<SourceFile> {
    let mut out = Vec::new();
    collect(&config.claude_dir, Tool::ClaudeCode, false, |name| name.ends_with(".jsonl"), &mut out);
    collect(&config.codex_dir.join("sessions"), Tool::Codex, false, is_rollout, &mut out);
    collect(&config.codex_dir.join("archived_sessions"), Tool::Codex, true, is_rollout, &mut out);
    out
}

fn is_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn collect(root: &Path, tool: Tool, archived: bool, want: impl Fn(&str) -> bool, out: &mut Vec<SourceFile>) {
    if !root.exists() { return; }
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() { continue; }
        let name = entry.file_name().to_string_lossy();
        if want(&name) {
            out.push(SourceFile { tool, path: entry.path().to_path_buf(), archived });
        }
    }
}

/// Load Codex session-index titles (id -> thread_name), if present.
fn codex_titles(config: &Config) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let path = config.codex_dir.join("session_index.jsonl");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(name)) = (
                    v.get("id").and_then(|x| x.as_str()),
                    v.get("thread_name").and_then(|x| x.as_str()),
                ) {
                    titles.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    titles
}

struct Prepared {
    file: SourceFile,
    line_count: i64,
    mtime: i64,
    size: i64,
    hash: String,
}

/// Full sync: discover, skip unchanged, parse in parallel, write each file in its
/// own transaction. Idempotent. Files that fail to read in the parallel phase are
/// counted in `SyncReport.failed` (non-fatal). Returns `Err` on the first file that
/// fails to write; files already committed in earlier iterations remain in the DB.
pub fn sync(conn: &mut Connection, config: &Config) -> Result<SyncReport> {
    use rayon::prelude::*;

    let files = discover(config);
    let titles = codex_titles(config);
    let mut report = SyncReport { scanned: files.len(), ..Default::default() };

    // Decide which files changed (cheap, serial: stat + lookup).
    let mut to_read: Vec<SourceFile> = Vec::new();
    for f in files {
        let meta = match std::fs::metadata(&f.path) { Ok(m) => m, Err(_) => continue };
        let size = meta.len() as i64;
        let mtime = mtime_secs(&meta);
        let path_str = f.path.to_string_lossy().to_string();
        let prior: Option<(i64, i64)> = conn
            .query_row("SELECT size, mtime FROM ingest_source WHERE path = ?1", params![path_str], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok();
        if prior == Some((size, mtime)) {
            report.skipped += 1;
        } else {
            to_read.push(f);
        }
    }

    // Read + hash + parse in parallel. `None` = a file we failed to read.
    let results: Vec<Option<(Prepared, ParsedSession)>> = to_read
        .par_iter()
        .map(|f| {
            let content = std::fs::read_to_string(&f.path).ok()?;
            let meta = std::fs::metadata(&f.path).ok()?;
            let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            let line_count = content.lines().count() as i64;
            let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let parsed = match f.tool {
                Tool::ClaudeCode => sources::claude::parse_session(&stem, &content),
                Tool::Codex => sources::codex::parse_session(&stem, &content, &titles),
            };
            Some((Prepared { file: f.clone(), line_count, mtime: mtime_secs(&meta), size: meta.len() as i64, hash }, parsed))
        })
        .collect();
    report.failed = results.iter().filter(|r| r.is_none()).count();
    let prepared: Vec<(Prepared, ParsedSession)> = results.into_iter().flatten().collect();

    // Write each file in its own transaction (per-file atomicity: one bad file
    // rolls back only itself; SQLite is single-writer so writes are serialized).
    for (prep, mut parsed) in prepared {
        parsed.session.is_archived = prep.file.archived;
        let path_str = prep.file.path.to_string_lossy().to_string();
        let tx = conn.transaction()?;
        // Release any FK reference from ingest_source -> session so upsert_session
        // can safely DELETE the old session row without a constraint violation.
        tx.execute("UPDATE ingest_source SET session_id = NULL WHERE path = ?1", params![path_str])?;
        let session_id = upsert_session(&tx, &parsed, &path_str, prep.mtime, prep.size, &prep.hash)?;
        // Clear any prior issues for this path so re-ingest doesn't accumulate them.
        tx.execute("DELETE FROM ingest_issue WHERE source_path = ?1", params![path_str])?;
        for issue in &parsed.issues {
            tx.execute(
                "INSERT INTO ingest_issue(source_path, line_no, error, raw_line, created_at)
                 VALUES (?1,?2,?3,?4,datetime('now'))",
                params![path_str, issue.line_no as i64, issue.error, issue.raw_line],
            )?;
            report.issues += 1;
        }
        let status = if parsed.issues.is_empty() { "ok" } else { "ok_with_issues" };
        tx.execute(
            "INSERT INTO ingest_source(path, tool, size, mtime, hash, session_id, line_count, status, last_ingested_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))
             ON CONFLICT(path) DO UPDATE SET size=?3, mtime=?4, hash=?5, session_id=?6, line_count=?7, status=?8, last_ingested_at=datetime('now')",
            params![path_str, prep.file.tool.as_str(), prep.size, prep.mtime, prep.hash, session_id, prep.line_count, status],
        )?;
        tx.commit()?;
        report.ingested += 1;
    }
    Ok(report)
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

    use std::fs;

    fn write(path: &std::path::Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn sync_is_idempotent_and_logs_issues() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        // one good Claude session + one bad line appended
        let mut claude = claude_fixture();
        claude.push_str("\n{not valid json\n");
        write(&claude_dir.join("proj/sess.jsonl"), &claude);

        let config = Config { db_path: dir.path().join("d.db"), claude_dir, codex_dir };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();

        let r1 = sync(&mut conn, &config).unwrap();
        assert_eq!(r1.ingested, 1);
        assert_eq!(r1.issues, 1);
        let issues1: i64 = conn.query_row("SELECT COUNT(*) FROM ingest_issue", [], |r| r.get(0)).unwrap();
        assert_eq!(issues1, 1);

        let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1);

        // Second run: nothing changed -> skipped, no duplicates.
        let r2 = sync(&mut conn, &config).unwrap();
        assert_eq!(r2.ingested, 0);
        assert_eq!(r2.skipped, 1);
        let sessions2: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions2, 1);

        // Modify the file (different bad line) -> re-ingest; issues must NOT accumulate.
        let mut claude3 = claude_fixture();
        claude3.push_str("\nanother bad line {\n");
        write(&config.claude_dir.join("proj/sess.jsonl"), &claude3);
        let r3 = sync(&mut conn, &config).unwrap();
        assert_eq!(r3.ingested, 1, "changed file re-ingests");
        let issues3: i64 = conn.query_row("SELECT COUNT(*) FROM ingest_issue", [], |r| r.get(0)).unwrap();
        assert_eq!(issues3, 1, "issues must not accumulate across re-ingests");
        let sessions3: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions3, 1);
    }
}
