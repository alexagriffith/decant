use crate::Result;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = include_str!("schema_v1.sql");

/// Apply pending migrations. Idempotent.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations(
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    if current < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    #[test]
    fn migrate_creates_core_tables_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // second run must not error or duplicate

        for t in [
            "project", "session", "message", "block", "tool_call",
            "block_fts", "ingest_source", "ingest_issue", "model_pricing",
        ] {
            assert!(table_exists(&conn, t), "missing table {t}");
        }

        let versions: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 1, "migration recorded exactly once");
    }

    #[test]
    fn fts_search_round_trips_through_triggers() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO project(id, path) VALUES (1, '/p');
             INSERT INTO session(id, tool, source_session_id, project_id)
                VALUES (1, 'claude_code', 's1', 1);
             INSERT INTO message(id, session_id, seq, role, raw)
                VALUES (1, 1, 0, 'assistant', '{}');
             INSERT INTO block(id, message_id, session_id, ordinal, type, text)
                VALUES (1, 1, 1, 0, 'text', 'the quick brown fox');",
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH 'brown'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }
}
