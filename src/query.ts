import type { Database } from "bun:sqlite";
import { sessionDatePredicate } from "./date-filter.ts";

export interface SessionSummary {
  id: number;
  tool: string;
  source_session_id: string;
  title: string | null;
  project_path: string | null;
  model: string | null;
  started_at: string | null;
  ended_at: string | null;
  message_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  estimated_cost_usd: number;
  is_archived: boolean;
}

export interface ListFilter {
  tool?: string | null;
  limit?: number;
  offset?: number;
  from?: string | null;
  to?: string | null;
}

interface SessionSummaryRow extends Omit<SessionSummary, "is_archived"> {
  is_archived: number;
}

export function listSessions(db: Database, filter: ListFilter = {}): SessionSummary[] {
  const limit = normalizeLimit(filter.limit, 50);
  const offset = normalizeOffset(filter.offset);
  const clauses: string[] = [];
  const params: (string | number)[] = [];
  if (filter.tool != null) {
    clauses.push("s.tool = ?");
    params.push(filter.tool);
  }
  const date = sessionDatePredicate("s", filter);
  if (date.sql !== "") {
    clauses.push(date.sql);
    params.push(...date.params);
  }
  const where = clauses.length === 0 ? "" : ` WHERE ${clauses.join(" AND ")}`;
  const base = `
    SELECT s.id, s.tool, s.source_session_id, s.title, p.path AS project_path,
           s.model, s.started_at, s.ended_at, s.message_count,
           s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd,
           s.is_archived
    FROM session s LEFT JOIN project p ON p.id = s.project_id`;
  const rows = db
    .query(`${base}${where} ORDER BY s.started_at DESC LIMIT ? OFFSET ?`)
    .all(...params, limit, offset) as SessionSummaryRow[];
  return rows.map(mapSessionSummary);
}

export interface SearchHit {
  session_id: number;
  session_title: string | null;
  tool: string;
  block_id: number;
  snippet: string;
}

export function search(db: Database, query: string, limitValue = 30): SearchHit[] {
  const limit = normalizeLimit(limitValue, 30);
  return db
    .query(
      `SELECT b.session_id, s.title AS session_title, s.tool, b.id AS block_id,
              COALESCE(snippet(block_fts, 0, '[', ']', '…', 12),
                       snippet(block_fts, 1, '[', ']', '…', 12),
                       snippet(block_fts, 2, '[', ']', '…', 12), '') AS snippet
       FROM block_fts
       JOIN block b ON b.id = block_fts.rowid
       JOIN session s ON s.id = b.session_id
       WHERE block_fts MATCH ?1
       ORDER BY bm25(block_fts)
       LIMIT ?2`,
    )
    .all(query, limit) as SearchHit[];
}

export interface BlockView {
  ordinal: number;
  block_type: string;
  text: string | null;
  tool_name: string | null;
  tool_input: string | null;
  tool_result: string | null;
}

export interface MessageView {
  role: string;
  timestamp: string | null;
  model: string | null;
  blocks: BlockView[];
}

export interface SessionDetail {
  summary: SessionSummary;
  messages: MessageView[];
}

interface MessageBlockRow {
  message_id: number;
  role: string | null;
  timestamp: string | null;
  model: string | null;
  block_ordinal: number | null;
  block_type: string | null;
  text: string | null;
  tool_name: string | null;
  tool_input: string | null;
  tool_result: string | null;
}

export function getSession(db: Database, id: number): SessionDetail | null {
  const summaryRow = db
    .query(
      `SELECT s.id, s.tool, s.source_session_id, s.title, p.path AS project_path,
              s.model, s.started_at, s.ended_at, s.message_count,
              s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd,
              s.is_archived
       FROM session s LEFT JOIN project p ON p.id = s.project_id
       WHERE s.id = ?1`,
    )
    .get(id) as SessionSummaryRow | null;
  if (summaryRow == null) {
    return null;
  }

  const rows = db
    .query(
      `SELECT m.id AS message_id, m.role, m.timestamp, m.model,
              b.ordinal AS block_ordinal, b.type AS block_type, b.text,
              b.tool_name, b.tool_input, b.tool_result
       FROM message m
       LEFT JOIN block b ON b.message_id = m.id
       WHERE m.session_id = ?1
       ORDER BY m.seq, b.ordinal`,
    )
    .all(id) as MessageBlockRow[];

  const messages: MessageView[] = [];
  let currentMessageId: number | null = null;
  for (const row of rows) {
    if (currentMessageId !== row.message_id) {
      currentMessageId = row.message_id;
      messages.push({
        role: row.role ?? "unknown",
        timestamp: row.timestamp,
        model: row.model,
        blocks: [],
      });
    }
    if (row.block_type != null) {
      messages.at(-1)?.blocks.push({
        ordinal: row.block_ordinal ?? 0,
        block_type: row.block_type,
        text: row.text,
        tool_name: row.tool_name,
        tool_input: row.tool_input,
        tool_result: row.tool_result,
      });
    }
  }

  return { summary: mapSessionSummary(summaryRow), messages };
}

export interface ProjectSummary {
  id: number;
  path: string;
  name: string | null;
  sessions: number;
  estimated_cost_usd: number;
  last_seen_at: string | null;
}

export function listProjects(db: Database): ProjectSummary[] {
  return db
    .query(
      `SELECT p.id, p.path, p.name,
              COUNT(s.id) AS sessions,
              COALESCE(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd,
              MAX(s.ended_at) AS last_seen_at
       FROM project p
       LEFT JOIN session s ON s.project_id = p.id
       GROUP BY p.id, p.path, p.name
       ORDER BY sessions DESC`,
    )
    .all() as ProjectSummary[];
}

function mapSessionSummary(row: SessionSummaryRow): SessionSummary {
  return { ...row, is_archived: row.is_archived !== 0 };
}

function normalizeLimit(value: number | null | undefined, fallback: number): number {
  return value != null && value > 0 ? value : fallback;
}

function normalizeOffset(value: number | null | undefined): number {
  return value != null && value > 0 ? value : 0;
}
