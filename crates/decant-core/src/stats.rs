use crate::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct Totals {
    pub sessions: i64,
    pub messages: i64,
    pub tool_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub estimated_cost_usd: f64,
}

/// Whole-archive rollup.
pub fn totals(conn: &Connection) -> Result<Totals> {
    let t = conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM session),
           (SELECT COUNT(*) FROM message),
           (SELECT COUNT(*) FROM tool_call),
           (SELECT COALESCE(SUM(total_input_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_output_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_cache_read_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_cache_creation_tokens),0) FROM session),
           (SELECT COALESCE(SUM(estimated_cost_usd),0.0) FROM session)",
        [],
        |r| {
            Ok(Totals {
                sessions: r.get(0)?,
                messages: r.get(1)?,
                tool_calls: r.get(2)?,
                input_tokens: r.get(3)?,
                output_tokens: r.get(4)?,
                cache_read_tokens: r.get(5)?,
                cache_creation_tokens: r.get(6)?,
                estimated_cost_usd: r.get(7)?,
            })
        },
    )?;
    Ok(t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Tool,
    Model,
    Project,
    Day,
}

impl Dimension {
    /// Parse the `--by` value. Returns None for unknown (caller reports a clear error).
    pub fn parse(s: &str) -> Option<Dimension> {
        match s {
            "tool" => Some(Dimension::Tool),
            "model" => Some(Dimension::Model),
            "project" => Some(Dimension::Project),
            "day" => Some(Dimension::Day),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DimRow {
    pub key: String,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
}

/// Per-dimension rollup, ordered by session count desc. The grouping expression is
/// chosen from a fixed match (never user text) so the format! is injection-safe.
pub fn by_dimension(conn: &Connection, dim: Dimension) -> Result<Vec<DimRow>> {
    let (group_expr, join) = match dim {
        Dimension::Tool => ("s.tool", ""),
        Dimension::Model => ("COALESCE(s.model, '(unknown)')", ""),
        Dimension::Project => (
            "COALESCE(p.path, '(none)')",
            "LEFT JOIN project p ON p.id = s.project_id",
        ),
        Dimension::Day => ("substr(s.started_at, 1, 10)", ""),
    };
    let sql = format!(
        "SELECT {group_expr} AS k,
                COUNT(*) AS sessions,
                COALESCE(SUM(s.total_input_tokens),0),
                COALESCE(SUM(s.total_output_tokens),0),
                COALESCE(SUM(s.estimated_cost_usd),0.0)
         FROM session s {join}
         GROUP BY k
         ORDER BY sessions DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DimRow {
                key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                sessions: r.get(1)?,
                input_tokens: r.get(2)?,
                output_tokens: r.get(3)?,
                estimated_cost_usd: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct ToolStatRow {
    pub tool_name: String,
    pub tool_kind: String,
    pub mcp_server: Option<String>,
    pub calls: i64,
    pub errors: i64,
}

/// Per-tool usage, most-called first. `errors_only` keeps only tools with >0 errors.
pub fn tool_usage(conn: &Connection, errors_only: bool, limit: i64) -> Result<Vec<ToolStatRow>> {
    let limit = if limit > 0 { limit } else { 50 };
    let having = if errors_only { "HAVING errors > 0" } else { "" };
    let sql = format!(
        "SELECT tool_name, tool_kind, mcp_server,
                COUNT(*) AS calls,
                COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
         FROM tool_call
         GROUP BY tool_name, tool_kind, mcp_server
         {having}
         ORDER BY calls DESC
         LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ToolStatRow {
                tool_name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tool_kind: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                mcp_server: r.get(2)?,
                calls: r.get(3)?,
                errors: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct McpStatRow {
    pub mcp_server: String,
    pub tools: i64,
    pub calls: i64,
    pub errors: i64,
}

/// Per-MCP-server usage, most-called first.
pub fn mcp_usage(conn: &Connection, limit: i64) -> Result<Vec<McpStatRow>> {
    let limit = if limit > 0 { limit } else { 50 };
    let mut stmt = conn.prepare(
        "SELECT mcp_server,
                COUNT(DISTINCT tool_name) AS tools,
                COUNT(*) AS calls,
                COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
         FROM tool_call
         WHERE tool_kind = 'mcp' AND mcp_server IS NOT NULL
         GROUP BY mcp_server
         ORDER BY calls DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(McpStatRow {
                mcp_server: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tools: r.get(1)?,
                calls: r.get(2)?,
                errors: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGroup {
    /// Hotspot files, keyed by project-relative path.
    Path,
    /// Language lens, keyed by file extension.
    Ext,
}

impl FileGroup {
    pub fn parse(s: &str) -> Option<FileGroup> {
        match s {
            "path" => Some(FileGroup::Path),
            "ext" => Some(FileGroup::Ext),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FileStatRow {
    /// rel_path (path fallback) for `Path`, extension for `Ext`.
    pub key: String,
    /// Owning project path (`Path` grouping only).
    pub project: Option<String>,
    pub reads: i64,
    pub edits: i64,
    pub writes: i64,
    pub deletes: i64,
    pub sessions: i64,
    pub last_touched_at: Option<String>,
}

/// File hotspots: which files (or extensions) the agents touch most, split by
/// operation. Ordered by total operations desc. The grouping expression comes
/// from a fixed match (never user text), so the format! is injection-safe.
pub fn file_hotspots(
    conn: &Connection,
    group: FileGroup,
    op: Option<crate::enrich::Operation>,
    limit: i64,
) -> Result<Vec<FileStatRow>> {
    let limit = if limit > 0 { limit } else { 50 };
    let (key_expr, proj_expr, join) = match group {
        FileGroup::Path => (
            "COALESCE(f.rel_path, f.path)",
            "p.path",
            "JOIN session s ON s.id = f.session_id LEFT JOIN project p ON p.id = s.project_id",
        ),
        FileGroup::Ext => (
            "COALESCE(f.ext, '(none)')",
            "NULL",
            "JOIN session s ON s.id = f.session_id",
        ),
    };
    let op_filter = match op {
        Some(_) => "WHERE f.operation = ?1",
        None => "",
    };
    let sql = format!(
        "SELECT {key_expr} AS k, {proj_expr} AS proj,
                SUM(f.operation = 'read') AS reads,
                SUM(f.operation = 'edit') AS edits,
                SUM(f.operation = 'write') AS writes,
                SUM(f.operation = 'delete') AS deletes,
                COUNT(DISTINCT f.session_id) AS sessions,
                MAX(f.timestamp) AS last_touched
         FROM file_ref f {join}
         {op_filter}
         GROUP BY k, proj
         ORDER BY (reads + edits + writes + deletes) DESC, k ASC
         LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(FileStatRow {
            key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            project: r.get(1)?,
            reads: r.get(2)?,
            edits: r.get(3)?,
            writes: r.get(4)?,
            deletes: r.get(5)?,
            sessions: r.get(6)?,
            last_touched_at: r.get(7)?,
        })
    };
    let rows = match op {
        Some(o) => stmt.query_map([o.as_str()], map_row)?,
        None => stmt.query_map([], map_row)?,
    }
    .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One session's facet counters + classification, for CLI `show`.
#[derive(Debug, Serialize)]
pub struct SessionFacetRow {
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
    pub outcome: Option<String>,
    pub work_type: Option<String>,
}

/// Fetch the deterministic facets for one session (None if the id is unknown).
pub fn session_facets(conn: &Connection, session_id: i64) -> Result<Option<SessionFacetRow>> {
    let row = conn
        .query_row(
            "SELECT turn_count, error_count, interruption_count, compaction_count,
                    sidechain_message_count, agent_spawn_count, skill_count, command_count,
                    thinking_block_count, thinking_chars, active_seconds, outcome, work_type
             FROM session WHERE id = ?1",
            [session_id],
            |r| {
                Ok(SessionFacetRow {
                    turn_count: r.get(0)?,
                    error_count: r.get(1)?,
                    interruption_count: r.get(2)?,
                    compaction_count: r.get(3)?,
                    sidechain_message_count: r.get(4)?,
                    agent_spawn_count: r.get(5)?,
                    skill_count: r.get(6)?,
                    command_count: r.get(7)?,
                    thinking_block_count: r.get(8)?,
                    thinking_chars: r.get(9)?,
                    active_seconds: r.get(10)?,
                    outcome: r.get(11)?,
                    work_type: r.get(12)?,
                })
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, schema, sources};

    fn seeded() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sample.jsonl"
        ))
        .unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &content);
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &parsed, "/x.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();
        conn
    }

    fn seeded_enriched() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        for (fixture, id) in [
            ("/../../fixtures/claude/enriched.jsonl", "claude"),
            ("/../../fixtures/codex/enriched.jsonl", "codex"),
        ] {
            let content =
                std::fs::read_to_string(format!("{}{}", env!("CARGO_MANIFEST_DIR"), fixture))
                    .unwrap();
            let parsed = if id == "claude" {
                sources::claude::parse_session("sess-enr-claude", &content)
            } else {
                sources::codex::parse_session("sess-enr-codex", &content, &Default::default())
            };
            let tx = conn.unchecked_transaction().unwrap();
            ingest::upsert_session(&tx, &parsed, &format!("/x/{id}.jsonl"), 1, 2, id).unwrap();
            tx.commit().unwrap();
        }
        conn
    }

    #[test]
    fn file_hotspots_by_path_orders_by_total_ops() {
        let conn = seeded_enriched();
        let rows = file_hotspots(&conn, FileGroup::Path, None, 50).unwrap();
        // claude: src/main.rs (read+edit), README.md (write), nb.ipynb (edit)
        // codex: docs/new.md (write), src/lib.rs (edit), old.txt (delete)
        assert_eq!(rows[0].key, "src/main.rs");
        assert_eq!((rows[0].reads, rows[0].edits), (1, 1));
        assert_eq!(rows[0].sessions, 1);
        assert_eq!(rows[0].project.as_deref(), Some("/Users/dev/proj"));
        assert_eq!(rows.len(), 6);
        assert!(rows[0].last_touched_at.is_some());
    }

    #[test]
    fn file_hotspots_op_filter_keeps_only_that_operation() {
        let conn = seeded_enriched();
        let rows = file_hotspots(
            &conn,
            FileGroup::Path,
            Some(crate::enrich::Operation::Edit),
            50,
        )
        .unwrap();
        let keys: Vec<_> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["nb.ipynb", "src/lib.rs", "src/main.rs"]);
        assert!(rows.iter().all(|r| r.reads == 0 && r.writes == 0));
    }

    #[test]
    fn file_hotspots_by_ext_rolls_up_languages() {
        let conn = seeded_enriched();
        let rows = file_hotspots(&conn, FileGroup::Ext, None, 50).unwrap();
        let rs = rows.iter().find(|r| r.key == "rs").unwrap();
        // src/main.rs read+edit (claude) + src/lib.rs edit (codex)
        assert_eq!((rs.reads, rs.edits), (1, 2));
        assert_eq!(rs.sessions, 2, "rs ops span both sessions");
        assert!(rs.project.is_none());
    }

    #[test]
    fn file_group_parse() {
        assert_eq!(FileGroup::parse("path"), Some(FileGroup::Path));
        assert_eq!(FileGroup::parse("ext"), Some(FileGroup::Ext));
        assert_eq!(FileGroup::parse("bogus"), None);
    }

    #[test]
    fn totals_rollup() {
        let conn = seeded();
        let t = totals(&conn).unwrap();
        assert_eq!(t.sessions, 1);
        assert_eq!(t.messages, 4);
        assert_eq!(t.tool_calls, 1);
        assert_eq!(t.input_tokens, 2700);
    }

    #[test]
    fn by_tool_and_day() {
        let conn = seeded();
        let by_tool = by_dimension(&conn, Dimension::Tool).unwrap();
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool[0].key, "claude_code");
        assert_eq!(by_tool[0].sessions, 1);

        let by_day = by_dimension(&conn, Dimension::Day).unwrap();
        assert_eq!(by_day[0].key, "2026-05-01");
    }

    #[test]
    fn dimension_parse() {
        assert_eq!(Dimension::parse("model"), Some(Dimension::Model));
        assert_eq!(Dimension::parse("nope"), None);
    }

    #[test]
    fn tool_usage_counts_builtin_call() {
        let conn = seeded();
        let tools = tool_usage(&conn, false, 50).unwrap();
        // fixture has one tool_use: builtin "Read"
        let read = tools.iter().find(|t| t.tool_name == "Read").unwrap();
        assert_eq!(read.tool_kind, "builtin");
        assert_eq!(read.calls, 1);
        assert_eq!(read.errors, 0);

        // errors_only filters it out (no errors in the fixture)
        assert!(tool_usage(&conn, true, 50).unwrap().is_empty());
    }

    #[test]
    fn mcp_usage_empty_for_builtin_only_fixture() {
        let conn = seeded();
        assert!(mcp_usage(&conn, 50).unwrap().is_empty());
    }

    #[test]
    fn by_dimension_project_and_model_group_correctly() {
        let conn = seeded();
        let by_project = by_dimension(&conn, Dimension::Project).unwrap();
        assert_eq!(by_project.len(), 1);
        assert_eq!(by_project[0].key, "/Users/dev/proj");
        assert_eq!(by_project[0].sessions, 1);

        let by_model = by_dimension(&conn, Dimension::Model).unwrap();
        assert_eq!(by_model[0].key, "claude-opus-4-7");
    }

    #[test]
    fn by_dimension_project_uses_placeholder_when_unlinked() {
        // A session with no project_id rolls up under "(none)".
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO session(id, tool, source_session_id) VALUES (1, 'codex', 's1');",
        )
        .unwrap();
        let by_project = by_dimension(&conn, Dimension::Project).unwrap();
        assert_eq!(by_project[0].key, "(none)");
    }

    #[test]
    fn session_facets_returns_row_for_known_id_and_none_for_unknown() {
        let conn = seeded_enriched();
        // The first ingested (claude) session has facet counters populated.
        let id: i64 = conn
            .query_row(
                "SELECT id FROM session WHERE source_session_id = 'sess-enr-claude'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let facets = session_facets(&conn, id).unwrap().expect("known session");
        assert_eq!(facets.turn_count, 1);
        assert!(facets.active_seconds > 0);

        // Unknown id → Ok(None), not an error.
        assert!(session_facets(&conn, 999_999).unwrap().is_none());
    }

    #[test]
    fn stat_queries_propagate_db_errors() {
        // An un-migrated DB lacks every table, so `totals`, `mcp_usage`, and
        // `session_facets` all surface the SQLite error via their `?`.
        let bare = db::open_in_memory().unwrap();
        assert!(totals(&bare).is_err());
        assert!(mcp_usage(&bare, 10).is_err());
        // session_facets' error is a non-NoRows SQLite error (no `session`
        // table), exercising its `other => Err(other)` arm.
        assert!(session_facets(&bare, 1).is_err());
    }
}
