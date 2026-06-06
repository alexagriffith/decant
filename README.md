# decant

Extract your Claude Code and Codex CLI sessions into a normalized, full-text-searchable
SQLite archive — then browse, search, and read them.

## Status

Plan 1 of 3 (Rust core + CLI). Design: `docs/superpowers/specs/2026-06-06-decant-design.md`.
A Phoenix LiveView web app (analytics dashboard, transcript reader) is planned (Plan 3).
Validated on real data: ~1,500 sessions / 185k messages / 60k tool calls ingested cleanly.

## Quick start

```bash
cargo build --release
./target/release/decant sync                 # ingest ~/.claude + ~/.codex into SQLite
./target/release/decant session ls           # list sessions (newest first)
./target/release/decant search "auth bug"    # full-text search across all sessions
./target/release/decant show 1               # read a full transcript
./target/release/decant stats                # usage & cost rollup (--by tool|model|project|day)
./target/release/decant mcp stats            # MCP server leaderboard (calls, tools, errors)
./target/release/decant tool stats           # tool usage: built-in vs MCP, with error counts
./target/release/decant export 1 > s1.md     # export a transcript (Markdown, or --json)
./target/release/decant session ls --json    # machine-readable output (stable DTO contract)
./target/release/decant project ls           # projects by session count + cost
./target/release/decant db info               # db path, size, schema version, row counts
./target/release/decant completion zsh        # shell completion script (bash|zsh|fish)
```

## Configuration

Flags or env vars (precedence: flag > env > platform default):
`--db` / `DECANT_DB`, `--claude-dir` / `DECANT_CLAUDE_DIR`, `--codex-dir` / `DECANT_CODEX_DIR`.
The default database lives under your platform data dir (macOS: `~/Library/Application Support/decant/decant.db`).

## Notes

Sync is idempotent — re-running only re-ingests files whose size/mtime changed. Malformed
JSON lines are recorded in the `ingest_issue` table (non-fatal); exit code `3` signals
"completed with parse issues" for CI. Global flags: `--json`, `-q/--quiet`, `--no-color` (honors `NO_COLOR`).
