# AGENTS.md — Rust workspace

Scope: `decant-core` (library) and `decant-cli` (binary `decant`). See the root
`AGENTS.md` for repo-wide rules.

## Commands

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo run -p decant-cli -- <args>
```

## Rules specific to this workspace

- **`decant-core` must not print.** No `println!`/`eprintln!`/`print!`/`dbg!` or
  stdout/stderr writes in the library — it returns data and `Result`s. All
  user-facing output, formatting, colors, and exit codes live in `decant-cli`
  (see its `output` module). This keeps the core reusable by the web app and
  tests.
- **Schema is the data contract.** It lives in
  `decant-core/src/schema_v1.sql`, applied by `schema::migrate`. Any change
  needs a new migration and must stay compatible with the read-only web reader.
- **Pricing/normalization** live in `decant-core/src/cost.rs`. Add new models to
  `default_pricing` and map their string variants in `canonical_model`. Costs
  are computed at ingest, so rebuild the DB to recompute after a change.
- Tests live inline (`#[cfg(test)] mod tests`) next to the code, and parsers use
  the shared files under `../fixtures/`. Write the test first.
- Keep `clippy` clean at `-D warnings`; put `#[cfg(test)]` modules at the end of
  a file (clippy warns on items after a test module).
