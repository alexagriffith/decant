import type { Database } from "bun:sqlite";
import {
  ACTIVITY_BUCKETS,
  type ActivityBucket,
  blockBucket,
  isCodeEditTool,
  toolBucket,
} from "./buckets.ts";
import { defaultPricing, estimateCostParts } from "./cost.ts";
import { type DateFilter, sessionDatePredicate, whereClause } from "./date-filter.ts";

const CHARS_PER_TOKEN = 4;
const encoder = new TextEncoder();

/** A run splits into two phases at the first file edit: "orientation" (reading
 * and planning HOW to change the code) and "implementation" (writing it). The
 * phase breakdown is orthogonal to the activity buckets -- every bucket carries
 * how much of it happened before vs after the first edit. */
export type Phase = "orientation" | "implementation";

export interface PhaseAmounts {
  generation_tokens: number;
  context_window_tokens: number;
  estimated_cost_usd: number;
}

export interface TokenEconomicsBucket {
  bucket: ActivityBucket;
  generation_tokens: number;
  context_window_tokens: number;
  estimated_cost_usd: number;
  tool_calls: number;
  sessions: number;
  cost_share: number;
  // Present only from the ordered whole-archive path (`tokens`), which can place
  // each contribution before/after the first edit. Absent from the fast
  // per-session path, which distributes by aggregate weights and has no order.
  phases?: Record<Phase, PhaseAmounts>;
}

export interface TokenEconomics {
  buckets: TokenEconomicsBucket[];
  totals: {
    generation_tokens: number;
    context_window_tokens: number;
    estimated_cost_usd: number;
    input_cost_usd: number;
    output_cost_usd: number;
    phases?: Record<Phase, PhaseAmounts>;
  };
}

interface SessionRow {
  id: number;
  model: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read_tokens: number;
  total_cache_creation_tokens: number;
  total_reasoning_tokens: number;
  est_reasoning_tokens: number;
}

interface FastToolRow {
  session_id: number;
  tool_name: string | null;
  input: string | null;
  calls: number;
  output_bytes: number | null;
}

interface BlockRow {
  session_id: number;
  message_id: number;
  seq: number;
  role: string | null;
  output_tokens: number | null;
  type: string | null;
  text: string | null;
  tool_name: string | null;
  tool_input: string | null;
}

interface ResultRow {
  session_id: number;
  tool_name: string | null;
  input: string | null;
  tool_result: string | null;
  output_bytes: number | null;
  call_seq: number | null;
}

interface MutableBucket {
  generation: number;
  contextWindow: number;
  cost: number;
  toolCalls: number;
  sessions: Set<number>;
  // Orientation-phase portions (pre-first-edit). Implementation = total - these.
  // genOrientation feeds windowOrientation the same way generation feeds
  // contextWindow, so the phase cost split mirrors the whole-bucket formula.
  genOrientation: number;
  windowOrientation: number;
  costOrientation: number;
}

type QueryParam = string | number;

export function tokenEconomics(db: Database, filter?: DateFilter | null): TokenEconomics {
  const date = sessionDatePredicate("s", filter);
  return tokenEconomicsForScope(
    db,
    `WITH scoped_session AS (
       SELECT s.id FROM session s ${whereClause(date)}
     )`,
    date.params,
  );
}

export function tokenEconomicsForSession(db: Database, sessionId: number): TokenEconomics | null {
  return fastTokenEconomicsForSession(db, sessionId);
}

function tokenEconomicsForScope(
  db: Database,
  scopeCte: string,
  params: QueryParam[],
): TokenEconomics {
  const sessions = db
    .query(
      `${scopeCte}
       SELECT s.id, s.model, s.total_input_tokens, s.total_output_tokens,
              s.total_cache_read_tokens, s.total_cache_creation_tokens,
              s.total_reasoning_tokens, s.est_reasoning_tokens
       FROM session s
       JOIN scoped_session fs ON fs.id = s.id`,
    )
    .all(...params) as SessionRow[];
  const sessionIds = sessions.map((session) => session.id);
  const buckets = emptyBuckets();

  if (sessionIds.length === 0) {
    return finish(buckets, 0, 0, true);
  }

  const blocks = db
    .query(
      `${scopeCte}
       SELECT b.session_id, m.id AS message_id, m.seq AS seq, m.role, m.output_tokens, b.type,
              b.text, b.tool_name, b.tool_input
       FROM block b
       JOIN message m ON m.id = b.message_id
       JOIN scoped_session fs ON fs.id = b.session_id
       ORDER BY b.session_id, m.seq, b.ordinal`,
    )
    .all(...params) as BlockRow[];
  const boundaries = firstEditSeqBySession(blocks);
  allocateGeneration(sessions, blocks, buckets, boundaries);

  const results = db
    .query(
      `${scopeCte}
       SELECT t.session_id, t.tool_name, t.input, rb.tool_result, t.output_bytes,
              cm.seq AS call_seq
       FROM tool_call t
       JOIN scoped_session fs ON fs.id = t.session_id
       LEFT JOIN block rb ON rb.id = t.result_block_id
       LEFT JOIN block cb ON cb.id = t.call_block_id
       LEFT JOIN message cm ON cm.id = cb.message_id`,
    )
    .all(...params) as ResultRow[];
  for (const row of results) {
    const bucket = toolBucket(row.tool_name, row.input);
    const size = row.output_bytes ?? byteLength(row.tool_result ?? "");
    const entry = buckets.get(bucket);
    if (entry == null) {
      continue;
    }
    const tokens = size / CHARS_PER_TOKEN;
    entry.contextWindow += tokens;
    entry.toolCalls += 1;
    entry.sessions.add(row.session_id);
    // A tool result belongs to orientation if its call happened before the
    // session's first edit. Unplaceable calls (no call block) default to
    // implementation so orientation is never overstated.
    if (phaseOf(boundaries, row.session_id, row.call_seq) === "orientation") {
      entry.windowOrientation += tokens;
    }
  }

  let inputCost = 0;
  let outputCost = 0;
  for (const session of sessions) {
    const parts = estimateCostParts(
      session.model,
      {
        input: session.total_input_tokens,
        output: session.total_output_tokens,
        cacheRead: session.total_cache_read_tokens,
        cacheCreation: session.total_cache_creation_tokens,
        reasoning: session.total_reasoning_tokens,
      },
      defaultPricing(),
    );
    inputCost += parts.input + parts.cacheRead + parts.cacheCreation;
    outputCost += parts.output;
  }

  const totalGeneration = sumBuckets(buckets, "generation");
  const totalWindow = sumBuckets(buckets, "contextWindow");
  for (const entry of buckets.values()) {
    // Generation is part of the window; mirror it into the orientation portion
    // so the phase cost split uses the same window basis as the whole bucket.
    entry.contextWindow += entry.generation;
    entry.windowOrientation += entry.genOrientation;
  }
  const totalWindowWithGeneration = sumBuckets(buckets, "contextWindow");
  const windowBasis = totalWindowWithGeneration || totalWindow;
  for (const entry of buckets.values()) {
    entry.cost =
      outputCost * share(entry.generation, totalGeneration) +
      inputCost * share(entry.contextWindow, windowBasis);
    entry.costOrientation =
      outputCost * share(entry.genOrientation, totalGeneration) +
      inputCost * share(entry.windowOrientation, windowBasis);
  }
  return finish(buckets, inputCost, outputCost, true);
}

function fastTokenEconomicsForSession(db: Database, sessionId: number): TokenEconomics | null {
  const scopeCte = `WITH RECURSIVE scoped_session(id) AS (
    SELECT id FROM session WHERE id = ?1
    UNION ALL
    SELECT child.id
    FROM session child
    JOIN scoped_session parent ON parent.id = child.parent_session_id
  )`;
  const sessions = db
    .query(
      `${scopeCte}
       SELECT s.id, s.model, s.total_input_tokens, s.total_output_tokens,
              s.total_cache_read_tokens, s.total_cache_creation_tokens,
              s.total_reasoning_tokens, s.est_reasoning_tokens
       FROM session s
       JOIN scoped_session fs ON fs.id = s.id`,
    )
    .all(sessionId) as SessionRow[];
  if (sessions.length === 0) {
    return null;
  }

  const buckets = emptyBuckets();
  const toolRows = db
    .query(
      `${scopeCte}
       SELECT t.session_id, t.tool_name, t.input, 1 AS calls,
              COALESCE(t.output_bytes, 0) AS output_bytes
       FROM tool_call t
       JOIN scoped_session fs ON fs.id = t.session_id`,
    )
    .all(sessionId) as FastToolRow[];

  let inputCost = 0;
  let outputCost = 0;
  let inputWindowTokens = 0;
  let remainingOutputTokens = 0;
  const toolCallWeights = new Map<ActivityBucket, number>();
  for (const session of sessions) {
    const planning = Math.min(
      session.total_output_tokens,
      session.total_reasoning_tokens || session.est_reasoning_tokens,
    );
    addBucket(buckets, "planning", planning, session.id);
    remainingOutputTokens += Math.max(0, session.total_output_tokens - planning);
    inputWindowTokens +=
      session.total_input_tokens +
      session.total_cache_read_tokens +
      session.total_cache_creation_tokens;
    const parts = estimateCostParts(
      session.model,
      {
        input: session.total_input_tokens,
        output: session.total_output_tokens,
        cacheRead: session.total_cache_read_tokens,
        cacheCreation: session.total_cache_creation_tokens,
        reasoning: session.total_reasoning_tokens,
      },
      defaultPricing(),
    );
    inputCost += parts.input + parts.cacheRead + parts.cacheCreation;
    outputCost += parts.output;
  }

  for (const row of toolRows) {
    const bucket = toolBucket(row.tool_name, row.input);
    const entry = buckets.get(bucket);
    if (entry == null) {
      continue;
    }
    entry.contextWindow += (row.output_bytes ?? 0) / CHARS_PER_TOKEN;
    entry.toolCalls += row.calls;
    entry.sessions.add(row.session_id);
    toolCallWeights.set(bucket, (toolCallWeights.get(bucket) ?? 0) + row.calls);
  }

  distributeByWeights(remainingOutputTokens, toolCallWeights, buckets, sessions);
  const windowWeights = toolCallWeights.size > 0 ? toolCallWeights : generationWeights(buckets);
  distributeWindow(inputWindowTokens, windowWeights, buckets, sessions);
  for (const entry of buckets.values()) {
    entry.contextWindow += entry.generation;
  }

  const totalGeneration = sumBuckets(buckets, "generation");
  const totalWindow = sumBuckets(buckets, "contextWindow");
  for (const entry of buckets.values()) {
    entry.cost =
      outputCost * share(entry.generation, totalGeneration) +
      inputCost * share(entry.contextWindow, totalWindow);
  }
  return finish(buckets, inputCost, outputCost);
}

function distributeByWeights(
  tokens: number,
  weights: Map<ActivityBucket, number>,
  buckets: Map<ActivityBucket, MutableBucket>,
  sessions: SessionRow[],
): void {
  if (tokens <= 0) {
    return;
  }
  const totalWeight = sumWeights(weights);
  if (totalWeight <= 0) {
    addSharedGeneration(buckets, "communicating", tokens, sessions);
    return;
  }
  for (const [bucket, weight] of weights) {
    addSharedGeneration(buckets, bucket, tokens * (weight / totalWeight), sessions);
  }
}

function distributeWindow(
  tokens: number,
  weights: Map<ActivityBucket, number>,
  buckets: Map<ActivityBucket, MutableBucket>,
  sessions: SessionRow[],
): void {
  if (tokens <= 0) {
    return;
  }
  const totalWeight = sumWeights(weights);
  if (totalWeight <= 0) {
    addSharedWindow(buckets, "context", tokens, sessions);
    return;
  }
  for (const [bucket, weight] of weights) {
    addSharedWindow(buckets, bucket, tokens * (weight / totalWeight), sessions);
  }
}

function generationWeights(
  buckets: Map<ActivityBucket, MutableBucket>,
): Map<ActivityBucket, number> {
  const weights = new Map<ActivityBucket, number>();
  for (const [bucket, entry] of buckets) {
    if (entry.generation > 0) {
      weights.set(bucket, entry.generation);
    }
  }
  return weights;
}

function addSharedGeneration(
  buckets: Map<ActivityBucket, MutableBucket>,
  bucket: ActivityBucket,
  tokens: number,
  sessions: SessionRow[],
): void {
  const entry = buckets.get(bucket);
  if (entry == null || tokens <= 0) {
    return;
  }
  entry.generation += tokens;
  for (const session of sessions) {
    entry.sessions.add(session.id);
  }
}

function addSharedWindow(
  buckets: Map<ActivityBucket, MutableBucket>,
  bucket: ActivityBucket,
  tokens: number,
  sessions: SessionRow[],
): void {
  const entry = buckets.get(bucket);
  if (entry == null || tokens <= 0) {
    return;
  }
  entry.contextWindow += tokens;
  for (const session of sessions) {
    entry.sessions.add(session.id);
  }
}

function sumWeights(weights: Map<ActivityBucket, number>): number {
  let total = 0;
  for (const value of weights.values()) {
    total += value;
  }
  return total;
}

/** First-edit boundary per session: the message seq of the first file-editing
 * tool_use. Messages with seq < boundary are orientation; the edit's own
 * message and everything after are implementation. Sessions that never edit
 * have no boundary (Infinity) -> the whole run is orientation. */
function firstEditSeqBySession(blocks: BlockRow[]): Map<number, number> {
  const boundary = new Map<number, number>();
  for (const block of blocks) {
    if (block.type !== "tool_use" || !isCodeEditTool(block.tool_name)) {
      continue;
    }
    const current = boundary.get(block.session_id);
    if (current == null || block.seq < current) {
      boundary.set(block.session_id, block.seq);
    }
  }
  return boundary;
}

function phaseOf(
  boundaries: Map<number, number>,
  sessionId: number,
  seq: number | null | undefined,
): Phase {
  if (seq == null) {
    return "implementation";
  }
  const boundary = boundaries.get(sessionId) ?? Number.POSITIVE_INFINITY;
  return seq < boundary ? "orientation" : "implementation";
}

function allocateGeneration(
  sessions: SessionRow[],
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
  boundaries: Map<number, number>,
): void {
  const blocksBySession = groupBy(blocks, (block) => block.session_id);
  for (const session of sessions) {
    const sessionBlocks = blocksBySession.get(session.id) ?? [];
    const messageBlocks = groupBy(sessionBlocks, (block) => block.message_id);
    const messageOutput = new Map<number, number>();
    for (const block of sessionBlocks) {
      if (block.output_tokens != null && block.output_tokens > 0) {
        messageOutput.set(block.message_id, block.output_tokens);
      }
    }
    if (messageOutput.size > 0) {
      for (const [messageId, outputTokens] of messageOutput) {
        const rows = messageBlocks.get(messageId) ?? [];
        const phase = phaseOf(boundaries, session.id, rows[0]?.seq);
        allocateOutput(session.id, outputTokens, rows, buckets, phase);
      }
      continue;
    }
    const planning = Math.min(
      session.total_output_tokens,
      session.total_reasoning_tokens || session.est_reasoning_tokens,
    );
    const assistantBlocks = sessionBlocks.filter(
      (block) => block.role === "assistant" && block.type !== "thinking",
    );
    // No per-message usage: split the aggregate planning lump between phases by
    // the size-weight of assistant blocks on each side of the boundary.
    const orientationWeight = assistantBlocks
      .filter((block) => phaseOf(boundaries, session.id, block.seq) === "orientation")
      .reduce((sum, block) => sum + Math.max(1, blockSize(block)), 0);
    const totalWeight =
      assistantBlocks.reduce((sum, block) => sum + Math.max(1, blockSize(block)), 0) || 1;
    addBucket(
      buckets,
      "planning",
      planning * (orientationWeight / totalWeight),
      session.id,
      "orientation",
    );
    addBucket(
      buckets,
      "planning",
      planning * (1 - orientationWeight / totalWeight),
      session.id,
      "implementation",
    );
    distribute(
      session.id,
      Math.max(0, session.total_output_tokens - planning),
      assistantBlocks,
      buckets,
      boundaries,
    );
  }
}

function allocateOutput(
  sessionId: number,
  outputTokens: number,
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
  phase: Phase,
): void {
  const hasThinking = blocks.some((block) => block.type === "thinking");
  const visible = blocks
    .filter((block) => block.type !== "thinking")
    .reduce((sum, block) => sum + blockSize(block), 0);
  const planning = hasThinking ? Math.max(0, outputTokens - visible / CHARS_PER_TOKEN) : 0;
  addBucket(buckets, "planning", planning, sessionId, phase);
  distributeVisible(
    sessionId,
    Math.max(0, outputTokens - planning),
    blocks.filter((block) => block.type !== "thinking"),
    buckets,
    phase,
  );
}

/** Distribute across blocks that all share one phase (used per-message). */
function distributeVisible(
  sessionId: number,
  tokens: number,
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
  phase: Phase,
): void {
  if (tokens <= 0 || blocks.length === 0) {
    return;
  }
  const weighted = blocks.map((block) => ({ block, weight: Math.max(1, blockSize(block)) }));
  const totalWeight = weighted.reduce((sum, item) => sum + item.weight, 0);
  for (const item of weighted) {
    addBucket(
      buckets,
      blockBucket(item.block.type, item.block.tool_name, item.block.tool_input),
      tokens * (item.weight / totalWeight),
      sessionId,
      phase,
    );
  }
}

/** Distribute across blocks that may straddle the boundary (fallback branch);
 * each block's phase is decided by its own seq. */
function distribute(
  sessionId: number,
  tokens: number,
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
  boundaries: Map<number, number>,
): void {
  if (tokens <= 0 || blocks.length === 0) {
    return;
  }
  const weighted = blocks.map((block) => ({
    block,
    weight: Math.max(1, blockSize(block)),
  }));
  const totalWeight = weighted.reduce((sum, item) => sum + item.weight, 0);
  for (const item of weighted) {
    addBucket(
      buckets,
      blockBucket(item.block.type, item.block.tool_name, item.block.tool_input),
      tokens * (item.weight / totalWeight),
      sessionId,
      phaseOf(boundaries, sessionId, item.block.seq),
    );
  }
}

function blockSize(block: BlockRow): number {
  if (block.type === "tool_use") {
    return byteLength(`${block.tool_name ?? ""}\n${block.tool_input ?? ""}`);
  }
  return byteLength(block.text ?? "");
}

function addBucket(
  buckets: Map<ActivityBucket, MutableBucket>,
  bucket: ActivityBucket,
  tokens: number,
  sessionId: number,
  phase?: Phase,
): void {
  const entry = buckets.get(bucket);
  if (entry == null || tokens <= 0) {
    return;
  }
  entry.generation += tokens;
  if (phase === "orientation") {
    entry.genOrientation += tokens;
  }
  entry.sessions.add(sessionId);
}

function emptyBuckets(): Map<ActivityBucket, MutableBucket> {
  return new Map(
    ACTIVITY_BUCKETS.map((bucket) => [
      bucket,
      {
        generation: 0,
        contextWindow: 0,
        cost: 0,
        toolCalls: 0,
        sessions: new Set<number>(),
        genOrientation: 0,
        windowOrientation: 0,
        costOrientation: 0,
      },
    ]),
  );
}

function phasesFor(entry: MutableBucket | undefined): Record<Phase, PhaseAmounts> {
  const gen = entry?.generation ?? 0;
  const win = entry?.contextWindow ?? 0;
  const cost = entry?.cost ?? 0;
  const genO = entry?.genOrientation ?? 0;
  const winO = entry?.windowOrientation ?? 0;
  const costO = entry?.costOrientation ?? 0;
  return {
    orientation: {
      generation_tokens: Math.round(genO),
      context_window_tokens: Math.round(winO),
      estimated_cost_usd: costO,
    },
    implementation: {
      generation_tokens: Math.round(gen - genO),
      context_window_tokens: Math.round(win - winO),
      estimated_cost_usd: cost - costO,
    },
  };
}

function finish(
  buckets: Map<ActivityBucket, MutableBucket>,
  inputCost: number,
  outputCost: number,
  withPhases = false,
): TokenEconomics {
  const totalCost = sumBuckets(buckets, "cost");
  const rows: TokenEconomicsBucket[] = ACTIVITY_BUCKETS.map((bucket) => {
    const entry = buckets.get(bucket);
    const row: TokenEconomicsBucket = {
      bucket,
      generation_tokens: Math.round(entry?.generation ?? 0),
      context_window_tokens: Math.round(entry?.contextWindow ?? 0),
      estimated_cost_usd: entry?.cost ?? 0,
      tool_calls: entry?.toolCalls ?? 0,
      sessions: entry?.sessions.size ?? 0,
      cost_share: share(entry?.cost ?? 0, totalCost),
    };
    if (withPhases) {
      row.phases = phasesFor(entry);
    }
    return row;
  });
  const totals: TokenEconomics["totals"] = {
    generation_tokens: rows.reduce((sum, row) => sum + row.generation_tokens, 0),
    context_window_tokens: rows.reduce((sum, row) => sum + row.context_window_tokens, 0),
    estimated_cost_usd: totalCost,
    input_cost_usd: inputCost,
    output_cost_usd: outputCost,
  };
  if (withPhases) {
    totals.phases = {
      orientation: sumPhase(rows, "orientation"),
      implementation: sumPhase(rows, "implementation"),
    };
  }
  return { buckets: rows, totals };
}

function sumPhase(rows: TokenEconomicsBucket[], phase: Phase): PhaseAmounts {
  const acc: PhaseAmounts = {
    generation_tokens: 0,
    context_window_tokens: 0,
    estimated_cost_usd: 0,
  };
  for (const row of rows) {
    const p = row.phases?.[phase];
    if (p == null) {
      continue;
    }
    acc.generation_tokens += p.generation_tokens;
    acc.context_window_tokens += p.context_window_tokens;
    acc.estimated_cost_usd += p.estimated_cost_usd;
  }
  return acc;
}

function sumBuckets(
  buckets: Map<ActivityBucket, MutableBucket>,
  key: "generation" | "contextWindow" | "cost",
): number {
  let total = 0;
  for (const bucket of buckets.values()) {
    total += bucket[key];
  }
  return total;
}

function share(value: number, total: number): number {
  return total > 0 ? value / total : 0;
}

function groupBy<T, K>(items: T[], keyFor: (item: T) => K): Map<K, T[]> {
  const groups = new Map<K, T[]>();
  for (const item of items) {
    const key = keyFor(item);
    const group = groups.get(key);
    if (group == null) {
      groups.set(key, [item]);
    } else {
      group.push(item);
    }
  }
  return groups;
}

function byteLength(value: string): number {
  return encoder.encode(value).length;
}
