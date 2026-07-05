import type { Json } from "./model.ts";

export const ACTIVITY_BUCKETS = ["planning", "communicating", "context", "code"] as const;
export type ActivityBucket = (typeof ACTIVITY_BUCKETS)[number];

const CONTEXT_TOOLS = new Set([
  "Read",
  "Grep",
  "Glob",
  "LS",
  "NotebookRead",
  "WebFetch",
  "WebSearch",
  "Task",
  "ToolSearch",
  "BashOutput",
  "ListMcpResourcesTool",
  "ReadMcpResourceTool",
]);
const CODE_TOOLS = new Set(["Edit", "Write", "NotebookEdit", "MultiEdit"]);
const PLANNING_TOOLS = new Set(["TodoWrite", "ExitPlanMode", "EnterPlanMode"]);
const READONLY_BASH = new Set([
  "ls",
  "cat",
  "grep",
  "rg",
  "find",
  "head",
  "tail",
  "wc",
  "pwd",
  "echo",
  "tree",
  "stat",
  "file",
  "which",
  "type",
  "env",
  "printenv",
  "diff",
]);
const READONLY_GIT = new Set(["status", "diff", "log", "show", "branch", "remote", "blame"]);

export function toolBucket(
  toolName: string | null | undefined,
  input?: string | Json,
): ActivityBucket {
  const name = toolName ?? "";
  if (name === "Bash") {
    return bashBucket(commandFromInput(input));
  }
  if (PLANNING_TOOLS.has(name)) {
    return "planning";
  }
  if (CODE_TOOLS.has(name)) {
    return "code";
  }
  if (CONTEXT_TOOLS.has(name) || name.startsWith("mcp__")) {
    return "context";
  }
  return "context";
}

export function blockBucket(
  blockType: string | null,
  toolName?: string | null,
  input?: string | Json,
): ActivityBucket {
  if (blockType === "thinking") {
    return "planning";
  }
  if (blockType === "tool_use" || blockType === "tool_result" || blockType === "web_search") {
    return toolBucket(toolName, input);
  }
  return "communicating";
}

export function bashBucket(command: string | null): ActivityBucket {
  const [head, subcommand] = commandHead(command);
  if (head == null) {
    return "code";
  }
  if (head === "git") {
    return subcommand != null && READONLY_GIT.has(subcommand) ? "context" : "code";
  }
  return READONLY_BASH.has(head) ? "context" : "code";
}

function commandFromInput(input: string | Json | undefined): string | null {
  if (input == null) {
    return null;
  }
  const value = typeof input === "string" ? parseJson(input) : input;
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    const command = value.command ?? value.cmd;
    return typeof command === "string" ? command : null;
  }
  return null;
}

function parseJson(value: string): Json | string {
  try {
    return JSON.parse(value) as Json;
  } catch {
    return value;
  }
}

function commandHead(command: string | null): [string | null, string | null] {
  const tokens = splitCommand(command);
  if (tokens.length === 0) {
    return [null, null];
  }
  const head = basename(tokens[0] ?? "");
  const subcommand = tokens[1] ?? null;
  return [head, subcommand];
}

function splitCommand(command: string | null): string[] {
  if (command == null) {
    return [];
  }
  const trimmed = command.trim();
  if (trimmed === "") {
    return [];
  }
  return trimmed.split(/\s+/).filter((token) => token !== "");
}

function basename(value: string): string {
  const clean = value.replace(/^['"]|['"]$/g, "");
  return clean.split("/").filter(Boolean).at(-1) ?? clean;
}
