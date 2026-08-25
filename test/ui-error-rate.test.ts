import { describe, expect, test } from "bun:test";
import { errorRateDisplay } from "../src/ui/error-rate.ts";

describe("tool error rate display", () => {
  test("shows a dash and stays quiet when nothing has been called", () => {
    expect(errorRateDisplay(0, 0)).toEqual({ label: "—", alert: false });
    // A zero denominator still produces a zero rate upstream; neither should alert.
    expect(errorRateDisplay(0, 12.5)).toEqual({ label: "—", alert: false });
  });

  test("stays quiet at a clean zero", () => {
    expect(errorRateDisplay(1000, 0)).toEqual({ label: "0.0%", alert: false });
  });

  test("does not alert on a rate that rounds away to zero", () => {
    // 1 error in 100,000 calls. The card would read "0.0%", and colouring that
    // red is a false alarm the reader cannot distinguish from a real one.
    expect(errorRateDisplay(100_000, 0.001)).toEqual({ label: "0.0%", alert: false });
    expect(errorRateDisplay(100_000, 0.049)).toEqual({ label: "0.0%", alert: false });
  });

  test("alerts as soon as the displayed value is non-zero", () => {
    expect(errorRateDisplay(100_000, 0.05)).toEqual({ label: "0.1%", alert: true });
    expect(errorRateDisplay(500, 3.24)).toEqual({ label: "3.2%", alert: true });
    expect(errorRateDisplay(7, 100)).toEqual({ label: "100.0%", alert: true });
  });

  test("the alert tracks the label, not the underlying rate", () => {
    // Every alert=true case must show something other than a zero, and every
    // quiet case must show a zero or a dash. This is the whole contract.
    for (const [calls, rate] of [
      [0, 0],
      [10, 0],
      [10, 0.04],
      [10, 0.05],
      [10, 50],
      [10, 100],
    ] as const) {
      const { label, alert } = errorRateDisplay(calls, rate);
      expect(alert).toBe(label !== "—" && label !== "0.0%");
    }
  });
});
