# decant — Phoenix Web App (Plan 3) Status & Continuation

**Status:** Foundation complete on branch `feat/web` (commit `28bce7e`), verified end-to-end against the real 1,521-session archive. Built exploratorily (Phoenix scaffolding is generated, not hand-written), so this doc records what's done and lays out the remaining LiveViews as the continuation plan.

## Architecture (as built)

- Phoenix 1.8 LiveView app under `web/` — OTP app `decant`, contexts `Decant.*`, web layer `DecantWeb.*`.
- `Decant.Repo` (ecto_sqlite3) points at **`DECANT_DB`** (the Rust-owned archive) via `config/runtime.exs`, read-only. No Ecto migrations — the Rust CLI owns the schema (the data contract).
- `Decant.Archive` — the read context. Raw SQL (`Repo.query!`) → plain maps, mirroring the Rust query API:
  - `list_sessions/1`, `get_session/1` (summary + grouped messages/blocks), `search/2` (FTS5 `MATCH` + `snippet` + `bm25`).
- LiveViews (wired in `router.ex`):
  - `DecantWeb.SessionLive.Index` — `/` — sessions table (tool, title, model, msgs, cost, started), links to reader.
  - `DecantWeb.SessionLive.Show` — `/sessions/:id` — full transcript (messages → blocks: text/thinking/tool_use/tool_result).
  - `DecantWeb.SessionLive.Search` — `/search` — live FTS as-you-type (`phx-change` + `phx-debounce`), ranked snippets.

## Verified

`DECANT_DB=<archive> mix phx.server` → `GET /` 200 (renders 200 sessions), `GET /sessions/:id` 200 (transcript with role labels), `GET /search` 200. `Decant.Archive` confirmed reading the real DB (list/get/search all work; exqlite includes FTS5). `mix test` green (4).

## Remaining (continuation)

1. **Analytics dashboard** (`/analytics`) — sessions/tokens/cost over time, model split, top projects. Render with **contex** (pure-Elixir SVG charts, zero JS) over `Decant.Archive` aggregation queries (mirror the Rust `stats` module: totals, by_dimension).
2. **Tools/MCP dashboard** (`/tools`) — tool leaderboard, MCP-server leaderboard (calls, tools-per-server, error rates) from `tool_call`. Mirror the Rust `stats::tool_usage`/`mcp_usage`.
3. **"Sync now" button** — header control that runs `System.cmd("decant", ["sync"])` in a `Task`, streams progress to the LiveView, refreshes on completion, shows last-sync time. (The CLI is the writer; the web app stays read-only otherwise.)
4. **LiveView tests** — seed a small fixture `decant.db` (build it once with the `decant` CLI from `fixtures/`), point `config/test.exs` `Decant.Repo` at it, and add `Phoenix.LiveViewTest` coverage for index/show/search.
5. **Polish** — index filters (tool/project/date), pagination, transcript styling (collapsible thinking, code highlighting, MCP-server badges, error indicators), empty-state ("run `decant sync`").
6. **Distribution** — `mix release` + a `just web` recipe; document the two-process model (CLI syncs, web reads).

## Notes / contracts

- The SQLite schema is the contract; `Decant.Archive` reads columns the Rust side writes. If the Rust schema bumps, update `Decant.Archive` (and consider a shared schema-version check).
- WAL mode (set by the Rust side) lets the web app read while `decant sync` writes concurrently.
- Built assets (`priv/static/assets`), `_build/`, and `deps/` are gitignored; run `mix setup` after clone.
