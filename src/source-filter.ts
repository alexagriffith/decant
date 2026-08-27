export const SESSION_SOURCES = ["claude_code", "codex_app", "codex_cli", "gemini_cli"] as const;

export type SessionSource = (typeof SESSION_SOURCES)[number];

export function parseSessionSource(value: string | null): SessionSource | null {
  return SESSION_SOURCES.includes(value as SessionSource) ? (value as SessionSource) : null;
}

export function sessionSourceLabel(source: SessionSource): string {
  switch (source) {
    case "claude_code":
      return "Claude Code";
    case "codex_app":
      return "Codex App";
    case "codex_cli":
      return "Codex CLI";
    case "gemini_cli":
      return "Gemini CLI";
  }
}

export function sessionSourcePredicate(
  alias: string,
  source: SessionSource | null | undefined,
): { sql: string; params: string[] } {
  switch (source) {
    case "claude_code":
      return { sql: `${alias}.tool = ?`, params: ["claude_code"] };
    case "codex_app":
      return {
        sql: `${alias}.tool = 'codex'
          AND CASE WHEN json_valid(${alias}.raw_meta)
            THEN json_extract(${alias}.raw_meta, '$.originator')
          END IN ('Codex Desktop', 'codex_work_desktop')`,
        params: [],
      };
    case "codex_cli":
      return {
        sql: `${alias}.tool = 'codex'
          AND (
            CASE WHEN json_valid(${alias}.raw_meta)
              THEN json_extract(${alias}.raw_meta, '$.originator')
            END = 'codex-tui'
            OR CASE WHEN json_valid(${alias}.raw_meta)
              THEN json_extract(${alias}.raw_meta, '$.source')
            END = 'cli'
          )`,
        params: [],
      };
    case "gemini_cli":
      return { sql: `${alias}.tool = ?`, params: ["gemini"] };
    default:
      return { sql: "", params: [] };
  }
}
