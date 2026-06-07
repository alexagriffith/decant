# AGENTS.md

Guidance for AI coding agents (and humans) working in this repo. Keep changes
small, tested, and consistent with the patterns already here. `CLAUDE.md` is a
symlink to this file; tool-specific files should stay thin and defer here.

## What this is

**decant** extracts Claude Code (`~/.claude/projects/*.jsonl`) and Codex
(`~/.codex/sessions/rollout-*.jsonl`) CLI sessions into a normalized,
full-text-searchable SQLite archive (WAL + FTS5), exposed via a Rust CLI and a
Phoenix LiveView web app.

## Layout

| Path | Responsibility |
|---|---|
| `crates/decant-core/` | Library: parsing, schema, ingest, cost, queries, stats, export. **UI-agnostic.** |
| `crates/decant-cli/` | Binary `decant`: argument parsing (clap) + all I/O/printing. |
| `crates/decant-core/src/schema_v1.sql` | The SQLite schema. **The data contract** between CLI (writer) and web (reader). |
| `crates/decant-core/src/sources/` | Per-tool parsers: `claude.rs`, `codex.rs`. |
| `web/` | Phoenix 1.8 LiveView app (OTP app `:decant`); **reads the DB read-only**. |
| `fixtures/` | Tiny synthetic session files used by Rust tests. |
| `web/test/fixtures/decant.db` | Tiny synthetic archive DB for web tests (the only DB in git). |
| `docs/superpowers/` | Design specs and implementation plans. |

## Setup

- Rust: stable toolchain (`rustup toolchain install stable`). SQLite is bundled (rusqlite `bundled`), no system lib needed.
- Web: Erlang/OTP 27+, Elixir 1.18+. `cd web && mix deps.get`.

## Commands

Run from the repo root unless noted. These are exactly what CI and pre-commit run.

```sh
# Rust
cargo build --workspace                       # debug build
cargo test --workspace                        # all Rust tests
cargo fmt --all -- --check                    # format check (drop --check to fix)
cargo clippy --all-targets -- -D warnings     # lint; warnings are errors

# CLI (binary is `decant`; or `cargo run -p decant-cli -- <args>`)
cargo run -p decant-cli -- sync               # ingest sessions into the archive
cargo run -p decant-cli -- ls                 # list sessions
cargo run -p decant-cli -- search "<query>"   # full-text search
cargo run -p decant-cli -- --db /tmp/x.db sync # use a specific DB

# Web (run inside web/)
cd web && mix test                            # LiveView + context tests
cd web && mix format --check-formatted        # format check
cd web && mix compile --warnings-as-errors    # compile clean
DECANT_DB=/path/to/decant.db mix phx.server   # dev server at http://localhost:4000
```

Config (CLI): `--db` flag > `DECANT_DB` env > platform default
(`~/Library/Application Support/decant/decant.db` on macOS,
`~/.local/share/decant/decant.db` on Linux). Source dirs override with
`DECANT_CLAUDE_DIR` / `DECANT_CODEX_DIR`. The web app reads `DECANT_DB`.

## Definition of done

A change is ready when, for the area you touched:

- Rust: `cargo test --workspace` **and** `cargo fmt --all -- --check` **and** `cargo clippy --all-targets -- -D warnings` all pass.
- Web: `cd web && mix test` **and** `mix format --check-formatted` **and** `mix compile --warnings-as-errors` all pass.
- New behavior has tests (this repo is built test-first). Don't weaken or delete a test to make it pass.

## Project invariants (do not break)

1. **`decant-core` is UI-agnostic.** No `println!`, `eprintln!`, `print!`, or direct stdout/stderr in `crates/decant-core`. All output and exit-code policy lives in `decant-cli` (see its `output` module). Core returns data and `Result`s.
2. **Rust owns the schema; the web app is read-only.** Schema changes go in `crates/decant-core/src/schema_v1.sql` + a migration in `schema.rs`. Do **not** add Ecto migrations for the archive tables, and do not write to the DB from `web/`.
3. **Costs are computed at ingest** (`cost::estimate_cost`) and stored in `session.estimated_cost_usd`. `sync` skips files whose size+mtime are unchanged, so editing pricing does **not** retroactively update existing rows — rebuild the DB (delete it and re-`sync`) to recompute. Model strings are normalized in `cost::canonical_model` (handles Bedrock ARNs, date/`[1m]` suffixes, aliases).
4. **WAL mode** lets the web app read while the CLI writes. Don't change journal mode in the writer.

## Security & data privacy

- **Never commit secrets** (API keys, tokens, `.env`). `detect-private-key` runs in pre-commit, but don't rely on it.
- **Never commit real session data or a personal archive DB.** `~/.claude` and `~/.codex` hold private transcripts. The only DB in git is the tiny synthetic `web/test/fixtures/decant.db`. Generate fixtures from synthetic data only.
- Don't add network calls to `decant-core` or the CLI; decant is a local-first, offline tool.

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
