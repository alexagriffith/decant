use crate::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub tool: String,
    pub source_session_id: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub estimated_cost_usd: f64,
    pub is_archived: bool,
}

fn map_session_summary(r: &rusqlite::Row) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: r.get(0)?,
        tool: r.get(1)?,
        source_session_id: r.get(2)?,
        title: r.get(3)?,
        project_path: r.get(4)?,
        model: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
        message_count: r.get(8)?,
        total_input_tokens: r.get(9)?,
        total_output_tokens: r.get(10)?,
        estimated_cost_usd: r.get(11)?,
        is_archived: r.get::<_, i64>(12)? != 0,
    })
}

#[derive(Debug, Default)]
pub struct ListFilter {
    pub tool: Option<String>,
    pub limit: i64,
}

pub fn list_sessions(conn: &Connection, filter: &ListFilter) -> Result<Vec<SessionSummary>> {
    let limit = if filter.limit > 0 { filter.limit } else { 50 };
    let mut sql = String::from(
        "SELECT s.id, s.tool, s.source_session_id, s.title, p.path, s.model, s.started_at, s.ended_at,
                s.message_count, s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd, s.is_archived
         FROM session s LEFT JOIN project p ON p.id = s.project_id",
    );
    if filter.tool.is_some() {
        sql.push_str(" WHERE s.tool = ?1");
    }
    sql.push_str(" ORDER BY s.started_at DESC LIMIT ");
    sql.push_str(&limit.to_string());

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(tool) = &filter.tool {
        stmt.query_map(params![tool], map_session_summary)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], map_session_summary)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub session_id: i64,
    pub session_title: Option<String>,
    pub tool: String,
    pub block_id: i64,
    pub snippet: String,
}

pub fn search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
    let limit = if limit > 0 { limit } else { 30 };
    let mut stmt = conn.prepare(
        "SELECT b.session_id, s.title, s.tool, b.id,
                COALESCE(snippet(block_fts, 0, '[', ']', '…', 12),
                         snippet(block_fts, 1, '[', ']', '…', 12),
                         snippet(block_fts, 2, '[', ']', '…', 12), '') AS snip
         FROM block_fts
         JOIN block b ON b.id = block_fts.rowid
         JOIN session s ON s.id = b.session_id
         WHERE block_fts MATCH ?1
         ORDER BY bm25(block_fts)
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |r| {
            Ok(SearchHit {
                session_id: r.get(0)?,
                session_title: r.get(1)?,
                tool: r.get(2)?,
                block_id: r.get(3)?,
                snippet: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct BlockView {
    pub block_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub tool_result: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub blocks: Vec<BlockView>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub messages: Vec<MessageView>,
}

pub fn get_session(conn: &Connection, id: i64) -> Result<Option<SessionDetail>> {
    let summary = {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.tool, s.source_session_id, s.title, p.path, s.model, s.started_at, s.ended_at,
                    s.message_count, s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd, s.is_archived
             FROM session s LEFT JOIN project p ON p.id = s.project_id WHERE s.id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_session_summary)?;
        match rows.next() {
            Some(r) => r?,
            None => return Ok(None),
        }
    };

    // Single query: messages LEFT JOIN their blocks, ordered. Grouped in Rust.
    let mut stmt = conn.prepare(
        "SELECT m.id, m.role, m.timestamp, m.model,
                b.type, b.text, b.tool_name, b.tool_input, b.tool_result
         FROM message m
         LEFT JOIN block b ON b.message_id = m.id
         WHERE m.session_id = ?1
         ORDER BY m.seq, b.ordinal",
    )?;
    let rows = stmt
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut messages: Vec<MessageView> = Vec::new();
    let mut current_mid: Option<i64> = None;
    for (mid, role, ts, model, btype, text, tool_name, tool_input, tool_result) in rows {
        if current_mid != Some(mid) {
            current_mid = Some(mid);
            messages.push(MessageView {
                role: role.unwrap_or_else(|| "unknown".to_string()),
                timestamp: ts,
                model,
                blocks: Vec::new(),
            });
        }
        if let Some(block_type) = btype {
            if let Some(last) = messages.last_mut() {
                last.blocks.push(BlockView {
                    block_type,
                    text,
                    tool_name,
                    tool_input,
                    tool_result,
                });
            }
        }
    }
    Ok(Some(SessionDetail { summary, messages }))
}

#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
    pub sessions: i64,
    pub estimated_cost_usd: f64,
    pub last_seen_at: Option<String>,
}

pub fn list_projects(conn: &Connection) -> Result<Vec<ProjectSummary>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.path, p.name,
                COUNT(s.id) AS sessions,
                COALESCE(SUM(s.estimated_cost_usd), 0.0) AS cost,
                MAX(s.ended_at) AS last_seen
         FROM project p
         LEFT JOIN session s ON s.project_id = p.id
         GROUP BY p.id, p.path, p.name
         ORDER BY sessions DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProjectSummary {
                id: r.get(0)?,
                path: r.get(1)?,
                name: r.get(2)?,
                sessions: r.get(3)?,
                estimated_cost_usd: r.get(4)?,
                last_seen_at: r.get(5)?,
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
    fn list_and_get_and_search() {
        let conn = seeded();
        let list = list_sessions(&conn, &ListFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title.as_deref(), Some("Fix the failing auth test"));

        let detail = get_session(&conn, list[0].id).unwrap().unwrap();
        assert_eq!(detail.messages.len(), 4);

        let hits = search(&conn, "auth", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].tool, "claude_code");
    }

    #[test]
    fn list_sessions_filters_by_tool() {
        let conn = seeded();
        // Matching tool returns the session via the WHERE s.tool = ?1 branch.
        let claude = list_sessions(
            &conn,
            &ListFilter {
                tool: Some("claude_code".into()),
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(claude.len(), 1);
        // A non-matching tool filters everything out.
        let codex = list_sessions(
            &conn,
            &ListFilter {
                tool: Some("codex".into()),
                limit: 10,
            },
        )
        .unwrap();
        assert!(codex.is_empty());
    }

    #[test]
    fn get_session_unknown_id_is_none() {
        let conn = seeded();
        assert!(get_session(&conn, 999_999).unwrap().is_none());
    }

    #[test]
    fn search_no_match_returns_empty() {
        let conn = seeded();
        assert!(search(&conn, "zzznotpresentzzz", 10).unwrap().is_empty());
    }

    #[test]
    fn list_projects_rolls_up() {
        let conn = seeded();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sessions, 1);
        assert_eq!(projects[0].path, "/Users/dev/proj");
    }

    #[test]
    fn queries_propagate_db_errors() {
        // An un-migrated DB has none of the tables, so each query's `prepare`/
        // `query_map` `?` propagates the SQLite error.
        let bare = db::open_in_memory().unwrap();
        assert!(search(&bare, "x", 10).is_err());
        assert!(get_session(&bare, 1).is_err());
        assert!(list_projects(&bare).is_err());
    }

    #[test]
    fn get_session_propagates_messages_query_error() {
        // The summary query succeeds (session exists), then the messages query's
        // `prepare` `?` fails because the `message` table is gone.
        let conn = seeded();
        let id = list_sessions(&conn, &ListFilter::default()).unwrap()[0].id;
        conn.execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE message;")
            .unwrap();
        assert!(get_session(&conn, id).is_err());
    }
}
