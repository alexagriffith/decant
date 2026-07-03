#!/usr/bin/env bun
// Regenerates src/schema.sql from the Rust implementation, which stays the
// source of truth for the schema until cutover (Phase 6). Requires cargo.
//
// Usage: bun run scripts/gen-schema.ts

import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repoRoot = join(import.meta.dir, "..");
const workDir = mkdtempSync(join(tmpdir(), "decant-schema-"));
const dbPath = join(workDir, "baseline.db");

async function main(): Promise<void> {
  const proc = Bun.spawnSync(
    ["cargo", "run", "-q", "-p", "decant-cli", "--", "--db", dbPath, "db", "migrate"],
    { cwd: repoRoot, stdout: "inherit", stderr: "inherit" },
  );
  if (!proc.success) {
    throw new Error("cargo run db migrate failed; is the Rust toolchain installed?");
  }

  // Not readonly: a WAL-mode database needs writable -shm/-wal sidecars even to read.
  const db = new Database(dbPath);
  // Raw DDL in creation order. Excludes SQLite internals and the FTS5 shadow
  // tables (block_fts_data/idx/docsize/config), which the virtual-table
  // statement recreates on its own.
  const rows = db
    .query(
      `SELECT sql FROM sqlite_master
       WHERE sql IS NOT NULL
         AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         AND NOT (name GLOB 'block_fts_*')
       ORDER BY rowid`,
    )
    .all() as { sql: string }[];
  const version = (
    db.query("SELECT MAX(version) AS v FROM schema_migrations").get() as { v: number }
  ).v;
  db.close();

  const header = [
    `-- decant:schema_version=${version}`,
    `-- Effective decant schema (migrations 1..${version} applied), generated from`,
    "-- the Rust implementation by scripts/gen-schema.ts. Do not edit by hand.",
    "",
  ].join("\n");
  const body = rows.map((r) => `${r.sql};`).join("\n");
  await Bun.write(join(repoRoot, "src", "schema.sql"), `${header}${body}\n`);
  console.log(`src/schema.sql regenerated: ${rows.length} statements, schema version ${version}.`);
}

try {
  await main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  rmSync(workDir, { recursive: true, force: true });
}
