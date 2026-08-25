import type { Database } from "bun:sqlite";
import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openDb } from "../src/db.ts";
import { refreshDerivedMetadata } from "../src/derived.ts";
import { upsertSession } from "../src/ingest.ts";
import { setSessionUserState } from "../src/session-user-state.ts";
import { parseClaudeSession } from "../src/sources/claude.ts";
import { parseCodexSession } from "../src/sources/codex.ts";
import type { PhaseAmounts, TokenEconomics } from "../src/token-economics.ts";
import {
  aggregateEconomicsVectors,
  computeSessionEconomicsVectors,
  economicsVectorMatchesFilter,
  materializeMissingSessionEconomics,
  SESSION_ECONOMICS_FORMAT_VERSION,
  tokenEconomics,
  tokenEconomicsForSession,
} from "../src/token-economics.ts";

const workDir = mkdtempSync(join(tmpdir(), "decant-token-economics-test-"));
afterAll(() => rmSync(workDir, { recursive: true, force: true }));

let dbCounter = 0;
function freshDb(): Database {
  dbCounter += 1;
  return openDb(join(workDir, `tokens-${dbCounter}.db`));
}

function fixture(tool: "claude" | "codex", name: string): string {
  return readFileSync(join(import.meta.dir, "..", "fixtures", tool, name), "utf8");
}

describe("token economics", () => {
  test("allocates generation, context-window footprint, tool calls, and cost by bucket", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseCodexSession("sess-enr-codex", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/codex.jsonl",
      1,
      2,
      "codex",
    );

    const economics = tokenEconomics(db);
    expect(economics.buckets.map((row) => row.bucket)).toEqual([
      "context",
      "planning",
      "code",
      "communicating",
    ]);
    expect(economics.totals.generation_tokens).toBeGreaterThan(0);
    expect(economics.totals.context_window_tokens).toBeGreaterThan(
      economics.totals.generation_tokens,
    );
    expect(economics.totals.estimated_cost_usd).toBeGreaterThan(0);
    expect(economics.buckets.find((row) => row.bucket === "context")?.tool_calls).toBeGreaterThan(
      0,
    );
    expect(economics.buckets.find((row) => row.bucket === "code")?.tool_calls).toBeGreaterThan(0);
    expect(economics.buckets.reduce((sum, row) => sum + row.estimated_cost_usd, 0)).toBeCloseTo(
      economics.totals.estimated_cost_usd,
      12,
    );
    db.close();
  });

  test("splits each bucket into orientation/implementation phases that sum to the whole", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );

    const economics = tokenEconomics(db);
    // Every bucket carries a phase split whose parts sum back to the bucket total.
    for (const row of economics.buckets) {
      expect(row.phases).toBeDefined();
      const { orientation, implementation } = row.phases as NonNullable<typeof row.phases>;
      expect(orientation.generation_tokens + implementation.generation_tokens).toBe(
        row.generation_tokens,
      );
      expect(orientation.context_window_tokens + implementation.context_window_tokens).toBe(
        row.context_window_tokens,
      );
      expect(orientation.estimated_cost_usd + implementation.estimated_cost_usd).toBeCloseTo(
        row.estimated_cost_usd,
        12,
      );
      expect(orientation.estimated_cost_usd).toBeGreaterThanOrEqual(0);
      expect(implementation.estimated_cost_usd).toBeGreaterThanOrEqual(0);
    }
    // Totals phase split sums to the run total.
    const phases = economics.totals.phases as NonNullable<typeof economics.totals.phases>;
    expect(phases).toBeDefined();
    expect(
      phases.orientation.estimated_cost_usd + phases.implementation.estimated_cost_usd,
    ).toBeCloseTo(economics.totals.estimated_cost_usd, 12);
    // The fixture reads before it edits, so some context is gathered in orientation.
    const context = economics.buckets.find((row) => row.bucket === "context");
    expect(context?.phases?.orientation.context_window_tokens).toBeGreaterThan(0);

    // The per-session path uses the same ordered allocator while retaining its
    // billed-input Window total.
    const scoped = tokenEconomicsForSession(db, sessionId);
    expect(scoped?.totals.generation_tokens).toBe(economics.totals.generation_tokens);
    const billedInput = (
      db
        .query(
          `SELECT total_input_tokens + total_cache_read_tokens + total_cache_creation_tokens AS tokens
           FROM session WHERE id = ?1`,
        )
        .get(sessionId) as { tokens: number }
    ).tokens;
    expect(
      Math.abs(
        (scoped?.totals.context_window_tokens ?? 0) -
          economics.totals.context_window_tokens -
          billedInput,
      ),
    ).toBeLessThanOrEqual(2);
    expect(scoped?.totals.estimated_cost_usd).toBe(economics.totals.estimated_cost_usd);
    expect(scoped?.buckets.every((row) => row.phases !== undefined)).toBe(true);
    db.close();
  });

  test("reports a cost share for each phase that sums to one", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );

    const economics = tokenEconomics(db);
    const phases = economics.totals.phases;
    expect(phases).toBeDefined();
    const orientation = phases?.orientation.cost_share ?? 0;
    const implementation = phases?.implementation.cost_share ?? 0;
    expect(orientation + implementation).toBeCloseTo(1, 12);
    // Totals carry the archive-wide share, not the sum of the per-bucket ones.
    expect(orientation).toBeLessThanOrEqual(1);
    for (const row of economics.buckets) {
      const bucketShares =
        (row.phases?.orientation.cost_share ?? 0) + (row.phases?.implementation.cost_share ?? 0);
      // A bucket with no spend has no share to divide, so it stays at zero.
      expect(bucketShares).toBeCloseTo(row.estimated_cost_usd > 0 ? 1 : 0, 12);
    }
    db.close();
  });

  test("gives an edit-free session a full orientation share", () => {
    const db = freshDb();
    // Hand-written: a session that only reads, so the first-edit boundary is
    // never crossed and every dollar stays in orientation.
    const content = [
      JSON.stringify({
        type: "user",
        uuid: "u1",
        parentUuid: null,
        timestamp: "2026-05-06T09:00:00.000Z",
        message: { role: "user", content: "Explain the auth module" },
      }),
      JSON.stringify({
        type: "assistant",
        uuid: "a1",
        parentUuid: "u1",
        timestamp: "2026-05-06T09:00:30.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          stop_reason: "tool_use",
          usage: { input_tokens: 800, output_tokens: 120 },
          content: [
            { type: "text", text: "Reading the module." },
            {
              type: "tool_use",
              id: "toolu_read",
              name: "Read",
              input: { file_path: "/Users/dev/proj/src/auth.rs" },
            },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        uuid: "u2",
        parentUuid: "a1",
        timestamp: "2026-05-06T09:00:50.000Z",
        message: {
          role: "user",
          content: [{ type: "tool_result", tool_use_id: "toolu_read", content: "fn auth() {}" }],
        },
      }),
      JSON.stringify({
        type: "assistant",
        uuid: "a2",
        parentUuid: "u2",
        timestamp: "2026-05-06T09:01:20.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 950, output_tokens: 300 },
          content: [{ type: "text", text: "It authenticates the caller and returns a bool." }],
        },
      }),
    ].join("\n");
    upsertSession(
      db,
      parseClaudeSession("sess-no-edit", `${content}\n`),
      "/x/no-edit.jsonl",
      1,
      2,
      "claude",
    );

    const phases = tokenEconomics(db).totals.phases;
    expect(phases?.orientation.cost_share).toBeCloseTo(1, 12);
    expect(phases?.implementation.cost_share).toBeCloseTo(0, 12);
    db.close();
  });

  test("attributes wall-clock time to activity buckets and phases", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );

    const economics = tokenEconomics(db);
    // This synthetic fixture has one blockless system message. The four agent
    // buckets plus explicit user wait approximately reconcile to the broader
    // active_seconds chain, within rounding.
    const activeSeconds = (
      db.query("SELECT active_seconds FROM session WHERE id = ?1").get(sessionId) as {
        active_seconds: number;
      }
    ).active_seconds;
    expect(activeSeconds).toBeGreaterThan(0);
    expect(economics.totals.active_ms).toBeGreaterThan(0);
    expect(economics.totals.waiting_on_user_ms).toBe(330_000);
    expect(economics.totals.attributed_ms).toBeCloseTo(activeSeconds * 1000, -2);
    // The fixture spends 30s generating mutating tool calls and 30s executing
    // an Edit result; both portions belong to code.
    expect(economics.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(60_000);

    // Buckets' time sums to the total, and each bucket's phase split sums back
    // to the bucket (rounding can drift the two rounded halves by <=1ms).
    const bucketSum = economics.buckets.reduce((sum, row) => sum + row.active_ms, 0);
    expect(bucketSum).toBe(economics.totals.active_ms);
    for (const row of economics.buckets) {
      const { orientation, implementation } = row.phases as NonNullable<typeof row.phases>;
      expect(orientation.active_ms).toBeGreaterThanOrEqual(0);
      expect(implementation.active_ms).toBeGreaterThanOrEqual(0);
      expect(
        Math.abs(orientation.active_ms + implementation.active_ms - row.active_ms),
      ).toBeLessThanOrEqual(1);
    }
    const phases = economics.totals.phases as NonNullable<typeof economics.totals.phases>;
    expect(phases.orientation.active_ms + phases.implementation.active_ms).toBe(
      economics.totals.active_ms,
    );
    // The fixture edits only after orienting, so edit time lands in implementation.
    expect(
      economics.buckets.find((row) => row.bucket === "code")?.phases?.orientation.active_ms,
    ).toBe(0);
    // The scoped result uses the same block-level allocation for generated
    // messages and time, while allocating the full billed input window.
    const scoped = tokenEconomicsForSession(db, sessionId);
    expect(scoped?.totals.active_ms).toBe(economics.totals.active_ms);
    expect(scoped?.totals.waiting_on_user_ms).toBe(economics.totals.waiting_on_user_ms);
    expect(scoped?.totals.attributed_ms).toBe(economics.totals.attributed_ms);
    expect(scoped?.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(60_000);
    const communicating = scoped?.buckets.find((row) => row.bucket === "communicating");
    expect(communicating?.active_ms).toBeGreaterThan(0);
    expect(communicating?.generation_tokens).toBeGreaterThan(0);
    expect(communicating?.estimated_cost_usd).toBeGreaterThan(0);
    expect(communicating?.sessions).toBe(1);
    db.close();
  });

  test("an empty exclusion set leaves every reported number unchanged", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );

    const before = tokenEconomics(db);
    const after = tokenEconomics(db, null, { excludeMcpServers: [] });
    // The default exclusion set is empty, so the headline numbers a report
    // already prints must not move when a caller opts into the parameter.
    expect(after.totals.phases).toEqual(before.totals.phases);
    expect(after.buckets).toEqual(before.buckets);
    // by_server ships regardless, so any reader can re-derive the split for an
    // allowlist Decant did not pick.
    expect(after.totals.retrieval?.excluded_servers).toEqual([]);
    expect(before.totals.retrieval?.excluded_servers).toEqual([]);
    expect(Object.keys(after.totals.retrieval?.by_server ?? {})).toContain("github");
    db.close();
  });

  test("reports orientation retrieval separately without moving it out of context", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );

    const plain = tokenEconomics(db);
    const split = tokenEconomics(db, null, { excludeMcpServers: ["github"] });
    // The MCP call still lives in context, and orientation still means all
    // pre-edit spend. Only the extra block below reports the slice.
    expect(split.buckets).toEqual(plain.buckets);
    expect(split.totals.phases).toEqual(plain.totals.phases);
    const retrieval = split.totals.retrieval as NonNullable<typeof split.totals.retrieval>;
    expect(retrieval.attributed.orientation.estimated_cost_usd).toBeGreaterThan(0);
    expect(retrieval.attributed.orientation.context_window_tokens).toBeGreaterThan(0);
    expect(retrieval.remainder.orientation.estimated_cost_usd).toBeGreaterThan(0);
    // The retrieval slice is a subset of context, never an extra bucket.
    const context = split.buckets.find((row) => row.bucket === "context");
    expect(retrieval.attributed.orientation.context_window_tokens).toBeLessThanOrEqual(
      context?.phases?.orientation.context_window_tokens ?? 0,
    );
    db.close();
  });

  test("primary, retrieval, and net orientation reconcile", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseCodexSession("sess-mcp-codex", fixture("codex", "mcp.jsonl"), new Map()),
      "/x/codex-mcp.jsonl",
      1,
      2,
      "codex",
    );
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );

    const economics = tokenEconomics(db, null, {
      excludeMcpServers: ["github", "dosu", "context7"],
    });
    const { phases, retrieval } = economics.totals as {
      phases: NonNullable<TokenEconomics["totals"]["phases"]>;
      retrieval: NonNullable<TokenEconomics["totals"]["retrieval"]>;
    };
    for (const phase of ["orientation", "implementation"] as const) {
      const whole = phases[phase];
      const part = retrieval.attributed[phase];
      const rest = retrieval.remainder[phase];
      expect(part.generation_tokens + rest.generation_tokens).toBe(whole.generation_tokens);
      expect(part.context_window_tokens + rest.context_window_tokens).toBe(
        whole.context_window_tokens,
      );
      expect(part.active_ms + rest.active_ms).toBe(whole.active_ms);
      expect(part.estimated_cost_usd + rest.estimated_cost_usd).toBeCloseTo(
        whole.estimated_cost_usd,
        12,
      );
    }
    // remainder's cost_share is the corrected headline: the orientation share
    // once the named servers' spend is taken out of both halves.
    expect(retrieval.remainder.orientation.cost_share).toBeLessThan(phases.orientation.cost_share);
    expect(
      retrieval.remainder.orientation.cost_share + retrieval.remainder.implementation.cost_share,
    ).toBeCloseTo(1, 12);
    db.close();
  });

  test("decomposes retrieval by server so any exclusion set can be re-derived", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseCodexSession("sess-mcp-codex", fixture("codex", "mcp.jsonl"), new Map()),
      "/x/codex-mcp.jsonl",
      1,
      2,
      "codex",
    );

    // by_server ships whether or not anything was named, and does not move when
    // the exclusion set changes -- that is what makes an outside allowlist
    // derivable from a published payload.
    const none = tokenEconomics(db).totals.retrieval as NonNullable<
      TokenEconomics["totals"]["retrieval"]
    >;
    const some = tokenEconomics(db, null, { excludeMcpServers: ["exa"] }).totals
      .retrieval as NonNullable<TokenEconomics["totals"]["retrieval"]>;
    expect(some.by_server).toEqual(none.by_server);
    expect(Object.keys(none.by_server).sort()).toEqual([
      "codex_apps",
      "context7",
      "dosu",
      "exa",
      "github",
      "node_repl",
    ]);
    expect(none.attributed.orientation.estimated_cost_usd).toBe(0);

    const chosen = ["dosu", "github", "context7"];
    const reported = tokenEconomics(db, null, { excludeMcpServers: chosen }).totals
      .retrieval as NonNullable<TokenEconomics["totals"]["retrieval"]>;
    const rederived = chosen.reduce(
      (sum, server) => sum + (none.by_server[server]?.orientation.estimated_cost_usd ?? 0),
      0,
    );
    expect(reported.attributed.orientation.estimated_cost_usd).toBeCloseTo(rederived, 12);
    db.close();
  });

  test("excludes only the named servers", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseCodexSession("sess-mcp-codex", fixture("codex", "mcp.jsonl"), new Map()),
      "/x/codex-mcp.jsonl",
      1,
      2,
      "codex",
    );

    const retrieval = tokenEconomics(db, null, {
      excludeMcpServers: ["dosu", "dosu", "", "not-installed"],
    }).totals.retrieval as NonNullable<TokenEconomics["totals"]["retrieval"]>;
    // Duplicates and blanks collapse, an unknown slug contributes nothing, and
    // the set is echoed back so a published figure carries it.
    expect(retrieval.excluded_servers).toEqual(["dosu", "not-installed"]);
    expect(retrieval.attributed.orientation).toEqual(
      retrieval.by_server.dosu?.orientation as PhaseAmounts,
    );
    // The unqualified Codex namespace (`dosu__unqualified_tool`, no `mcp__`
    // prefix) is a builtin, not another registration of dosu. Pinning exact
    // volume is what makes that load-bearing: a naive `split("__")[0]` parser
    // yields the key "dosu" too, so an absent-key assertion cannot fail under
    // either implementation, and the window cannot discriminate because that
    // call has no output bytes. Its 1000ms of latency is the only witness.
    //   window: (16 + 57) result bytes / 4 chars-per-token = 18
    //   active: the two mcp__dosu__read_knowledge calls only, not 6000
    expect(retrieval.by_server.dosu?.orientation.context_window_tokens).toBe(18);
    expect(retrieval.by_server.dosu?.orientation.active_ms).toBe(5000);
    expect(Object.keys(retrieval.by_server)).not.toContain("dosu__unqualified_tool");
    expect(retrieval.by_server.exa?.orientation.context_window_tokens).toBeGreaterThan(0);
    db.close();
  });

  test("attributes retrieval that happens after the first edit to implementation", () => {
    const db = freshDb();
    const content = [
      JSON.stringify({
        type: "assistant",
        uuid: "a1",
        parentUuid: null,
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:00.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 400, output_tokens: 100 },
          content: [
            { type: "tool_use", id: "toolu_mcp_a", name: "mcp__dosu__read_knowledge", input: {} },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        uuid: "u1",
        parentUuid: "a1",
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:10.000Z",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "toolu_mcp_a", content: "prior decisions here" },
          ],
        },
      }),
      JSON.stringify({
        type: "assistant",
        uuid: "a2",
        parentUuid: "u1",
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:20.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 500, output_tokens: 120 },
          content: [
            {
              type: "tool_use",
              id: "toolu_edit",
              name: "Edit",
              input: { file_path: "/proj/src/auth.rs", old_string: "a", new_string: "b" },
            },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        uuid: "u2",
        parentUuid: "a2",
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:30.000Z",
        message: {
          role: "user",
          content: [{ type: "tool_result", tool_use_id: "toolu_edit", content: "ok" }],
        },
      }),
      JSON.stringify({
        type: "assistant",
        uuid: "a3",
        parentUuid: "u2",
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:40.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 600, output_tokens: 140 },
          content: [
            { type: "tool_use", id: "toolu_mcp_b", name: "mcp__dosu__read_knowledge", input: {} },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        uuid: "u3",
        parentUuid: "a3",
        sessionId: "sess-mcp-phases",
        timestamp: "2026-05-07T09:00:50.000Z",
        message: {
          role: "user",
          content: [
            {
              type: "tool_result",
              tool_use_id: "toolu_mcp_b",
              content: "a much longer follow-up answer than the first one",
            },
          ],
        },
      }),
    ].join("\n");
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-phases", `${content}\n`),
      "/x/mcp-phases.jsonl",
      1,
      2,
      "claude",
    );

    const retrieval = tokenEconomics(db, null, { excludeMcpServers: ["dosu"] }).totals
      .retrieval as NonNullable<TokenEconomics["totals"]["retrieval"]>;
    // The phase split is real: the same server lands on both sides of the
    // first edit, so retrieval is not silently all-orientation.
    expect(retrieval.attributed.orientation.context_window_tokens).toBeGreaterThan(0);
    expect(retrieval.attributed.implementation.context_window_tokens).toBeGreaterThan(0);
    expect(retrieval.attributed.orientation.generation_tokens).toBeGreaterThan(0);
    expect(retrieval.attributed.implementation.generation_tokens).toBeGreaterThan(0);
    db.close();
  });

  test("per-session retrieval reconciles against the billed-input window", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );

    const scoped = tokenEconomicsForSession(db, sessionId, {
      excludeMcpServers: ["github"],
    }) as TokenEconomics;
    const { phases, retrieval } = scoped.totals as {
      phases: NonNullable<TokenEconomics["totals"]["phases"]>;
      retrieval: NonNullable<TokenEconomics["totals"]["retrieval"]>;
    };
    for (const phase of ["orientation", "implementation"] as const) {
      expect(
        retrieval.attributed[phase].context_window_tokens +
          retrieval.remainder[phase].context_window_tokens,
      ).toBe(phases[phase].context_window_tokens);
      expect(
        retrieval.attributed[phase].estimated_cost_usd +
          retrieval.remainder[phase].estimated_cost_usd,
      ).toBeCloseTo(phases[phase].estimated_cost_usd, 12);
    }
    // A session panel inflates every window by its share of the run's billed
    // input. If the retrieval slice skipped that allocation it would be priced
    // against a smaller window than the phases it decomposes, and this ratio
    // would collapse toward the un-inflated archive-wide one.
    const archive = tokenEconomics(db, null, { excludeMcpServers: ["github"] }).totals
      .retrieval as NonNullable<TokenEconomics["totals"]["retrieval"]>;
    const scopedWindow = retrieval.attributed.orientation.context_window_tokens;
    const archiveWindow = archive.attributed.orientation.context_window_tokens;
    expect(scopedWindow).toBeGreaterThan(archiveWindow);
    expect(scopedWindow / phases.orientation.context_window_tokens).toBeCloseTo(
      archiveWindow /
        (tokenEconomics(db).totals.phases as NonNullable<TokenEconomics["totals"]["phases"]>)
          .orientation.context_window_tokens,
      2,
    );
    db.close();
  });

  test("attributes Codex MCP calls to their server", () => {
    const db = freshDb();
    // Codex reports MCP work as event-only mcp_tool_call_end records with no
    // matching response_item, so this path is the only one a Codex archive has.
    const content = [
      JSON.stringify({
        type: "session_meta",
        timestamp: "2026-05-08T09:00:00.000Z",
        payload: {
          id: "sess-codex-event-mcp",
          cwd: "/Users/dev/proj",
          originator: "codex_cli_rs",
          cli_version: "0.116.0",
          source: "cli",
          model_provider: "openai",
        },
      }),
      JSON.stringify({
        type: "turn_context",
        timestamp: "2026-05-08T09:00:01.000Z",
        payload: { cwd: "/Users/dev/proj", model: "gpt-5.4", effort: "medium" },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-08T09:00:02.000Z",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "Check durable project context" }],
        },
      }),
      JSON.stringify({
        type: "event_msg",
        timestamp: "2026-05-08T09:00:06.000Z",
        payload: {
          type: "mcp_tool_call_end",
          call_id: "77777777-8888-4999-8aaa-bbbbbbbbbbbb",
          invocation: { server: "dosu", tool: "read_knowledge", arguments: { query: "synthetic" } },
          duration: { secs: 2, nanos: 0 },
          result: {
            Ok: { content: [{ type: "text", text: "synthetic knowledge for the event path" }] },
          },
        },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-08T09:00:08.000Z",
        payload: {
          type: "message",
          role: "assistant",
          content: [{ type: "output_text", text: "Context checked." }],
        },
      }),
      // Without this the session has no output tokens, and the Codex
      // generation path below never runs -- the test would pin window and
      // latency only, and silently pass with generation attribution removed.
      JSON.stringify({
        type: "event_msg",
        timestamp: "2026-05-08T09:00:09.000Z",
        payload: {
          type: "token_count",
          info: {
            total_token_usage: {
              input_tokens: 900,
              cached_input_tokens: 300,
              output_tokens: 400,
              reasoning_output_tokens: 40,
              total_tokens: 1340,
            },
          },
        },
      }),
    ].join("\n");
    upsertSession(
      db,
      parseCodexSession("sess-codex-event-mcp", `${content}\n`, new Map()),
      "/x/codex-event-mcp.jsonl",
      1,
      2,
      "codex",
    );

    const economics = tokenEconomics(db, null, { excludeMcpServers: ["dosu"] });
    const { phases, retrieval } = economics.totals as {
      phases: NonNullable<TokenEconomics["totals"]["phases"]>;
      retrieval: NonNullable<TokenEconomics["totals"]["retrieval"]>;
    };
    // Codex normalizes the invocation to mcp__<server>__<tool>, so the same
    // parser that reads Claude names attributes it.
    expect(Object.keys(retrieval.by_server)).toEqual(["dosu"]);
    expect(retrieval.attributed.orientation.active_ms).toBeGreaterThan(0);
    expect(retrieval.attributed.orientation.context_window_tokens).toBeGreaterThan(0);
    // Codex takes a different generation path from Claude: allocateGeneration
    // short-circuits on session.tool === "codex" and routes the aggregate lump
    // through distribute(), so distributeVisible() -- the path the Claude tests
    // cover -- never runs here. Without this assertion, dropping server
    // attribution from distribute() leaves the whole suite green while Codex
    // MCP generation silently reports zero.
    expect(retrieval.attributed.orientation.generation_tokens).toBeGreaterThan(0);
    // The assistant's own text is not retrieval, so both sides stay non-zero.
    expect(retrieval.remainder.orientation.generation_tokens).toBeGreaterThan(0);
    for (const phase of ["orientation", "implementation"] as const) {
      expect(
        retrieval.attributed[phase].generation_tokens +
          retrieval.remainder[phase].generation_tokens,
      ).toBe(phases[phase].generation_tokens);
      expect(
        retrieval.attributed[phase].context_window_tokens +
          retrieval.remainder[phase].context_window_tokens,
      ).toBe(phases[phase].context_window_tokens);
    }
    db.close();
  });

  test("persists versioned vectors and serves economics without scanning transcript rows", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseClaudeSession("sess-mcp-claude", fixture("claude", "mcp.jsonl")),
      "/x/mcp.jsonl",
      1,
      2,
      "claude",
    );
    const expected = tokenEconomicsForSession(db, sessionId);
    const expectedAggregate = tokenEconomics(db);
    const expectedRetrieval = tokenEconomics(db, null, { excludeMcpServers: ["github"] }).totals
      .retrieval;
    expect(expectedRetrieval?.attributed.orientation.estimated_cost_usd).toBeGreaterThan(0);
    const stored = db
      .query(
        "SELECT format_version, json_valid(vector_json) AS valid FROM session_economics WHERE session_id = ?1",
      )
      .get(sessionId) as { format_version: number; valid: number };
    expect(stored).toEqual({ format_version: SESSION_ECONOMICS_FORMAT_VERSION, valid: 1 });

    db.exec(`
      DELETE FROM file_ref;
      DELETE FROM tool_call;
      DELETE FROM block;
      DELETE FROM message;
    `);
    expect(tokenEconomicsForSession(db, sessionId)).toEqual(expected);
    expect(tokenEconomics(db)).toEqual(expectedAggregate);
    // The per-server slice is genuinely persisted, not re-derived from the
    // tool_call rows this test just deleted.
    expect(tokenEconomics(db, null, { excludeMcpServers: ["github"] }).totals.retrieval).toEqual(
      expectedRetrieval as NonNullable<typeof expectedRetrieval>,
    );
    db.close();
  });

  test("server cache warmup never falls back to an uncached transcript scan", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    db.exec("DELETE FROM session_economics");

    expect(computeSessionEconomicsVectors(db)).toEqual([]);
    expect(tokenEconomics(db).totals.generation_tokens).toBeGreaterThan(0);
    refreshDerivedMetadata(db);
    expect(
      aggregateEconomicsVectors(computeSessionEconomicsVectors(db)).totals.generation_tokens,
    ).toBeGreaterThan(0);
    db.close();
  });

  test("backfills stale, malformed, or structurally incomplete vectors", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    db.query("UPDATE session_economics SET format_version = ?1 WHERE session_id = ?2").run(
      SESSION_ECONOMICS_FORMAT_VERSION - 1,
      sessionId,
    );

    expect(materializeMissingSessionEconomics(db)).toBe(1);
    expect(
      (
        db
          .query("SELECT format_version FROM session_economics WHERE session_id = ?1")
          .get(sessionId) as { format_version: number }
      ).format_version,
    ).toBe(SESSION_ECONOMICS_FORMAT_VERSION);
    expect(materializeMissingSessionEconomics(db)).toBe(0);

    db.query("UPDATE session_economics SET vector_json = '{' WHERE session_id = ?1").run(sessionId);
    expect(materializeMissingSessionEconomics(db)).toBe(1);
    expect(
      (
        db
          .query(
            "SELECT json_valid(vector_json) AS valid FROM session_economics WHERE session_id = ?1",
          )
          .get(sessionId) as { valid: number }
      ).valid,
    ).toBe(1);

    db.query(
      "UPDATE session_economics SET vector_json = json_remove(vector_json, '$.billed_input_tokens') WHERE session_id = ?1",
    ).run(sessionId);
    expect(materializeMissingSessionEconomics(db)).toBe(1);
    expect(tokenEconomicsForSession(db, sessionId)).not.toBeNull();

    db.query(
      "UPDATE session_economics SET vector_json = json_remove(vector_json, '$.retrieval') WHERE session_id = ?1",
    ).run(sessionId);
    expect(materializeMissingSessionEconomics(db)).toBe(1);
    expect(tokenEconomicsForSession(db, sessionId)).not.toBeNull();

    db.query(
      "UPDATE session_economics SET vector_json = json_remove(vector_json, '$.buckets.context.generation') WHERE session_id = ?1",
    ).run(sessionId);
    expect(materializeMissingSessionEconomics(db)).toBe(1);
    const repaired = db
      .query("SELECT vector_json FROM session_economics WHERE session_id = ?1")
      .get(sessionId) as { vector_json: string };
    expect(
      (
        JSON.parse(repaired.vector_json) as {
          buckets: { context: { generation: number } };
        }
      ).buckets.context.generation,
    ).toBeNumber();
    db.close();
  });

  test("caps waiting on the user and keeps it out of agent activity", () => {
    const db = freshDb();
    // A blockless system message splits the raw 3600s gap for active_seconds,
    // while block-based attribution sees one gap capped at 300s.
    const content = [
      JSON.stringify({
        type: "assistant",
        timestamp: "2026-05-06T09:00:00.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 10, output_tokens: 5 },
          content: [{ type: "text", text: "Ready for your response." }],
        },
      }),
      JSON.stringify({
        type: "system",
        timestamp: "2026-05-06T09:04:10.000Z",
        subtype: "compact_boundary",
        content: "Conversation compacted",
      }),
      JSON.stringify({
        type: "user",
        timestamp: "2026-05-06T10:00:00.000Z",
        message: { role: "user", content: [{ type: "text", text: "Continue." }] },
      }),
    ].join("\n");
    upsertSession(
      db,
      parseClaudeSession("sess-idle", `${content}\n`),
      "/x/idle.jsonl",
      1,
      2,
      "idle",
    );

    const economics = tokenEconomics(db);
    expect(economics.totals.active_ms).toBe(0);
    expect(economics.totals.waiting_on_user_ms).toBe(300_000);
    expect(economics.totals.attributed_ms).toBe(300_000);
    const activeSeconds = (
      db.query("SELECT active_seconds FROM session").get() as { active_seconds: number }
    ).active_seconds;
    expect(economics.totals.attributed_ms).toBeLessThan(activeSeconds * 1000);
    db.close();
  });

  test("counts an agent run when it contributes only wall-clock activity", () => {
    const db = freshDb();
    const content = [
      JSON.stringify({
        type: "assistant",
        timestamp: "2026-05-06T09:00:00.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 0, output_tokens: 0 },
          content: [{ type: "text", text: "Starting." }],
        },
      }),
      JSON.stringify({
        type: "assistant",
        timestamp: "2026-05-06T09:00:10.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 0, output_tokens: 0 },
          content: [{ type: "text", text: "Done." }],
        },
      }),
    ].join("\n");
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-time-only", `${content}\n`),
      "/x/time-only.jsonl",
      1,
      2,
      "time-only",
    );

    const economics = tokenEconomicsForSession(db, sessionId);
    expect(economics?.buckets.find((row) => row.bucket === "communicating")).toMatchObject({
      active_ms: 10_000,
      generation_tokens: 0,
      sessions: 1,
    });
    db.close();
  });

  test("weights mixed tool results and user text by their actual bytes", () => {
    const db = freshDb();
    const content = [
      JSON.stringify({
        type: "assistant",
        timestamp: "2026-05-06T09:00:00.000Z",
        message: {
          role: "assistant",
          model: "claude-opus-4-7",
          usage: { input_tokens: 10, output_tokens: 5 },
          content: [
            {
              type: "tool_use",
              id: "toolu_edit",
              name: "Edit",
              input: { file_path: "/x/a.ts", old_string: "a", new_string: "b" },
            },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        timestamp: "2026-05-06T09:00:10.000Z",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "toolu_edit", content: "123456789" },
            { type: "text", text: "x" },
          ],
        },
      }),
    ].join("\n");
    const sessionId = upsertSession(
      db,
      parseClaudeSession("sess-mixed-result", `${content}\n`),
      "/x/mixed-result.jsonl",
      1,
      2,
      "mixed-result",
    );

    const economics = tokenEconomics(db);
    expect(economics.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(9_000);
    expect(economics.totals.waiting_on_user_ms).toBe(1_000);
    expect(economics.totals.attributed_ms).toBe(10_000);

    const scoped = tokenEconomicsForSession(db, sessionId);
    expect(scoped?.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(9_000);
    expect(scoped?.totals.waiting_on_user_ms).toBe(1_000);
    db.close();
  });

  test("classifies Codex patch edits as code and read-only shell as context", () => {
    const db = freshDb();
    const sessionId = upsertSession(
      db,
      parseCodexSession("sess-enr-codex", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/codex.jsonl",
      1,
      2,
      "codex",
    );

    const aggregate = tokenEconomics(db);
    expect(aggregate.buckets.find((row) => row.bucket === "code")).toMatchObject({
      tool_calls: 1,
      sessions: 1,
    });
    expect(
      aggregate.buckets.find((row) => row.bucket === "code")?.generation_tokens,
    ).toBeGreaterThan(0);
    expect(aggregate.buckets.find((row) => row.bucket === "context")).toMatchObject({
      tool_calls: 1,
      sessions: 1,
    });
    expect(aggregate.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(3_000);
    expect(aggregate.buckets.find((row) => row.bucket === "context")?.active_ms).toBe(2_000);

    const scoped = tokenEconomicsForSession(db, sessionId);
    expect(scoped?.buckets.find((row) => row.bucket === "code")).toMatchObject({
      tool_calls: 1,
      sessions: 1,
    });
    expect(scoped?.buckets.find((row) => row.bucket === "context")).toMatchObject({
      tool_calls: 1,
      sessions: 1,
    });
    expect(scoped?.buckets.find((row) => row.bucket === "code")?.active_ms).toBe(3_000);
    expect(scoped?.buckets.find((row) => row.bucket === "context")?.active_ms).toBe(2_000);
    db.close();
  });

  test("classifies Codex shell build commands as code in aggregate and session economics", () => {
    const db = freshDb();
    const content = [
      JSON.stringify({
        type: "session_meta",
        timestamp: "2026-05-05T09:00:00.000Z",
        payload: { id: "sess-codex-shell", cwd: "/Users/dev/proj" },
      }),
      JSON.stringify({
        type: "turn_context",
        timestamp: "2026-05-05T09:00:01.000Z",
        payload: { cwd: "/Users/dev/proj", model: "gpt-5.4" },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-05T09:00:02.000Z",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "Run tests" }],
        },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-05T09:00:03.000Z",
        payload: {
          type: "reasoning",
          summary: [{ type: "summary_text", text: "Run validation." }],
        },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-05T09:00:04.000Z",
        payload: {
          type: "function_call",
          name: "shell",
          call_id: "call_shell",
          arguments: JSON.stringify({ cmd: "bun test", workdir: "/Users/dev/proj" }),
        },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-05T09:00:05.000Z",
        payload: { type: "function_call_output", call_id: "call_shell", output: "231 pass" },
      }),
      JSON.stringify({
        type: "event_msg",
        timestamp: "2026-05-05T09:00:06.000Z",
        payload: {
          type: "token_count",
          info: {
            total_token_usage: {
              input_tokens: 800,
              cached_input_tokens: 100,
              output_tokens: 80,
              reasoning_output_tokens: 20,
              total_tokens: 880,
            },
            last_token_usage: {
              input_tokens: 800,
              cached_input_tokens: 100,
              output_tokens: 80,
              reasoning_output_tokens: 20,
              total_tokens: 880,
            },
          },
        },
      }),
      JSON.stringify({
        type: "response_item",
        timestamp: "2026-05-05T09:00:07.000Z",
        payload: {
          type: "message",
          role: "assistant",
          content: [{ type: "output_text", text: "Tests pass." }],
        },
      }),
    ].join("\n");
    const sessionId = upsertSession(
      db,
      parseCodexSession("sess-codex-shell", `${content}\n`, new Map()),
      "/x/codex-shell.jsonl",
      1,
      2,
      "codex-shell",
    );

    const aggregateCode = tokenEconomics(db).buckets.find((row) => row.bucket === "code");
    expect(aggregateCode).toMatchObject({ tool_calls: 1, sessions: 1 });
    expect(aggregateCode?.generation_tokens).toBeGreaterThan(0);
    // Persisting Codex's last_token_usage powers context-window tooltips, but
    // must not move its reported reasoning output out of planning economics.
    expect(
      tokenEconomics(db).buckets.find((row) => row.bucket === "planning")?.generation_tokens,
    ).toBe(20);
    // One second generated the call and one second executed it. The latter is
    // resolved through tool_call.result_block_id rather than defaulting to context.
    expect(aggregateCode?.active_ms).toBe(2_000);

    const scopedEconomics = tokenEconomicsForSession(db, sessionId);
    const scopedCode = scopedEconomics?.buckets.find((row) => row.bucket === "code");
    expect(scopedCode).toMatchObject({ tool_calls: 1, sessions: 1 });
    expect(scopedCode?.generation_tokens).toBeGreaterThan(0);
    expect(scopedCode?.active_ms).toBe(2_000);

    db.close();
  });

  test("date filters scope the economics rollup", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseCodexSession("sess-enr-codex", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/codex.jsonl",
      1,
      2,
      "codex",
    );

    const scoped = tokenEconomics(db, { from: "2026-05-04", to: "2026-05-04" });
    expect(scoped.buckets.find((row) => row.bucket === "planning")?.generation_tokens).toBe(40);
    expect(scoped.totals.estimated_cost_usd).toBeGreaterThan(0);
    db.close();
  });

  test("session scope includes nested subagents", () => {
    const db = freshDb();
    const rootId = upsertSession(
      db,
      parseClaudeSession("sess-root", fixture("claude", "sample.jsonl")),
      "/x/root.jsonl",
      1,
      2,
      "root",
    );
    const childId = upsertSession(
      db,
      parseCodexSession("sess-child", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/child.jsonl",
      1,
      2,
      "child",
    );
    db.query(
      `UPDATE session
       SET is_subagent = 1, parent_session_id = ?1, spawn_tool_use_id = 'toolu_agent'
       WHERE id = ?2`,
    ).run(rootId, childId);

    const scoped = tokenEconomicsForSession(db, rootId);
    expect(scoped?.totals.estimated_cost_usd).toBeCloseTo(
      tokenEconomics(db).totals.estimated_cost_usd,
      12,
    );
    expect(scoped?.buckets.some((row) => row.sessions > 1)).toBe(true);
    expect(tokenEconomicsForSession(db, 999_999)).toBeNull();
    db.close();
  });

  test("archive-wide economics excludes archived trees while exact session reads remain available", () => {
    const db = freshDb();
    const rootId = upsertSession(
      db,
      parseClaudeSession("economics-archive-root", fixture("claude", "enriched.jsonl")),
      "/x/economics-archive-root.jsonl",
      1,
      2,
      "archive-root",
    );
    const childId = upsertSession(
      db,
      parseCodexSession("economics-archive-child", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/economics-archive-child.jsonl",
      1,
      2,
      "archive-child",
    );
    const visibleId = upsertSession(
      db,
      parseClaudeSession("economics-visible", fixture("claude", "enriched.jsonl")),
      "/x/economics-visible.jsonl",
      1,
      2,
      "visible",
    );
    db.query(
      `UPDATE session
       SET is_subagent = 1, parent_session_id = ?1
       WHERE id = ?2`,
    ).run(rootId, childId);

    const exactBeforeArchive = tokenEconomicsForSession(db, rootId);
    const aggregateBeforeArchive = tokenEconomics(db);
    expect(computeSessionEconomicsVectors(db).map((vector) => vector.id)).toEqual([
      rootId,
      childId,
      visibleId,
    ]);

    expect(setSessionUserState(db, rootId, "archived")).toBe(true);

    const visibleVectors = computeSessionEconomicsVectors(db);
    expect(visibleVectors.map((vector) => vector.id)).toEqual([visibleId]);
    expect(tokenEconomics(db)).toEqual(aggregateEconomicsVectors(visibleVectors));
    expect(tokenEconomics(db).totals.estimated_cost_usd).toBeLessThan(
      aggregateBeforeArchive.totals.estimated_cost_usd,
    );
    expect(tokenEconomicsForSession(db, rootId)).toEqual(exactBeforeArchive);
    db.close();
  });

  test("precomputed vectors reproduce tokenEconomics for any date filter", () => {
    const db = freshDb();
    upsertSession(
      db,
      parseClaudeSession("sess-enr-claude", fixture("claude", "enriched.jsonl")),
      "/x/claude.jsonl",
      1,
      2,
      "claude",
    );
    upsertSession(
      db,
      parseCodexSession("sess-enr-codex", fixture("codex", "enriched.jsonl"), new Map()),
      "/x/codex.jsonl",
      1,
      2,
      "codex",
    );
    // Split the sessions across days, and leave one session dateless to pin
    // the SQL NULL semantics: excluded whenever a bound is set.
    db.exec(`
      UPDATE session SET started_at = '2026-01-01T09:00:00Z' WHERE id = 1;
      UPDATE session SET started_at = NULL WHERE id = 2;
    `);

    const vectors = computeSessionEconomicsVectors(db);
    expect(vectors).toHaveLength(2);
    const filters = [
      undefined,
      { from: "2026-01-01", to: "2026-01-01" },
      { from: "2026-01-02", to: null },
      { from: null, to: "2025-12-31" },
    ] as const;
    for (const filter of filters) {
      const fromVectors = aggregateEconomicsVectors(
        vectors.filter((vector) => economicsVectorMatchesFilter(vector, filter)),
      );
      expect(fromVectors).toEqual(tokenEconomics(db, filter));
    }

    const bounded = vectors.filter((vector) =>
      economicsVectorMatchesFilter(vector, { from: "2026-01-01", to: null }),
    );
    expect(bounded.map((vector) => vector.id)).toEqual([1]);
    expect(aggregateEconomicsVectors([])).toEqual(tokenEconomics(db, { from: "2030-01-01" }));
    db.close();
  });
});
