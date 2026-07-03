// serde_json-compatible serialization (see the Phase 1 parity notes).
//
// decant-core serializes JSON through serde_json's default Value, whose maps
// are BTreeMaps: recursively sorted keys, compact separators. Every JSON TEXT
// column in the archive (`message.raw`, `session.raw_meta`, `block.tool_input`,
// stringified unknown blocks) has that shape, and byte-level golden parity
// depends on reproducing it. JSON.stringify already matches serde_json's
// string escaping and number formatting for the values that occur in session
// logs; the delta is key order, which must be Unicode code-point order
// (= UTF-8 byte order, what BTreeMap<String, _> uses), not JS UTF-16 order.
//
// Known bounds (accepted): integer-valued floats ("1.0") and integers beyond
// 2^53 lose their original form at JSON.parse time, before this function runs.
// Neither occurs in session logs; the golden round-trip test pins reality.
import type { Json } from "./model.ts";

/** Serialize a JSON value exactly as serde_json's `Value::to_string()` would. */
export function canonicalJson(value: Json): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(",")}]`;
  }
  const parts = Object.keys(value)
    .sort(compareCodePoints)
    .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key] as Json)}`);
  return `{${parts.join(",")}}`;
}

function compareCodePoints(a: string, b: string): number {
  const ai = a[Symbol.iterator]();
  const bi = b[Symbol.iterator]();
  for (;;) {
    const av = ai.next();
    const bv = bi.next();
    if (av.done && bv.done) return 0;
    if (av.done) return -1;
    if (bv.done) return 1;
    const ac = av.value.codePointAt(0) as number;
    const bc = bv.value.codePointAt(0) as number;
    if (ac !== bc) return ac - bc;
  }
}
