# decant — Design

**Status:** Approved (design phase)
**Date:** 2026-06-06
**Author:** onlydole + Claude

## 1. Overview

`decant` extracts every Claude Code and Codex CLI session from local disk, normalizes
them into a well-structured SQLite database (a self-contained, queryable archive), and
presents them through a Phoenix LiveView web app for browsing, full-text search, usage &
cost analytics, and export.

The name reflects the job: decanting sessions out of their raw per-tool log files into a
clean, shared store.

### Goals (use cases, all in scope)

1. **Browse & re-read** — revisit past sessions; read full transcripts faithfully; jump
   back to a conversation by project or date.
2. **Full-text search** — search across every session by keyword, topic, file path, or
   command.
3. **Usage & cost analytics** — token usage, estimated cost, model breakdown, activity
   over time, per-project stats, and **tool / MCP usage** (which tools and MCP servers,
   how often, with what success rate).
4. **Export & backup** — a durable, lossless archive that guards against loss; export
   individual sessions back out to Markdown/JSON.

### Non-goals (v1)

- No editing or replaying of sessions.
- No multi-user / hosted deployment; this is a local, single-user tool.
- No web-app-owned state (saved searches, settings). The web app is read-only except for
  triggering a sync. Any such state is deferred to v2 and, if added, lives in separate
  `app_*` tables to preserve the schema seam.
- No real-time/continuous watcher. Sync is on-demand and idempotent.

## 2. Architecture

Two artifacts, with **the SQLite file as the contract** between them.

```
   ~/.claude/projects/**/<uuid>.jsonl  ─┐
   ~/.codex/sessions/**/rollout-*.jsonl ├─►  decant (Rust CLI)  ──writes──►  decant.db
   ~/.codex/archived_sessions/**       ─┘     parse → normalize            (SQLite + FTS5, WAL)
                                                                                  ▲
                                                          reads (Ecto/ecto_sqlite3)│
                                                                                  │
                                              Phoenix LiveView web app  ◄──────────┘
                                              browse · search · analytics · tools/MCP · export
                                              "Sync now" button → System.cmd("decant", ["sync"])
```

### Key decisions

1. **Core/CLI in Rust.** The dominant technical challenge is correctly and durably modeling
   messy, polymorphic, version-drifting JSONL. `serde` tagged enums model each record
   variant explicitly; a `#[serde(other)]` catch-all means unknown record/block types
   never crash or get silently dropped. `rusqlite` with the `bundled` + FTS5 features
   compiles SQLite into the binary → a hermetic, dependency-free `decant` executable.
   (Go was the runner-up; Zig ruled out for pre-1.0 churn and immature JSON ergonomics.)

2. **Web app in Phoenix LiveView.** Best-in-class for read-heavy, interactive internal
   tools: live server-side full-text search, a comfortable transcript reader, and
   dashboards with minimal/zero JavaScript.

3. **Integration via shared SQLite + shell-out** (not a Rustler NIF, not an HTTP API in
   v1). Each half is independently usable and buildable; zero FFI/runtime coupling. The
   web "Sync now" button runs the `decant` binary as a subprocess. The Rustler-NIF path
   stays open for later if in-process speed is ever wanted.

4. **Schema ownership: Rust owns the DDL and migrations** (a `schema_migrations` table it
   manages). Phoenix treats `decant.db` as an **external, read-mostly** database: Ecto
   schemas for queries only, Ecto migrations disabled for that repo, so the two sides
   never fight over DDL.

5. **Concurrency & location.** SQLite in **WAL mode**: Phoenix reads concurrently while
   `decant sync` writes (sole writer), with `busy_timeout` set. The DB defaults to
   `~/.local/share/decant/decant.db`, overridable via `DECANT_DB` / `--db` so both halves
   agree.

### Repo layout (polyglot monorepo)

```
decant/
├── Cargo.toml                  # Rust workspace
├── crates/
│   ├── decant-core/            # library: the real logic, unit-tested
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── db.rs           # rusqlite connection, WAL pragmas
│   │       ├── schema.rs       # SQLite DDL + migrations (owns schema)
│   │       ├── model.rs        # normalized domain types
│   │       ├── ingest.rs       # discover, idempotent upsert, parallel parse
│   │       ├── cost.rs         # model pricing → estimated cost
│   │       ├── tools.rs        # tool/MCP classification + pairing
│   │       └── sources/
│   │           ├── claude.rs   # Claude Code JSONL parser (serde enums)
│   │           └── codex.rs    # Codex rollout JSONL parser (serde enums)
│   └── decant-cli/             # thin `decant` binary
│       └── src/main.rs
├── web/                        # Phoenix LiveView app
│   └── lib/{decant,decant_web}/    # read-mostly Ecto repo + LiveViews
├── fixtures/                   # sanitized sample sessions for tests
├── docs/superpowers/specs/     # this design doc
├── justfile                    # just sync | web | test | build
└── README.md
```

## 3. Data sources

### Claude Code — `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`

One JSON object per line. Each record has a `type`: `user`, `assistant`, `system`, plus
custom/extension types (`summary`, `ai-title`, `last-prompt`, `permission-mode`,
`attachment`, `file-history-snapshot`, `queue-operation`, …). Records carry `uuid`,
`parentUuid` (a message tree), `sessionId`, `timestamp`, `cwd`, `gitBranch`, `version`,
`userType`, `isSidechain`, `requestId`.

- `message` holds content:
  - user → `{ role: "user", content: string | array }`
  - assistant → `{ role, model, content: [blocks], usage: {...}, stop_reason, id }`
  - content block types: `text`, `thinking`, `tool_use`, `tool_result` (tool_result
    appears inside user messages; a richer `toolUseResult` top-level field mirrors it).
- assistant `usage` is **per-message**: `input_tokens`, `output_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens` (+ detailed breakdown).
- `tool_result` carries **`is_error`** (true/false/absent) → per-call success/failure.
- `toolUseResult` is tool-specific (Bash → stdout/stderr/interrupted; Edit → patch;
  `Agent` → toolStats/totalTokens/totalToolUseCount/usage).
- Project is taken from each record's real `cwd` (not the encoded directory name).
- Scale on this machine: ~1,198 files across ~70 project folders, ~690 MB.

### Codex — `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` (+ `~/.codex/archived_sessions/**`)

One JSON object per line, each `{ type, timestamp, payload }`. Line `type` ∈:

- `session_meta` (first line): `id`, `timestamp`, `cwd`, `originator`, `cli_version`,
  `source`, `model_provider`, `base_instructions` (system prompt text). **No model here.**
- `turn_context` (per turn): `cwd`, **`model`** (e.g. `gpt-5.4`), `effort`,
  `approval_policy`, `sandbox_policy`, `personality`, `current_date`, `timezone`,
  `turn_id`, … → the model lives here.
- `response_item` (the conversation): `payload.type` ∈ `message`, `reasoning`,
  `function_call`, `function_call_output`, `web_search_call`, `custom_tool_call`,
  `custom_tool_call_output`, `tool_search_call`, `tool_search_output`. Codex also records
  some MCP calls as a dedicated `mcp_tool_call` item.
- `event_msg` (telemetry): `payload.type` ∈ `agent_message`, `user_message`,
  `task_started`, `task_complete`, **`token_count`** (cumulative usage), …

- Token usage from Codex is **cumulative** (via `token_count` events), not per-message.
- `~/.codex/session_index.jsonl` maps session `id` → `thread_name` (title) → `updated_at`
  and is read during sync to title Codex sessions.
- Success/failure for Codex tool calls is **best-effort** (exit codes in `exec` output);
  full payload preserved in `message.raw`.

## 4. SQLite schema

Entity hierarchy: `project` → `session` → `message` → `block`, with `tool_call` as a
classified projection over tool blocks, plus FTS, file-tracking, issue-logging, and
pricing side tables. Surrogate integer PKs throughout for clean joins. Timestamps stored
as ISO-8601 UTC `TEXT` (sortable; queryable via SQLite `strftime`).

```sql
-- A workspace, identified by its absolute cwd.
project(
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,           -- cwd
  name TEXT,                           -- basename(path)
  first_seen_at TEXT,
  last_seen_at TEXT
);

-- One row per session file.
session(
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,                  -- 'claude_code' | 'codex'
  source_session_id TEXT NOT NULL,     -- tool's session uuid
  project_id INTEGER REFERENCES project(id),
  title TEXT,                          -- explicit title record / session_index, else first user msg (≤120 chars)
  cwd TEXT,
  git_branch TEXT,
  model TEXT,                          -- most frequent non-null model among turns
  cli_version TEXT,
  started_at TEXT,                     -- first record timestamp
  ended_at TEXT,                       -- last record timestamp
  message_count INTEGER,
  total_input_tokens INTEGER,
  total_output_tokens INTEGER,
  total_cache_read_tokens INTEGER,
  total_cache_creation_tokens INTEGER,
  estimated_cost_usd REAL,
  is_archived INTEGER DEFAULT 0,
  source_path TEXT,                    -- file on disk
  raw_meta TEXT,                       -- JSON: session_meta / first-record extras (lossless)
  ingested_at TEXT,
  source_mtime INTEGER,
  source_size INTEGER,
  source_hash TEXT,
  UNIQUE(tool, source_session_id)
);

-- One row per "turn record". Carries usage; holds the lossless raw record.
message(
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id),
  seq INTEGER NOT NULL,                -- ordinal within session (file order)
  source_uuid TEXT,                    -- Claude uuid; Codex item id
  parent_source_uuid TEXT,             -- Claude parentUuid
  parent_id INTEGER REFERENCES message(id),
  role TEXT,                           -- user | assistant | system | tool | other
  model TEXT,
  stop_reason TEXT,
  timestamp TEXT,
  input_tokens INTEGER,                -- Claude: per-msg; Codex: NULL
  output_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_creation_tokens INTEGER,
  raw TEXT NOT NULL                    -- JSON: the FULL original record (lossless)
);

-- The uniform granular unit: one row per content block. Extracted fields only
-- (full fidelity lives in message.raw), to avoid doubling storage.
block(
  id INTEGER PRIMARY KEY,
  message_id INTEGER NOT NULL REFERENCES message(id),
  session_id INTEGER NOT NULL REFERENCES session(id),
  ordinal INTEGER NOT NULL,
  type TEXT,                           -- text | thinking | tool_use | tool_result | web_search | image | other
  text TEXT,                           -- human-readable text (text/thinking)
  tool_name TEXT,                      -- for tool_use/tool_result
  tool_use_id TEXT,                    -- pairs tool_use ↔ tool_result
  tool_input TEXT,                     -- JSON
  tool_result TEXT                     -- text/JSON
);

-- Classified, paired projection over tool blocks — built for querying.
tool_call(
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id),
  message_id INTEGER REFERENCES message(id),
  call_block_id INTEGER REFERENCES block(id),
  result_block_id INTEGER REFERENCES block(id),
  tool_kind TEXT,                      -- builtin | mcp | custom | web_search | tool_search
  tool_name TEXT,                      -- full name as logged
  mcp_server TEXT,                     -- parsed from mcp__<server>__… ; NULL if builtin
  tool_base_name TEXT,                 -- name minus the mcp__<server>__ prefix
  tool_use_id TEXT,
  input TEXT,                          -- JSON; usually small, useful for analytics
  is_error INTEGER,                    -- Claude: tool_result.is_error; Codex: best-effort
  output_preview TEXT,                 -- truncated; full output in the result block
  output_bytes INTEGER,
  duration_ms INTEGER,                 -- where available (Agent toolStats / exec timing)
  ordinal INTEGER,
  timestamp TEXT
);

-- Full-text search over blocks (external-content FTS5; triggers keep it in sync).
block_fts USING fts5(
  text, tool_name, tool_input,
  content='block', content_rowid='id'
);
-- + AFTER INSERT/UPDATE/DELETE triggers on block to maintain block_fts.

-- File tracking for idempotent, incremental sync.
ingest_source(
  path TEXT PRIMARY KEY,
  tool TEXT,
  size INTEGER,
  mtime INTEGER,
  hash TEXT,
  session_id INTEGER REFERENCES session(id),
  line_count INTEGER,
  status TEXT,                         -- ok | error | skipped
  error TEXT,
  last_ingested_at TEXT
);

-- Parse failures, logged not fatal.
ingest_issue(
  id INTEGER PRIMARY KEY,
  source_path TEXT,
  line_no INTEGER,
  error TEXT,
  raw_line TEXT,
  created_at TEXT
);

-- Transparent, recomputable cost model.
model_pricing(
  model TEXT PRIMARY KEY,
  input_per_mtok REAL,
  output_per_mtok REAL,
  cache_read_per_mtok REAL,
  cache_write_per_mtok REAL,
  source TEXT,
  updated_at TEXT
);

schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT);
```

### Indexes

- `session(project_id)`, `session(tool)`, `session(started_at)`, `session(model)`
- `message(session_id, seq)`, `message(role)`
- `block(session_id)`, `block(message_id, ordinal)`, `block(type)`, `block(tool_name)`
- `tool_call(session_id)`, `tool_call(tool_kind)`, `tool_call(mcp_server)`, `tool_call(tool_name)`

### Normalization mapping

| Source record | → role | → block type |
|---|---|---|
| Claude `user` (string) | user | text |
| Claude `assistant` content blocks | assistant | text / thinking / tool_use |
| Claude `user` w/ tool_result | tool | tool_result |
| Codex `message` (payload.role) | user or assistant | text |
| Codex `reasoning` | assistant | thinking |
| Codex `function_call` / `custom_tool_call` / `mcp_tool_call` | assistant | tool_use |
| Codex `function_call_output` / `custom_tool_call_output` | tool | tool_result |
| Codex `web_search_call` | assistant | web_search |
| Codex `tool_search_call` / `tool_search_output` | assistant / tool | tool_use / tool_result |
| Unknown top-level record type | other | other (raw preserved) |

### Design rationale (recap)

- **`message` is lossless (`raw` JSON); `block` stores extracted fields only.** Faithful
  export/audit reads `message.raw`; rendering/search/analytics read lean `block` rows.
  Expected DB size ~1–1.5 GB for the current corpus — trivial for SQLite.
- **`block` is the uniform granular unit.** A Claude assistant record (thinking + text +
  2 tool_use) → 4 blocks; each Codex `response_item` → exactly 1 block.
- **Token-usage asymmetry, handled explicitly.** Claude per-message usage → `message.*`.
  Codex cumulative usage → `session.total_*`. Both always have session-level totals, so
  every analytic works for both; Claude adds per-message drill-down.

## 5. Tool / MCP layer

`tool_call` rows are built during the same ingest pass that creates blocks, by pairing
`tool_use` ↔ `tool_result` via `tool_use_id` (Claude) or call/output id (Codex).

**Classification rule (both tools):** a tool name starting with `mcp__` →
`tool_kind='mcp'`; `mcp_server` = segment after `mcp__` up to the next `__`;
`tool_base_name` = the remainder. This handles nested gateways such as
`mcp__codex_apps__hubspot__create_deal` (server `codex_apps`, base
`hubspot__create_deal`). Otherwise `tool_kind` is `builtin` (or `custom` / `web_search` /
`tool_search` based on the source item type).

**MCP server set is derived** from observed calls (neither tool writes an explicit "available
servers" manifest into the transcript) — which is what usage analytics wants anyway.
Per-session server lists and global leaderboards come from queries over `tool_call`
(indexed on `session_id`, `mcp_server`, `tool_name`, `tool_kind`).

## 6. Sync pipeline (`decant sync`)

Parallel parse, serialized write.

1. **Discover.** Glob `~/.claude/projects/*/*.jsonl`,
   `~/.codex/sessions/**/rollout-*.jsonl`, and `~/.codex/archived_sessions/**`
   (archived → `is_archived=1`). Roots overridable via `--claude-dir` / `--codex-dir` /
   `DECANT_CLAUDE_DIR` / `DECANT_CODEX_DIR`; missing roots warn and skip. Read
   `~/.codex/session_index.jsonl` for Codex titles.
2. **Skip unchanged.** Stat each file; if `ingest_source` size+mtime match, skip. Else
   hash; if hash matches, touch mtime and skip. Only new/changed files parse.
3. **Parse.** `serde` tagged enums per tool, each with a `#[serde(other)]` catch-all so
   unknown record/block types never crash — they land as `role='other'` with raw JSON,
   and malformed lines go to `ingest_issue`. Sync never aborts on a bad file/line.
4. **Write.** N parser threads (rayon) → 1 writer thread (SQLite single-writer), batched
   transactions. Re-ingesting a changed file replaces that session's rows atomically
   (delete-then-insert by `session_id`), so sync is fully **idempotent** — running twice
   yields identical state.
5. **Finish.** Recompute cached aggregates (`message_count`, token totals,
   `estimated_cost_usd` from `model_pricing`), update `ingest_source`, print a summary
   (scanned / new / changed / skipped / issues).

Flags: `--full` (force re-parse), `--dry-run`, `--verbose`, `--db`, `--claude-dir`,
`--codex-dir`.

## 7. CLI surface (`decant`)

Useful on its own, no web app required.

| Command | Purpose |
|---|---|
| `sync` | ingest / refresh (above) |
| `list` | sessions table; filter `--tool/--project/--since/--until/--model`, sortable |
| `search <q>` | FTS across blocks → session + snippet hits; `--tool` filter |
| `show <id>` | render a full transcript to terminal (`--format md\|json`) |
| `stats` | usage/cost rollups; `--by tool\|project\|model\|day\|tool-kind\|server` |
| `tools` | tool / MCP-server usage; `--server`, `--errors-only` |
| `export <id\|--all>` | write Markdown/JSON (`--out`, `--format md\|json`) |
| `db <migrate\|info\|vacuum>` | schema + maintenance |

## 8. Phoenix LiveView web app

`ecto_sqlite3` repo pointed read-mostly at `DECANT_DB`; Ecto migrations disabled (Rust
owns DDL); Ecto schemas map `project`/`session`/`message`/`block`/`tool_call` for queries.

- **`/` Sessions index** — filterable/sortable table (tool, project, model, date,
  archived; **MCP-server badge** column; filter by tool/server used), live server-side
  filtering, pagination.
- **`/sessions/:id` Reader** — faithful transcript: blocks in order grouped into turns;
  collapsible thinking; `tool_use` paired with `tool_result`; **MCP-server badge** and a
  red indicator on errors; code highlighting; copy + export buttons; metadata sidebar
  (model, cwd, branch, tokens, cost, timestamps); filter the transcript by tool.
- **`/search` Search** — live full-text search as you type (debounced server-side),
  highlighted snippets, jump-to-block, filters by tool/project/date.
- **`/analytics` Dashboard** — sessions / tokens / estimated cost over time; top projects;
  model split. Rendered with **contex** (pure-Elixir SVG charts → zero JS). Date-range
  selector.
- **`/tools` Dashboard** — tool leaderboard; **MCP-server leaderboard** (calls,
  tools-per-server); built-in vs MCP ratio over time; per-tool / per-server **error
  rates** (from `is_error`).
- **Header "Sync now"** — runs `System.cmd("decant", ["sync"])` in a `Task`, streams
  progress to the LiveView, refreshes on completion, shows last-sync time. If the binary
  isn't on PATH or the DB is missing, show a clear, actionable empty state.

## 9. Testing

- **Rust core (TDD where it counts).** Parser unit tests against small **anonymized
  fixture JSONL** in `fixtures/` covering every Claude + Codex record/block type, plus
  malformed and unknown-type lines. Assertions on normalization output, tool/MCP
  classification, and cost calc. **Idempotency test:** sync twice → identical DB state.
  **Incremental-skip test:** unchanged file is not re-parsed. Temp SQLite DB per test.
- **Phoenix.** LiveView tests against a seeded fixture `decant.db` (built once from
  fixtures): assert index rendering, search results, filters, reader output, tools
  dashboard aggregates.

## 10. Error handling

- Sync never aborts on a bad file/line: per-file/per-line try, record `ingest_issue`,
  continue. Exit code reflects whether any hard errors occurred; the summary lists issues.
- Missing source dirs → warn + skip (e.g., no Codex installed).
- DB locked (Phoenix reading) → WAL + `busy_timeout` handles it; the writer retries.
- Phoenix: missing binary → clear error on the Sync button; missing/empty DB → friendly
  "run `decant sync` first" empty state.

## 11. Distribution & dev ergonomics

- Rust: `cargo build --release` → single `decant` binary (later `cargo install` /
  Homebrew / prebuilt releases).
- Phoenix: `mix setup && mix phx.server` (later a `mix release`).
- Top-level `justfile`: `just sync`, `just web`, `just test`, `just build`.
- macOS-first (the dev environment), cross-platform paths/globs.

## 12. Build order (→ implementation plan)

1. Rust core: `db` + `schema` + migrations.
2. Claude & Codex parsers (TDD with fixtures) + `model`.
3. Ingest pipeline (idempotent, parallel) + `cost` + `tools` classification/pairing.
4. CLI commands (`sync`, `list`, `search`, `show`, `stats`, `tools`, `export`, `db`).
5. Phoenix scaffold + Ecto read models against a synced DB.
6. LiveViews: sessions index → reader → search → analytics → tools → sync button.
7. Tests, README, `justfile`.

## 13. Open / deferred

- Exact `model_pricing` seed values are published-rate estimates, user-editable in the
  table; costs are labeled **estimates** in the UI. Re-running sync recomputes.
- Repo-level grouping (collapsing many worktree cwds under one git repo) — possible UI
  nicety later; v1 groups by cwd.
- Web-app-owned state (saved searches, settings), bulk export, and a Rustler-NIF
  integration path are v2 considerations.
