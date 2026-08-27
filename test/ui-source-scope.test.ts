import { describe, expect, test } from "bun:test";
import { DASHBOARD_SOURCES, sourceScopeQuery } from "../src/ui/source-scope.ts";

describe("dashboard source scope", () => {
  test("combines source and date filters in one request scope", () => {
    expect(sourceScopeQuery("from=2026-08-20&to=2026-08-26", "codex_app")).toBe(
      "from=2026-08-20&to=2026-08-26&source=codex_app",
    );
  });

  test("uses only source labels backed by current local session formats", () => {
    expect(DASHBOARD_SOURCES.map((source) => source.label)).toEqual([
      "All sources",
      "Claude Code",
      "Codex App",
      "Codex CLI",
      "Gemini CLI",
    ]);
  });
});
