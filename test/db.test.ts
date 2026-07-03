import type { Database } from "bun:sqlite";
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LATEST_SCHEMA_VERSION, openDb } from "../src/db.ts";
import schemaSql from "../src/schema.sql" with { type: "text" };

const workDir = mkdtempSync(join(tmpdir(), "decant-db-test-"));
afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let dbCounter = 0;
function freshPath(): string {
  dbCounter += 1;
  return join(workDir, `archive-${dbCounter}.db`);
}

// Inventory of the Rust-generated baseline (schema version 8). Shadow tables
// of block_fts are excluded; they are implementation details of FTS5.
const BASELINE_TABLES = [
  "block",
  "block_fts",
  "file_ref",
  "ingest_issue",
  "ingest_source",
  "message",
  "model_pricing",
  "project",
  "recommendation",
  "schema_migrations",
  "session",
  "tool_call",
];
const BASELINE_TRIGGERS = ["block_ad", "block_ai", "block_au"];
const BASELINE_INDEX_COUNT = 22;

function inventory(db: Database, type: string): string[] {
  return (
    db
      .query(
        `SELECT name FROM sqlite_master
         WHERE type = ?1 AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
           AND NOT (name GLOB 'block_fts_*')
         ORDER BY name`,
      )
      .all(type) as { name: string }[]
  ).map((r) => r.name);
}

describe("openDb", () => {
  test("generated schema header agrees with LATEST_SCHEMA_VERSION", () => {
    const match = /^-- decant:schema_version=(\d+)$/m.exec(schemaSql);
    expect(match).not.toBeNull();
    expect(Number(match?.[1])).toBe(LATEST_SCHEMA_VERSION);
  });

  test("creates a fresh archive with Rust-parity connection pragmas", () => {
    const db = openDb(freshPath());
    expect((db.query("PRAGMA busy_timeout").get() as { timeout: number }).timeout).toBe(5000);
    expect((db.query("PRAGMA synchronous").get() as { synchronous: number }).synchronous).toBe(1);
    expect((db.query("PRAGMA journal_mode").get() as { journal_mode: string }).journal_mode).toBe(
      "wal",
    );
    expect((db.query("PRAGMA foreign_keys").get() as { foreign_keys: number }).foreign_keys).toBe(
      1,
    );
    db.close();
  });

  test("creates the exact baseline inventory of tables, triggers, and indexes", () => {
    const db = openDb(freshPath());
    expect(inventory(db, "table")).toEqual(BASELINE_TABLES);
    expect(inventory(db, "trigger")).toEqual(BASELINE_TRIGGERS);
    expect(inventory(db, "index")).toHaveLength(BASELINE_INDEX_COUNT);
    db.close();
  });

  test("records schema_migrations 1..LATEST_SCHEMA_VERSION like the Rust engine", () => {
    const db = openDb(freshPath());
    const versions = (
      db.query("SELECT version FROM schema_migrations ORDER BY version").all() as {
        version: number;
      }[]
    ).map((r) => r.version);
    expect(versions).toEqual(
      Array.from({ length: LATEST_SCHEMA_VERSION }, (_, index) => index + 1),
    );
    const nullTimestamps = db
      .query("SELECT COUNT(*) AS n FROM schema_migrations WHERE applied_at IS NULL")
      .get() as { n: number };
    expect(nullTimestamps.n).toBe(0);
    db.close();
  });

  test("is idempotent: reopening an existing archive changes nothing", () => {
    const path = freshPath();
    openDb(path).close();
    const db = openDb(path);
    const count = db.query("SELECT COUNT(*) AS n FROM schema_migrations").get() as { n: number };
    expect(count.n).toBe(LATEST_SCHEMA_VERSION);
    db.close();
  });

  test("keeps block_fts in sync through insert, update, and delete triggers", () => {
    const db = openDb(freshPath());
    db.exec(`
      INSERT INTO session (id, tool, source_session_id) VALUES (1, 'claude', 'abc');
      INSERT INTO message (id, session_id, seq, raw) VALUES (10, 1, 0, '{}');
      INSERT INTO block (id, message_id, session_id, ordinal, type, text)
        VALUES (100, 10, 1, 0, 'text', 'porting the rust daemon to typescript');
    `);

    const match = (q: string) =>
      db
        .query(
          `SELECT b.id, bm25(block_fts) AS rank, highlight(block_fts, 0, '[', ']') AS hl
           FROM block_fts JOIN block b ON b.id = block_fts.rowid
           WHERE block_fts MATCH ?1 ORDER BY rank`,
        )
        .all(q) as { id: number; rank: number; hl: string }[];

    expect(match("typescript")).toMatchObject([
      { id: 100, hl: "porting the rust daemon to [typescript]" },
    ]);

    db.exec("UPDATE block SET text = 'nothing to see here' WHERE id = 100");
    expect(match("typescript")).toHaveLength(0);
    expect(match("nothing")).toHaveLength(1);

    db.exec("DELETE FROM block WHERE id = 100");
    expect(match("nothing")).toHaveLength(0);
    db.close();
  });

  test("enforces foreign keys on the write path", () => {
    const db = openDb(freshPath());
    expect(() =>
      db.exec("INSERT INTO message (session_id, seq, raw) VALUES (999, 0, '{}')"),
    ).toThrow(/FOREIGN KEY/i);
    db.close();
  });

  test("rejects an archive from a newer schema than this build understands", () => {
    const path = freshPath();
    const db = openDb(path);
    db.exec(
      `INSERT INTO schema_migrations (version, applied_at)
       VALUES (${LATEST_SCHEMA_VERSION + 1}, datetime('now'))`,
    );
    db.close();
    expect(() => openDb(path)).toThrow(/newer/i);
  });

  test("rejects a pre-baseline archive and points at rebuilding it", () => {
    const path = freshPath();
    const db = openDb(path);
    db.exec(`DELETE FROM schema_migrations WHERE version = ${LATEST_SCHEMA_VERSION}`);
    db.close();
    expect(() => openDb(path)).toThrow(/rebuild/i);
  });
});
