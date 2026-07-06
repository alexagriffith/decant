import { describe, expect, test } from "bun:test";
import { bashBucket, blockBucket, toolBucket } from "../src/buckets.ts";

describe("activity bucket classifier", () => {
  test("classifies fixed tool families", () => {
    expect(toolBucket("TodoWrite")).toBe("planning");
    expect(toolBucket("Edit")).toBe("code");
    expect(toolBucket("MultiEdit")).toBe("code");
    expect(toolBucket("Read")).toBe("context");
    expect(toolBucket("Task")).toBe("context");
    expect(toolBucket("mcp__github__search_issues")).toBe("context");
    expect(toolBucket("UnknownFutureTool")).toBe("context");
  });

  test("classifies Bash by command head and git subcommand", () => {
    expect(bashBucket("rg auth src")).toBe("context");
    expect(bashBucket("/bin/cat README.md")).toBe("context");
    expect(bashBucket("git status --short")).toBe("context");
    expect(bashBucket("git diff")).toBe("context");
    expect(bashBucket("git commit -m test")).toBe("code");
    expect(bashBucket("bun test")).toBe("code");
  });

  test("extracts Bash command from JSON input", () => {
    expect(toolBucket("Bash", { command: "ls -la" })).toBe("context");
    expect(toolBucket("Bash", '{"command":"npm install"}')).toBe("code");
  });

  test("classifies transcript block families", () => {
    expect(blockBucket("thinking")).toBe("planning");
    expect(blockBucket("text")).toBe("communicating");
    expect(blockBucket("tool_use", "Write")).toBe("code");
    expect(blockBucket("tool_result", "Read")).toBe("context");
  });
});
