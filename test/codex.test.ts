import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { parseCodexSession } from "../src/sources/codex.ts";

async function fixture(): Promise<string> {
  return await Bun.file(join(import.meta.dir, "..", "fixtures", "codex", "sample.jsonl")).text();
}

describe("parseCodexSession", () => {
  test("parses meta model and conversation", async () => {
    const parsed = parseCodexSession("fallback", await fixture(), new Map());
    const session = parsed.session;
    expect(parsed.issues).toHaveLength(0);
    expect(session.tool).toBe("codex");
    expect(session.sourceSessionId).toBe("sess-codex-1");
    expect(session.cwd).toBe("/Users/dev/proj");
    expect(session.model).toBe("gpt-5.4");
    expect(session.messages).toHaveLength(4);
    expect(session.messages[0]?.role).toBe("user");
    expect(session.messages[1]?.blocks[0]?.blockType).toBe("tool_use");
    expect(session.messages[2]?.role).toBe("tool");
    expect(session.title).toBe("List the open TODOs");
  });

  test("cumulative token count becomes session totals", async () => {
    const parsed = parseCodexSession("fallback", await fixture(), new Map());
    expect(parsed.session.totals.input).toBe(500);
    expect(parsed.session.totals.output).toBe(150);
    expect(parsed.session.totals.cacheRead).toBe(400);
    expect(parsed.session.totals.reasoning).toBe(60);
    expect(parsed.session.reasoningSource).toBe("reported");
    expect(parsed.session.estReasoningTokens).toBe(0);
  });

  test("session index title overrides", async () => {
    const parsed = parseCodexSession(
      "fallback",
      await fixture(),
      new Map([["sess-codex-1", "TODO audit"]]),
    );
    expect(parsed.session.title).toBe("TODO audit");
  });

  test("parses subagent session metadata", () => {
    const content = [
      JSON.stringify({
        type: "session_meta",
        timestamp: "2026-05-01T10:00:00Z",
        payload: {
          id: "child-thread",
          cwd: "/tmp/proj",
          parent_thread_id: "parent-thread",
          agent_nickname: "Ada",
          agent_role: "explorer",
          source: {
            subagent: {
              thread_spawn: {
                parent_thread_id: "parent-thread",
                depth: 1,
                agent_nickname: "Ada",
                agent_role: "explorer",
              },
            },
          },
        },
      }),
    ].join("\n");

    const session = parseCodexSession("fallback", `${content}\n`, new Map()).session;
    expect(session.isSubagent).toBe(true);
    expect(session.rootSourceSessionId).toBe("parent-thread");
    expect(session.agentId).toBe("Ada");
    expect(session.agentType).toBe("explorer");
    expect(session.spawnDepth).toBe(1);
  });

  test("malformed and blank lines and unknown top types", () => {
    const content = [
      "",
      "{oops not json",
      '{"type":"turn_context","timestamp":"2026-05-01T10:00:00Z","payload":{"model":"gpt-5.4","cwd":"/tmp/proj"}}',
      '{"type":"session_meta","timestamp":"2026-05-01T10:00:01Z","payload":{"id":"sx","cli_version":"1.2"}}',
      '{"type":"unknown-top","timestamp":"2026-05-01T10:00:02Z"}',
    ].join("\n");
    const parsed = parseCodexSession("fallback", `${content}\n`, new Map());
    expect(parsed.issues).toHaveLength(1);
    expect(parsed.issues[0]?.lineNo).toBe(2);
    const session = parsed.session;
    expect(session.sourceSessionId).toBe("sx");
    expect(session.cwd).toBe("/tmp/proj");
    expect(session.model).toBe("gpt-5.4");
    expect(session.cliVersion).toBe("1.2");
    expect(session.startedAt).toBe("2026-05-01T10:00:00Z");
    expect(session.endedAt).toBe("2026-05-01T10:00:02Z");
  });

  test("response item variants cover every block kind", () => {
    const content = [
      '{"type":"response_item","timestamp":"2026-05-01T10:00:00Z","payload":{"type":"reasoning","summary":[],"content":[{"text":"deep thought"}]}}',
      '{"type":"response_item","payload":{"type":"web_search_call"}}',
      '{"type":"response_item","payload":{"type":"mystery","foo":1}}',
      '{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"plain"}}',
      '{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c2","output":{"k":1}}}',
      '{"type":"response_item","payload":{"type":"tool_search_output","call_id":"c3"}}',
      '{"type":"response_item","payload":{"type":"message","role":"system","content":"sys note"}}',
      '{"type":"response_item","payload":{"type":"message","role":"assistant","content":42}}',
      '{"type":"response_item","payload":{"type":"custom_tool_call","name":"do_thing","call_id":"k1","input":{"a":1}}}',
    ].join("\n");
    const messages = parseCodexSession("fallback", `${content}\n`, new Map()).session.messages;
    expect(messages[0]?.role).toBe("assistant");
    expect(messages[0]?.blocks[0]?.blockType).toBe("thinking");
    expect(messages[0]?.blocks[0]?.text).toBe("deep thought");
    expect(messages[1]?.blocks[0]?.blockType).toBe("web_search");
    expect(messages[2]?.blocks[0]?.blockType).toBe("other");
    expect(messages[3]?.blocks[0]?.toolResult).toBe("plain");
    expect(messages[4]?.blocks[0]?.toolResult).toBe('{"k":1}');
    expect(messages[5]?.blocks[0]?.toolResult).toBe("");
    expect(messages[6]?.role).toBe("system");
    expect(messages[6]?.blocks[0]?.text).toBe("sys note");
    expect(messages[7]?.blocks[0]?.text).toBe("");
    expect(messages[8]?.blocks[0]?.blockType).toBe("tool_use");
    expect(messages[8]?.blocks[0]?.toolName).toBe("do_thing");
    expect(messages[8]?.blocks[0]?.toolInput).toEqual({ a: 1 });
  });
});
