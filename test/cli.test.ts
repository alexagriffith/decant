import { afterAll, describe, expect, test } from "bun:test";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCli } from "../src/cli.ts";

const workDir = mkdtempSync(join(tmpdir(), "decant-cli-test-"));
afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let caseCounter = 0;
function freshCase(): { dbPath: string; claudeDir: string; codexDir: string } {
  caseCounter += 1;
  const dir = join(workDir, `case-${caseCounter}`);
  const claudeDir = join(dir, "sources", "claude");
  const codexDir = join(dir, "sources", "codex");
  mkdirSync(claudeDir, { recursive: true });
  mkdirSync(join(codexDir, "sessions"), { recursive: true });
  for (const name of ["distill.jsonl", "enriched.jsonl", "mcp.jsonl", "sample.jsonl"]) {
    copyFileSync(join(import.meta.dir, "..", "fixtures", "claude", name), join(claudeDir, name));
  }
  for (const name of ["distill.jsonl", "enriched.jsonl", "sample.jsonl"]) {
    copyFileSync(
      join(import.meta.dir, "..", "fixtures", "codex", name),
      join(codexDir, "sessions", `rollout-${name}`),
    );
  }
  return { dbPath: join(dir, "archive.db"), claudeDir, codexDir };
}

async function syncedCase(): Promise<{ dbPath: string }> {
  const fixtureCase = freshCase();
  const result = await runCli([
    "--db",
    fixtureCase.dbPath,
    "--json",
    "sync",
    "--claude-dir",
    fixtureCase.claudeDir,
    "--codex-dir",
    fixtureCase.codexDir,
  ]);
  expect(result).toMatchObject({ code: 0, stderr: "" });
  expect(JSON.parse(result.stdout)).toMatchObject({ scanned: 7, ingested: 7, issues: 0 });
  return { dbPath: fixtureCase.dbPath };
}

describe("runCli", () => {
  test("sync then list, project, db, stats, search, files, tool, and export", async () => {
    const { dbPath } = await syncedCase();
    const base = ["--db", dbPath, "--json", "--no-sync"];

    const list = await runCli([...base, "ls"]);
    expect(list.code).toBe(0);
    const sessions = JSON.parse(list.stdout) as { id: number; source_session_id: string }[];
    expect(sessions).toHaveLength(7);
    expect(sessions[0]?.source_session_id).toBe("sess-codex-distill");

    const projects = await runCli([...base, "project", "ls"]);
    expect(projects.code).toBe(0);
    expect((JSON.parse(projects.stdout) as { path: string; sessions: number }[])[0]).toMatchObject({
      path: "/Users/dev/proj",
      sessions: 7,
    });

    const dbInfo = await runCli([...base, "db", "info"]);
    expect(dbInfo.code).toBe(0);
    expect(JSON.parse(dbInfo.stdout)).toMatchObject({
      path: dbPath,
      schema_version: 8,
      sessions: 7,
    });

    const migrated = await runCli(["--db", dbPath, "--no-sync", "db", "migrate"]);
    expect(migrated).toMatchObject({ code: 0, stdout: "" });
    expect(migrated.stderr).toContain(`schema up to date at ${dbPath}`);

    const vacuumed = await runCli(["--db", dbPath, "--no-sync", "db", "vacuum"]);
    expect(vacuumed).toMatchObject({ code: 0, stdout: "" });
    expect(vacuumed.stderr).toContain(`vacuumed ${dbPath}`);

    const stats = await runCli([...base, "stats", "--by", "model"]);
    expect(stats.code).toBe(0);
    expect(JSON.parse(stats.stdout)).toMatchObject([
      { key: "claude-opus-4-7", sessions: 4 },
      { key: "gpt-5.4", sessions: 3 },
    ]);

    const search = await runCli([...base, "search", "auth", "--limit", "5"]);
    expect(search.code).toBe(0);
    expect(JSON.parse(search.stdout).length).toBeGreaterThan(0);

    const files = await runCli([...base, "files", "--group", "ext"]);
    expect(files.code).toBe(0);
    expect((JSON.parse(files.stdout) as { key: string }[]).some((row) => row.key === "rs")).toBe(
      true,
    );

    const tools = await runCli([...base, "tool", "stats", "--limit", "3"]);
    expect(tools.code).toBe(0);
    expect((JSON.parse(tools.stdout) as { tool_name: string }[])[0]?.tool_name).toBe("Bash");

    const exported = await runCli(["--db", dbPath, "--no-sync", "export", String(sessions[0]?.id)]);
    expect(exported.code).toBe(0);
    expect(exported.stdout).toStartWith("# Build and patch");
  });

  test("invalid options return code 2 with an error", async () => {
    const { dbPath } = await syncedCase();
    const result = await runCli(["--db", dbPath, "--no-sync", "stats", "--by", "nope"]);
    expect(result.code).toBe(2);
    expect(result.stderr).toContain("unknown --by value");
  });
});
