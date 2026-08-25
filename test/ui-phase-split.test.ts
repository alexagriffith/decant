import { describe, expect, test } from "bun:test";
import { phaseSplitRows } from "../src/ui/phase-split.ts";

type Economics = NonNullable<Parameters<typeof phaseSplitRows>[0]>;

const totals = {
  generation_tokens: 100,
  context_window_tokens: 300,
  estimated_cost_usd: 0.1,
  input_cost_usd: 0.08,
  output_cost_usd: 0.02,
  active_ms: 15_000,
  waiting_on_user_ms: 0,
  attributed_ms: 15_000,
};

function economicsWithoutPhases(): Economics {
  return { buckets: [], totals };
}

function economicsWithPhases(): Economics {
  return {
    buckets: [],
    totals: {
      ...totals,
      phases: {
        orientation: {
          generation_tokens: 60,
          context_window_tokens: 180,
          estimated_cost_usd: 0.048,
          active_ms: 9_000,
          cost_share: 0.48,
        },
        implementation: {
          generation_tokens: 40,
          context_window_tokens: 120,
          estimated_cost_usd: 0.052,
          active_ms: 6_000,
          cost_share: 0.52,
        },
      },
    },
  };
}

function editFreeEconomics(): Economics {
  return {
    buckets: [],
    totals: {
      ...totals,
      phases: {
        orientation: {
          generation_tokens: 100,
          context_window_tokens: 300,
          estimated_cost_usd: 0.1,
          active_ms: 15_000,
          cost_share: 1,
        },
        implementation: {
          generation_tokens: 0,
          context_window_tokens: 0,
          estimated_cost_usd: 0,
          active_ms: 0,
          cost_share: 0,
        },
      },
    },
  };
}

describe("phase split rows", () => {
  test("derives cost and time shares for both phases", () => {
    const rows = phaseSplitRows(economicsWithPhases());
    expect(rows).toHaveLength(2);
    expect(rows[0]?.phase).toBe("orientation");
    expect(rows[0]?.tone).toBe("info");
    expect(rows[1]?.tone).toBe("success");
    expect(rows[0]?.costShare).toBeCloseTo(0.48, 12);
    expect(rows[0]?.timeShare).toBeCloseTo(0.6, 12);
    expect(rows[1]?.timeShare).toBeCloseTo(0.4, 12);
  });

  test("returns no rows when the server omits phases", () => {
    expect(phaseSplitRows(economicsWithoutPhases())).toEqual([]);
    expect(phaseSplitRows(null)).toEqual([]);
  });

  test("reports a full orientation share for an edit-free session", () => {
    const rows = phaseSplitRows(editFreeEconomics());
    expect(rows[0]?.costShare).toBeCloseTo(1, 12);
    expect(rows[1]?.costShare).toBeCloseTo(0, 12);
  });

  test("leaves both time shares at zero when no active time was attributed", () => {
    const base = economicsWithPhases();
    const rows = phaseSplitRows({ ...base, totals: { ...base.totals, active_ms: 0 } });
    expect(rows.map((row) => row.timeShare)).toEqual([0, 0]);
  });
});
