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
pub fn upsert_session(
    conn: &Connection,
    parsed: &ParsedSession,
    source_path: &str,
    mtime: i64,
    size: i64,
    hash: &str,
) -> Result<i64> {
    let s = &parsed.session;

    let project_id: Option<i64> = if let Some(path) = &s.project_path {
        conn.execute(
            "INSERT INTO project(path, name, first_seen_at, last_seen_at)
             VALUES (?1, ?2, datetime('now'), datetime('now'))
             ON CONFLICT(path) DO UPDATE SET last_seen_at = datetime('now')",
            params![path, basename(path)],
        )?;
        Some(conn.query_row(
            "SELECT id FROM project WHERE path = ?1",
            params![path],
            |r| r.get(0),
        )?)
    } else {
        None
    };

    conn.execute(
        "DELETE FROM session WHERE tool = ?1 AND source_session_id = ?2",
        params![s.tool.as_str(), s.source_session_id],
    )?;

    let cost = estimate_cost(s.model.as_deref(), &s.totals, &default_pricing());
    let refs = crate::enrich::file_refs(s);
    let facets = crate::enrich::facets(s);
    let outcome = crate::classify::outcome(s);
    let work_type = crate::classify::work_type(s, &refs);
    conn.execute(
        "INSERT INTO session(
            tool, source_session_id, project_id, title, cwd, git_branch, model, cli_version,
            started_at, ended_at, message_count,
            total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_creation_tokens,
            total_reasoning_tokens,
            estimated_cost_usd, is_archived, source_path, raw_meta,
            ingested_at, source_mtime, source_size, source_hash,
            turn_count, error_count, interruption_count, compaction_count, sidechain_message_count,
            agent_spawn_count, skill_count, command_count, thinking_block_count, thinking_chars,
            active_seconds, outcome, work_type)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,datetime('now'),?21,?22,?23,
                 ?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36)",
        params![
            s.tool.as_str(), s.source_session_id, project_id, s.title, s.cwd, s.git_branch, s.model, s.cli_version,
            s.started_at, s.ended_at, s.messages.len() as i64,
            s.totals.input, s.totals.output, s.totals.cache_read, s.totals.cache_creation,
            s.totals.reasoning,
            cost, s.is_archived as i64, source_path, s.raw_meta.to_string(),
            mtime, size, hash,
            facets.turn_count, facets.error_count, facets.interruption_count, facets.compaction_count,
            facets.sidechain_message_count, facets.agent_spawn_count, facets.skill_count,
            facets.command_count, facets.thinking_block_count, facets.thinking_chars,
            facets.active_seconds, outcome.map(|o| o.as_str()), work_type.map(|w| w.as_str()),
        ],
    )?;
    let session_id = conn.last_insert_rowid();

    let mut results: HashMap<String, i64> = HashMap::new();
    let mut result_errors: HashMap<String, Option<bool>> = HashMap::new();
    // (message_id, call_block_id, message_timestamp, block)
    let mut tool_use_blocks: Vec<(i64, i64, Option<String>, NormalizedBlock)> = Vec::new();
    let mut msg_ids: Vec<i64> = Vec::with_capacity(s.messages.len());

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
        msg_ids.push(message_id);

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

    for (message_id, call_block_id, ts, b) in &tool_use_blocks {
        let name = b.tool_name.clone().unwrap_or_default();
        let (kind, server, base) = classify_tool(&name);
        let result_block_id = b
            .tool_use_id
            .as_ref()
            .and_then(|id| results.get(id))
            .copied();
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

    for fr in &refs {
        conn.execute(
            "INSERT INTO file_ref(session_id, message_id, path, rel_path, ext, operation, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                session_id,
                msg_ids.get(fr.message_idx),
                fr.path,
                fr.rel_path,
                fr.ext,
                fr.operation.as_str(),
                fr.timestamp,
            ],
        )?;
    }

    Ok(session_id)
}

fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub scanned: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub issues: usize,
    pub failed: usize,
    /// Sync stopped early because the caller's cancel flag was set; everything
    /// ingested so far is committed.
    pub cancelled: bool,
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
    collect(
        &config.claude_dir,
        Tool::ClaudeCode,
        false,
        |name| name.ends_with(".jsonl"),
        &mut out,
    );
    collect(
        &config.codex_dir.join("sessions"),
        Tool::Codex,
        false,
        is_rollout,
        &mut out,
    );
    collect(
        &config.codex_dir.join("archived_sessions"),
        Tool::Codex,
        true,
        is_rollout,
        &mut out,
    );
    out
}

fn is_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn collect(
    root: &Path,
    tool: Tool,
    archived: bool,
    want: impl Fn(&str) -> bool,
    out: &mut Vec<SourceFile>,
) {
    if !root.exists() {
        return;
    }
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if want(&name) {
            out.push(SourceFile {
                tool,
                path: entry.path().to_path_buf(),
                archived,
            });
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
    sync_cancellable(conn, config, &std::sync::atomic::AtomicBool::new(false))
}

/// Files parsed per parallel batch. Bounds both peak memory (parsed sessions
/// held at once) and cancellation latency (the flag is checked at every batch
/// and file boundary).
const PARSE_BATCH: usize = 64;

/// [`sync`], stopping early (after the in-flight file) once `cancel` reads true.
pub fn sync_cancellable(
    conn: &mut Connection,
    config: &Config,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<SyncReport> {
    use rayon::prelude::*;
    use std::sync::atomic::Ordering;

    let files = discover(config);
    let titles = codex_titles(config);
    let mut report = SyncReport {
        scanned: files.len(),
        ..Default::default()
    };

    // Decide which files changed (cheap, serial: stat + lookup).
    let mut to_read: Vec<SourceFile> = Vec::new();
    for f in files {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return Ok(report);
        }
        let meta = match std::fs::metadata(&f.path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len() as i64;
        let mtime = mtime_secs(&meta);
        let path_str = f.path.to_string_lossy().to_string();
        let prior: Option<(i64, i64)> = conn
            .query_row(
                "SELECT size, mtime FROM ingest_source WHERE path = ?1",
                params![path_str],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if prior == Some((size, mtime)) {
            report.skipped += 1;
        } else {
            to_read.push(f);
        }
    }

    'batches: for batch in to_read.chunks(PARSE_BATCH) {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }

        // Read + hash + parse in parallel. `None` = a file we failed to read.
        let results: Vec<Option<(Prepared, ParsedSession)>> = batch
            .par_iter()
            .map(|f| {
                let content = std::fs::read_to_string(&f.path).ok()?;
                let meta = std::fs::metadata(&f.path).ok()?;
                let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
                let line_count = content.lines().count() as i64;
                let stem = f
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let parsed = match f.tool {
                    Tool::ClaudeCode => sources::claude::parse_session(&stem, &content),
                    Tool::Codex => sources::codex::parse_session(&stem, &content, &titles),
                };
                Some((
                    Prepared {
                        file: f.clone(),
                        line_count,
                        mtime: mtime_secs(&meta),
                        size: meta.len() as i64,
                        hash,
                    },
                    parsed,
                ))
            })
            .collect();
        report.failed += results.iter().filter(|r| r.is_none()).count();
        let prepared: Vec<(Prepared, ParsedSession)> = results.into_iter().flatten().collect();

        // Write each file in its own transaction (per-file atomicity: one bad file
        // rolls back only itself; SQLite is single-writer so writes are serialized).
        for (prep, mut parsed) in prepared {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                break 'batches;
            }
            parsed.session.is_archived = prep.file.archived;
            let path_str = prep.file.path.to_string_lossy().to_string();
            let tx = conn.transaction()?;
            // Release any FK reference from ingest_source -> session so upsert_session
            // can safely DELETE the old session row without a constraint violation.
            tx.execute(
                "UPDATE ingest_source SET session_id = NULL WHERE path = ?1",
                params![path_str],
            )?;
            let session_id =
                upsert_session(&tx, &parsed, &path_str, prep.mtime, prep.size, &prep.hash)?;
            // Clear any prior issues for this path so re-ingest doesn't accumulate them.
            tx.execute(
                "DELETE FROM ingest_issue WHERE source_path = ?1",
                params![path_str],
            )?;
            for issue in &parsed.issues {
                tx.execute(
                    "INSERT INTO ingest_issue(source_path, line_no, error, raw_line, created_at)
                 VALUES (?1,?2,?3,?4,datetime('now'))",
                    params![path_str, issue.line_no as i64, issue.error, issue.raw_line],
                )?;
                report.issues += 1;
            }
            let status = if parsed.issues.is_empty() {
                "ok"
            } else {
                "ok_with_issues"
            };
            tx.execute(
            "INSERT INTO ingest_source(path, tool, size, mtime, hash, session_id, line_count, status, last_ingested_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))
             ON CONFLICT(path) DO UPDATE SET size=?3, mtime=?4, hash=?5, session_id=?6, line_count=?7, status=?8, last_ingested_at=datetime('now')",
            params![path_str, prep.file.tool.as_str(), prep.size, prep.mtime, prep.hash, session_id, prep.line_count, status],
        )?;
            tx.commit()?;
            report.ingested += 1;
        }
    }
    // Roll-up identity is data-derived and cheap; refresh it whenever new
    // projects/sessions landed so worktrees link to roots (and synthetic
    // attributions upgrade as real roots appear). Runs on cancelled syncs too:
    // committed sessions must not be left pointing at unresolved roots.
    if report.ingested > 0 {
        crate::worktree::resolve_worktree_roots(conn)?;
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
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sample.jsonl"
        ))
        .unwrap()
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
            .query_row("SELECT tool_kind, tool_base_name FROM tool_call", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(kind, "builtin");
        assert_eq!(base, "Read");

        let msg_id_nulls: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tool_call WHERE message_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(msg_id_nulls, 0, "tool_call.message_id must be populated");

        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH 'auth'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(fts >= 1, "FTS should find 'auth'");
    }

    #[test]
    fn upsert_writes_file_refs_and_facets() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();

        let claude = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/enriched.jsonl"
        ))
        .unwrap();
        let parsed = sources::claude::parse_session("sess-claude-enr", &claude);
        let tx = conn.unchecked_transaction().unwrap();
        let sid = upsert_session(&tx, &parsed, "/x/enr.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();

        let codex = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/enriched.jsonl"
        ))
        .unwrap();
        let parsed2 =
            sources::codex::parse_session("fallback", &codex, &std::collections::HashMap::new());
        let tx = conn.unchecked_transaction().unwrap();
        upsert_session(&tx, &parsed2, "/x/enr2.jsonl", 1, 2, "h2").unwrap();
        tx.commit().unwrap();

        let (claude_refs, codex_refs): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM file_ref WHERE session_id = ?1),
                        (SELECT COUNT(*) FROM file_ref WHERE session_id != ?1)",
                params![sid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(claude_refs, 4, "Read+Edit+Write+NotebookEdit");
        assert_eq!(codex_refs, 3, "apply_patch add/update/delete");

        // Spot row: the Read ref keeps its message link, rel path, ext, op.
        let (rel, ext, op, msg_linked): (String, String, String, i64) = conn
            .query_row(
                "SELECT rel_path, ext, operation, message_id IS NOT NULL
                 FROM file_ref WHERE session_id = ?1 AND operation = 'read'",
                params![sid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (rel.as_str(), ext.as_str(), op.as_str()),
            ("src/main.rs", "rs", "read")
        );
        assert_eq!(
            msg_linked, 1,
            "file_ref.message_id must link to the tool-use message"
        );

        let (
            turns,
            errors,
            interruptions,
            compactions,
            sidechain,
            agents,
            skills,
            commands,
            active,
        ): (i64, i64, i64, i64, i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT turn_count, error_count, interruption_count, compaction_count,
                        sidechain_message_count, agent_spawn_count, skill_count, command_count,
                        active_seconds
                 FROM session WHERE id = ?1",
                params![sid],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            (
                turns,
                errors,
                interruptions,
                compactions,
                sidechain,
                agents,
                skills,
                commands,
                active
            ),
            (1, 1, 1, 1, 2, 1, 1, 1, 490)
        );
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

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();

        let r1 = sync(&mut conn, &config).unwrap();
        assert_eq!(r1.ingested, 1);
        assert_eq!(r1.issues, 1);
        let issues1: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingest_issue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(issues1, 1);

        let sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sessions, 1);

        // Second run: nothing changed -> skipped, no duplicates.
        let r2 = sync(&mut conn, &config).unwrap();
        assert_eq!(r2.ingested, 0);
        assert_eq!(r2.skipped, 1);
        let sessions2: i64 = conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sessions2, 1);

        // Modify the file (different bad line) -> re-ingest; issues must NOT accumulate.
        let mut claude3 = claude_fixture();
        claude3.push_str("\nanother bad line {\n");
        write(&config.claude_dir.join("proj/sess.jsonl"), &claude3);
        let r3 = sync(&mut conn, &config).unwrap();
        assert_eq!(r3.ingested, 1, "changed file re-ingests");
        let issues3: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingest_issue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(issues3, 1, "issues must not accumulate across re-ingests");
        let sessions3: i64 = conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sessions3, 1);
    }

    #[test]
    fn cancelled_sync_stops_early_and_next_sync_completes() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        write(&claude_dir.join("proj/sess.jsonl"), &claude_fixture());

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();

        let cancel = std::sync::atomic::AtomicBool::new(true);
        let r = sync_cancellable(&mut conn, &config, &cancel).unwrap();
        assert!(r.cancelled);
        assert_eq!(r.ingested, 0);

        let r2 = sync(&mut conn, &config).unwrap();
        assert!(!r2.cancelled);
        assert_eq!(r2.ingested, 1);
    }

    #[test]
    fn sync_resolves_worktree_roots() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        write(&claude_dir.join("proj/sess.jsonl"), &claude_fixture());

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();
        let r = sync(&mut conn, &config).unwrap();
        assert_eq!(r.ingested, 1);

        // Every project produced by ingest has a resolved root.
        let unresolved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project WHERE root_source IS NULL OR root_path IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            unresolved, 0,
            "sync must resolve worktree roots for new projects"
        );
    }

    #[test]
    fn upsert_session_without_project_path_stores_null_project() {
        // A session whose project_path is None takes the `else` branch (no
        // project row, project_id stays NULL).
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let mut parsed = sources::claude::parse_session("sess-claude-1", &claude_fixture());
        parsed.session.project_path = None;
        let tx = conn.unchecked_transaction().unwrap();
        let sid = upsert_session(&tx, &parsed, "/x/sample.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();

        let project_id: Option<i64> = conn
            .query_row(
                "SELECT project_id FROM session WHERE id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(project_id, None);
        let projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM project", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            projects, 0,
            "no project row created for a None project_path"
        );
    }

    #[test]
    fn sync_reads_codex_session_index_titles() {
        // A Codex rollout file plus a session_index.jsonl that names it: the
        // title from the index must land on the ingested session (exercises
        // codex_titles' line/JSON/field-extraction path).
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");

        let codex_fx = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/sample.jsonl"
        ))
        .unwrap();
        // The rollout file's stem is the fallback id; the index keys by the
        // session's own id. Discover what id the parser assigns, then key the
        // index to it.
        let parsed = sources::codex::parse_session("rollout-x", &codex_fx, &HashMap::new());
        let sid = parsed.session.source_session_id.clone();
        write(&codex_dir.join("sessions/rollout-x.jsonl"), &codex_fx);
        write(
            &codex_dir.join("session_index.jsonl"),
            &format!(
                "{{\"id\":\"{sid}\",\"thread_name\":\"Indexed Title\"}}\nnot json, skipped\n{{\"id\":\"other\"}}\n"
            ),
        );

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();
        let r = sync(&mut conn, &config).unwrap();
        assert_eq!(r.ingested, 1);

        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM session WHERE source_session_id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title.as_deref(), Some("Indexed Title"));
    }

    #[test]
    fn sync_handles_duplicate_session_id_across_files() {
        // Two files in different project dirs share the SAME stem -> same
        // source_session_id. The second must REPLACE the first without an FK
        // violation. Regression test for: ingest_source.session_id orphaned when
        // a session row is deleted (fixed via ON DELETE SET NULL).
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        let fx = claude_fixture();
        write(&claude_dir.join("projA/dup.jsonl"), &fx);
        write(&claude_dir.join("projB/dup.jsonl"), &fx);

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();

        // Must NOT return Err (previously failed with FOREIGN KEY constraint failed).
        let r = sync(&mut conn, &config).unwrap();
        assert_eq!(r.ingested, 2);
        // Both files map to source_session_id "dup" (the stem) -> collapse to one session.
        let sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sessions, 1);
    }

    /// Parse the enriched Claude fixture (project path + messages + blocks +
    /// tool_use + file_refs), so `upsert_session` touches every table.
    fn enriched_parsed() -> ParsedSession {
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/enriched.jsonl"
        ))
        .unwrap();
        sources::claude::parse_session("sess-claude-enr", &content)
    }

    /// Migrate a fresh in-memory DB, then run `f` to break the schema before
    /// `upsert_session` runs against it. Foreign keys are disabled so a single
    /// table can be dropped/recreated in isolation. Asserts `upsert_session`
    /// surfaces the SQLite error (the `?` propagation on the failing statement).
    fn assert_upsert_errors(break_schema: &str) {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        conn.execute_batch(break_schema).unwrap();
        let parsed = enriched_parsed();
        let err = upsert_session(&conn, &parsed, "/x/enr.jsonl", 1, 2, "h").unwrap_err();
        assert!(matches!(err, crate::Error::Sqlite(_)));
    }

    #[test]
    fn upsert_errors_when_project_insert_fails() {
        // No `project` table -> the project INSERT `?` propagates.
        assert_upsert_errors("DROP TABLE project;");
    }

    #[test]
    fn upsert_errors_when_project_select_fails() {
        // `project` accepts the INSERT (path/name) but has no `id` column, so the
        // follow-up `SELECT id` `?` propagates.
        assert_upsert_errors(
            "DROP TABLE project;
             CREATE TABLE project(path TEXT PRIMARY KEY, name TEXT, first_seen_at TEXT, last_seen_at TEXT);",
        );
    }

    #[test]
    fn upsert_errors_when_session_delete_fails() {
        // No `session` table -> the pre-insert DELETE `?` propagates.
        assert_upsert_errors("DROP TABLE session;");
    }

    #[test]
    fn upsert_errors_when_session_insert_fails() {
        // `session` exists (so the DELETE succeeds) but is missing the wide column
        // set the INSERT lists, so the session INSERT `?` propagates.
        assert_upsert_errors(
            "DROP TABLE session;
             CREATE TABLE session(id INTEGER PRIMARY KEY, tool TEXT, source_session_id TEXT);",
        );
    }

    #[test]
    fn upsert_errors_when_message_insert_fails() {
        // No `message` table -> the per-message INSERT `?` propagates.
        assert_upsert_errors("DROP TABLE message;");
    }

    #[test]
    fn upsert_errors_when_block_insert_fails() {
        // No `block` table -> the per-block INSERT `?` propagates.
        assert_upsert_errors("DROP TABLE block;");
    }

    #[test]
    fn upsert_errors_when_tool_call_insert_fails() {
        // No `tool_call` table -> the tool-use INSERT `?` propagates (the fixture
        // has a Read tool_use).
        assert_upsert_errors("DROP TABLE tool_call;");
    }

    #[test]
    fn upsert_errors_when_file_ref_insert_fails() {
        // No `file_ref` table -> the file-ref INSERT `?` propagates (the fixture
        // has Read/Edit/Write/NotebookEdit refs).
        assert_upsert_errors("DROP TABLE file_ref;");
    }
}
