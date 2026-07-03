import type { Database } from "bun:sqlite";
import type { Operation } from "./enrich.ts";

export interface Totals {
  sessions: number;
  messages: number;
  tool_calls: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  reasoning_tokens: number;
  est_reasoning_tokens: number;
  estimated_cost_usd: number;
}

export function totals(db: Database): Totals {
  return db
    .query(
      `SELECT
         (SELECT COUNT(*) FROM session) AS sessions,
         (SELECT COUNT(*) FROM message) AS messages,
         (SELECT COUNT(*) FROM tool_call) AS tool_calls,
         (SELECT COALESCE(SUM(total_input_tokens), 0) FROM session) AS input_tokens,
         (SELECT COALESCE(SUM(total_output_tokens), 0) FROM session) AS output_tokens,
         (SELECT COALESCE(SUM(total_cache_read_tokens), 0) FROM session) AS cache_read_tokens,
         (SELECT COALESCE(SUM(total_cache_creation_tokens), 0) FROM session) AS cache_creation_tokens,
         (SELECT COALESCE(SUM(total_reasoning_tokens), 0) FROM session) AS reasoning_tokens,
         (SELECT COALESCE(SUM(est_reasoning_tokens), 0) FROM session) AS est_reasoning_tokens,
         (SELECT COALESCE(SUM(estimated_cost_usd), 0.0) FROM session) AS estimated_cost_usd`,
    )
    .get() as Totals;
}

export type Dimension = "tool" | "model" | "project" | "day";

export function parseDimension(value: string): Dimension | null {
  return value === "tool" || value === "model" || value === "project" || value === "day"
    ? value
    : null;
}

export interface DimRow {
  key: string;
  sessions: number;
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  est_reasoning_tokens: number;
  estimated_cost_usd: number;
}

interface DimRowDb extends Omit<DimRow, "key"> {
  key: string | null;
}

export function byDimension(db: Database, dimension: Dimension): DimRow[] {
  const { groupExpr, join } = dimensionSql(dimension);
  const rows = db
    .query(
      `SELECT ${groupExpr} AS key,
              COUNT(*) AS sessions,
              COALESCE(SUM(s.total_input_tokens), 0) AS input_tokens,
              COALESCE(SUM(s.total_output_tokens), 0) AS output_tokens,
              COALESCE(SUM(s.total_reasoning_tokens), 0) AS reasoning_tokens,
              COALESCE(SUM(s.est_reasoning_tokens), 0) AS est_reasoning_tokens,
              COALESCE(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
       FROM session s ${join}
       GROUP BY key
       ORDER BY sessions DESC`,
    )
    .all() as DimRowDb[];
  return rows.map((row) => ({ ...row, key: row.key ?? "" }));
}

export interface ToolStatRow {
  tool_name: string;
  tool_kind: string;
  mcp_server: string | null;
  calls: number;
  errors: number;
}

interface ToolStatDb extends Omit<ToolStatRow, "tool_name" | "tool_kind"> {
  tool_name: string | null;
  tool_kind: string | null;
}

export function toolUsage(db: Database, errorsOnly: boolean, limitValue = 50): ToolStatRow[] {
  const limit = normalizeLimit(limitValue, 50);
  const having = errorsOnly ? "HAVING errors > 0" : "";
  const rows = db
    .query(
      `SELECT tool_name, tool_kind, mcp_server,
              COUNT(*) AS calls,
              COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
       FROM tool_call
       GROUP BY tool_name, tool_kind, mcp_server
       ${having}
       ORDER BY calls DESC
       LIMIT ${limit}`,
    )
    .all() as ToolStatDb[];
  return rows.map((row) => ({
    ...row,
    tool_name: row.tool_name ?? "",
    tool_kind: row.tool_kind ?? "",
  }));
}

export interface McpStatRow {
  mcp_server: string;
  tools: number;
  calls: number;
  errors: number;
}

interface McpStatDb extends Omit<McpStatRow, "mcp_server"> {
  mcp_server: string | null;
}

export function mcpUsage(db: Database, limitValue = 50): McpStatRow[] {
  const limit = normalizeLimit(limitValue, 50);
  const rows = db
    .query(
      `SELECT mcp_server,
              COUNT(DISTINCT tool_name) AS tools,
              COUNT(*) AS calls,
              COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
       FROM tool_call
       WHERE tool_kind = 'mcp' AND mcp_server IS NOT NULL
       GROUP BY mcp_server
       ORDER BY calls DESC
       LIMIT ?1`,
    )
    .all(limit) as McpStatDb[];
  return rows.map((row) => ({ ...row, mcp_server: row.mcp_server ?? "" }));
}

export type FileGroup = "path" | "ext";

export function parseFileGroup(value: string): FileGroup | null {
  return value === "path" || value === "ext" ? value : null;
}

export interface FileStatRow {
  key: string;
  project: string | null;
  reads: number;
  edits: number;
  writes: number;
  deletes: number;
  sessions: number;
  last_touched_at: string | null;
}

interface FileStatDb extends Omit<FileStatRow, "key"> {
  key: string | null;
}

export function fileHotspots(
  db: Database,
  group: FileGroup,
  op: Operation | null,
  limitValue = 50,
): FileStatRow[] {
  const limit = normalizeLimit(limitValue, 50);
  const { keyExpr, projectExpr, join } = fileGroupSql(group);
  const opFilter = op == null ? "" : "WHERE f.operation = ?1";
  const sql = `SELECT ${keyExpr} AS key, ${projectExpr} AS project,
                      SUM(f.operation = 'read') AS reads,
                      SUM(f.operation = 'edit') AS edits,
                      SUM(f.operation = 'write') AS writes,
                      SUM(f.operation = 'delete') AS deletes,
                      COUNT(DISTINCT f.session_id) AS sessions,
                      MAX(f.timestamp) AS last_touched_at
               FROM file_ref f ${join}
               ${opFilter}
               GROUP BY key, project
               ORDER BY (reads + edits + writes + deletes) DESC, key ASC
               LIMIT ${limit}`;
  const rows = (op == null ? db.query(sql).all() : db.query(sql).all(op)) as FileStatDb[];
  return rows.map((row) => ({ ...row, key: row.key ?? "" }));
}

export interface SessionFacetRow {
  turn_count: number;
  error_count: number;
  interruption_count: number;
  compaction_count: number;
  sidechain_message_count: number;
  agent_spawn_count: number;
  skill_count: number;
  command_count: number;
  thinking_block_count: number;
  thinking_chars: number;
  active_seconds: number;
  outcome: string | null;
  work_type: string | null;
}

export function sessionFacets(db: Database, sessionId: number): SessionFacetRow | null {
  return db
    .query(
      `SELECT turn_count, error_count, interruption_count, compaction_count,
              sidechain_message_count, agent_spawn_count, skill_count, command_count,
              thinking_block_count, thinking_chars, active_seconds, outcome, work_type
       FROM session WHERE id = ?1`,
    )
    .get(sessionId) as SessionFacetRow | null;
}

export interface Activity {
  by_hour: number[];
  by_weekday: number[];
  timezone: string;
  peak_hour: number | null;
  peak_weekday: number | null;
}

export function activity(db: Database): Activity {
  const byHour = bucket(db, "strftime('%H', s.started_at, 'localtime')", 24);
  const byWeekday = bucket(db, "strftime('%w', s.started_at, 'localtime')", 7);
  return {
    by_hour: byHour,
    by_weekday: byWeekday,
    timezone: localTimezone(),
    peak_hour: peak(byHour),
    peak_weekday: peak(byWeekday),
  };
}

export interface ModelSparklines {
  models: Record<string, number[]>;
  days: string[];
}

export function modelSparklines(db: Database): ModelSparklines {
  const rows = db
    .query(
      `SELECT COALESCE(s.model, '(unknown)') AS model,
              substr(s.started_at, 1, 10) AS day,
              COUNT(*) AS count
       FROM session s
       WHERE s.started_at IS NOT NULL
       GROUP BY model, day
       ORDER BY model ASC, day ASC`,
    )
    .all() as { model: string; day: string; count: number }[];
  const days = [...new Set(rows.map((row) => row.day))].sort();
  const dayIndex = new Map(days.map((day, index) => [day, index]));
  const models: Record<string, number[]> = {};
  for (const row of rows) {
    if (models[row.model] == null) {
      models[row.model] = Array.from({ length: days.length }, () => 0);
    }
    const modelCounts = models[row.model];
    const index = dayIndex.get(row.day);
    if (index != null && modelCounts != null) {
      modelCounts[index] = row.count;
    }
  }
  return { models, days };
}

export interface DateBounds {
  min: string | null;
  max: string | null;
}

export function dateBounds(db: Database): DateBounds {
  return db
    .query(
      `SELECT MIN(substr(started_at, 1, 10)) AS min,
              MAX(substr(started_at, 1, 10)) AS max
       FROM session
       WHERE started_at IS NOT NULL`,
    )
    .get() as DateBounds;
}

export function todayTotals(db: Database): Totals {
  return db
    .query(
      `SELECT
         COUNT(*) AS sessions,
         (SELECT COUNT(*) FROM message m JOIN session s2 ON s2.id = m.session_id
          WHERE substr(s2.started_at, 1, 10) = date('now', 'localtime')) AS messages,
         (SELECT COUNT(*) FROM tool_call t JOIN session s3 ON s3.id = t.session_id
          WHERE substr(s3.started_at, 1, 10) = date('now', 'localtime')) AS tool_calls,
         COALESCE(SUM(total_input_tokens), 0) AS input_tokens,
         COALESCE(SUM(total_output_tokens), 0) AS output_tokens,
         COALESCE(SUM(total_cache_read_tokens), 0) AS cache_read_tokens,
         COALESCE(SUM(total_cache_creation_tokens), 0) AS cache_creation_tokens,
         COALESCE(SUM(total_reasoning_tokens), 0) AS reasoning_tokens,
         COALESCE(SUM(est_reasoning_tokens), 0) AS est_reasoning_tokens,
         COALESCE(SUM(estimated_cost_usd), 0.0) AS estimated_cost_usd
       FROM session
       WHERE substr(started_at, 1, 10) = date('now', 'localtime')`,
    )
    .get() as Totals;
}

function dimensionSql(dimension: Dimension): { groupExpr: string; join: string } {
  switch (dimension) {
    case "tool":
      return { groupExpr: "s.tool", join: "" };
    case "model":
      return { groupExpr: "COALESCE(s.model, '(unknown)')", join: "" };
    case "project":
      return {
        groupExpr: "COALESCE(p.path, '(none)')",
        join: "LEFT JOIN project p ON p.id = s.project_id",
      };
    case "day":
      return { groupExpr: "substr(s.started_at, 1, 10)", join: "" };
  }
}

function fileGroupSql(group: FileGroup): { keyExpr: string; projectExpr: string; join: string } {
  switch (group) {
    case "path":
      return {
        keyExpr: "COALESCE(f.rel_path, f.path)",
        projectExpr: "p.path",
        join: "JOIN session s ON s.id = f.session_id LEFT JOIN project p ON p.id = s.project_id",
      };
    case "ext":
      return {
        keyExpr: "COALESCE(f.ext, '(none)')",
        projectExpr: "NULL",
        join: "JOIN session s ON s.id = f.session_id",
      };
  }
}

function bucket(db: Database, expr: string, size: number): number[] {
  const out = Array.from({ length: size }, () => 0);
  const rows = db
    .query(
      `SELECT ${expr} AS key, COUNT(*) AS count
       FROM session s
       WHERE s.started_at IS NOT NULL
       GROUP BY key`,
    )
    .all() as { key: string | null; count: number }[];
  for (const row of rows) {
    const key = row.key == null || row.key === "" ? Number.NaN : Number.parseInt(row.key, 10);
    if (Number.isInteger(key) && key >= 0 && key < size) {
      out[key] = row.count;
    }
  }
  return out;
}

function peak(counts: number[]): number | null {
  let bestIndex: number | null = null;
  let bestCount = 0;
  for (const [index, count] of counts.entries()) {
    if (count > bestCount) {
      bestIndex = index;
      bestCount = count;
    }
  }
  return bestIndex;
}

function localTimezone(): string {
  const offset = -new Date().getTimezoneOffset();
  const sign = offset < 0 ? "-" : "+";
  const absolute = Math.abs(offset);
  const hours = Math.floor(absolute / 60)
    .toString()
    .padStart(2, "0");
  const minutes = (absolute % 60).toString().padStart(2, "0");
  return `UTC${sign}${hours}:${minutes}`;
}

function normalizeLimit(value: number | null | undefined, fallback: number): number {
  return value != null && value > 0 ? Math.trunc(value) : fallback;
}
