import { describe, expect, test } from "bun:test";
import { defaultPricing, estimateCost, isPriceable, type Price } from "../src/cost.ts";
import { emptyUsage, type TokenUsage } from "../src/model.ts";

// Ports cost.rs tests verbatim — these are the spec for model normalization
// (Bedrock ARNs, date/[1m] suffixes, aliases) and estimate-at-ingest.
function usage1m(): TokenUsage {
  return { input: 1_000_000, output: 1_000_000, cacheRead: 0, cacheCreation: 0, reasoning: 0 };
}

describe("estimateCost", () => {
  test("opus input+output costs add up", () => {
    const cost = estimateCost("claude-opus-4-7", usage1m(), defaultPricing());
    expect(cost).toBeCloseTo(30.0, 6); // 1M @ $5 + 1M @ $25
  });

  test("reasoning tokens do not change cost", () => {
    const pricing = defaultPricing();
    const without = usage1m();
    const withReasoning = { ...usage1m(), reasoning: 750_000 };
    expect(estimateCost("claude-opus-4-8", withReasoning, pricing)).toBe(
      estimateCost("claude-opus-4-8", without, pricing),
    );
  });

  test("cache tokens are priced", () => {
    const usage: TokenUsage = { ...emptyUsage(), cacheRead: 1_000_000, cacheCreation: 1_000_000 };
    // opus: cache read $0.50 + cache write $6.25.
    expect(estimateCost("claude-opus-4-8", usage, defaultPricing())).toBeCloseTo(6.75, 6);
  });

  test("claude variants normalize to their tier", () => {
    const pricing = defaultPricing();
    const u = usage1m();
    const opus = estimateCost("claude-opus-4-8", u, pricing);
    expect(opus).toBeCloseTo(30.0, 6);
    for (const m of [
      "claude-opus-4-6",
      "claude-opus-4-8[1m]",
      "opus",
      "us.anthropic.claude-opus-4-6-v1",
    ]) {
      expect(estimateCost(m, u, pricing)).toBeCloseTo(opus, 6);
    }
    const haiku = estimateCost("claude-haiku-4-5", u, pricing);
    expect(estimateCost("us.anthropic.claude-haiku-4-5-20251001-v1:0", u, pricing)).toBeCloseTo(
      haiku,
      6,
    );
    const sonnet = estimateCost("claude-sonnet-4-6", u, pricing);
    expect(estimateCost("us.anthropic.claude-sonnet-4-5-20250929-v1:0", u, pricing)).toBeCloseTo(
      sonnet,
      6,
    );
  });

  test("gpt family is priced", () => {
    const pricing = defaultPricing();
    const u = usage1m();
    expect(estimateCost("gpt-5", u, pricing)).toBeCloseTo(11.25, 6);
    expect(estimateCost("gpt-5.1", u, pricing)).toBeCloseTo(11.25, 6); // shares gpt-5
    expect(estimateCost("gpt-5.4", u, pricing)).toBeCloseTo(17.5, 6);
    expect(estimateCost("gpt-5.4-mini", u, pricing)).toBeCloseTo(5.25, 6);
    expect(estimateCost("gpt-5.4-nano", u, pricing)).toBeCloseTo(1.45, 6);
    expect(estimateCost("gpt-5.5", u, pricing)).toBeCloseTo(35.0, 6);
  });

  test("codex models are priced", () => {
    const pricing = defaultPricing();
    const u = usage1m();
    expect(estimateCost("gpt-5.3-codex", u, pricing)).toBeCloseTo(15.75, 6);
    expect(estimateCost("gpt-5.2", u, pricing)).toBeCloseTo(15.75, 6);
    expect(estimateCost("codex-auto-review", u, pricing)).toBeCloseTo(15.75, 6);
  });

  test("fable canonical model forms", () => {
    const pricing = defaultPricing();
    const u = usage1m();
    const fable = estimateCost("claude-fable-5", u, pricing);
    for (const m of ["claude-fable-5[1m]", "fable"]) {
      expect(estimateCost(m, u, pricing)).toBeCloseTo(fable, 6);
    }
    expect(estimateCost("claude-mythos-5", u, pricing)).toBeCloseTo(fable, 6);
  });

  test("fable input+output costs add up", () => {
    expect(estimateCost("claude-fable-5", usage1m(), defaultPricing())).toBeCloseTo(60.0, 6);
  });

  test("unknown models are zero", () => {
    const pricing = defaultPricing();
    const usage: TokenUsage = { ...emptyUsage(), input: 5_000, output: 5_000 };
    expect(estimateCost("<synthetic>", usage, pricing)).toBe(0.0);
    expect(estimateCost("exa-research-pro", usage, pricing)).toBe(0.0);
    expect(estimateCost("some-future-llm", usage, pricing)).toBe(0.0);
    expect(estimateCost(null, usage, pricing)).toBe(0.0);
  });

  test("unrecognized claude tier is unpriceable", () => {
    expect(estimateCost("claude-quill-9", usage1m(), defaultPricing())).toBe(0.0);
    expect(isPriceable("claude-quill-9")).toBe(false);
  });

  test("known canonical key missing from pricing table is zero", () => {
    const pricing = new Map<string, Price>([
      [
        "claude-opus",
        { inputPerMtok: 5.0, outputPerMtok: 25.0, cacheReadPerMtok: 0.5, cacheWritePerMtok: 6.25 },
      ],
    ]);
    expect(estimateCost("claude-haiku-4-5", usage1m(), pricing)).toBe(0.0);
  });
});

describe("isPriceable", () => {
  test("distinguishes known from unknown", () => {
    expect(isPriceable("claude-haiku-4-5")).toBe(true);
    expect(isPriceable("gpt-5.4-mini")).toBe(true);
    expect(isPriceable("opus")).toBe(true);
    expect(isPriceable("<synthetic>")).toBe(false);
    expect(isPriceable("exa-research-pro")).toBe(false);
  });
});
