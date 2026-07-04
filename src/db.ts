import { Database } from "bun:sqlite";
import schemaSql from "./schema.sql" with { type: "text" };

/// Highest schema version this build understands. src/schema.sql is the
/// effective DDL with migrations 1..LATEST_SCHEMA_VERSION already applied
/// and is now the frozen baseline, so a fresh archive is created in one step
/// and stamped with the full migration history.
export const LATEST_SCHEMA_VERSION = 8;

/**
 * Open (or create) a decant archive and guarantee it is at
 * LATEST_SCHEMA_VERSION. The connection comes back in WAL mode with foreign
 * keys enforced and a busy timeout set.
 */
export function openDb(path: string): Database {
  const db = new Database(path, { create: true, strict: true });
  db.exec("PRAGMA busy_timeout = 5000;");
  db.exec("PRAGMA foreign_keys = ON;");
  db.exec("PRAGMA synchronous = NORMAL;");
  db.exec("PRAGMA journal_mode = WAL;");
  try {
    ensureSchema(db);
  } catch (error) {
    db.close();
    throw error;
  }
  return db;
}

function ensureSchema(db: Database): void {
  const hasMigrations = db
    .query("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'")
    .get();

  if (!hasMigrations) {
    db.exec("BEGIN IMMEDIATE;");
    try {
      db.exec(schemaSql);
      const mark = db.prepare(
        "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
      );
      for (let version = 1; version <= LATEST_SCHEMA_VERSION; version += 1) {
        mark.run(version);
      }
      db.exec("COMMIT;");
    } catch (error) {
      db.exec("ROLLBACK;");
      throw error;
    }
    return;
  }

  const current =
    (db.query("SELECT MAX(version) AS v FROM schema_migrations").get() as { v: number | null }).v ??
    0;
  if (current > LATEST_SCHEMA_VERSION) {
    throw new Error(
      `archive schema version ${current} is newer than this build supports ` +
        `(${LATEST_SCHEMA_VERSION}); upgrade decant`,
    );
  }
  if (current < LATEST_SCHEMA_VERSION) {
    throw new Error(
      `archive schema version ${current} predates this build's baseline ` +
        `(${LATEST_SCHEMA_VERSION}); rebuild the archive: delete it and re-ingest ` +
        "(ingest is idempotent over the source directories)",
    );
  }
}
