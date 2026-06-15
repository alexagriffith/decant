# AGENTS.md

Guidance for AI coding agents (and humans) working in this repo. Keep changes
small, tested, and consistent with the patterns already here. `CLAUDE.md` is a
symlink to this file; tool-specific files should stay thin and defer here.

## What this is

**decant** extracts Claude Code (`~/.claude/projects/*.jsonl`) and Codex
(`~/.codex/sessions/rollout-*.jsonl`) CLI sessions into a normalized,
full-text-searchable SQLite archive (WAL + FTS5). A long-running Rust **daemon**
(`decant-daemon`) owns that archive — it watches the source directories, ingests
automatically, and serves a versioned HTTP+JSON API on `127.0.0.1:4577`. The
`decant` CLI runs/manages the daemon and provides a scriptable interface, and a
Phoenix LiveView web app is a pure HTTP client of the daemon API.

## Layout

| Path | Responsibility |
|---|---|
| `crates/decant-core/` | Library: parsing, schema, ingest, cost, queries, stats, export. **UI-agnostic.** |
| `crates/decant-daemon/` | The long-running service: watcher + ingest task (single SQLite writer), `axum` HTTP API, SSE, auth/lock. **Owns the DB.** |
| `crates/decant-cli/` | Binary `decant`: argument parsing (clap) + all I/O/printing; runs the daemon (`daemon serve`) and manages it (`install`/`status`/…). |
| `docs/api/openapi.yaml` | The HTTP+JSON API (OpenAPI 3.1). **The data contract** between the daemon (server) and its clients (web, CLI). Also served at `/api/v1/openapi.yaml`. |
| `crates/decant-core/src/schema_v1.sql` | The SQLite schema — private to the daemon (an internal detail, no longer the cross-process contract). |
| `crates/decant-core/src/sources/` | Per-tool parsers: `claude.rs`, `codex.rs`. |
| `web/` | Phoenix 1.8 LiveView app (OTP app `:decant`); **a pure HTTP client of the daemon — never opens SQLite**. |
| `fixtures/` | Tiny synthetic session files used by Rust tests. |
| `web/test/fixtures/decant.db` | Tiny synthetic archive DB for daemon/core tests (the only DB in git). |
| `docs/superpowers/` | Design specs and implementation plans. |

## Setup

- Rust: stable toolchain (`rustup toolchain install stable`). SQLite is bundled (rusqlite `bundled`), no system lib needed.
- Web: Erlang/OTP 27+, Elixir 1.18+. `cd web && mix deps.get`.

## Commands

Run from the repo root unless noted. These are exactly what CI and pre-commit run.
The `justfile` mirrors every context below (`just` lists recipes by group;
`just check` runs all gates; `just daemon` / `just web` run the services).

```sh
# Rust
cargo build --workspace                       # debug build
cargo test --workspace                        # all Rust tests
cargo fmt --all -- --check                    # format check (drop --check to fix)
cargo clippy --all-targets -- -D warnings     # lint; warnings are errors

# Daemon (the service that owns the archive; binary `decant`)
cargo run -p decant-cli -- daemon serve       # run the daemon in the foreground (dev)
cargo run -p decant-cli -- daemon install     # install + load a macOS LaunchAgent (background)
cargo run -p decant-cli -- daemon status      # running? + health + last sync
cargo run -p decant-cli -- daemon logs -f     # tail the daemon log

# CLI read commands (binary is `decant`; or `cargo run -p decant-cli -- <args>`)
cargo run -p decant-cli -- ls                 # list sessions
cargo run -p decant-cli -- search "<query>"   # full-text search
cargo run -p decant-cli -- distill script     # runnable workflow script from real history (also: replay, skill)
cargo run -p decant-cli -- --db /tmp/x.db ls  # read a specific DB file directly (headless)

# Web (run inside web/; needs a running daemon — start `daemon serve` first)
cd web && mix test                            # LiveView + context tests (mock the daemon; no live one needed)
cd web && mix format --check-formatted        # format check
cd web && mix compile --warnings-as-errors    # compile clean
mix phx.server                                # dev server at http://localhost:4000 (talks to the daemon)
```

Config: the **daemon** owns the DB at `~/.decant/decant.db` (`DECANT_DB`), the
loopback port `4577` (`DECANT_DAEMON_PORT`), and the token/lock under `~/.decant`
(`DECANT_CONFIG_DIR`); source dirs override with `DECANT_CLAUDE_DIR` /
`DECANT_CODEX_DIR`. The **web app** reaches the daemon via `DECANT_DAEMON_URL`
(default `http://127.0.0.1:4577`) + `DECANT_DAEMON_TOKEN` (else
`~/.decant/daemon.token`). CLI read commands accept `--db` / `DECANT_DB`.

## Definition of done

A change is ready when, for the area you touched:

- Rust: `cargo test --workspace` **and** `cargo fmt --all -- --check` **and** `cargo clippy --all-targets -- -D warnings` all pass.
- Web: `cd web && mix test` **and** `mix format --check-formatted` **and** `mix compile --warnings-as-errors` all pass.
- New behavior has tests (this repo is built test-first). Don't weaken or delete a test to make it pass.

## Project invariants (do not break)

1. **`decant-core` is UI-agnostic.** No `println!`, `eprintln!`, `print!`, or direct stdout/stderr in `crates/decant-core`. All output and exit-code policy lives in `decant-cli` (see its `output` module). Core returns data and `Result`s. (The daemon likewise emits no stdout/stderr beyond `tracing` logs.)
2. **The daemon is the single owner of SQLite; the web app is a pure HTTP client.** Only `decant-daemon` opens the database — it is the only writer (one exclusive write connection, owned by the ingest task) *and* the only reader (an r2d2 read pool for handlers). The data contract is the **versioned HTTP+JSON API** (`docs/api/openapi.yaml`), not the SQLite schema. Schema changes still go in `crates/decant-core/src/schema_v1.sql` + a migration in `schema.rs`, but the schema is now an internal detail. **Phoenix never opens SQLite** — no Ecto, no `Decant.Repo`, no direct DB access in `web/`; add new data needs as daemon API endpoints and consume them over HTTP.
3. **Costs are computed at ingest** (`cost::estimate_cost`) and stored in `session.estimated_cost_usd`. Ingest skips files whose size+mtime are unchanged, so editing pricing does **not** retroactively update existing rows — rebuild the DB (delete it and let the daemon re-ingest) to recompute. Model strings are normalized in `cost::canonical_model` (handles Bedrock ARNs, date/`[1m]` suffixes, aliases).
4. **WAL mode** lets the daemon's read pool serve requests while its single writer ingests. Don't change journal mode in the writer.
5. **The daemon is local-first and loopback-only.** It binds `127.0.0.1` only, guards Host/Origin, and requires the bearer token; don't widen the bind address or add outbound network calls.

## Security & data privacy

- **Never commit secrets** (API keys, tokens, `.env`). `detect-private-key` runs in pre-commit, but don't rely on it.
- **Never commit real session data or a personal archive DB.** `~/.claude` and `~/.codex` hold private transcripts. The only DB in git is the tiny synthetic `web/test/fixtures/decant.db`. Generate fixtures from synthetic data only.
- Don't add **outbound/remote** network calls anywhere; decant is local-first and offline. The only networking is the daemon's loopback HTTP API (`127.0.0.1`) and the CLI/web clients talking to it over loopback. `decant-core` stays I/O-policy-free (no networking at all).

## Conventions

- Commits: Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `test:`, scope optional, e.g. `fix(cost): ...`). Sign commits if your environment is set up for it.
- Match the surrounding code's style, naming, and comment density. Prefer small, focused files.
- Branch off `main`; open a PR. CI (`.github/workflows/ci.yml`) must be green.
- Install hooks once: `pre-commit install` (see `.pre-commit-config.yaml`).

## Scope of these instructions

Agents read AGENTS.md from the repo root down to the working directory; the
closest file wins on conflicts. Sub-project specifics live in
`crates/AGENTS.md` and `web/AGENTS.md`. Reusable workflows may be packaged as
skills (`SKILL.md`) and external tools exposed over MCP; prefer those over
ad-hoc scripts when available.

## When stuck

If tests fail in a way you can't resolve, the plan looks wrong, or a change
needs a schema/data-contract change that affects both Rust and web, stop and
ask rather than guessing. Report what failed with the actual output.
