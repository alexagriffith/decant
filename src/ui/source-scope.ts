import type { SessionSource } from "../source-filter.ts";

export type DashboardSource = "" | SessionSource;

export const DASHBOARD_SOURCES: readonly { key: DashboardSource; label: string }[] = [
  { key: "", label: "All sources" },
  { key: "claude_code", label: "Claude Code" },
  { key: "codex_app", label: "Codex App" },
  { key: "codex_cli", label: "Codex CLI" },
  { key: "gemini_cli", label: "Gemini CLI" },
];

export function sourceScopeQuery(dateQuery: string, source: DashboardSource): string {
  const params = new URLSearchParams(dateQuery);
  if (source !== "") {
    params.set("source", source);
  }
  return params.toString();
}
