import type { Database } from "bun:sqlite";
import { createHash } from "node:crypto";
import type { Stats } from "node:fs";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, basename as pathBasename } from "node:path";
import { outcome, workType } from "./classify.ts";
import { defaultPricing, estimateCost } from "./cost.ts";
import { facets, fileRefs } from "./enrich.ts";
import { canonicalJson } from "./json.ts";
import type { Json, NormalizedBlock, ParsedSession, Tool } from "./model.ts";
import { parseClaudeSession } from "./sources/claude.ts";
import { parseCodexSession } from "./sources/codex.ts";
import { classifyTool } from "./tools.ts";
import { resolveWorktreeRoots } from "./worktree.ts";

export interface IngestConfig {
  claudeDir: string;
  codexDir: string;
}

export interface SyncReport {
  scanned: number;
  ingested: number;
  skipped: number;
  issues: number;
  failed: number;
  cancelled: boolean;
}

export interface SourceFile {
  tool: Tool;
  path: string;
  archived: boolean;
}

interface Prepared {
  file: SourceFile;
  lineCount: number;
  mtime: number;
  size: number;
  hash: string;
}

interface ToolUseBlock {
  messageId: number;
  callBlockId: number;
  timestamp: string | null;
  block: NormalizedBlock;
}

export function discover(config: IngestConfig): SourceFile[] {
  const out: SourceFile[] = [];
  collect(config.claudeDir, "claude_code", false, (name) => name.endsWith(".jsonl"), out);
  collect(
    join(config.codexDir, "sessions"),
    "codex",
    false,
    (name) => name.startsWith("rollout-") && name.endsWith(".jsonl"),
    out,
  );
  collect(
    join(config.codexDir, "archived_sessions"),
    "codex",
    true,
    (name) => name.startsWith("rollout-") && name.endsWith(".jsonl"),
    out,
  );
  return out;
}

export function sync(
  db: Database,
  config: IngestConfig,
  cancel?: { aborted: boolean },
): SyncReport {
  seedModelPricing(db);
  const files = discover(config);
  const titles = codexTitles(config);
  const report: SyncReport = {
    scanned: files.length,
    ingested: 0,
    skipped: 0,
    issues: 0,
    failed: 0,
    cancelled: false,
  };

  for (const file of files) {
    if (cancel?.aborted === true) {
      report.cancelled = true;
      break;
    }

    let stats: Stats;
    try {
      stats = statSync(file.path);
    } catch {
      continue;
    }
    const size = stats.size;
    const mtime = mtimeSecs(stats);
    const prior = db
      .query("SELECT size, mtime FROM ingest_source WHERE path = ?1")
      .get(file.path) as { size: number; mtime: number } | null;
    if (prior != null && prior.size === size && prior.mtime === mtime) {
      report.skipped += 1;
      continue;
    }

    let content: string;
    try {
      content = readFileSync(file.path, "utf8");
      stats = statSync(file.path);
    } catch {
      report.failed += 1;
      continue;
    }

    const stem = fileStem(file.path);
    const parsed =
      file.tool === "claude_code"
        ? parseClaudeSession(stem, content)
        : parseCodexSession(stem, content, titles);
    parsed.session.isArchived = file.archived;

    const prepared: Prepared = {
      file,
      lineCount: lineCount(content),
      mtime: mtimeSecs(stats),
      size: stats.size,
      hash: hashContent(content),
    };

    writeIngestedFile(db, prepared, parsed);
    report.ingested += 1;
    report.issues += parsed.issues.length;
  }

  if (report.ingested > 0) {
    resolveWorktreeRoots(db);
  }

  return report;
}

export function upsertSession(
  db: Database,
  parsed: ParsedSession,
  sourcePath: string,
  mtime: number,
  size: number,
  hash: string,
): number {
  const write = db.transaction(() => writeSession(db, parsed, sourcePath, mtime, size, hash));
  return write();
}

export function seedModelPricing(db: Database): void {
  const insert = db.prepare(
    `INSERT INTO model_pricing(
       model, input_per_mtok, output_per_mtok, cache_read_per_mtok,
       cache_write_per_mtok, source, updated_at
     )
     VALUES (?1, ?2, ?3, ?4, ?5, 'seed', datetime('now'))
     ON CONFLICT(model) DO NOTHING`,
  );
  const apply = db.transaction((entries: ReturnType<typeof defaultPricing>) => {
    for (const [model, price] of entries) {
      insert.run(
        model,
        price.inputPerMtok,
        price.outputPerMtok,
        price.cacheReadPerMtok,
        price.cacheWritePerMtok,
      );
    }
  });
  apply(defaultPricing());
}

function writeIngestedFile(db: Database, prepared: Prepared, parsed: ParsedSession): void {
  const write = db.transaction(() => {
    db.query("UPDATE ingest_source SET session_id = NULL WHERE path = ?1").run(prepared.file.path);
    const sessionId = writeSession(
      db,
      parsed,
      prepared.file.path,
      prepared.mtime,
      prepared.size,
      prepared.hash,
    );
    db.query("DELETE FROM ingest_issue WHERE source_path = ?1").run(prepared.file.path);
    const insertIssue = db.prepare(
      `INSERT INTO ingest_issue(source_path, line_no, error, raw_line, created_at)
       VALUES (?1, ?2, ?3, ?4, datetime('now'))`,
    );
    for (const issue of parsed.issues) {
      insertIssue.run(prepared.file.path, issue.lineNo, issue.error, issue.rawLine);
    }
    const status = parsed.issues.length === 0 ? "ok" : "ok_with_issues";
    db.query(
      `INSERT INTO ingest_source(
         path, tool, size, mtime, hash, session_id, line_count, status, last_ingested_at
       )
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))
       ON CONFLICT(path) DO UPDATE SET
         size = ?3, mtime = ?4, hash = ?5, session_id = ?6,
         line_count = ?7, status = ?8, last_ingested_at = datetime('now')`,
    ).run(
      prepared.file.path,
      prepared.file.tool,
      prepared.size,
      prepared.mtime,
      prepared.hash,
      sessionId,
      prepared.lineCount,
      status,
    );
  });
  write();
}

function writeSession(
  db: Database,
  parsed: ParsedSession,
  sourcePath: string,
  mtime: number,
  size: number,
  hash: string,
): number {
  const s = parsed.session;
  let projectId: number | null = null;
  if (s.projectPath != null) {
    db.query(
      `INSERT INTO project(path, name, first_seen_at, last_seen_at)
       VALUES (?1, ?2, datetime('now'), datetime('now'))
       ON CONFLICT(path) DO UPDATE SET last_seen_at = datetime('now')`,
    ).run(s.projectPath, basename(s.projectPath));
    projectId = (
      db.query("SELECT id FROM project WHERE path = ?1").get(s.projectPath) as { id: number }
    ).id;
  }

  db.query("DELETE FROM session WHERE tool = ?1 AND source_session_id = ?2").run(
    s.tool,
    s.sourceSessionId,
  );

  const refs = fileRefs(s);
  const gotFacets = facets(s);
  const gotOutcome = outcome(s);
  const gotWorkType = workType(s, refs);
  const cost = estimateCost(s.model, s.totals, defaultPricing());

  db.query(
    `INSERT INTO session(
       tool, source_session_id, project_id, title, cwd, git_branch, model, cli_version,
       started_at, ended_at, message_count,
       total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_creation_tokens,
       total_reasoning_tokens,
       estimated_cost_usd, is_archived, source_path, raw_meta,
       ingested_at, source_mtime, source_size, source_hash,
       turn_count, error_count, interruption_count, compaction_count, sidechain_message_count,
       agent_spawn_count, skill_count, command_count, thinking_block_count, thinking_chars,
       active_seconds, outcome, work_type,
       est_reasoning_tokens, reasoning_source
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
             ?19, ?20, datetime('now'), ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31,
             ?32, ?33, ?34, ?35, ?36, ?37, ?38)`,
  ).run(
    s.tool,
    s.sourceSessionId,
    projectId,
    s.title,
    s.cwd,
    s.gitBranch,
    s.model,
    s.cliVersion,
    s.startedAt,
    s.endedAt,
    s.messages.length,
    s.totals.input,
    s.totals.output,
    s.totals.cacheRead,
    s.totals.cacheCreation,
    s.totals.reasoning,
    cost,
    Number(s.isArchived),
    sourcePath,
    canonicalJson(s.rawMeta),
    mtime,
    size,
    hash,
    gotFacets.turnCount,
    gotFacets.errorCount,
    gotFacets.interruptionCount,
    gotFacets.compactionCount,
    gotFacets.sidechainMessageCount,
    gotFacets.agentSpawnCount,
    gotFacets.skillCount,
    gotFacets.commandCount,
    gotFacets.thinkingBlockCount,
    gotFacets.thinkingChars,
    gotFacets.activeSeconds,
    gotOutcome,
    gotWorkType,
    s.estReasoningTokens,
    s.reasoningSource,
  );
  const sessionId = lastInsertRowid(db);

  const results = new Map<string, number>();
  const resultErrors = new Map<string, boolean | null>();
  const toolUseBlocks: ToolUseBlock[] = [];
  const messageIds: number[] = [];

  const insertMessage = db.prepare(
    `INSERT INTO message(
       session_id, seq, source_uuid, parent_source_uuid, role, model, stop_reason,
       timestamp, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, raw
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)`,
  );
  const insertBlock = db.prepare(
    `INSERT INTO block(
       message_id, session_id, ordinal, type, text, tool_name, tool_use_id, tool_input, tool_result
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)`,
  );

  for (const message of s.messages) {
    insertMessage.run(
      sessionId,
      message.seq,
      message.sourceUuid,
      message.parentSourceUuid,
      message.role,
      message.model,
      message.stopReason,
      message.timestamp,
      message.usage?.input ?? null,
      message.usage?.output ?? null,
      message.usage?.cacheRead ?? null,
      message.usage?.cacheCreation ?? null,
      canonicalJson(message.raw),
    );
    const messageId = lastInsertRowid(db);
    messageIds.push(messageId);

    for (const block of message.blocks) {
      insertBlock.run(
        messageId,
        sessionId,
        block.ordinal,
        block.blockType,
        block.text,
        block.toolName,
        block.toolUseId,
        block.toolInput === undefined ? null : canonicalJson(block.toolInput),
        block.toolResult,
      );
      const blockId = lastInsertRowid(db);
      if (block.blockType === "tool_use") {
        toolUseBlocks.push({
          messageId,
          callBlockId: blockId,
          timestamp: message.timestamp,
          block,
        });
      } else if (block.blockType === "tool_result" && block.toolUseId != null) {
        results.set(block.toolUseId, blockId);
        resultErrors.set(block.toolUseId, block.isError);
      }
    }
  }

  const insertToolCall = db.prepare(
    `INSERT INTO tool_call(
       session_id, message_id, call_block_id, result_block_id, tool_kind, tool_name,
       mcp_server, tool_base_name, tool_use_id, input, is_error, ordinal, timestamp
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)`,
  );
  for (const call of toolUseBlocks) {
    const name = call.block.toolName ?? "";
    const classified = classifyTool(name);
    const resultBlockId =
      call.block.toolUseId == null ? null : (results.get(call.block.toolUseId) ?? null);
    const isError =
      call.block.toolUseId == null || !resultErrors.has(call.block.toolUseId)
        ? null
        : resultErrors.get(call.block.toolUseId);
    insertToolCall.run(
      sessionId,
      call.messageId,
      call.callBlockId,
      resultBlockId,
      classified.kind,
      name,
      classified.mcpServer,
      classified.baseName,
      call.block.toolUseId,
      call.block.toolInput === undefined ? null : canonicalJson(call.block.toolInput),
      isError == null ? null : Number(isError),
      call.block.ordinal,
      call.timestamp,
    );
  }

  const insertFileRef = db.prepare(
    `INSERT INTO file_ref(session_id, message_id, path, rel_path, ext, operation, timestamp)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)`,
  );
  for (const ref of refs) {
    insertFileRef.run(
      sessionId,
      messageIds[ref.messageIdx] ?? null,
      ref.path,
      ref.relPath,
      ref.ext,
      ref.operation,
      ref.timestamp,
    );
  }

  return sessionId;
}

function collect(
  root: string,
  tool: Tool,
  archived: boolean,
  want: (name: string) => boolean,
  out: SourceFile[],
): void {
  if (!existsSync(root)) {
    return;
  }
  for (const path of walk(root)) {
    const name = pathBasename(path);
    if (want(name)) {
      out.push({ tool, path, archived });
    }
  }
}

function walk(root: string): string[] {
  const entries = readdirSync(root, { withFileTypes: true }).sort((left, right) =>
    left.name.localeCompare(right.name),
  );
  const out: string[] = [];
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(path));
    } else if (entry.isFile()) {
      out.push(path);
    }
  }
  return out;
}

function codexTitles(config: IngestConfig): Map<string, string> {
  const titles = new Map<string, string>();
  const path = join(config.codexDir, "session_index.jsonl");
  let content: string;
  try {
    content = readFileSync(path, "utf8");
  } catch {
    return titles;
  }
  for (const line of content.split(/\n/)) {
    if (line.trim() === "") {
      continue;
    }
    try {
      const value = JSON.parse(line) as Json;
      const id = asString(get(value, "id"));
      const threadName = asString(get(value, "thread_name"));
      if (id != null && threadName != null) {
        titles.set(id, threadName);
      }
    } catch {
      // Corrupt session-index rows should not make session ingest fail.
    }
  }
  return titles;
}

function basename(path: string): string {
  return path.replace(/\/+$/, "").split("/").filter(Boolean).at(-1) ?? path;
}

function fileStem(path: string): string {
  const base = pathBasename(path);
  const ext = extname(base);
  return ext === "" ? base : base.slice(0, -ext.length);
}

function lineCount(content: string): number {
  if (content === "") {
    return 0;
  }
  const trimmed = content.endsWith("\n") ? content.slice(0, -1) : content;
  return trimmed.split(/\r?\n/).length;
}

function mtimeSecs(stats: Stats): number {
  return Math.trunc(stats.mtimeMs / 1000);
}

function hashContent(content: string): string {
  return createHash("sha256").update(content).digest("hex");
}

function lastInsertRowid(db: Database): number {
  return Number((db.query("SELECT last_insert_rowid() AS id").get() as { id: number }).id);
}

function asString(value: Json | undefined): string | null {
  return typeof value === "string" ? value : null;
}

function get(value: Json | undefined, key: string): Json | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value[key] as Json | undefined)
    : undefined;
}
