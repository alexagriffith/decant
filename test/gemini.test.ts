import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseGeminiSession } from "../src/sources/gemini.ts";

function fixture(name: string): string {
  return readFileSync(join(import.meta.dir, "..", "fixtures", "gemini", name), "utf8");
}

describe("parseGeminiSession", () => {
  test("parses messages, usage, thoughts, and linked tool calls", () => {
    const parsed = parseGeminiSession(
      "fallback",
      fixture("sample.jsonl"),
      "/synthetic/gemini-project",
    );
    const { session } = parsed;

    expect(parsed.issues).toEqual([]);
    expect(session).toMatchObject({
      tool: "gemini",
      sourceSessionId: "gemini-synthetic",
      projectPath: "/synthetic/gemini-project",
      cwd: "/synthetic/gemini-project",
      title: "Inspect the synthetic example.",
      model: "example-model",
      startedAt: "2026-06-01T10:00:00.000Z",
      endedAt: "2026-06-01T10:00:04.000Z",
      reasoningSource: "reported",
      totals: {
        input: 15,
        output: 10,
        cacheRead: 4,
        cacheCreation: 0,
        cacheCreation1h: 0,
        reasoning: 2,
      },
    });
    expect(session.messages.map((message) => message.role)).toEqual([
      "user",
      "assistant",
      "tool",
      "assistant",
    ]);

    const call = session.messages[1]?.blocks.find((block) => block.blockType === "tool_use");
    const result = session.messages[2]?.blocks.find((block) => block.blockType === "tool_result");
    expect(call).toMatchObject({
      ordinal: 1,
      toolName: "read_file",
      toolUseId: "call-1",
      toolInput: { file_path: "/tmp/example.txt" },
    });
    expect(result).toMatchObject({
      ordinal: 0,
      toolUseId: "call-1",
      toolResult: '{"output":"example"}',
      isError: false,
    });
    expect(session.messages[1]?.blocks.map((block) => block.ordinal)).toEqual([0, 1, 2]);
  });

  test("continues after malformed lines and reports unknown record types once", () => {
    const content = [
      JSON.stringify({
        sessionId: "gemini-drift",
        startTime: "2026-06-02T10:00:00.000Z",
      }),
      JSON.stringify({
        id: "user-1",
        timestamp: "2026-06-02T10:00:01.000Z",
        type: "user",
        content: "Continue after malformed input.",
      }),
      '{"broken":',
      JSON.stringify({ type: "future_record", timestamp: "2026-06-02T10:00:02.000Z" }),
      JSON.stringify({ type: "future_record", timestamp: "2026-06-02T10:00:02.500Z" }),
      JSON.stringify({
        id: "assistant-1",
        timestamp: "2026-06-02T10:00:03.000Z",
        type: "gemini",
        model: "example-model",
        content: [{ text: "Parsing continued." }],
        tokens: { input: 2, output: 2, cached: 0, thoughts: 0 },
      }),
    ].join("\n");
    const parsed = parseGeminiSession("fallback", content, null);

    expect(parsed.session.sourceSessionId).toBe("gemini-drift");
    expect(parsed.session.messages).toHaveLength(2);
    expect(parsed.issues).toHaveLength(2);
    expect(parsed.issues[0]).toMatchObject({ code: "unparsed_line", lineNo: 3 });
    expect(parsed.issues[1]).toMatchObject({
      code: "unknown_record_type",
      lineNo: 4,
      error: 'unknown record type "future_record" on 2 line(s); ignored',
    });
  });

  test("skips injected session context and marks error tool results", () => {
    const content = [
      JSON.stringify({ sessionId: "gemini-error", startTime: "2026-06-03T10:00:00.000Z" }),
      JSON.stringify({
        id: "context",
        timestamp: "2026-06-03T10:00:01.000Z",
        type: "user",
        content: [
          { text: "  <session_context>synthetic boilerplate</session_context>" },
          { text: "<hook_context>synthetic hook boilerplate</hook_context>" },
        ],
      }),
      JSON.stringify({
        id: "call",
        timestamp: "2026-06-03T10:00:02.000Z",
        type: "gemini",
        model: "example-model",
        content: [],
        toolCalls: [{ id: "call-error", name: "read_file", args: {} }],
      }),
      JSON.stringify({
        id: "result",
        timestamp: "2026-06-03T10:00:03.000Z",
        type: "user",
        content: [
          {
            functionResponse: {
              id: "call-error",
              name: "read_file",
              response: { error: "synthetic failure" },
            },
          },
        ],
      }),
    ].join("\n");

    const parsed = parseGeminiSession("fallback", content, null);
    expect(parsed.issues).toEqual([]);
    expect(parsed.session.messages).toHaveLength(2);
    expect(parsed.session.messages[1]?.role).toBe("tool");
    expect(parsed.session.messages[1]?.blocks[0]).toMatchObject({
      toolUseId: "call-error",
      isError: true,
    });
  });

  test("does not mark explicit false or empty error values as failures", () => {
    const content = [
      JSON.stringify({ sessionId: "gemini-success", startTime: "2026-06-03T10:00:00.000Z" }),
      JSON.stringify({
        id: "calls",
        timestamp: "2026-06-03T10:00:01.000Z",
        type: "gemini",
        model: "example-model",
        content: [],
        toolCalls: [
          { id: "call-false", name: "read_file", args: {} },
          { id: "call-empty", name: "read_file", args: {} },
        ],
      }),
      JSON.stringify({
        id: "results",
        timestamp: "2026-06-03T10:00:02.000Z",
        type: "user",
        content: [
          {
            functionResponse: {
              id: "call-false",
              name: "read_file",
              response: { error: false },
            },
          },
          {
            functionResponse: {
              id: "call-empty",
              name: "read_file",
              response: { error: "" },
            },
          },
        ],
      }),
    ].join("\n");

    const parsed = parseGeminiSession("fallback", content, null);
    expect(parsed.issues).toEqual([]);
    expect(parsed.session.messages[1]?.blocks).toHaveLength(2);
    expect(parsed.session.messages[1]?.blocks.map((block) => block.isError)).toEqual([
      false,
      false,
    ]);
  });

  test("retains repeated tool responses without duplicating their linkage", () => {
    const content = [
      JSON.stringify({ sessionId: "gemini-repeat", startTime: "2026-06-04T10:00:00.000Z" }),
      JSON.stringify({
        id: "call",
        timestamp: "2026-06-04T10:00:01.000Z",
        type: "gemini",
        model: "example-model",
        toolCalls: [{ id: "call-1", name: "read_file", args: {} }],
      }),
      JSON.stringify({
        id: "result-1",
        timestamp: "2026-06-04T10:00:02.000Z",
        type: "user",
        content: [
          {
            functionResponse: {
              id: "call-1",
              name: "read_file",
              response: { output: "complete synthetic output" },
            },
          },
        ],
      }),
      JSON.stringify({
        id: "result-2",
        timestamp: "2026-06-04T10:00:03.000Z",
        type: "user",
        content: [
          {
            functionResponse: {
              id: "call-1",
              name: "read_file",
              response: { output: "short output" },
            },
          },
        ],
      }),
      JSON.stringify({
        id: "info-1",
        timestamp: "2026-06-04T10:00:04.000Z",
        type: "info",
        content: "Synthetic informational record.",
      }),
    ].join("\n");

    const parsed = parseGeminiSession("fallback", content, null);
    expect(parsed.issues).toEqual([]);
    expect(parsed.session.messages.map((message) => message.role)).toEqual([
      "assistant",
      "tool",
      "other",
      "other",
    ]);
    expect(parsed.session.messages[2]?.blocks[0]).toMatchObject({
      blockType: "other",
      toolUseId: null,
    });
    expect(parsed.session.messages[2]?.raw).toMatchObject({ id: "result-2" });
  });

  test("normalizes Gemini MCP display names", () => {
    const parsed = parseGeminiSession("fallback", fixture("mcp.jsonl"), null);
    const call = parsed.session.messages[1]?.blocks.find((block) => block.blockType === "tool_use");

    expect(parsed.issues).toEqual([]);
    expect(call).toMatchObject({
      toolName: "mcp__docs__lookup",
      toolUseId: "mcp-call-1",
    });
  });

  test("normalizes Gemini MCP internal names", () => {
    const content = [
      JSON.stringify({
        sessionId: "synthetic-mcp-internal",
        startTime: "2026-06-05T10:00:00.000Z",
      }),
      JSON.stringify({
        id: "assistant-1",
        timestamp: "2026-06-05T10:00:01.000Z",
        type: "gemini",
        content: "Calling the synthetic MCP tool.",
        toolCalls: [
          {
            id: "mcp-call-2",
            name: "mcp_docs_search_pages",
            args: { query: "synthetic" },
          },
        ],
      }),
      JSON.stringify({
        id: "result-1",
        timestamp: "2026-06-05T10:00:02.000Z",
        type: "user",
        content: [
          {
            functionResponse: {
              id: "mcp-call-2",
              name: "mcp_docs_search_pages",
              response: { output: "Synthetic result." },
            },
          },
        ],
      }),
    ].join("\n");

    const parsed = parseGeminiSession("fallback", content, null);
    const call = parsed.session.messages[0]?.blocks.find((block) => block.blockType === "tool_use");

    expect(parsed.issues).toEqual([]);
    expect(call).toMatchObject({
      toolName: "mcp__docs__search_pages",
      toolUseId: "mcp-call-2",
    });
  });
});
