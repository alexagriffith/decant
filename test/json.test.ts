import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { canonicalJson } from "../src/json.ts";

// canonicalJson must reproduce serde_json's default serialization byte-for-byte
// (BTreeMap ⇒ recursively sorted keys, compact separators): every JSON TEXT
// column the pre-TypeScript engine wrote (`raw`, `raw_meta`, `tool_input`, stringified
// other-blocks) was produced that way, and golden parity diffs those bytes.
describe("canonicalJson", () => {
  test("sorts object keys recursively, arrays keep order", () => {
    expect(canonicalJson({ b: 1, a: { d: 2, c: 3 }, e: [{ z: 1, y: 2 }] })).toBe(
      '{"a":{"c":3,"d":2},"b":1,"e":[{"y":2,"z":1}]}',
    );
  });

  test("compact separators and scalar forms match serde_json", () => {
    expect(canonicalJson(null)).toBe("null");
    expect(canonicalJson(true)).toBe("true");
    expect(canonicalJson(42)).toBe("42");
    expect(canonicalJson(0.25)).toBe("0.25");
    expect(canonicalJson("hi")).toBe('"hi"');
    expect(canonicalJson([])).toBe("[]");
    expect(canonicalJson({})).toBe("{}");
  });

  test("escapes strings like serde_json", () => {
    expect(canonicalJson('quote " backslash \\')).toBe('"quote \\" backslash \\\\"');
    expect(canonicalJson("tab\tnewline\ncr\r")).toBe('"tab\\tnewline\\ncr\\r"');
    expect(canonicalJson("")).toBe('"\\u0001"');
    // Non-ASCII passes through unescaped (both sides emit UTF-8).
    expect(canonicalJson("héllo 🎉")).toBe('"héllo 🎉"');
  });

  test("sorts keys by code point (UTF-8 byte order), not UTF-16 code units", () => {
    // U+FF61 sorts before U+10000 in code-point order; naive JS string sort
    // (UTF-16 units) would put the surrogate-pair key first.
    expect(canonicalJson({ "\u{10000}": 1, "｡": 2 })).toBe('{"｡":2,"𐀀":1}');
  });

  test("round-trips every JSON TEXT value in the goldens byte-for-byte", async () => {
    // The strongest oracle available: golden `raw` / `tool_input` strings were
    // serialized by serde_json; canonicalJson(parse(x)) must equal x.
    const goldenDir = join(import.meta.dir, "golden", "rows");
    let checked = 0;
    const messages = (await Bun.file(join(goldenDir, "messages.json")).json()) as {
      raw: string;
    }[];
    for (const m of messages) {
      expect(canonicalJson(JSON.parse(m.raw))).toBe(m.raw);
      checked += 1;
    }
    const blocks = (await Bun.file(join(goldenDir, "blocks.json")).json()) as {
      tool_input: string | null;
    }[];
    for (const b of blocks) {
      if (b.tool_input === null) continue;
      expect(canonicalJson(JSON.parse(b.tool_input))).toBe(b.tool_input);
      checked += 1;
    }
    expect(checked).toBeGreaterThan(45);
  });
});
