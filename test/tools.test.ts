import { describe, expect, test } from "bun:test";
import { classifyTool, preview } from "../src/tools.ts";

// Ports tools.rs tests verbatim.
describe("classifyTool", () => {
  test("builtin tool", () => {
    expect(classifyTool("Bash")).toEqual({ kind: "builtin", mcpServer: null, baseName: "Bash" });
  });

  test("simple mcp tool", () => {
    expect(classifyTool("mcp__claude_ai_Linear__create_issue")).toEqual({
      kind: "mcp",
      mcpServer: "claude_ai_Linear",
      baseName: "create_issue",
    });
  });

  test("nested gateway mcp tool keeps __ in the base name", () => {
    expect(classifyTool("mcp__codex_apps__hubspot__create_deal")).toEqual({
      kind: "mcp",
      mcpServer: "codex_apps",
      baseName: "hubspot__create_deal",
    });
  });

  test("mcp prefix without server separator", () => {
    expect(classifyTool("mcp__lonely")).toEqual({
      kind: "mcp",
      mcpServer: null,
      baseName: "lonely",
    });
  });
});

describe("preview", () => {
  test("truncates by characters with an ellipsis", () => {
    expect(preview("abcdef", 3)).toBe("abc…");
    expect(preview("ab", 3)).toBe("ab");
  });

  test("counts Unicode scalars, not UTF-16 code units (Rust chars() parity)", () => {
    // "🎉🎉🎉" is 6 UTF-16 code units but 3 scalars — must NOT truncate at max 3,
    // and truncation at 2 must not split a surrogate pair.
    expect(preview("🎉🎉🎉", 3)).toBe("🎉🎉🎉");
    expect(preview("🎉🎉🎉", 2)).toBe("🎉🎉…");
  });
});
