import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const main = readFileSync(join(import.meta.dir, "..", "src", "ui", "main.tsx"), "utf8");

describe("Gemini CLI source copy", () => {
  test("labels Gemini sessions instead of falling back to the wire string", () => {
    expect(main).toMatch(/tool === "gemini"[\s\S]{0,160}Gemini/);
    expect(main).toContain('{ key: "assistant", label: "Gemini"');
  });

  test("names Gemini CLI alongside Claude Code and Codex in product copy", () => {
    expect(main).toContain("Every Claude Code, Codex, and Gemini CLI session log on this device.");
    expect(main).toMatch(/JSONL logs Claude Code, Codex, and Gemini CLI already write/);
  });
});
