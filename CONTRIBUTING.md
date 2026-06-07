# Contributing to decant

Thanks for your interest in improving decant! This guide gets you set up and
explains how we work. [AGENTS.md](AGENTS.md) is the canonical reference for
commands, conventions, and project invariants — this file is the friendly
on-ramp.

## Prerequisites

- **Rust** (stable). SQLite is bundled via `rusqlite`, so no system SQLite is needed.
- **Elixir 1.18+ / Erlang OTP 27+** — only if you work on the `web/` app.
- **[pre-commit](https://pre-commit.com)** — `pipx install pre-commit` (or `brew install pre-commit`).

## Get set up

```bash
git clone https://github.com/onlydole/decant
cd decant
cargo build --workspace      # build the Rust CLI + core
pre-commit install           # install the git hooks (runs fmt/clippy/format on commit)
cd web && mix deps.get       # only if working on the web app
```

To try it end to end, run `cargo run -p decant-cli -- sync` to build your archive,
then `cargo run -p decant-cli -- ls`.

## Make your change

We work test-first. Add or update tests alongside your change, keep commits
small and focused, and match the style of the surrounding code.

Before you push, make sure the relevant gates pass — these are exactly what CI
and the pre-commit hooks run:

```bash
# Rust
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# Web (from web/)
mix test
mix format --check-formatted
mix compile --warnings-as-errors
```

`pre-commit run --all-files` runs the whole suite of hooks at once.

## Project invariants (please don't break these)

These keep the architecture clean — see AGENTS.md for the full rationale:

1. **`decant-core` is UI-agnostic.** No printing (`println!`/`eprintln!`) in the
   core library; all I/O and exit-code policy lives in `decant-cli`.
2. **Rust owns the SQLite schema; the web app reads it read-only.** Schema
   changes go in `crates/decant-core/src/schema_v1.sql` plus a migration in
   `schema.rs`, never as Ecto migrations.
3. **No network calls** in the CLI or core — decant is local-first and offline.
4. **Never commit secrets or real session data.** The only database in git is the
   tiny synthetic test fixture at `web/test/fixtures/decant.db`.

Adding a model's pricing? Update `default_pricing` and map its string variants in
`canonical_model` (both in `crates/decant-core/src/cost.rs`), and add a test.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org):
  `feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`, `style:`, `ci:` —
  with an optional scope, e.g. `fix(cost): …` or `test(web): …`.
- Signing commits (`git commit -S`) is appreciated if your environment supports it.
- Branch off `main`, open a PR, and make sure CI is green. Describe what changed
  and why; link any related issue.

## Reporting bugs and proposing features

Open an issue with enough detail to reproduce (decant version, OS, the command you
ran, and what happened vs. what you expected). For security issues, do **not** open
a public issue — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), the same license as the project.
