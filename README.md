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
- **Distill into runnable artifacts** — `decant distill` turns the history of what
  *actually worked* into a workflow **script** (frequency-ranked real commands), a
  faithful session **replay**, or a pre-filled **skill / AGENTS.md** file.
  Deterministic, secret-redacted, review-before-run — no LLM, no network.
- **Full-text search** (SQLite FTS5) across every message and tool call.
- **Usage & cost analytics** — rollups by tool, model, project, or day, with cost
  estimates (Claude and GPT-5 families, including Bedrock-style model ids).
- **Tool & MCP insights** — built-in vs MCP tool usage, per-server leaderboards, error rates.
- **Export** transcripts to Markdown or JSON.
- **Background daemon** — a long-running Rust service watches `~/.claude` + `~/.codex`,
  ingests automatically, and serves a local HTTP+JSON API (no manual sync).
- **Web UI** — a Phoenix LiveView app to browse, read, search, and visualize, talking to the daemon over HTTP.
- **Stable, scriptable CLI** — `--json` everywhere, shell completions, sensible exit codes.

Validated on a real corpus of ~1,500 sessions / 185k messages / 60k tool calls.

## Quick start

TypeScript distribution work is staged in
[`docs/distribution.md`](docs/distribution.md): `npx @dosu/decant`, Docker, and
source installs all route through the single Bun + TypeScript app. The Rust
commands below remain the pre-cutover path until Phase 6 removes the old tree.

```bash
cargo build --release
./target/release/decant sync                 # ingest ~/.claude + ~/.codex into SQLite
./target/release/decant ls                   # list sessions (newest first)
./target/release/decant search "auth bug"    # full-text search across all sessions
./target/release/decant show 1               # read a full transcript
./target/release/decant stats --by model     # usage & cost rollup (tool|model|project|day)
./target/release/decant mcp stats            # MCP server leaderboard (calls, tools, errors)
./target/release/decant tool stats           # tool usage: built-in vs MCP, with error counts
./target/release/decant distill script        # workflow script mined from your real command history
./target/release/decant distill replay 1       # reproduce a session's commands + file writes
./target/release/decant distill skill --kind agents  # AGENTS.md section from hot files + proven commands
./target/release/decant export 1 > s1.md     # export a transcript (Markdown, or --json)
./target/release/decant ls --json            # machine-readable output (stable DTO contract)
./target/release/decant project ls           # projects by session count + cost
./target/release/decant db info              # db path, size, schema version, row counts
./target/release/decant completion zsh       # shell completion script (bash|zsh|fish)
```

## The daemon + web app

decant runs as a local background **daemon** that owns the SQLite archive: it
watches `~/.claude` and `~/.codex`, ingests changes automatically (no manual
`sync`), and exposes a versioned HTTP+JSON API on `127.0.0.1:4577`. The Phoenix
web app is a **pure client** of that API — it never opens SQLite.

Run the daemon, then the web app:

```bash
cargo run -p decant-cli -- daemon serve      # run the daemon in the foreground (dev)
# …or run it in the background as a macOS LaunchAgent (starts at login):
cargo run -p decant-cli -- daemon install
cargo run -p decant-cli -- daemon status     # is it running? health + last sync

cd web && mix setup
mix phx.server                               # then open http://localhost:4000
```

The web app finds the daemon via `DECANT_DAEMON_URL` (default
`http://127.0.0.1:4577`) and authenticates with the bearer token the daemon
writes to `~/.decant/daemon.token` (override with `DECANT_DAEMON_TOKEN`). If the
daemon isn't running, the UI shows a clear "service isn't running" state.

Routes: sessions at `/`, a transcript at `/sessions/:id`, full-text search at
`/search`, usage/cost charts at `/analytics`, and tool/MCP usage at `/tools`.
Ingestion is automatic; the API contract is documented in
[`docs/api/openapi.yaml`](docs/api/openapi.yaml) (also served live at
`/api/v1/openapi.yaml`).

Manage the service with `decant daemon install | uninstall | start | stop |
status | logs [-f]` (macOS for now; Linux `systemd` is a documented future add).

## Configuration

**Daemon** (the owner of the archive): the private SQLite DB defaults to
`~/.decant/decant.db` (override with `DECANT_DB`); the loopback port defaults to
`4577` (override with `DECANT_DAEMON_PORT`). The bearer token and single-instance
lock live under `~/.decant/` (`daemon.token`, `daemon.lock`; the dir is
overridable with `DECANT_CONFIG_DIR`). Source directories override with
`DECANT_CLAUDE_DIR` / `DECANT_CODEX_DIR`.

**Web app**: `DECANT_DAEMON_URL` (default `http://127.0.0.1:4577`) and
`DECANT_DAEMON_TOKEN` (else read from `~/.decant/daemon.token`).

**CLI read commands** (`ls`, `search`, …) accept `--db` / `DECANT_DB` for
ad-hoc/headless use against a database file directly.

Global flags: `--json`, `-q/--quiet`, `--no-color` (honors `NO_COLOR`).

## How it works

```
~/.claude, ~/.codex ─►  decant-daemon (Rust)  ─►  SQLite (WAL + FTS5, private)
   JSONL logs            watch · ingest · cost          owned by the daemon
                              │  HTTP+JSON API (127.0.0.1:4577) + SSE
                              ▼
                         Phoenix web UI  ·  decant CLI  (HTTP clients)
```

- **The HTTP+JSON API is the contract.** The daemon (`crates/decant-daemon`) is
  the single owner of SQLite — the only reader *and* the only writer — and
  exposes a versioned API (OpenAPI 3.1, [`docs/api/openapi.yaml`](docs/api/openapi.yaml)).
  Phoenix never opens the database; it is a pure HTTP client. WAL mode lets the
  daemon's read pool serve requests while its single writer ingests.
- **Ingestion is automatic and idempotent** — the daemon watches the source
  directories and re-ingests only files whose size/mtime changed (a periodic
  fallback sync covers any missed filesystem events). Malformed JSON lines are
  recorded in the `ingest_issue` table (non-fatal).
- **Costs are estimated at ingest** from published per-model rates and stored on
  each session. Because ingest skips unchanged files, refreshing rates after an
  upgrade won't rewrite existing rows — rebuild the DB (delete it and let the
  daemon re-ingest) to recompute historical costs.

The workspace is three crates: `decant-core` (a UI-agnostic library: parsing,
schema, ingest, cost, queries), `decant-daemon` (the long-running service that
owns the archive and serves the API), and `decant-cli` (the `decant` binary,
which runs the daemon and provides the scriptable CLI).

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
