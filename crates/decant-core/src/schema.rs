use crate::Result;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = include_str!("schema_v1.sql");

/// The latest schema version `migrate` brings a database to.
pub const LATEST_VERSION: i64 = 2;

/// v2: the `recommendation` table (signals + evergreen catalog, materialized
/// with state by `recommendations::regenerate`). `CREATE TABLE IF NOT EXISTS`
/// makes this idempotent and harmless on a fresh DB where the v1 batch already
/// created it (the table lives in `schema_v1.sql` so new DBs get it directly);
/// running it here brings an existing v1 DB forward without touching its data.
const MIGRATION_V2: &str = "
CREATE TABLE IF NOT EXISTS recommendation (
  key            TEXT PRIMARY KEY,
  kind           TEXT NOT NULL,
  category       TEXT,
  title          TEXT NOT NULL,
  detail         TEXT,
  suggestion     TEXT,
  prompt         TEXT,
  url            TEXT,
  link_label     TEXT,
  icon           TEXT,
  tone           TEXT,
  score          REAL,
  status         TEXT NOT NULL DEFAULT 'open',
  status_source  TEXT,
  note           TEXT,
  first_seen_at  TEXT,
  updated_at     TEXT,
  implemented_at TEXT
);";

/// Apply pending migrations. Idempotent: each version is applied at most once,
/// gated by the highest recorded version in `schema_migrations`.
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
        apply(conn, 1, SCHEMA_V1)?;
    }
    if current < 2 {
        apply(conn, 2, MIGRATION_V2)?;
    }
    Ok(())
}

/// Apply one migration batch and record its version atomically; rolls back on
/// any error so a half-applied version is never recorded.
fn apply(conn: &Connection, version: i64, sql: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(sql)?;
    tx.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, datetime('now'))",
        [version],
    )?;
    tx.commit()?;
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
            "project",
            "session",
            "message",
            "block",
            "tool_call",
            "block_fts",
            "ingest_source",
            "ingest_issue",
            "model_pricing",
            "recommendation",
        ] {
            assert!(table_exists(&conn, t), "missing table {t}");
        }

        let versions: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 2, "each migration recorded exactly once");
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(max, LATEST_VERSION);
    }

    #[test]
    fn v2_migration_brings_a_v1_db_forward_without_data_loss() {
        // Simulate an existing DB that only ever saw v1: apply v1's batch and
        // record version 1, with a row of data present.
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )
        .unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute_batch(SCHEMA_V1).unwrap();
        tx.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )
        .unwrap();
        tx.execute_batch("INSERT INTO project(id, path) VALUES (1, '/p');")
            .unwrap();
        tx.commit().unwrap();

        // recommendation already exists because it lives in schema_v1.sql; drop
        // it to truly model a DB created before v2 existed.
        conn.execute_batch("DROP TABLE recommendation;").unwrap();
        assert!(!table_exists(&conn, "recommendation"));

        migrate(&conn).unwrap();
        assert!(table_exists(&conn, "recommendation"));
        // Existing data survived.
        let projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM project", [], |r| r.get(0))
            .unwrap();
        assert_eq!(projects, 1);
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(max, 2);
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
