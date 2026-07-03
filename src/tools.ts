// Tool-name classification and text previews (port of tools.rs).
import type { ToolKind } from "./model.ts";

export interface ClassifiedTool {
  kind: ToolKind;
  mcpServer: string | null;
  baseName: string;
}

/** Classify a logged tool name into (kind, mcp_server, base_name).
 * MCP convention: `mcp__<server>__<base>` (base may itself contain `__`). */
export function classifyTool(name: string): ClassifiedTool {
  if (name.startsWith("mcp__")) {
    const rest = name.slice("mcp__".length);
    const sep = rest.indexOf("__");
    if (sep !== -1) {
      return { kind: "mcp", mcpServer: rest.slice(0, sep), baseName: rest.slice(sep + 2) };
    }
    return { kind: "mcp", mcpServer: null, baseName: rest };
  }
  return { kind: "builtin", mcpServer: null, baseName: name };
}

/** First `max` characters of a string, with an ellipsis if truncated.
 * Counts Unicode scalars like Rust `chars()`, never splitting surrogate pairs. */
export function preview(s: string, max: number): string {
  const chars = [...s];
  if (chars.length <= max) {
    return s;
  }
  return `${chars.slice(0, max).join("")}…`;
}
