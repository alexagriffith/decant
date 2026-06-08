# decant Service Architecture — Design

**Goal:** Turn decant from a one-shot CLI that Phoenix reads SQLite behind into a long-running Rust **service** that owns the data path (watch, ingest, quantify, recommend) and exposes its own HTTP API, with Phoenix as a pure client.

**Architecture:** A new `decant-daemon` binary runs a `tokio` runtime hosting a filesystem watcher, incremental ingest, an `axum` HTTP+JSON API on loopback, and an SSE change-stream. SQLite becomes the daemon's private store. Phoenix talks to the daemon over `http://127.0.0.1` with a bearer token and consumes change events over SSE, rebroadcasting them to LiveViews via `Phoenix.PubSub`.

**Tech stack:** Rust (`tokio`, `axum`, `tower`, `notify`, `r2d2` + `rusqlite`, `serde`/`serde_json`, `tracing`), reusing `decant-core` unchanged. Elixir/Phoenix client (`Req` over a `Finch` pool, `server_sent_events`, `Phoenix.PubSub`, `assign_async` + streams). Contract documented as OpenAPI 3.1.

---

## 1. Context and motivation

Today (locked design as of the foundation work): the Rust CLI (`decant-core` + `decant-cli`) ingests Claude Code and Codex session logs into a SQLite archive; Phoenix 1.8 / LiveView 1.1 reads that **same** SQLite file read-only; a Phoenix `AutoSync` GenServer watches `~/.claude` + `~/.codex` and shells out to `decant sync`; the "Sync" button shells out too.

We are deliberately revising that. The owner wants **Rust to own its own API and be the service that watches and quantifies coding-agent runs, with Phoenix as the client.** Two structural wins fall out immediately:

- The shared-reader / WAL concurrency dance disappears: only the daemon touches SQLite.
- The watch + ingest logic leaves Phoenix (`AutoSync`) and lives where the data does.

This also unblocks the originating feature request: **storing recommendations and their state in the DB**, behind a real API contract, with "implemented" auto-detected — which becomes a first-class API resource owned by the daemon (see §5).

### Revised invariant

This **supersedes** the prior invariant "Rust owns the schema; the web app reads the same SQLite file read-only." The new boundary:

- **Rust (daemon) owns the schema, all reads, and all writes to SQLite.** SQLite is private to the daemon.
- **Phoenix never opens SQLite.** It is a pure HTTP client of the daemon API.
- The data contract is no longer "the SQLite schema"; it is the **versioned HTTP+JSON API** (OpenAPI 3.1).

`decant-core` stays UI-agnostic and gains no I/O policy; the daemon and CLI own process/output concerns.

---

## 2. Architecture overview

```
~/.claude, ~/.codex
      │  (notify watcher, debounced)  +  periodic fallback sync
      ▼
[ decant-daemon (tokio) ]
   ├─ ingest task ──► exclusive write conn ──►  SQLite (WAL, private)
   │                                              ▲
   ├─ axum HTTP API  ── r2d2 read pool ───────────┘
   │     GET /api/v1/... (reads), POST /api/v1/recommendations/:key/mark-implemented
   ├─ SSE  GET /api/v1/events  (change-stream)
   └─ graceful shutdown (broadcast channel) + PID lock file
      ▲ 127.0.0.1 only, bearer token, Host/Origin checks
      │ HTTP+JSON (Req/Finch)        │ SSE (server_sent_events)
[ Phoenix (pure client) ]
   ├─ Decant.Daemon (API module: Req + token + version check + service-down state)
   ├─ DaemonEvents GenServer: consumes SSE ─► Phoenix.PubSub "daemon:changes"
   ├─ HealthCheck GenServer: polls /health, drives degraded UI
   └─ LiveViews: assign_async + streams, cursor pagination, server-side search
```

---

## 3. Component: `decant-daemon` crate

A new workspace crate `crates/decant-daemon` with binary entrypoint `decant-daemon` (and a `decant daemon …` CLI surface, see §8). It depends on `decant-core` with **no changes to that library** (reuses parsing, schema, ingest, cost, queries, stats).

**Runtime (tokio multi-thread):**
- **Watcher task** — `notify` v6 over `~/.claude/projects`, `~/.codex/sessions`, `~/.codex/archived_sessions`. File events are debounced (~1–2 s) into a single "sync needed" flag.
- **Sync task (the only writer)** — wakes on the debounced flag and on a periodic interval (fallback, ~30–60 s), acquires the exclusive write connection, runs `decant_core` ingest (the existing incremental, size+mtime-skipping sync), and on commit broadcasts a change event (see SSE) and regenerates recommendations (§5). Failures are logged and retried next interval; the daemon never panics out of a sync.
- **HTTP server** — `axum` bound to `127.0.0.1:<port>` (default chosen + configurable). Handlers take a pooled read connection.
- **Shutdown** — `tokio::signal` (SIGINT/SIGTERM) triggers a `broadcast` channel; the sync task finishes its current transaction, the server drains in-flight requests (grace window), then exit 0.

**SQLite concurrency:** WAL mode; one **exclusive write connection** owned by the sync task; an **r2d2 read pool** (size ~3–5) for handlers; `PRAGMA busy_timeout = 5000` on all connections. A **PID lock file** (`~/.decant/daemon.lock`) prevents two daemons writing the same DB and lets the CLI detect a running daemon.

**Config & observability:** port (default `4577`, well clear of Phoenix's `4000`), DB path, log dir, sync interval, debounce delay via env/flag (later a `~/.config/decant/daemon.toml`). `tracing` structured logs (info: sync start/end/stats; debug: file events; error: parse/SQL failures). `GET /api/v1/health` returns `{api_version, db_schema_version, uptime_seconds, last_sync_at, connected: true}`.

---

## 4. Component: Security model

Loopback is **not** a trust boundary — a malicious web page can reach `127.0.0.1` via DNS-rebinding, and CORS does not prevent the request from executing (it only hides the response). Defense-in-depth, applied as `tower` middleware:

1. **Bind `127.0.0.1` (and `[::1]`) only** — never `0.0.0.0`.
2. **Host-header allowlist** — reject any request whose `Host` is not in `{127.0.0.1:<port>, localhost:<port>, [::1]:<port>}` with `403`. This defeats DNS-rebinding (the rebound request still carries the attacker's `Host`).
3. **Bearer token** — a 32-byte random token generated on first start, stored at `~/.decant/daemon.token` (mode `0600`, plain hex). Required on all `/api/*` requests; constant-time compare; missing/empty → `401`. Phoenix reads the token at startup (file or `DECANT_DAEMON_TOKEN`) and sends `Authorization: Bearer …` on every request.
4. **Origin checks on writes** — for state-mutating endpoints, require `Origin` to be absent (CLI/agent) or in the Phoenix-origin allowlist; otherwise `403`.
5. **Version stamping** — every response carries `X-Decant-API-Version`; Phoenix validates at startup and shows a "version mismatch, restart the service" state on skew.

The token protects against cross-process/drive-by access, not local privilege escalation (Unix perms handle that). Documented as such.

---

## 5. Component: HTTP API contract (`/api/v1`)

JSON over HTTP, URL-versioned (`/api/v1/…`; `/api/v2/…` reserved for breaking changes). **OpenAPI 3.1 is the source of truth**, committed at `docs/api/openapi.yaml` and served at `GET /api/v1/openapi.yaml`. Principles: **server-side aggregation only** (never raw rows), **cursor pagination**, a **consistent envelope**, idempotent writes.

**Envelope:**
```json
{ "data": <payload>, "meta": { "pagination": {"next_cursor": "…", "has_more": true, "total_count": 1576, "page_size": 50}, "filters_applied": {…}, "sync": {"in_progress": false, "last_sync_at": "…"}, "timestamp": "…" }, "errors": [] }
```
Errors: `{ "error": { "code": "INVALID_FILTER", "message": "…", "details": {…}, "hint": "…", "request_id": "…" } }` with appropriate 4xx/5xx.

**Endpoints (mirror today's pages):**

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/v1/health` | liveness + versions (no auth needed for liveness ping) |
| GET | `/api/v1/sessions` | list (filters `from,to,tool,model,project`, `sort`, `limit`, `cursor`) |
| GET | `/api/v1/sessions/:id` | detail: summary + stats + messages/blocks |
| POST | `/api/v1/search` | FTS5 over blocks; body `{q, limit, cursor}`; returns hits + snippets |
| GET | `/api/v1/analytics/summary` | totals scoped to filters |
| GET | `/api/v1/analytics/by-dimension?dim=tool\|model\|project\|day` | ranked rollups (paginated for high-cardinality) |
| GET | `/api/v1/analytics/activity` | `by_hour[24]`, `by_weekday[7]` (local time) + peak flags + `timezone` |
| GET | `/api/v1/analytics/model-sparklines` | `{models: {model: [counts]}, days: [dates]}` |
| GET | `/api/v1/tools/usage` | per-tool calls/errors/error_rate (`?errors_only`) |
| GET | `/api/v1/tools/mcp-usage` | per-MCP-server rollup |
| GET | `/api/v1/recommendations?status=open\|implemented\|all` | signals + catalog with state |
| POST | `/api/v1/recommendations/mark-implemented` | idempotent state write; body `{key, source, note}` (key in body, not path, since keys contain `:`) |
| GET | `/api/v1/metadata/date-bounds` | min/max session date |
| GET | `/api/v1/metadata/sync-status` | last sync, in-progress, pending, errors |
| GET | `/api/v1/events` | **SSE** change-stream (see §7) |

**Pagination:** opaque cursor `base64({rowid, sort_key})`; responses include `next_cursor`, `has_more`, `total_count`. Composite sort keys (e.g. `started_at DESC, id DESC`) break ties. Search uses `POST` (avoids URL limits) and validates FTS5 syntax (malformed → `400` with a hint, never a 500).

**Key JSON shapes** (abbreviated; full schema in OpenAPI): session summary (id, tool, source_session_id, title, model, project, started_at, ended_at, message_count, token totals incl. cache, estimated_cost_usd); session detail adds `stats` (duration_seconds, tokens, `cost_breakdown`) + `messages[]` (seq, role, timestamp, model, tokens, `blocks[]`); dimension row (key, sessions, messages, tool_calls, tokens, cost); tool row (tool_name, tool_kind, mcp_server, calls, errors, error_rate); recommendation (see §6).

---

## 6. Recommendations subsystem (the originating feature)

Recommendation **generation moves to `decant-core`** (ported from today's Elixir `Decant.Insights`): the data-signals (error hotspots, heavy MCP servers, heavy tools, cost concentration) and the evergreen catalog (AGENTS.md, CLAUDE.md, Skills, slash commands, subagents, MCP, hooks). At each sync, core regenerates them and **materializes them with state** into a Rust-owned table, preserving existing state.

**Schema (added to `schema_v1.sql` + a `schema.rs` migration; Rust-owned):**
```sql
CREATE TABLE recommendation (
  key            TEXT PRIMARY KEY,   -- stable: "catalog:agents-md", "signal:error:StructuredOutput", "signal:heavy-server:claude_ai_Exa"
  kind           TEXT NOT NULL,      -- "signal" | "catalog"
  category       TEXT,
  title          TEXT NOT NULL,
  detail         TEXT,
  suggestion     TEXT,
  prompt         TEXT,               -- agent handoff prompt
  url            TEXT,
  link_label     TEXT,
  icon           TEXT,
  tone           TEXT,
  score          REAL,               -- ranking
  status         TEXT NOT NULL DEFAULT 'open',   -- 'open' | 'implemented'
  status_source  TEXT,               -- 'agent' | 'activity' | 'manual'
  note           TEXT,
  first_seen_at  TEXT,
  updated_at     TEXT,
  implemented_at TEXT
);
```
(`dismissed`/`archived` are reserved as future statuses; out of scope for v1.)

**Stable keys** make state survive re-sync. Re-sync upserts by key and **never clobbers an existing `implemented`**.

**How "implemented" flips (auto-detect, per the owner's direction):**
1. **Agent reports back (primary).** When the user clicks "Run in <agent>", Phoenix seeds the prompt with the recommendation `key` and a trailing instruction: do the work, **verify it actually runs**, then call the contract to mark it implemented. The launched agent calls `POST /api/v1/recommendations/mark-implemented` with `{key, source: "agent", note}`. Optionally the daemon supplies a small verifying hook template the agent can install.
2. **Activity auto-resolve (secondary).** At sync, core flips activity-observable open signals to `implemented` (source `activity`) — e.g. an error-hotspot signal that no longer regenerates because the tool's error rate dropped.

Phoenix renders **open** signals/recommendations as today and moves **implemented** ones into a quieter "Implemented" section showing when/how.

---

## 7. Phoenix as a pure client

**API module `Decant.Daemon`.** Wraps `Req` over a configured `Finch` pool (size ~10, count 2; lazy). Injects the bearer token and `X-Requested-API-Version`. Normal calls time out at ~5 s; decodes the envelope; surfaces a clean `{:error, :service_unavailable}` on connection failure. `Decant.Archive`'s direct SQL is removed; contexts call `Decant.Daemon`.

**Realtime via SSE → PubSub.** The daemon exposes `GET /api/v1/events` (SSE). A supervised `Decant.DaemonEvents` GenServer holds a long-lived SSE connection (`Req` `into: :self` + the `server_sent_events` parser), stores `Last-Event-ID` for reconnect, and **rebroadcasts compact events** (event type + ids, never large payloads) to `Phoenix.PubSub` topic `"daemon:changes"`. This **replaces `AutoSync`**. LiveViews subscribe and re-fetch the affected slice (or `stream_insert`). Broadcasts are throttled (≤ ~1/s) to protect inboxes.

**LiveView patterns.** `assign_async/3` for initial loads; `stream/3` + cursor for paginated lists; server-side FTS via `POST /search`; `<.async_result>` for loading/error states. A `Decant.HealthCheck` GenServer polls `/health` (every few seconds) and exposes `ready?/0`; LiveViews render a "start the service" / degraded state when the daemon is down, and recover on reconnect.

**The "Sync" button and sidebar metrics** read from the daemon (`/metadata/sync-status`, `/analytics/summary`) instead of shelling out / reading SQLite.

---

## 8. Lifecycle and CLI

- `decant daemon serve [--foreground] [--port N]` — run the daemon (foreground logs to stderr for dev; otherwise to `~/Library/Logs/decant/daemon.log`).
- `decant daemon install` — write + load a macOS LaunchAgent (`~/Library/LaunchAgents/com.decant.daemon.plist`, `RunAtLoad`, `KeepAlive`); `uninstall` reverses it.
- `decant daemon start|stop|status|logs [-f]` — control + tail (via launchctl + the lock/PID file).
- `decant sync` and the read CLIs keep working for ad-hoc/headless use (they open SQLite directly **only when the daemon is not running**; when it is, the lock file signals them to defer or call the API). Linux `systemd` unit is a documented future add; Windows is future work.

Phoenix discovers the daemon via the known port + token file; if absent/unreachable it shows a clear "decant service isn't running — `decant daemon start`" state.

---

## 9. Build order (safe internal sequence, even though shipped as one effort)

1. `decant-daemon` skeleton: tokio + axum + `/health` + auth/Host/Origin middleware + token file + lock file. Tests for middleware.
2. Move watcher + ingest into the daemon (sync task, write conn, read pool, graceful shutdown). Parity with `decant sync`.
3. Read endpoints (sessions, search, analytics, tools, metadata) with envelope + cursor pagination; OpenAPI written alongside; contract tests.
4. SSE `/events` change-stream emitted on sync commit.
5. Phoenix client swap **page by page** behind `Decant.Daemon` (sessions → search → analytics → tools), each verified, then retire that page's SQL.
6. Recommendations: core generation + table + endpoints + agent mark + activity auto-resolve; Insights page consumes the API; "Implemented" section.
7. Retire `AutoSync` and all direct SQLite reads in `web/`; wire `DaemonEvents` + `HealthCheck`; the Sync button + sidebar metrics move to the API.
8. Lifecycle CLI + LaunchAgent; docs (README architecture, `docs/api/openapi.yaml`).

---

## 10. Testing strategy

- **decant-core:** existing ingest/cost/query/stats tests stay; new unit tests for recommendation generation, stable keys, and upsert-preserves-state.
- **decant-daemon:** integration tests spawn the server against a temp SQLite + synthetic fixtures; assert envelopes, cursor pagination stability (insert/delete mid-paginate), filter correctness (inclusive `to`), FTS malformed-query → 400, and the auth/Host/Origin middleware (401/403 paths). A contract test pins responses to the OpenAPI schema.
- **web:** mock `Decant.Daemon`/`Req` (Mimic) so LiveView + context tests run without a live daemon; cover the client decode/error/service-down paths, `DaemonEvents` parsing, and Insights open-vs-implemented rendering. The committed synthetic `web/test/fixtures/decant.db` stays for core/daemon tests.
- **Definition of done unchanged:** `cargo test/fmt/clippy` and `mix test/format/compile --warnings-as-errors` all green.

---

## 11. Risks and mitigations

- **DNS-rebinding against loopback (real, documented).** → Host-header allowlist + bearer token + Origin checks; never rely on CORS. (Oligo "0.0.0.0 Day"; MCP SDK rebinding patches; leo PR#46.)
- **SQLite write/read contention or WAL corruption on crash.** → single writer + read pool + WAL + busy_timeout; wrap sync in catch-all; `decant daemon repair` (integrity_check/VACUUM); lock file.
- **Two daemons.** → PID lock file checked at startup.
- **Daemon/Phoenix version skew.** → version stamp + startup check + clear UI state.
- **PubSub inbox overflow from chatty change events.** → throttle/batch broadcasts; send ids not payloads; LiveView re-fetches.
- **Service-down detection lag.** → `HealthCheck` polls every few seconds; handlers also mark down on error.
- **Watcher misses events.** → periodic fallback sync.
- **Cost staleness after pricing changes.** → unchanged gotcha (rebuild to recompute); surface `pricing_version` in responses.

---

## 12. Out of scope (v1)

`dismissed`/`archived` recommendation statuses; Linux/Windows service integration (documented stubs only); OpenAPI client codegen; auth beyond the local token (no multi-user); gRPC/UDS transport (clear future upgrade path if latency ever demands it).

---

## 13. Sources

Architecture and security research (2026-06-08), full set in the run transcript:

- Oligo — "0.0.0.0 Day: Exploiting Localhost APIs From the Browser" — https://www.oligo.security/blog/0-0-0-0-day-exploiting-localhost-apis-from-the-browser
- Rafter — "DNS Rebinding and Localhost MCP" — https://rafter.so/blog/mcp-dns-rebinding-localhost
- NCC Group Singularity — Preventing DNS Rebinding — https://github.com/nccgroup/singularity/wiki/Preventing-DNS-Rebinding-Attacks
- axum (HTTP framework) — https://github.com/tokio-rs/axum ; tower extractors — https://docs.rs/axum/latest/axum/extract/
- tokio — https://tokio.rs/tokio/tutorial ; notify — https://docs.rs/notify/latest/notify/ ; r2d2 — https://docs.rs/r2d2/latest/r2d2/ ; rusqlite WAL/busy_timeout — https://docs.rs/rusqlite/latest/rusqlite/
- macOS LaunchAgent — https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchAgentsDaemons.html
- Req — https://hexdocs.pm/req/readme.html ; Finch — https://hexdocs.pm/finch/Finch.html ; ServerSentEvents (Elixir) — https://hex.pm/packages/server_sent_events
- Phoenix LiveView async/streams — https://hexdocs.pm/phoenix_live_view/Phoenix.LiveView.html ; PubSub — https://hexdocs.pm/phoenix_pubsub/Phoenix.PubSub.html ; error handling — https://hexdocs.pm/phoenix_live_view/error-handling.html
- WebSockets vs SSE — https://ably.com/blog/websockets-vs-sse ; consuming SSE in Elixir — https://stackoverflow.com/questions/67739157
- Cursor pagination — https://www.citusdata.com/blog/2016/03/30/five-ways-to-paginate/ ; OpenAPI 3.1 — https://spec.openapis.org/oas/v3.1.0 ; SQLite FTS5 — https://www.sqlite.org/fts5.html
