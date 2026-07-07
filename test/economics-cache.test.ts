import { Database } from "bun:sqlite";
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openDb } from "../src/db.ts";
import { EconomicsCache } from "../src/economics-cache.ts";
import { upsertSession } from "../src/ingest.ts";
import { handleRequest } from "../src/server.ts";
import { parseClaudeSession } from "../src/sources/claude.ts";
import { parseCodexSession } from "../src/sources/codex.ts";
import { computeSessionEconomicsVectors, tokenEconomics } from "../src/token-economics.ts";

const workDir = mkdtempSync(join(tmpdir(), "decant-economics-cache-test-"));
afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let dbCounter = 0;
function seededDbPath(): string {
  dbCounter += 1;
  const path = join(workDir, `economics-${dbCounter}.db`);
  const db = openDb(path);
  upsertSession(
    db,
    parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
    "/x/claude.jsonl",
    1,
    2,
    "claude",
  );
  db.close();
  return path;
}

function fixture(tool: "claude" | "codex", name: string): string {
  return readFileSync(join(import.meta.dir, "..", "fixtures", tool, name), "utf8");
}

describe("economics cache", () => {
  test("serves tokenEconomics-identical answers from one computation", async () => {
    const dbPath = seededDbPath();
    const db = openDb(dbPath);
    let computeCalls = 0;
    const cache = new EconomicsCache({
      dbPath,
      db,
      computeVectors: (path) => {
        computeCalls += 1;
        const worker = new Database(path, { readonly: true, strict: true });
        try {
          return Promise.resolve(computeSessionEconomicsVectors(worker));
        } finally {
          worker.close();
        }
      },
    });

    expect(await cache.get()).toEqual(tokenEconomics(db));
    expect(await cache.get({ from: "2026-05-04", to: "2026-05-04" })).toEqual(
      tokenEconomics(db, { from: "2026-05-04", to: "2026-05-04" }),
    );
    expect(await cache.get({ from: "2030-01-01", to: null })).toEqual(
      tokenEconomics(db, { from: "2030-01-01" }),
    );
    expect(computeCalls).toBe(1);
    cache.dispose();
    db.close();
  });

  test("detects external writes, serves stale, then rebuilds and notifies", async () => {
    const dbPath = seededDbPath();
    const db = openDb(dbPath);
    let computeCalls = 0;
    let rebuilt = 0;
    const cache = new EconomicsCache({
      dbPath,
      db,
      onRebuilt: () => {
        rebuilt += 1;
      },
      computeVectors: (path) => {
        computeCalls += 1;
        const worker = new Database(path, { readonly: true, strict: true });
        try {
          return Promise.resolve(computeSessionEconomicsVectors(worker));
        } finally {
          worker.close();
        }
      },
    });

    const first = await cache.get();
    expect(computeCalls).toBe(1);
    expect(rebuilt).toBe(0);

    // Simulate a sync worker: another connection ingests a session.
    const writer = openDb(dbPath);
    upsertSession(
      writer,
      parseCodexSession("sess-enr-codex", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/codex.jsonl",
      1,
      2,
      "codex",
    );
    writer.close();
    // The main connection has to touch the database once before data_version
    // reflects the external commit.
    db.query("SELECT COUNT(*) FROM session").get();

    const stale = await cache.get();
    expect(stale).toEqual(first);
    await cache.settled();
    expect(computeCalls).toBe(2);
    expect(rebuilt).toBe(1);
    expect(await cache.get()).toEqual(tokenEconomics(db));
    cache.dispose();
    db.close();
  });

  test("invalidate() kicks a rebuild without a request", async () => {
    const dbPath = seededDbPath();
    const db = openDb(dbPath);
    let computeCalls = 0;
    const cache = new EconomicsCache({
      dbPath,
      db,
      computeVectors: (path) => {
        computeCalls += 1;
        const worker = new Database(path, { readonly: true, strict: true });
        try {
          return Promise.resolve(computeSessionEconomicsVectors(worker));
        } finally {
          worker.close();
        }
      },
    });
    cache.invalidate();
    await cache.settled();
    expect(computeCalls).toBe(1);
    expect(await cache.get()).toEqual(tokenEconomics(db));
    expect(computeCalls).toBe(1);
    cache.dispose();
    db.close();
  });

  test("stats worker computes the same vectors as an in-process run", async () => {
    const dbPath = seededDbPath();
    const cache = new EconomicsCache({ dbPath, db: openDb(dbPath) });
    const viaWorker = await cache.get();
    const direct = openDb(dbPath);
    expect(viaWorker).toEqual(tokenEconomics(direct));
    direct.close();
    cache.dispose();
  });

  test("token-economics route answers from the cache when provided", async () => {
    const dbPath = seededDbPath();
    const db = openDb(dbPath);
    const config = { dbPath, claudeDir: join(workDir, "none"), codexDir: join(workDir, "none") };
    const cache = new EconomicsCache({
      dbPath,
      db,
      computeVectors: (path) => {
        const worker = new Database(path, { readonly: true, strict: true });
        try {
          return Promise.resolve(computeSessionEconomicsVectors(worker));
        } finally {
          worker.close();
        }
      },
    });
    const response = await handleRequest(
      new Request("http://127.0.0.1:3000/api/analytics/token-economics"),
      config,
      { db, economics: cache },
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(JSON.parse(JSON.stringify(tokenEconomics(db))));
    cache.dispose();
    db.close();
  });
});
