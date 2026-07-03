import { Database } from "bun:sqlite";
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openDb } from "../src/db.ts";
import { upsertSession } from "../src/ingest.ts";
import { getSession, type ListFilter, listProjects, listSessions, search } from "../src/query.ts";
import { parseClaudeSession } from "../src/sources/claude.ts";

const workDir = mkdtempSync(join(tmpdir(), "decant-query-test-"));
afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let dbCounter = 0;
function freshDb(): Database {
  dbCounter += 1;
  return openDb(join(workDir, `query-${dbCounter}.db`));
}

function seeded(): Database {
  const db = freshDb();
  const content = readFileSync(
    join(import.meta.dir, "..", "fixtures", "claude", "sample.jsonl"),
    "utf8",
  );
  const parsed = parseClaudeSession("sess-claude-1", content);
  upsertSession(db, parsed, "/x.jsonl", 1, 2, "h");
  return db;
}

describe("query reads", () => {
  test("lists, gets, and searches sessions", () => {
    const db = seeded();

    const list = listSessions(db);
    expect(list).toHaveLength(1);
    expect(list[0]?.title).toBe("Fix the failing auth test");

    const detail = getSession(db, list[0]?.id ?? 0);
    expect(detail).not.toBeNull();
    expect(detail?.messages).toHaveLength(4);

    const hits = search(db, "auth", 10);
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]?.tool).toBe("claude_code");
    db.close();
  });

  test("listSessions filters by tool, offset, and uses the default limit", () => {
    const db = seeded();
    upsertSession(
      db,
      parseClaudeSession(
        "sess-claude-2",
        readFileSync(join(import.meta.dir, "..", "fixtures", "claude", "enriched.jsonl"), "utf8"),
      ),
      "/y.jsonl",
      1,
      2,
      "h2",
    );
    const claude = listSessions(db, { tool: "claude_code", limit: 10 });
    expect(claude).toHaveLength(2);

    const codex = listSessions(db, { tool: "codex", limit: 10 });
    expect(codex).toEqual([]);

    const defaulted = listSessions(db, { limit: 0 } satisfies ListFilter);
    expect(defaulted).toHaveLength(2);
    expect(listSessions(db, { limit: 1, offset: 1 })).toHaveLength(1);
    db.close();
  });

  test("getSession returns null for an unknown id", () => {
    const db = seeded();
    expect(getSession(db, 999_999)).toBeNull();
    db.close();
  });

  test("search with no match returns empty", () => {
    const db = seeded();
    expect(search(db, "zzznotpresentzzz", 10)).toEqual([]);
    db.close();
  });

  test("listProjects rolls up session counts and cost", () => {
    const db = seeded();
    const projects = listProjects(db);
    expect(projects).toHaveLength(1);
    expect(projects[0]?.sessions).toBe(1);
    expect(projects[0]?.path).toBe("/Users/dev/proj");
    expect(projects[0]?.estimated_cost_usd).toBeGreaterThan(0);
    db.close();
  });

  test("query functions propagate database errors", () => {
    const bare = new Database(":memory:");
    expect(() => search(bare, "x", 10)).toThrow();
    expect(() => getSession(bare, 1)).toThrow();
    expect(() => listProjects(bare)).toThrow();
    bare.close();
  });

  test("getSession propagates message query errors after summary lookup succeeds", () => {
    const db = seeded();
    const id = listSessions(db)[0]?.id ?? 0;
    db.exec("PRAGMA foreign_keys = OFF; DROP TABLE message;");
    expect(() => getSession(db, id)).toThrow();
    db.close();
  });
});
