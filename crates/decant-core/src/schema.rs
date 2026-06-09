use crate::Result;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = include_str!("schema_v1.sql");

/// The latest schema version `migrate` brings a database to.
pub const LATEST_VERSION: i64 = 3;

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
    if current < 3 {
        apply_v3(conn)?;
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

/// v3: add worktree roll-up columns to `project` (ALTER lacks IF NOT EXISTS, so
/// each is PRAGMA-guarded — harmless on a fresh DB where `schema_v1.sql` already
/// created them), then backfill the resolution for existing rows. The version is
/// recorded only after the backfill succeeds: if the backfill errors or the
/// process dies mid-way, the next `migrate` re-runs v3 from the top (the column
/// guards and the resolver are both idempotent), so a partial v3 always
/// self-heals instead of being recorded as done.
fn apply_v3(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    add_column_if_missing(&tx, "is_worktree", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&tx, "root_path", "TEXT")?;
    add_column_if_missing(&tx, "worktree_label", "TEXT")?;
    add_column_if_missing(&tx, "worktree_tool", "TEXT")?;
    add_column_if_missing(&tx, "root_source", "TEXT")?;
    tx.commit()?;
    crate::worktree::resolve_worktree_roots(conn)?;
    conn.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'))",
        [],
    )?;
    Ok(())
}

/// Add a column to `project` only if it does not already exist.
fn add_column_if_missing(conn: &Connection, col: &str, decl: &str) -> Result<()> {
    let exists = conn
        .prepare("PRAGMA table_info(project)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|x| x.ok())
        .any(|name| name == col);
    if !exists {
        conn.execute(&format!("ALTER TABLE project ADD COLUMN {col} {decl}"), [])?;
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
        assert_eq!(versions, 3, "each migration recorded exactly once");
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
        assert_eq!(max, 3);
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

    #[test]
    fn v3_adds_project_worktree_columns_idempotently() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // idempotent

        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(project)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for c in [
            "is_worktree",
            "root_path",
            "worktree_label",
            "worktree_tool",
            "root_source",
        ] {
            assert!(cols.contains(&c.to_string()), "missing column {c}");
        }
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(max, 3);
    }

    #[test]
    fn v3_backfills_existing_worktree_rows_on_a_v2_db() {
        // Model a DB at v2: project table WITHOUT the new columns + a minimal
        // session table for the resolve query's join.
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
             CREATE TABLE project(id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, name TEXT,
                                  first_seen_at TEXT, last_seen_at TEXT);
             CREATE TABLE session(id INTEGER PRIMARY KEY, project_id INTEGER, started_at TEXT);
             INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'));
             INSERT INTO project(path, name) VALUES ('/home/x/dosu/dosu', 'dosu');
             INSERT INTO project(path, name) VALUES ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let (is_wt, root): (i64, String) = conn
            .query_row(
                "SELECT is_worktree, root_path FROM project WHERE path = '/home/x/.warp-worktrees/dosu-agate-spire'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(is_wt, 1);
        assert_eq!(
            root, "/home/x/dosu/dosu",
            "warp worktree name-matched to root"
        );
    }
}
