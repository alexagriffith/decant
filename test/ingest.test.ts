import type { Database } from "bun:sqlite";
import { afterAll, describe, expect, test } from "bun:test";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { openDb } from "../src/db.ts";
import { discover, type IngestConfig, sync, upsertSession } from "../src/ingest.ts";
import { parseClaudeSession } from "../src/sources/claude.ts";

const repoRoot = join(import.meta.dir, "..");
const fixtureRoot = join(repoRoot, "fixtures");
const goldenDir = join(import.meta.dir, "golden");
const workDir = mkdtempSync(join(tmpdir(), "decant-ingest-test-"));

afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let caseCounter = 0;
function freshCase(): string {
  caseCounter += 1;
  const dir = join(workDir, `case-${caseCounter}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function openFreshDb(dir: string): Database {
  return openDb(join(dir, "archive.db"));
}

function fixture(tool: "claude" | "codex", name: string): string {
  return readFileSync(join(fixtureRoot, tool, name), "utf8");
}

function write(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

function stageFixtures(dir: string): IngestConfig {
  const claudeDir = join(dir, "sources", "claude");
  const codexDir = join(dir, "sources", "codex");
  mkdirSync(claudeDir, { recursive: true });
  mkdirSync(join(codexDir, "sessions"), { recursive: true });

  for (const name of ["distill.jsonl", "enriched.jsonl", "mcp.jsonl", "sample.jsonl"]) {
    copyFileSync(join(fixtureRoot, "claude", name), join(claudeDir, name));
  }
  for (const name of ["distill.jsonl", "enriched.jsonl", "sample.jsonl"]) {
    copyFileSync(join(fixtureRoot, "codex", name), join(codexDir, "sessions", `rollout-${name}`));
  }

  return { claudeDir, codexDir };
}

async function golden<T>(relPath: string): Promise<T> {
  return (await Bun.file(join(goldenDir, relPath)).json()) as T;
}

function rows(db: Database, sql: string): unknown[] {
  return db.query(sql).all();
}

function canonicalizeRows(value: unknown, dir: string): unknown {
  return JSON.parse(JSON.stringify(value).replaceAll(dir, "<TMP>")) as unknown;
}

const ROW_QUERIES = {
  sessions: `
    SELECT s.tool, s.source_session_id, p.path AS project_path, p.name AS project_name,
           s.title, s.cwd, s.git_branch, s.model, s.cli_version, s.started_at, s.ended_at,
           s.message_count, s.total_input_tokens, s.total_output_tokens,
           s.total_cache_read_tokens, s.total_cache_creation_tokens,
           s.total_reasoning_tokens, s.est_reasoning_tokens, s.reasoning_source,
           s.estimated_cost_usd, s.is_archived, s.source_path,
           s.turn_count, s.error_count, s.interruption_count, s.compaction_count,
           s.sidechain_message_count, s.agent_spawn_count, s.skill_count, s.command_count,
           s.thinking_block_count, s.thinking_chars, s.active_seconds, s.outcome, s.work_type
    FROM session s LEFT JOIN project p ON p.id = s.project_id
    ORDER BY s.tool, s.source_session_id`,
  messages: `
    SELECT s.tool, s.source_session_id, m.seq, m.source_uuid, m.parent_source_uuid,
           m.role, m.model, m.stop_reason, m.timestamp, m.input_tokens, m.output_tokens,
           m.cache_read_tokens, m.cache_creation_tokens, m.raw
    FROM message m JOIN session s ON s.id = m.session_id
    ORDER BY s.tool, s.source_session_id, m.seq`,
  blocks: `
    SELECT s.tool, s.source_session_id, m.seq, b.ordinal, b.type, b.text,
           b.tool_name, b.tool_use_id, b.tool_input, b.tool_result
    FROM block b JOIN message m ON m.id = b.message_id JOIN session s ON s.id = b.session_id
    ORDER BY s.tool, s.source_session_id, m.seq, b.ordinal`,
  tool_calls: `
    SELECT s.tool, s.source_session_id, m.seq, tc.ordinal, tc.tool_kind, tc.tool_name,
           tc.mcp_server, tc.tool_base_name, tc.tool_use_id, tc.input, tc.is_error,
           tc.output_preview, tc.output_bytes, tc.duration_ms, tc.timestamp
    FROM tool_call tc
    LEFT JOIN message m ON m.id = tc.message_id
    JOIN session s ON s.id = tc.session_id
    ORDER BY s.tool, s.source_session_id, m.seq, tc.ordinal, tc.tool_use_id`,
  file_refs: `
    SELECT s.tool, s.source_session_id, m.seq, f.path, f.rel_path, f.ext,
           f.operation, f.timestamp
    FROM file_ref f
    LEFT JOIN message m ON m.id = f.message_id
    JOIN session s ON s.id = f.session_id
    ORDER BY s.tool, s.source_session_id, m.seq, f.path, f.operation`,
  recommendations: `
    SELECT key, kind, category, title, detail, suggestion, prompt, url,
           link_label, icon, tone, score, status, status_source, note
    FROM recommendation
    ORDER BY key`,
} as const;

describe("upsertSession", () => {
  test("writes sessions, messages, blocks, tool calls, file refs, facets, and FTS rows", () => {
    const dir = freshCase();
    const db = openFreshDb(dir);
    const parsed = parseClaudeSession("enriched", fixture("claude", "enriched.jsonl"));
    const sessionId = upsertSession(db, parsed, "/x/enriched.jsonl", 1, 2, "hash");

    const counts = db
      .query(
        `SELECT
           (SELECT COUNT(*) FROM session) AS sessions,
           (SELECT COUNT(*) FROM message) AS messages,
           (SELECT COUNT(*) FROM block) AS blocks,
           (SELECT COUNT(*) FROM tool_call) AS calls,
           (SELECT COUNT(*) FROM file_ref) AS refs`,
      )
      .get() as { sessions: number; messages: number; blocks: number; calls: number; refs: number };
    expect(counts).toEqual({ sessions: 1, messages: 10, blocks: 15, calls: 6, refs: 4 });

    const ref = db
      .query(
        `SELECT f.rel_path, f.ext, f.operation, f.message_id IS NOT NULL AS linked
         FROM file_ref f WHERE f.session_id = ?1 AND f.operation = 'read'`,
      )
      .get(sessionId) as { rel_path: string; ext: string; operation: string; linked: number };
    expect(ref).toEqual({ rel_path: "src/main.rs", ext: "rs", operation: "read", linked: 1 });

    const session = db
      .query(
        `SELECT turn_count, error_count, interruption_count, compaction_count,
                sidechain_message_count, active_seconds, outcome, work_type
         FROM session WHERE id = ?1`,
      )
      .get(sessionId) as {
      turn_count: number;
      error_count: number;
      interruption_count: number;
      compaction_count: number;
      sidechain_message_count: number;
      active_seconds: number;
      outcome: string;
      work_type: string;
    };
    expect(session).toMatchObject({
      turn_count: 1,
      error_count: 1,
      interruption_count: 1,
      compaction_count: 1,
      sidechain_message_count: 2,
      active_seconds: 490,
      outcome: "abandoned",
      work_type: "refactor",
    });

    const fts = db
      .query("SELECT COUNT(*) AS n FROM block_fts WHERE block_fts MATCH 'auth'")
      .get() as { n: number };
    expect(fts.n).toBeGreaterThan(0);
    db.close();
  });

  test("replaces an existing natural session without duplicating children", () => {
    const dir = freshCase();
    const db = openFreshDb(dir);
    const parsed = parseClaudeSession("sample", fixture("claude", "sample.jsonl"));

    upsertSession(db, parsed, "/x/sample.jsonl", 1, 2, "a");
    upsertSession(db, parsed, "/x/sample-again.jsonl", 3, 4, "b");

    const counts = db
      .query(
        `SELECT
           (SELECT COUNT(*) FROM session) AS sessions,
           (SELECT COUNT(*) FROM message) AS messages,
           (SELECT COUNT(*) FROM block) AS blocks,
           (SELECT source_path FROM session) AS source_path`,
      )
      .get() as { sessions: number; messages: number; blocks: number; source_path: string };
    expect(counts).toMatchObject({
      sessions: 1,
      messages: 4,
      blocks: 6,
      source_path: "/x/sample-again.jsonl",
    });
    db.close();
  });
});

describe("sync", () => {
  test("discovers Claude files plus Codex rollout files only", () => {
    const dir = freshCase();
    const config: IngestConfig = {
      claudeDir: join(dir, "claude"),
      codexDir: join(dir, "codex"),
    };
    write(join(config.claudeDir, "project", "a.jsonl"), "");
    write(join(config.claudeDir, "project", "notes.txt"), "");
    write(join(config.codexDir, "sessions", "rollout-a.jsonl"), "");
    write(join(config.codexDir, "sessions", "a.jsonl"), "");
    write(join(config.codexDir, "archived_sessions", "rollout-b.jsonl"), "");

    expect(
      discover(config).map((file) => ({
        tool: file.tool,
        name: basename(file.path),
        archived: file.archived,
      })),
    ).toEqual([
      { tool: "claude_code", name: "a.jsonl", archived: false },
      { tool: "codex", name: "rollout-a.jsonl", archived: false },
      { tool: "codex", name: "rollout-b.jsonl", archived: true },
    ]);
  });

  test("is idempotent, records parse issues, and refreshes issues on reingest", () => {
    const dir = freshCase();
    const config: IngestConfig = {
      claudeDir: join(dir, "claude"),
      codexDir: join(dir, "codex"),
    };
    write(
      join(config.claudeDir, "proj", "sess.jsonl"),
      `${fixture("claude", "sample.jsonl")}\n{bad`,
    );
    const db = openFreshDb(dir);

    const first = sync(db, config);
    expect(first).toMatchObject({ scanned: 1, ingested: 1, skipped: 0, issues: 1, failed: 0 });
    expect((db.query("SELECT COUNT(*) AS n FROM ingest_issue").get() as { n: number }).n).toBe(1);
    expect(
      (db.query("SELECT COUNT(*) AS n FROM model_pricing").get() as { n: number }).n,
    ).toBeGreaterThan(0);

    const second = sync(db, config);
    expect(second).toMatchObject({ scanned: 1, ingested: 0, skipped: 1, issues: 0, failed: 0 });
    expect((db.query("SELECT COUNT(*) AS n FROM session").get() as { n: number }).n).toBe(1);

    write(
      join(config.claudeDir, "proj", "sess.jsonl"),
      `${fixture("claude", "sample.jsonl")}\nanother bad line {`,
    );
    const third = sync(db, config);
    expect(third).toMatchObject({ scanned: 1, ingested: 1, skipped: 0, issues: 1, failed: 0 });
    expect((db.query("SELECT COUNT(*) AS n FROM ingest_issue").get() as { n: number }).n).toBe(1);
    db.close();
  });

  test("reads Codex session_index titles and resolves project roots", () => {
    const dir = freshCase();
    const config: IngestConfig = {
      claudeDir: join(dir, "claude"),
      codexDir: join(dir, "codex"),
    };
    write(join(config.codexDir, "sessions", "rollout-x.jsonl"), fixture("codex", "sample.jsonl"));
    write(
      join(config.codexDir, "session_index.jsonl"),
      '{"id":"sess-codex-1","thread_name":"Indexed Title"}\nnot json\n{"id":"other"}\n',
    );
    const db = openFreshDb(dir);

    expect(sync(db, config).ingested).toBe(1);
    const row = db
      .query(
        `SELECT s.title, p.root_path, p.root_source
         FROM session s JOIN project p ON p.id = s.project_id`,
      )
      .get() as { title: string; root_path: string; root_source: string };
    expect(row).toEqual({
      title: "Indexed Title",
      root_path: "/Users/dev/proj",
      root_source: "self",
    });
    db.close();
  });

  test("fixture sync matches frozen natural-key golden rows", async () => {
    const dir = freshCase();
    const config = stageFixtures(dir);
    const db = openFreshDb(dir);

    const report = sync(db, config);
    expect(report).toMatchObject({ scanned: 7, ingested: 7, skipped: 0, issues: 0, failed: 0 });

    for (const [name, sql] of Object.entries(ROW_QUERIES)) {
      expect(canonicalizeRows(rows(db, sql), dir), name).toEqual(await golden(`rows/${name}.json`));
    }
    db.close();
  });
});
