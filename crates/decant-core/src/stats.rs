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
}
