# decant

[![CI](https://github.com/onlydole/decant/actions/workflows/ci.yml/badge.svg)](https://github.com/onlydole/decant/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Extract your Claude Code and Codex CLI sessions into a normalized,
full-text-searchable SQLite archive — then browse, search, read, and analyze
them from a fast CLI or a local web UI.

decant reads the JSONL logs those tools already write
(`~/.claude/projects/*.jsonl`, `~/.codex/sessions/rollout-*.jsonl`), normalizes
the two formats into one schema, and gives you durable, queryable history of
everything you've done with AI coding agents. It's local-first and offline — your
transcripts never leave your machine.

## Features

- **One archive for both tools** — Claude Code and Codex sessions in a single SQLite DB.
- **Full-text search** (SQLite FTS5) across every message and tool call.
- **Usage & cost analytics** — rollups by tool, model, project, or day, with cost
  estimates (Claude and GPT-5 families, including Bedrock-style model ids).
- **Tool & MCP insights** — built-in vs MCP tool usage, per-server leaderboards, error rates.
- **Export** transcripts to Markdown or JSON.
- **Web UI** — a Phoenix LiveView app to browse, read, search, and visualize, reading the same DB.
- **Stable, scriptable CLI** — `--json` everywhere, shell completions, sensible exit codes.

Validated on a real corpus of ~1,500 sessions / 185k messages / 60k tool calls.

## Quick start

```bash
cargo build --release
./target/release/decant sync                 # ingest ~/.claude + ~/.codex into SQLite
./target/release/decant ls                   # list sessions (newest first)
./target/release/decant search "auth bug"    # full-text search across all sessions
./target/release/decant show 1               # read a full transcript
./target/release/decant stats --by model     # usage & cost rollup (tool|model|project|day)
./target/release/decant mcp stats            # MCP server leaderboard (calls, tools, errors)
./target/release/decant tool stats           # tool usage: built-in vs MCP, with error counts
./target/release/decant export 1 > s1.md     # export a transcript (Markdown, or --json)
./target/release/decant ls --json            # machine-readable output (stable DTO contract)
./target/release/decant project ls           # projects by session count + cost
./target/release/decant db info              # db path, size, schema version, row counts
./target/release/decant completion zsh       # shell completion script (bash|zsh|fish)
```

## Web app

A Phoenix LiveView UI in `web/` reads the same SQLite archive (read-only):

```bash
cd web && mix setup
DECANT_DB=/path/to/decant.db mix phx.server   # then open http://localhost:4000
```

Routes: sessions at `/`, a transcript at `/sessions/:id`, full-text search at
`/search`, usage/cost charts at `/analytics`, and tool/MCP usage at `/tools`.
The "Sync now" button reruns ingestion; or run `decant sync` from the CLI.

## Configuration

Flags or env vars (precedence: flag > env > platform default):
`--db` / `DECANT_DB`, `--claude-dir` / `DECANT_CLAUDE_DIR`, `--codex-dir` / `DECANT_CODEX_DIR`.
The default database lives under your platform data dir (macOS:
`~/Library/Application Support/decant/decant.db`; Linux: `~/.local/share/decant/decant.db`).

Global flags: `--json`, `-q/--quiet`, `--no-color` (honors `NO_COLOR`).

## How it works

```
~/.claude, ~/.codex  ──►  decant CLI (Rust)  ──►  SQLite (WAL + FTS5)  ──►  Phoenix web UI
   JSONL logs            parse · normalize          the data contract          read-only
                          · ingest · cost
```

- **The SQLite schema is the contract.** The Rust CLI owns and writes it
  (`crates/decant-core/src/schema_v1.sql`); the web app only reads. WAL mode lets
  the UI read while the CLI writes.
- **Sync is idempotent** — re-running only re-ingests files whose size/mtime
  changed. Malformed JSON lines are recorded in the `ingest_issue` table
  (non-fatal); exit code `3` signals "completed with parse issues" for CI.
- **Costs are estimated at ingest** from published per-model rates and stored on
  each session. Because sync skips unchanged files, refreshing rates after an
  upgrade won't rewrite existing rows — rebuild the DB (delete it and re-`sync`)
  to recompute historical costs.

The workspace is two crates: `decant-core` (a UI-agnostic library: parsing,
schema, ingest, cost, queries) and `decant-cli` (the `decant` binary).

## Development

```bash
cargo test --workspace                       # Rust tests
cargo fmt --all -- --check                   # format
cargo clippy --all-targets -- -D warnings    # lint
cd web && mix test                           # web tests
pre-commit install                           # run the same checks before each commit
```

See [AGENTS.md](AGENTS.md) for the full command list, conventions, and project
invariants (it's the source of truth for both humans and AI agents), and
[CONTRIBUTING.md](CONTRIBUTING.md) to get set up. CI runs the same gates on every
PR and push to `main`.

## Security & privacy

decant is local-first: it reads files already on your disk and makes no network
calls. Your session transcripts and the archive stay on your machine. Please
don't commit real session data or a personal archive to the repo. To report a
vulnerability, see [SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License 2.0](LICENSE).
