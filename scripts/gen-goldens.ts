#!/usr/bin/env bun
// Regenerates test/golden/ from the Rust implementation — the parity oracle
// for the TypeScript port (see docs plan). Runs the Rust CLI over the
// synthetic fixtures/ transcripts into a throwaway archive, then snapshots:
//
//   rows/*.json  — normalized rows keyed by natural keys (no autoincrement
//                  ids), ordered deterministically
//   cli/*.json   — `--json` output of read commands
//   meta.json    — provenance (rust rev, fixture inventory)
//
// Absolute repo paths are canonicalized to <REPO> so goldens are
// machine-independent. Requires cargo. Usage: bun run scripts/gen-goldens.ts

import { Database } from "bun:sqlite";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  renameSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repoRoot = join(import.meta.dir, "..");
const goldenDir = join(repoRoot, "test", "golden");
const nextGoldenDir = join(repoRoot, "test", `.golden-next-${process.pid}-${Date.now()}`);
const backupGoldenDir = join(repoRoot, "test", `.golden-backup-${process.pid}-${Date.now()}`);
const workDir = mkdtempSync(join(tmpdir(), "decant-golden-"));
const dbPath = join(workDir, "golden.db");
const provenancePaths = [
  "Cargo.toml",
  "Cargo.lock",
  "crates",
  "fixtures",
  "scripts/gen-goldens.ts",
];

// Directory discovery only picks up files laid out like the real tools write
// them: any *.jsonl under the claude dir, but only `<codex_dir>/sessions/
// rollout-*.jsonl` for Codex (see ingest.rs discover/is_rollout). The
// fixtures are therefore staged into a temp source tree under those names;
// the Rust unit tests bypass discovery by parsing fixture files directly.
const stagedClaudeDir = join(workDir, "sources", "claude");
const stagedCodexDir = join(workDir, "sources", "codex");

const rustEnv = {
  ...process.env,
  DECANT_CLAUDE_DIR: stagedClaudeDir,
  DECANT_CODEX_DIR: stagedCodexDir,
};

function rust(args: string[], options: { allowExit3?: boolean } = {}): string {
  const proc = Bun.spawnSync(
    ["cargo", "run", "-q", "-p", "decant-cli", "--", "--db", dbPath, ...args],
    { cwd: repoRoot, env: rustEnv, stdout: "pipe", stderr: "pipe" },
  );
  if (options.allowExit3 && proc.exitCode === 3) {
    return proc.stdout.toString();
  }
  if (!proc.success) {
    throw new Error(
      `rust CLI failed: decant ${args.join(" ")} (exit ${proc.exitCode})\n${proc.stderr.toString()}`,
    );
  }
  return proc.stdout.toString();
}

function canonicalize(text: string): string {
  return text.replaceAll(repoRoot, "<REPO>").replaceAll(workDir, "<TMP>");
}

async function writeGolden(relPath: string, value: unknown): Promise<void> {
  const target = join(nextGoldenDir, relPath);
  await Bun.write(target, `${canonicalize(JSON.stringify(value, null, 2))}\n`);
  console.log(`wrote ${join("test/golden", relPath)}`);
}

function fixtureFiles(tool: string): string[] {
  return readdirSync(join(repoRoot, "fixtures", tool))
    .filter((file) => file.endsWith(".jsonl"))
    .sort();
}

function stageFixtures(): void {
  mkdirSync(stagedClaudeDir, { recursive: true });
  mkdirSync(join(stagedCodexDir, "sessions"), { recursive: true });
  for (const file of fixtureFiles("claude")) {
    copyFileSync(join(repoRoot, "fixtures", "claude", file), join(stagedClaudeDir, file));
  }
  for (const file of fixtureFiles("codex")) {
    copyFileSync(
      join(repoRoot, "fixtures", "codex", file),
      join(stagedCodexDir, "sessions", `rollout-${file}`),
    );
  }
}

function stripVolatileIds(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(stripVolatileIds);
  }
  if (value && typeof value === "object") {
    const entries = Object.entries(value)
      .filter(([key]) => key !== "id")
      .map(([key, child]) => [key, stripVolatileIds(child)] as const);
    return Object.fromEntries(entries);
  }
  return value;
}

function normalizeCliGolden(name: string, value: unknown): unknown {
  const normalized = stripVolatileIds(value);
  if (name !== "ls" || !Array.isArray(normalized)) {
    return normalized;
  }
  return [...normalized].sort((left, right) => {
    const a = left as Record<string, unknown>;
    const b = right as Record<string, unknown>;
    return (
      String(b.started_at ?? "").localeCompare(String(a.started_at ?? "")) ||
      String(a.tool ?? "").localeCompare(String(b.tool ?? "")) ||
      String(a.source_session_id ?? "").localeCompare(String(b.source_session_id ?? ""))
    );
  });
}

function replaceGoldenDir(): void {
  rmSync(backupGoldenDir, { recursive: true, force: true });
  let movedExisting = false;
  try {
    if (existsSync(goldenDir)) {
      renameSync(goldenDir, backupGoldenDir);
      movedExisting = true;
    }
    renameSync(nextGoldenDir, goldenDir);
    if (movedExisting) {
      rmSync(backupGoldenDir, { recursive: true, force: true });
    }
  } catch (error) {
    if (!existsSync(goldenDir) && movedExisting && existsSync(backupGoldenDir)) {
      renameSync(backupGoldenDir, goldenDir);
    }
    throw error;
  }
}

function gitOutput(args: string[]): string {
  const proc = Bun.spawnSync(["git", ...args], { cwd: repoRoot, stdout: "pipe", stderr: "pipe" });
  if (!proc.success) {
    throw new Error(`git ${args.join(" ")} failed: ${proc.stderr.toString()}`);
  }
  return proc.stdout.toString().trim();
}

function oracleInputsAreDirty(): boolean {
  return gitOutput(["status", "--porcelain", "--", ...provenancePaths]).length > 0;
}

function oracleInputsRevision(): string {
  return gitOutput(["log", "-1", "--format=%H", "--", ...provenancePaths]);
}

// Natural-key row dumps: no autoincrement ids or ingest bookkeeping, stable
// ordering, FKs expressed as (tool, source_session_id [, seq]) joins.
const ROW_QUERIES: Record<string, string> = {
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
};

// CLI read commands snapshotted as goldens (grows in later phases).
const CLI_COMMANDS: Record<string, string[]> = {
  ls: ["ls", "--json"],
  stats: ["stats", "--json"],
  "stats-by-model": ["stats", "--by", "model", "--json"],
};

async function main(): Promise<void> {
  stageFixtures();
  mkdirSync(join(nextGoldenDir, "rows"), { recursive: true });
  mkdirSync(join(nextGoldenDir, "cli"), { recursive: true });

  const syncOutput = rust(["sync"], { allowExit3: true });
  console.log(syncOutput.trim());

  const db = new Database(dbPath);
  for (const [name, sql] of Object.entries(ROW_QUERIES)) {
    await writeGolden(join("rows", `${name}.json`), db.query(sql).all());
  }
  const issueCount = (db.query("SELECT COUNT(*) AS n FROM ingest_issue").get() as { n: number }).n;
  db.close();
  if (issueCount > 0) {
    throw new Error(`fixtures produced ${issueCount} ingest issue(s); goldens would be unsound`);
  }

  for (const [name, args] of Object.entries(CLI_COMMANDS)) {
    await writeGolden(
      join("cli", `${name}.json`),
      normalizeCliGolden(name, JSON.parse(rust(args))),
    );
  }

  const rev = oracleInputsRevision();
  const dirty = oracleInputsAreDirty();
  const fixtures = ["claude", "codex"].flatMap((tool) =>
    fixtureFiles(tool).map((file) => `fixtures/${tool}/${file}`),
  );
  await writeGolden("meta.json", {
    generator: "scripts/gen-goldens.ts",
    source_of_truth: "rust implementation",
    rust_rev: dirty ? `${rev}-dirty` : rev,
    fixtures: fixtures.sort(),
    row_dumps: Object.keys(ROW_QUERIES).sort(),
    cli_commands: CLI_COMMANDS,
  });

  replaceGoldenDir();
}

try {
  await main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  rmSync(workDir, { recursive: true, force: true });
  rmSync(nextGoldenDir, { recursive: true, force: true });
  rmSync(backupGoldenDir, { recursive: true, force: true });
}
