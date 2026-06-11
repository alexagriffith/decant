use crate::Result;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = include_str!("schema_v1.sql");

/// The latest schema version `migrate` brings a database to.
pub const LATEST_VERSION: i64 = 5;

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

/// v5: indexes for every FK child column (and the re-ingest delete key).
/// SQLite checks FK constraints and cascades per deleted parent row; without an
/// index each check is a full scan of the child table, which made re-ingest of
/// one active session read the entire archive thousands of times over and
/// wedged the daemon's sync at ~100% CPU. `IF NOT EXISTS` makes this harmless
/// on fresh DBs where the v1 batch already created them.
const MIGRATION_V5: &str = "
CREATE INDEX IF NOT EXISTS idx_session_source ON session(tool, source_session_id);
CREATE INDEX IF NOT EXISTS idx_message_parent ON message(parent_id);
CREATE INDEX IF NOT EXISTS idx_toolcall_message ON tool_call(message_id);
CREATE INDEX IF NOT EXISTS idx_toolcall_call_block ON tool_call(call_block_id);
CREATE INDEX IF NOT EXISTS idx_toolcall_result_block ON tool_call(result_block_id);
CREATE INDEX IF NOT EXISTS idx_fileref_message ON file_ref(message_id);
CREATE INDEX IF NOT EXISTS idx_ingest_source_session ON ingest_source(session_id);";

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
    if current < 4 {
        apply_v4(conn)?;
    }
    if current < 5 {
        apply(conn, 5, MIGRATION_V5)?;
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
    add_column_if_missing(&tx, "project", "is_worktree", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&tx, "project", "root_path", "TEXT")?;
    add_column_if_missing(&tx, "project", "worktree_label", "TEXT")?;
    add_column_if_missing(&tx, "project", "worktree_tool", "TEXT")?;
    add_column_if_missing(&tx, "project", "root_source", "TEXT")?;
    tx.commit()?;
    crate::worktree::resolve_worktree_roots(conn)?;
    conn.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'))",
        [],
    )?;
    Ok(())
}

/// v4: the `file_ref` table + per-session facet columns (deterministic
/// enrichment), then invalidate the ingest size memo (`size = -1`) so the next
/// sync re-parses every source file and backfills enrichment for the whole
/// archive. Everything (DDL, memo, version record) commits in one transaction:
/// a mid-way failure re-runs v4 from the top, and each piece is idempotent.
fn apply_v4(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS file_ref (
           id INTEGER PRIMARY KEY,
           session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
           message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
           path TEXT NOT NULL,
           rel_path TEXT,
           ext TEXT,
           operation TEXT NOT NULL,
           timestamp TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_fileref_session ON file_ref(session_id);
         CREATE INDEX IF NOT EXISTS idx_fileref_path ON file_ref(rel_path, operation);",
    )?;
    for (col, decl) in [
        ("turn_count", "INTEGER NOT NULL DEFAULT 0"),
        ("error_count", "INTEGER NOT NULL DEFAULT 0"),
        ("interruption_count", "INTEGER NOT NULL DEFAULT 0"),
        ("compaction_count", "INTEGER NOT NULL DEFAULT 0"),
        ("sidechain_message_count", "INTEGER NOT NULL DEFAULT 0"),
        ("agent_spawn_count", "INTEGER NOT NULL DEFAULT 0"),
        ("skill_count", "INTEGER NOT NULL DEFAULT 0"),
        ("command_count", "INTEGER NOT NULL DEFAULT 0"),
        ("thinking_block_count", "INTEGER NOT NULL DEFAULT 0"),
        ("thinking_chars", "INTEGER NOT NULL DEFAULT 0"),
        ("active_seconds", "INTEGER NOT NULL DEFAULT 0"),
        ("outcome", "TEXT"),
        ("work_type", "TEXT"),
    ] {
        add_column_if_missing(&tx, "session", col, decl)?;
    }
    tx.execute("UPDATE ingest_source SET size = -1", [])?;
    tx.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (4, datetime('now'))",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// Add a column to `table` only if it does not already exist.
fn add_column_if_missing(conn: &Connection, table: &str, col: &str, decl: &str) -> Result<()> {
    let exists = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|x| x.ok())
        .any(|name| name == col);
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"), [])?;
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
            "file_ref",
        ] {
            assert!(table_exists(&conn, t), "missing table {t}");
        }

        let versions: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 5, "each migration recorded exactly once");
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
        assert_eq!(max, LATEST_VERSION);
    }

    #[test]
    fn every_foreign_key_column_is_indexed() {
        // An unindexed FK child column makes every parent-row delete scan the
        // child table (SQLite checks cascades/constraints per deleted row).
        // At archive scale that turns re-ingest's delete+reinsert into hours of
        // full-table scans and wedges the daemon. Every FK's leftmost child
        // column must be covered by an index (or be the table's sole PK).
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let mut missing: Vec<String> = Vec::new();
        for table in &tables {
            // Leftmost FK child column per FK (seq 0 of each foreign key).
            let fk_cols: Vec<String> = conn
                .prepare(&format!("PRAGMA foreign_key_list({table})"))
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, i64>(1)?, r.get::<_, String>(3)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .filter(|(seq, _)| *seq == 0)
                .map(|(_, col)| col)
                .collect();
            if fk_cols.is_empty() {
                continue;
            }

            // Leftmost column of every index on the table.
            let index_names: Vec<String> = conn
                .prepare(&format!("PRAGMA index_list({table})"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let mut covered: Vec<String> = index_names
                .iter()
                .filter_map(|idx| {
                    conn.query_row(&format!("PRAGMA index_info({idx})"), [], |r| {
                        r.get::<_, String>(2)
                    })
                    .ok()
                })
                .collect();
            // A single-column PRIMARY KEY is implicitly indexed (rowid alias).
            let pk_cols: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .filter(|(_, pk)| *pk > 0)
                .map(|(name, _)| name)
                .collect();
            if pk_cols.len() == 1 {
                covered.extend(pk_cols);
            }

            for col in fk_cols {
                if !covered.contains(&col) {
                    missing.push(format!("{table}.{col}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "FK columns without a covering index (parent deletes will full-scan): {missing:?}"
        );
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
    fn v4_adds_file_ref_and_facet_columns_idempotently() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // idempotent

        assert!(table_exists(&conn, "file_ref"), "missing table file_ref");
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(session)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for c in [
            "turn_count",
            "error_count",
            "interruption_count",
            "compaction_count",
            "sidechain_message_count",
            "agent_spawn_count",
            "skill_count",
            "command_count",
            "thinking_block_count",
            "thinking_chars",
            "active_seconds",
            "outcome",
            "work_type",
        ] {
            assert!(cols.contains(&c.to_string()), "missing session column {c}");
        }
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(max, LATEST_VERSION);
    }

    #[test]
    fn v4_invalidates_ingest_memo_for_backfill() {
        // Model a DB at v3: real-shaped session + ingest_source tables (old
        // columns only) with one already-ingested file memoized.
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
             CREATE TABLE session(id INTEGER PRIMARY KEY, tool TEXT NOT NULL,
                                  source_session_id TEXT NOT NULL, is_archived INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE message(id INTEGER PRIMARY KEY, session_id INTEGER, parent_id INTEGER);
             CREATE TABLE tool_call(id INTEGER PRIMARY KEY, session_id INTEGER, message_id INTEGER,
                                    call_block_id INTEGER, result_block_id INTEGER);
             CREATE TABLE ingest_source(path TEXT PRIMARY KEY, tool TEXT, size INTEGER, mtime INTEGER,
                                        hash TEXT, session_id INTEGER REFERENCES session(id) ON DELETE SET NULL,
                                        line_count INTEGER, status TEXT, error TEXT, last_ingested_at TEXT);
             INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'));
             INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'));
             INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'));
             INSERT INTO ingest_source(path, tool, size, mtime) VALUES ('/x/sess.jsonl', 'claude_code', 100, 1700000000);",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let (size, mtime, rows): (i64, i64, i64) = conn
            .query_row(
                "SELECT size, mtime, (SELECT COUNT(*) FROM ingest_source) FROM ingest_source",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            size, -1,
            "v4 must invalidate the size memo so sync re-ingests"
        );
        assert_eq!(
            mtime, 1700000000,
            "mtime preserved; size alone breaks the match"
        );
        assert_eq!(rows, 1, "memo rows preserved, not deleted");
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(max, LATEST_VERSION);
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
        assert_eq!(max, LATEST_VERSION);
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
             CREATE TABLE session(id INTEGER PRIMARY KEY, tool TEXT, source_session_id TEXT,
                                  project_id INTEGER, started_at TEXT);
             CREATE TABLE message(id INTEGER PRIMARY KEY, session_id INTEGER, parent_id INTEGER);
             CREATE TABLE tool_call(id INTEGER PRIMARY KEY, session_id INTEGER, message_id INTEGER,
                                    call_block_id INTEGER, result_block_id INTEGER);
             CREATE TABLE ingest_source(path TEXT PRIMARY KEY, size INTEGER, mtime INTEGER, session_id INTEGER);
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

    #[test]
    fn migrate_propagates_create_table_error_on_readonly_db() {
        use rusqlite::OpenFlags;
        // Create an empty file DB, then reopen it read-only: the very first
        // `CREATE TABLE IF NOT EXISTS schema_migrations` write fails, so
        // `migrate`'s opening `?` propagates.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ro.db");
        db::open(&path).unwrap(); // create the file
        let ro = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        assert!(migrate(&ro).is_err());
    }

    #[test]
    fn migrate_propagates_version_query_error() {
        // A pre-existing `schema_migrations` table without a `version` column:
        // the `CREATE ... IF NOT EXISTS` is a no-op (table exists), then the
        // `SELECT COALESCE(MAX(version), 0)` `?` propagates (no such column).
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE schema_migrations(applied_at TEXT);")
            .unwrap();
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn apply_v3_propagates_version_record_error() {
        // A v2 DB (so `migrate` runs `apply_v3`) whose `project` and worktree
        // resolution succeed, but the final version-record INSERT fails because
        // a trigger forbids writes to `schema_migrations` for version 3.
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        // Roll the recorded version back to 2 so `apply_v3` re-runs, and install
        // a trigger that aborts the v3 INSERT specifically.
        conn.execute_batch(
            "DELETE FROM schema_migrations WHERE version >= 3;
             CREATE TRIGGER block_v3 BEFORE INSERT ON schema_migrations
               WHEN NEW.version = 3
               BEGIN SELECT RAISE(ABORT, 'no v3'); END;",
        )
        .unwrap();
        let err = apply_v3(&conn).unwrap_err();
        assert!(matches!(err, crate::Error::Sqlite(_)));
    }
}
