import { describe, expect, test } from "bun:test";
import {
  applyDatePreset,
  dateRangeQuery,
  shiftDateRange,
  withDateQuery,
} from "../src/ui/date-range.ts";

describe("dashboard date ranges", () => {
  test("scopes Today to the current calendar date", () => {
    const today = new Date().toISOString().slice(0, 10);
    expect(applyDatePreset("today", { min: "2026-01-01", max: "2026-08-26" })).toEqual({
      preset: "today",
      from: today,
      to: today,
    });
  });

  test("anchors relative presets to the newest archived session date", () => {
    expect(applyDatePreset("7d", { min: "2026-01-01", max: "2026-08-26" })).toEqual({
      preset: "7d",
      from: "2026-08-20",
      to: "2026-08-26",
    });
  });

  test("passes arbitrary custom bounds through to API requests", () => {
    const query = dateRangeQuery({ preset: "custom", from: "2026-04-03", to: "2026-05-17" });
    expect(query).toBe("from=2026-04-03&to=2026-05-17");
    expect(withDateQuery("/api/stats/summary", query)).toBe(
      "/api/stats/summary?from=2026-04-03&to=2026-05-17",
    );
  });

  test("moves custom ranges by their inclusive span", () => {
    expect(shiftDateRange({ preset: "custom", from: "2026-08-20", to: "2026-08-26" }, -1)).toEqual({
      preset: "custom",
      from: "2026-08-13",
      to: "2026-08-19",
    });
  });
});
