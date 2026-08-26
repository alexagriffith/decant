import { linkageIssues } from "../diagnostics.ts";
import { canonicalJson } from "../json.ts";
import {
  emptyUsage,
  type Json,
  type NormalizedBlock,
  type NormalizedMessage,
  type NormalizedSession,
  type ParsedSession,
  type TokenUsage,
} from "../model.ts";
import { preview } from "../tools.ts";

type JsonObject = { [key: string]: Json };

/**
 * Gemini CLI session format (`.gemini/tmp/<project>/chats/session-<ts>-<hash>.jsonl`).
 *
 * Line types:
 *  - Header (line 0): `{ sessionId, projectHash, startTime, lastUpdated, kind }`
 *  - Message: `{ id, timestamp, type: "user"|"gemini", content, tokens?, model?, toolCalls?, thoughts? }`
 *  - Patch: `{ $set: { messages?, lastUpdated } }` — incremental updates; skip
 *  - Tool result (user turn): content array contains `{ functionResponse: { name, response } }`
 */
export function parseGeminiSession(
  fallbackId: string,
  content: string,
  projectPath: string | null,
): ParsedSession {
  const issues: ParsedSession["issues"] = [];
  const messages: NormalizedMessage[] = [];
  let sourceSessionId = fallbackId;
  let model: string | null = null;
  let startedAt: string | null = null;
  let endedAt: string | null = null;
  let title: string | null = null;
  let totals = emptyUsage();
  let sawReportedReasoning = false;
  let seq = 0;
  const seenToolResultIds = new Set<string>();
  const unknownTypes = new Map<string, { count: number; firstLine: number }>();

  for (const [index, line] of content.split(/\n/).entries()) {
    if (line.trim() === "") {
      continue;
    }

    let value: Json;
    try {
      value = JSON.parse(line) as Json;
    } catch (error) {
      issues.push({
        code: "unparsed_line",
        lineNo: index + 1,
        error: error instanceof Error ? error.message : String(error),
        rawLine: line,
      });
      continue;
    }

    // Skip patch records — they duplicate already-parsed message data
    if (hasKey(value, "$set")) {
      continue;
    }

    const typ = asString(get(value, "type"));
    const timestamp = asString(get(value, "timestamp"));

    // Header line: no `type`, but has `sessionId`
    if (typ == null && hasKey(value, "sessionId")) {
      sourceSessionId = asString(get(value, "sessionId")) ?? sourceSessionId;
      startedAt ??= asString(get(value, "startTime"));
      continue;
    }

    if (timestamp != null) {
      startedAt ??= timestamp;
      endedAt = timestamp;
    }

    if (typ === "user") {
      const blocks = parseUserContent(get(value, "content"), seenToolResultIds);
      if (blocks.length === 0) {
        continue;
      }
      const firstText = blocks.find((b) => b.blockType === "text")?.text ?? "";
      if (title == null && firstText !== "") {
        title = preview(firstText.trim(), 120);
      }
      messages.push({
        seq,
        sourceUuid: asString(get(value, "id")),
        parentSourceUuid: null,
        role: blocks.every((block) => block.blockType === "tool_result")
          ? "tool"
          : blocks.every((block) => block.blockType === "other")
            ? "other"
            : "user",
        model: null,
        stopReason: null,
        timestamp,
        usage: null,
        raw: value,
        blocks,
      });
      seq += 1;
    } else if (typ === "gemini") {
      const msgModel: string | null = asString(get(value, "model")) ?? model;
      model ??= msgModel;
      const tokenRecord = get(value, "tokens");
      sawReportedReasoning ||= isObject(tokenRecord) && hasKey(tokenRecord, "thoughts");
      const usage = usageFrom(tokenRecord);
      // Gemini records usage for each model turn, so session totals are the sum.
      totals = addUsage(totals, usage);

      const contentBlocks = parseGeminiContent(get(value, "content"));
      const toolCallBlocks = parseToolCalls(get(value, "toolCalls"), contentBlocks.length);
      const thoughtBlocks = parseThoughts(
        get(value, "thoughts"),
        contentBlocks.length + toolCallBlocks.length,
      );
      const allBlocks = [...contentBlocks, ...toolCallBlocks, ...thoughtBlocks];

      if (allBlocks.length === 0) {
        continue;
      }

      messages.push({
        seq,
        sourceUuid: asString(get(value, "id")),
        parentSourceUuid: null,
        role: "assistant",
        model: msgModel,
        stopReason: null,
        timestamp,
        usage,
        raw: value,
        blocks: allBlocks,
      });
      seq += 1;
    } else if (typ === "error" || typ === "info" || typ === "warning") {
      const text = asString(get(value, "content"));
      if (text == null || text === "") {
        continue;
      }
      messages.push({
        seq,
        sourceUuid: asString(get(value, "id")),
        parentSourceUuid: null,
        role: "other",
        model: null,
        stopReason: null,
        timestamp,
        usage: null,
        raw: value,
        blocks: [textBlock(0, text)],
      });
      seq += 1;
    } else if (typ != null) {
      const seen = unknownTypes.get(typ) ?? { count: 0, firstLine: index + 1 };
      seen.count += 1;
      unknownTypes.set(typ, seen);
    }
  }

  for (const [typ, seen] of unknownTypes) {
    issues.push({
      code: "unknown_record_type",
      lineNo: seen.firstLine,
      error: `unknown record type "${typ}" on ${seen.count} line(s); ignored`,
      rawLine: null,
    });
  }

  const normalized: NormalizedSession = {
    tool: "gemini",
    sourceSessionId,
    projectPath,
    title,
    cwd: projectPath,
    gitBranch: null,
    model,
    reasoningEffort: null,
    reasoningEffortLevels: [],
    cliVersion: null,
    startedAt,
    endedAt,
    isArchived: false,
    isSubagent: false,
    rootSourceSessionId: null,
    spawnToolUseId: null,
    agentId: null,
    agentType: null,
    spawnDepth: null,
    rawMeta: null,
    totals,
    estReasoningTokens: 0,
    reasoningSource: sawReportedReasoning ? "reported" : "none",
    messages,
  };
  issues.push(...linkageIssues(normalized));

  return { session: normalized, issues };
}

function parseUserContent(
  content: Json | undefined,
  seenToolResultIds: Set<string>,
): NormalizedBlock[] {
  if (!Array.isArray(content)) {
    const text = asString(content);
    if (text != null && text !== "") {
      return [textBlock(0, text)];
    }
    return [];
  }

  const blocks: NormalizedBlock[] = [];
  let ordinal = 0;

  for (const item of content) {
    const text = asString(get(item, "text"));
    if (text != null) {
      // Skip the boilerplate session-context injection Gemini prepends
      const trimmed = text.trimStart();
      if (!trimmed.startsWith("<session_context>") && !trimmed.startsWith("<hook_context>")) {
        blocks.push(textBlock(ordinal++, text));
      }
      continue;
    }

    const funcResponse = get(item, "functionResponse");
    if (funcResponse != null) {
      const name = asString(get(funcResponse, "name"));
      const callId = asString(get(funcResponse, "id")) ?? name;
      const response = get(funcResponse, "response");
      if (callId != null && seenToolResultIds.has(callId)) {
        // Gemini may repeat a completed function response in a later record.
        // Keep the source record without creating a second linked result.
        blocks.push(otherBlock(ordinal++, item));
        continue;
      }
      if (callId != null) {
        seenToolResultIds.add(callId);
      }
      blocks.push({
        ordinal: ordinal++,
        blockType: "tool_result",
        text: null,
        toolName: null,
        toolUseId: callId,
        toolInput: undefined,
        toolResult: canonicalJson(response ?? null),
        isError: toolResultIsError(response),
      });
    }
  }

  return blocks;
}

function parseGeminiContent(content: Json | undefined): NormalizedBlock[] {
  if (!Array.isArray(content)) {
    const text = asString(content);
    if (text != null && text !== "") {
      return [textBlock(0, text)];
    }
    return [];
  }

  const blocks: NormalizedBlock[] = [];
  let ordinal = 0;

  for (const item of content) {
    const text = asString(get(item, "text"));
    if (text != null && text !== "") {
      blocks.push(textBlock(ordinal++, text));
    }
  }

  return blocks;
}

function parseToolCalls(toolCalls: Json | undefined, startOrdinal: number): NormalizedBlock[] {
  if (!Array.isArray(toolCalls)) {
    return [];
  }
  const blocks: NormalizedBlock[] = [];
  let ordinal = startOrdinal;
  for (const call of toolCalls) {
    // `result` is a UI snapshot. Gemini also writes the canonical
    // functionResponse as its own user record, which supplies linkage and raw.
    const name = normalizedToolName(call);
    const args = get(call, "args") ?? get(call, "function_args") ?? get(call, "input");
    const callId = asString(get(call, "id")) ?? name;
    blocks.push({
      ordinal: ordinal++,
      blockType: "tool_use",
      text: null,
      toolName: name,
      toolUseId: callId,
      toolInput: args,
      toolResult: null,
      isError: null,
    });
  }
  return blocks;
}

function normalizedToolName(call: Json): string | null {
  const name = asString(get(call, "name")) ?? asString(get(call, "function_name"));
  const displayName = asString(get(call, "displayName"));
  const displayMatch = displayName?.match(/^(.+) \((.+) MCP Server\)$/);
  if (displayMatch?.[1] != null && displayMatch[2] != null) {
    return `mcp__${displayMatch[2]}__${displayMatch[1]}`;
  }
  const qualifiedMatch = name?.match(/^mcp_([^_]+)_(.+)$/);
  if (qualifiedMatch?.[1] != null && qualifiedMatch[2] != null) {
    return `mcp__${qualifiedMatch[1]}__${qualifiedMatch[2]}`;
  }
  return name;
}

function toolResultIsError(response: Json | undefined): boolean | null {
  if (!isObject(response)) {
    return null;
  }
  const explicit = get(response, "isError");
  if (explicit === true || explicit === "true") {
    return true;
  }
  const error = get(response, "error");
  if (error != null) {
    return true;
  }
  return false;
}

function parseThoughts(thoughts: Json | undefined, startOrdinal: number): NormalizedBlock[] {
  if (!Array.isArray(thoughts)) {
    return [];
  }
  const blocks: NormalizedBlock[] = [];
  let ordinal = startOrdinal;
  for (const thought of thoughts) {
    const text = asString(get(thought, "text")) ?? asString(thought);
    if (text != null && text !== "") {
      blocks.push({
        ordinal: ordinal++,
        blockType: "thinking",
        text,
        toolName: null,
        toolUseId: null,
        toolInput: undefined,
        toolResult: null,
        isError: null,
      });
    }
  }
  return blocks;
}

function textBlock(ordinal: number, text: string): NormalizedBlock {
  return {
    ordinal,
    blockType: "text",
    text,
    toolName: null,
    toolUseId: null,
    toolInput: undefined,
    toolResult: null,
    isError: null,
  };
}

function otherBlock(ordinal: number, value: Json): NormalizedBlock {
  return {
    ordinal,
    blockType: "other",
    text: canonicalJson(value),
    toolName: null,
    toolUseId: null,
    toolInput: undefined,
    toolResult: null,
    isError: null,
  };
}

function usageFrom(tokens: Json | undefined): TokenUsage {
  if (!isObject(tokens)) {
    return emptyUsage();
  }
  return {
    input: getInteger(tokens, "input"),
    output: getInteger(tokens, "output"),
    cacheRead: getInteger(tokens, "cached"),
    cacheCreation: 0,
    cacheCreation1h: 0,
    reasoning: getInteger(tokens, "thoughts"),
  };
}

function addUsage(a: TokenUsage, b: TokenUsage): TokenUsage {
  return {
    input: a.input + b.input,
    output: a.output + b.output,
    cacheRead: a.cacheRead + b.cacheRead,
    cacheCreation: a.cacheCreation + b.cacheCreation,
    cacheCreation1h: a.cacheCreation1h + b.cacheCreation1h,
    reasoning: a.reasoning + b.reasoning,
  };
}

function get(value: Json | undefined, key: string): Json | undefined {
  if (!isObject(value)) {
    return undefined;
  }
  return value[key];
}

function hasKey(value: Json | undefined, key: string): boolean {
  return isObject(value) && Object.hasOwn(value as JsonObject, key);
}

function isObject(value: Json | undefined): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asString(value: Json | undefined): string | null {
  return typeof value === "string" ? value : null;
}

function getInteger(value: JsonObject, key: string): number {
  const v = value[key];
  return typeof v === "number" && Number.isInteger(v) ? v : 0;
}
