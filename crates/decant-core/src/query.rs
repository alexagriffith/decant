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
    let map = |r: &rusqlite::Row| {
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
    };
    let rows = if let Some(tool) = &filter.tool {
        stmt.query_map(params![tool], map)?.collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?
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
    let summaries = {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.tool, s.source_session_id, s.title, p.path, s.model, s.started_at, s.ended_at,
                    s.message_count, s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd, s.is_archived
             FROM session s LEFT JOIN project p ON p.id = s.project_id WHERE s.id = ?1",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(SessionSummary {
                id: r.get(0)?, tool: r.get(1)?, source_session_id: r.get(2)?, title: r.get(3)?,
                project_path: r.get(4)?, model: r.get(5)?, started_at: r.get(6)?, ended_at: r.get(7)?,
                message_count: r.get(8)?, total_input_tokens: r.get(9)?, total_output_tokens: r.get(10)?,
                estimated_cost_usd: r.get(11)?, is_archived: r.get::<_, i64>(12)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let Some(summary) = summaries.into_iter().next() else { return Ok(None) };

    let mut stmt = conn.prepare(
        "SELECT m.id, m.role, m.timestamp, m.model FROM message m WHERE m.session_id = ?1 ORDER BY m.seq",
    )?;
    let msg_rows = stmt
        .query_map(params![id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut messages = Vec::new();
    for (mid, role, ts, model) in msg_rows {
        let mut bstmt = conn.prepare(
            "SELECT type, text, tool_name, tool_input, tool_result FROM block WHERE message_id = ?1 ORDER BY ordinal",
        )?;
        let blocks = bstmt
            .query_map(params![mid], |r| {
                Ok(BlockView {
                    block_type: r.get(0)?, text: r.get(1)?, tool_name: r.get(2)?,
                    tool_input: r.get(3)?, tool_result: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        messages.push(MessageView { role: role.unwrap_or_default(), timestamp: ts, model, blocks });
    }
    Ok(Some(SessionDetail { summary, messages }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, schema, sources};

    fn seeded() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
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
}
