# AGENTS.md — Rust workspace

Scope: `decant-core` (library), `decant-daemon` (the service that owns the
archive + serves the HTTP API), and `decant-cli` (binary `decant`). See the root
`AGENTS.md` for repo-wide rules.

## Commands

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo run -p decant-cli -- <args>            # the CLI (incl. `daemon serve`)
cargo run -p decant-daemon                   # run the daemon binary directly
```

## Rules specific to this workspace

- **`decant-core` must not print.** No `println!`/`eprintln!`/`print!`/`dbg!` or
  stdout/stderr writes in the library — it returns data and `Result`s. All
  user-facing output, formatting, colors, and exit codes live in `decant-cli`
  (see its `output` module). This keeps the core reusable by the daemon and
  tests.
- **The daemon owns SQLite; the API is the data contract.** `decant-daemon` is
  the only process that opens the DB — single exclusive writer (the ingest task)
  plus an r2d2 read pool for handlers. The cross-process contract is the HTTP+JSON
  API in `docs/api/openapi.yaml` (served at `/api/v1/openapi.yaml`, embedded via
  `include_str!`); keep handlers, the OpenAPI doc, and the daemon's contract tests
  in sync. `decant-daemon` emits no stdout/stderr beyond `tracing` logs.
- **Schema is an internal detail of the daemon.** It lives in
  `decant-core/src/schema_v1.sql`, applied by `schema::migrate`. Any change needs
  a new migration; it no longer needs to stay compatible with an external reader
  (only the daemon reads it), but API responses that expose it must keep the
  contract stable.
- **Pricing/normalization** live in `decant-core/src/cost.rs`. Add new models to
  `default_pricing` and map their string variants in `canonical_model`. Costs
  are computed at ingest, so rebuild the DB to recompute after a change.
- Tests live inline (`#[cfg(test)] mod tests`) next to the code, and parsers use
  the shared files under `../fixtures/`. Write the test first.
- Keep `clippy` clean at `-D warnings`; put `#[cfg(test)]` modules at the end of
  a file (clippy warns on items after a test module).
