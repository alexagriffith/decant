import type { Database } from "bun:sqlite";
import { ACTIVITY_BUCKETS, type ActivityBucket, blockBucket, toolBucket } from "./buckets.ts";
import { defaultPricing, estimateCostParts } from "./cost.ts";
import { type DateFilter, sessionDatePredicate, whereClause } from "./date-filter.ts";

const CHARS_PER_TOKEN = 4;
const encoder = new TextEncoder();

export interface TokenEconomicsBucket {
  bucket: ActivityBucket;
  generation_tokens: number;
  context_window_tokens: number;
  estimated_cost_usd: number;
  tool_calls: number;
  sessions: number;
  cost_share: number;
}

export interface TokenEconomics {
  buckets: TokenEconomicsBucket[];
  totals: {
    generation_tokens: number;
    context_window_tokens: number;
    estimated_cost_usd: number;
    input_cost_usd: number;
    output_cost_usd: number;
  };
}

interface SessionRow {
  id: number;
  started_at: string | null;
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
  role: string | null;
  output_tokens: number | null;
  type: string | null;
  tool_name: string | null;
  tool_input: string | null;
  text_bytes: number;
}

interface ResultRow {
  session_id: number;
  tool_name: string | null;
  input: string | null;
  bytes: number;
}

interface MutableBucket {
  generation: number;
  contextWindow: number;
  cost: number;
  toolCalls: number;
  sessions: Set<number>;
}

type QueryParam = string | number;

export interface SessionEconomicsVector {
  id: number;
  started_at: string | null;
  input_cost: number;
  output_cost: number;
  buckets: Record<
    ActivityBucket,
    { generation: number; context_window: number; tool_calls: number; touched: boolean }
  >;
}

export function tokenEconomics(db: Database, filter?: DateFilter | null): TokenEconomics {
  const date = sessionDatePredicate("s", filter);
  return aggregateEconomicsVectors(
    vectorsForScope(
      db,
      `WITH scoped_session AS (
         SELECT s.id FROM session s ${whereClause(date)}
       )`,
      date.params,
    ),
  );
}

/**
 * Per-session activity vectors for the whole archive. Sums of these vectors
 * reproduce tokenEconomics() exactly for any date scope, so a caller can
 * compute them once (off the request path) and answer arbitrary date filters
 * from memory via aggregateEconomicsVectors().
 */
export function computeSessionEconomicsVectors(db: Database): SessionEconomicsVector[] {
  return vectorsForScope(db, "WITH scoped_session AS (SELECT id FROM session)", []);
}

/** Mirrors sessionDatePredicate: string-compare on the YYYY-MM-DD prefix. */
export function economicsVectorMatchesFilter(
  vector: SessionEconomicsVector,
  filter?: DateFilter | null,
): boolean {
  const from = filter?.from ?? null;
  const to = filter?.to ?? null;
  if (from == null && to == null) {
    return true;
  }
  if (vector.started_at == null) {
    return false;
  }
  const day = vector.started_at.slice(0, 10);
  return (from == null || day >= from) && (to == null || day <= to);
}

export function aggregateEconomicsVectors(
  vectors: Iterable<SessionEconomicsVector>,
): TokenEconomics {
  const buckets = emptyBuckets();
  let inputCost = 0;
  let outputCost = 0;
  for (const vector of vectors) {
    inputCost += vector.input_cost;
    outputCost += vector.output_cost;
    for (const bucket of ACTIVITY_BUCKETS) {
      const entry = buckets.get(bucket);
      const part = vector.buckets[bucket];
      if (entry == null || part == null) {
        continue;
      }
      entry.generation += part.generation;
      entry.contextWindow += part.context_window;
      entry.toolCalls += part.tool_calls;
      if (part.touched) {
        entry.sessions.add(vector.id);
      }
    }
  }

  const totalGeneration = sumBuckets(buckets, "generation");
  const totalWindow = sumBuckets(buckets, "contextWindow");
  for (const entry of buckets.values()) {
    entry.contextWindow += entry.generation;
  }
  const totalWindowWithGeneration = sumBuckets(buckets, "contextWindow");
  for (const entry of buckets.values()) {
    entry.cost =
      outputCost * share(entry.generation, totalGeneration) +
      inputCost * share(entry.contextWindow, totalWindowWithGeneration || totalWindow);
  }
  return finish(buckets, inputCost, outputCost);
}

export function tokenEconomicsForSession(db: Database, sessionId: number): TokenEconomics | null {
  return fastTokenEconomicsForSession(db, sessionId);
}

function vectorsForScope(
  db: Database,
  scopeCte: string,
  params: QueryParam[],
): SessionEconomicsVector[] {
  const sessions = db
    .query(
      `${scopeCte}
       SELECT s.id, s.started_at, s.model, s.total_input_tokens, s.total_output_tokens,
              s.total_cache_read_tokens, s.total_cache_creation_tokens,
              s.total_reasoning_tokens, s.est_reasoning_tokens
       FROM session s
       JOIN scoped_session fs ON fs.id = s.id`,
    )
    .all(...params) as SessionRow[];
  if (sessions.length === 0) {
    return [];
  }

  const vectorBySession = new Map<number, { session: SessionRow; buckets: PerSessionBuckets }>();
  for (const session of sessions) {
    vectorBySession.set(session.id, { session, buckets: emptyBuckets() });
  }

  // Ship byte lengths, not text: bucket math only needs sizes, plus the tool
  // input for the block types whose bucket depends on the command being run.
  // Row order is irrelevant to the math, so no ORDER BY.
  const blocks = db
    .query(
      `${scopeCte}
       SELECT b.session_id, b.message_id, m.role, m.output_tokens, b.type,
              b.tool_name,
              CASE WHEN b.type IN ('tool_use', 'tool_result', 'web_search')
                   THEN b.tool_input END AS tool_input,
              COALESCE(length(CAST(b.text AS BLOB)), 0) AS text_bytes
       FROM block b
       JOIN message m ON m.id = b.message_id
       JOIN scoped_session fs ON fs.id = b.session_id`,
    )
    .all(...params) as BlockRow[];
  const blocksBySession = groupBy(blocks, (block) => block.session_id);
  for (const [sessionId, sessionBlocks] of blocksBySession) {
    const vector = vectorBySession.get(sessionId);
    if (vector != null) {
      allocateGeneration([vector.session], sessionBlocks, vector.buckets);
    }
  }
  for (const vector of vectorBySession.values()) {
    if (!blocksBySession.has(vector.session.id)) {
      allocateGeneration([vector.session], [], vector.buckets);
    }
  }

  const results = db
    .query(
      `${scopeCte}
       SELECT t.session_id, t.tool_name, t.input,
              COALESCE(t.output_bytes, length(CAST(rb.tool_result AS BLOB)), 0) AS bytes
       FROM tool_call t
       JOIN scoped_session fs ON fs.id = t.session_id
       LEFT JOIN block rb ON rb.id = t.result_block_id`,
    )
    .all(...params) as ResultRow[];
  for (const row of results) {
    const vector = vectorBySession.get(row.session_id);
    const entry = vector?.buckets.get(toolBucket(row.tool_name, row.input));
    if (entry == null) {
      continue;
    }
    entry.contextWindow += row.bytes / CHARS_PER_TOKEN;
    entry.toolCalls += 1;
    entry.sessions.add(row.session_id);
  }

  return sessions.map((session) => {
    const mutable = vectorBySession.get(session.id);
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
    const buckets = {} as SessionEconomicsVector["buckets"];
    for (const bucket of ACTIVITY_BUCKETS) {
      const entry = mutable?.buckets.get(bucket);
      buckets[bucket] = {
        generation: entry?.generation ?? 0,
        context_window: entry?.contextWindow ?? 0,
        tool_calls: entry?.toolCalls ?? 0,
        touched: (entry?.sessions.size ?? 0) > 0,
      };
    }
    return {
      id: session.id,
      started_at: session.started_at,
      input_cost: parts.input + parts.cacheRead + parts.cacheCreation,
      output_cost: parts.output,
      buckets,
    };
  });
}

type PerSessionBuckets = Map<ActivityBucket, MutableBucket>;

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

function allocateGeneration(
  sessions: SessionRow[],
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
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
        allocateOutput(session.id, outputTokens, messageBlocks.get(messageId) ?? [], buckets);
      }
      continue;
    }
    const planning = Math.min(
      session.total_output_tokens,
      session.total_reasoning_tokens || session.est_reasoning_tokens,
    );
    addBucket(buckets, "planning", planning, session.id);
    const assistantBlocks = sessionBlocks.filter(
      (block) => block.role === "assistant" && block.type !== "thinking",
    );
    distribute(
      session.id,
      Math.max(0, session.total_output_tokens - planning),
      assistantBlocks,
      buckets,
    );
  }
}

function allocateOutput(
  sessionId: number,
  outputTokens: number,
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
): void {
  const hasThinking = blocks.some((block) => block.type === "thinking");
  const visible = blocks
    .filter((block) => block.type !== "thinking")
    .reduce((sum, block) => sum + blockSize(block), 0);
  const planning = hasThinking ? Math.max(0, outputTokens - visible / CHARS_PER_TOKEN) : 0;
  addBucket(buckets, "planning", planning, sessionId);
  distribute(
    sessionId,
    Math.max(0, outputTokens - planning),
    blocks.filter((block) => block.type !== "thinking"),
    buckets,
  );
}

function distribute(
  sessionId: number,
  tokens: number,
  blocks: BlockRow[],
  buckets: Map<ActivityBucket, MutableBucket>,
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
    );
  }
}

function blockSize(block: BlockRow): number {
  if (block.type === "tool_use") {
    return byteLength(`${block.tool_name ?? ""}\n${block.tool_input ?? ""}`);
  }
  return block.text_bytes;
}

function addBucket(
  buckets: Map<ActivityBucket, MutableBucket>,
  bucket: ActivityBucket,
  tokens: number,
  sessionId: number,
): void {
  const entry = buckets.get(bucket);
  if (entry == null || tokens <= 0) {
    return;
  }
  entry.generation += tokens;
  entry.sessions.add(sessionId);
}

function emptyBuckets(): Map<ActivityBucket, MutableBucket> {
  return new Map(
    ACTIVITY_BUCKETS.map((bucket) => [
      bucket,
      { generation: 0, contextWindow: 0, cost: 0, toolCalls: 0, sessions: new Set<number>() },
    ]),
  );
}

function finish(
  buckets: Map<ActivityBucket, MutableBucket>,
  inputCost: number,
  outputCost: number,
): TokenEconomics {
  const totalCost = sumBuckets(buckets, "cost");
  const rows = ACTIVITY_BUCKETS.map((bucket) => {
    const entry = buckets.get(bucket);
    return {
      bucket,
      generation_tokens: Math.round(entry?.generation ?? 0),
      context_window_tokens: Math.round(entry?.contextWindow ?? 0),
      estimated_cost_usd: entry?.cost ?? 0,
      tool_calls: entry?.toolCalls ?? 0,
      sessions: entry?.sessions.size ?? 0,
      cost_share: share(entry?.cost ?? 0, totalCost),
    };
  });
  return {
    buckets: rows,
    totals: {
      generation_tokens: rows.reduce((sum, row) => sum + row.generation_tokens, 0),
      context_window_tokens: rows.reduce((sum, row) => sum + row.context_window_tokens, 0),
      estimated_cost_usd: totalCost,
      input_cost_usd: inputCost,
      output_cost_usd: outputCost,
    },
  };
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
