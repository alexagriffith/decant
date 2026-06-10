# Deterministic Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> *Note:* this plan is being executed inline in the authoring session immediately
> after writing. Algorithmic tasks carry full code; plumbing tasks that mirror an
> existing handler/command pattern carry exact files, signatures, params, and test
> assertions instead of duplicated boilerplate.

**Goal:** Extract file hotspots, session facets, and outcome/work-type classification deterministically at ingest; expose them via `/analytics/files`, `/analytics/now` (with live-session SSE), session facets, new determinism-shifting recommendation signals, and a `decant files` CLI command.

**Architecture:** Two new pure `decant-core` modules (`enrich`, `classify`) run inside `upsert_session`'s existing transaction, writing a new `file_ref` table and new `session` facet columns (schema v4, which also invalidates the ingest memo so the next sync backfills the archive). The daemon adds two read endpoints backed by `stats::file_hotspots` and an in-memory `ActivityTracker` fed by the existing watcher, broadcasting a new additive SSE event type.

**Tech Stack:** Rust only (`rusqlite`, `axum`, existing crates — no new dependencies). `docs/api/openapi.yaml` is the contract of record. Web UI consumption is a separate follow-up plan.

**Spec:** `docs/superpowers/specs/2026-06-10-deterministic-enrichment-design.md`

---

## File Structure

**Create:**
- `crates/decant-core/src/enrich.rs` — pure extraction: `file_refs()` + `facets()` over a `NormalizedSession`.
- `crates/decant-core/src/classify.rs` — pure heuristics: `outcome()` + `work_type()`.
- `crates/decant-daemon/src/activity.rs` — `ActivityTracker` (source-path → last write) + idle/active transitions.
- `fixtures/claude/enriched.jsonl`, `fixtures/codex/enriched.jsonl` — synthetic fixtures exercising every marker.

**Modify:**
- `crates/decant-core/src/lib.rs` — register `enrich`, `classify`.
- `crates/decant-core/src/schema_v1.sql` — `file_ref` table + session facet columns (fresh DBs).
- `crates/decant-core/src/schema.rs` — v4 migration; `LATEST_VERSION = 4`.
- `crates/decant-core/src/ingest.rs` — call enrich/classify in `upsert_session`; insert `file_ref` rows + facet columns.
- `crates/decant-core/src/stats.rs` — `file_hotspots()` (+ row types).
- `crates/decant-core/src/recommendations.rs` — four new signal generators.
- `crates/decant-daemon/src/api/analytics.rs` (+ `api/mod.rs` routes) — `/analytics/files`, `/analytics/now`.
- `crates/decant-daemon/src/api/sessions.rs` + `api/filters.rs` — facets in payloads; `outcome`/`work_type` filters.
- `crates/decant-daemon/src/events.rs`, `watcher.rs`, `lib.rs` — tracker wiring + `session_activity` SSE.
- `crates/decant-cli/src/...` — `decant files` command; facets in `show` (follow existing command/output pattern).
- `docs/api/openapi.yaml` — new paths, params, schemas, SSE event documentation.

---

## Task 1: Schema v4 — `file_ref`, facet columns, memo invalidation

**Files:** Modify `crates/decant-core/src/schema_v1.sql`, `crates/decant-core/src/schema.rs`.

- [ ] **Step 1: failing tests** in `schema.rs` tests mod: `v4_adds_file_ref_and_facet_columns_idempotently` (migrate twice; assert `file_ref` table exists, `session` has all facet columns, `MAX(version)=4`) and `v4_invalidates_ingest_memo_for_backfill` (build a v3-state DB with one `ingest_source` row `size=100`; migrate; assert `size = -1`, row count unchanged).
- [ ] **Step 2: run** `cargo test -p decant-core schema` → both FAIL.
- [ ] **Step 3: implement.** In `schema_v1.sql` add the `file_ref` table + two indexes (spec §3) and the facet columns on `session`:

```sql
  turn_count INTEGER NOT NULL DEFAULT 0,
  error_count INTEGER NOT NULL DEFAULT 0,
  interruption_count INTEGER NOT NULL DEFAULT 0,
  compaction_count INTEGER NOT NULL DEFAULT 0,
  sidechain_message_count INTEGER NOT NULL DEFAULT 0,
  agent_spawn_count INTEGER NOT NULL DEFAULT 0,
  skill_count INTEGER NOT NULL DEFAULT 0,
  command_count INTEGER NOT NULL DEFAULT 0,
  thinking_block_count INTEGER NOT NULL DEFAULT 0,
  thinking_chars INTEGER NOT NULL DEFAULT 0,
  active_seconds INTEGER NOT NULL DEFAULT 0,
  outcome TEXT,
  work_type TEXT,
```

In `schema.rs`: `LATEST_VERSION = 4`; generalize `add_column_if_missing` to take a table name; `apply_v4` mirrors `apply_v3` (tx: create `file_ref` + indexes `IF NOT EXISTS`, PRAGMA-guarded session columns, then `UPDATE ingest_source SET size = -1`, record version 4 in the same tx — unlike v3 there is no out-of-tx backfill step).
- [ ] **Step 4: run** → PASS. **Step 5: commit** `feat(core): schema v4 — file_ref table, session facets, backfill memo invalidation`.

## Task 2: `enrich.rs` — file refs + facets (pure)

**Files:** Create `crates/decant-core/src/enrich.rs`; modify `lib.rs`; create both `enriched.jsonl` fixtures.

- [ ] **Step 1: fixtures.** `fixtures/claude/enriched.jsonl`: synthetic session with cwd `/Users/dev/proj`, containing — user text turn; assistant turn with thinking + `Read {file_path:/Users/dev/proj/src/main.rs}` + `Edit {file_path:/Users/dev/proj/src/main.rs}` + `Write {file_path:/Users/dev/proj/README.md}` + `Agent` + `Skill` tool_use blocks; tool result with `is_error:true`; user block text `<command-name>/commit</command-name>`; user block text `[Request interrupted by user]`; system line `subtype:compact_boundary`; two messages with `isSidechain:true`; timestamps spanning gaps of 60s and 900s (active = 60+300 capped). `fixtures/codex/enriched.jsonl`: session_meta (cwd `/Users/dev/proj`); `apply_patch` function_call whose `arguments` is a JSON **string** containing `*** Begin Patch\n*** Add File: docs/new.md\n…\n*** Update File: src/lib.rs\n…\n*** Delete File: old.txt\n*** End Patch`; an `exec_command` call (must produce no file_refs); reasoning item; token_count event.
- [ ] **Step 2: failing tests** in `enrich.rs`: parse each fixture via the real parsers, then assert —
  `file_refs`: claude → `[(src/main.rs, read), (src/main.rs, edit), (README.md, write)]` with `rel_path` stripped of cwd and `ext ∈ {rs, md}`; codex → `[(docs/new.md, write), (src/lib.rs, edit), (old.txt, delete)]`.
  `facets`: claude → `turn_count=2`-ish per fixture construction, `error_count=1`, `interruption_count=1`, `compaction_count=1`, `sidechain_message_count=2`, `agent_spawn_count=1`, `skill_count=1`, `command_count=1`, `thinking_block_count≥1`, `active_seconds=360`.
- [ ] **Step 3: implement.**

```rust
//! Deterministic enrichment: extract file references and session facets from a
//! normalized session. Pure functions of the parsed data — no I/O, no LLM —
//! so the whole tier is recomputable by re-ingesting source files.

use crate::model::*;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct FileRef {
    /// Index into `session.messages` (ingest maps it to a message row id).
    pub message_idx: usize,
    pub path: String,
    pub rel_path: Option<String>,
    pub ext: Option<String>,
    pub operation: Operation,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation { Read, Edit, Write, Delete }

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Read => "read", Operation::Edit => "edit",
            Operation::Write => "write", Operation::Delete => "delete",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Facets {
    pub turn_count: i64, pub error_count: i64, pub interruption_count: i64,
    pub compaction_count: i64, pub sidechain_message_count: i64,
    pub agent_spawn_count: i64, pub skill_count: i64, pub command_count: i64,
    pub thinking_block_count: i64, pub thinking_chars: i64, pub active_seconds: i64,
}

/// Gap cap for active-duration: gaps longer than this count as idle.
const ACTIVE_GAP_CAP_SECS: i64 = 300;

pub fn file_refs(s: &NormalizedSession) -> Vec<FileRef> { /* dispatch per tool, see below */ }

fn claude_refs(...) // ToolUse blocks: Read/Edit/Write/NotebookEdit -> file_path|notebook_path (Value::Object)
fn codex_refs(...)  // ToolUse blocks named apply_patch: tool_input is Value::String of raw patch text;
                    // scan lines for "*** Add File: "->Write, "*** Update File: "->Edit, "*** Delete File: "->Delete
fn relativize(path:&str, cwd:Option<&str>) -> Option<String> // strip "<cwd>/" prefix; non-absolute paths pass through as-is
fn extension(path:&str) -> Option<String> // lowercased final ext of leaf, None if none

pub fn facets(s: &NormalizedSession) -> Facets {
    // turn_count: messages with role==User containing at least one Text block
    //   that is NOT an interruption marker and NOT a command tag wrapper.
    // error_count: ToolResult blocks with is_error == Some(true).
    // interruption_count: Text blocks starting with "[Request interrupted by user".
    // command_count: Text blocks containing "<command-name>".
    // compaction_count: raw.subtype == "compact_boundary" || raw.isCompactSummary == true.
    // sidechain_message_count: raw.isSidechain == true.
    // agent_spawn_count / skill_count: ToolUse blocks named Agent|Task / Skill.
    // thinking_*: Thinking blocks / sum of text len.
    // active_seconds: sum over consecutive message timestamp pairs of
    //   min(gap_secs, ACTIVE_GAP_CAP_SECS), parsing RFC3339 via a tiny local parser
    //   (no chrono; epoch math like the rest of the codebase — see cost/queries
    //   for precedent; if none exists, hand-roll days-from-civil here).
}
```

- [ ] **Step 4: run** `cargo test -p decant-core enrich` → PASS. **Step 5: commit** `feat(core): enrich module — deterministic file refs + session facets`.

## Task 3: wire enrichment into `upsert_session`

**Files:** Modify `crates/decant-core/src/ingest.rs`.

- [ ] **Step 1: failing test** `upsert_writes_file_refs_and_facets` (in `ingest.rs` tests): ingest both enriched fixtures through `upsert_session`; assert `file_ref` row counts (3 claude / 3 codex), a spot row (`rel_path='src/main.rs' AND operation='read'`), and session facet columns (`error_count=1`, `interruption_count=1`, `active_seconds=360`, …) round-trip.
- [ ] **Step 2: run** → FAIL. **Step 3: implement.** In `upsert_session`: compute `let refs = enrich::file_refs(&s); let facets = enrich::facets(&s);` before the session INSERT; extend the INSERT's column list with the 11 facet columns + `outcome, work_type` (NULL until Task 4); after message loop (message row ids now known, collected into `msg_ids: Vec<i64>` while inserting), insert each ref:

```rust
for fr in &refs {
    conn.execute(
        "INSERT INTO file_ref(session_id, message_id, path, rel_path, ext, operation, timestamp)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![session_id, msg_ids.get(fr.message_idx), fr.path, fr.rel_path, fr.ext,
                fr.operation.as_str(), fr.timestamp],
    )?;
}
```

- [ ] **Step 4: run full** `cargo test -p decant-core` → PASS (existing fixtures get zero refs / zero facets — defaults hold). **Step 5: commit** `feat(core): persist file refs + facets at ingest`.

## Task 4: `classify.rs` — outcome + work type

**Files:** Create `crates/decant-core/src/classify.rs`; modify `lib.rs`, `ingest.rs`.

- [ ] **Step 1: failing table-driven tests** building minimal `NormalizedSession`s per rule:
  outcome — ends with assistant text/end_turn → `Completed`; ends with user text → `Abandoned`; last assistant stop_reason `tool_use` and nothing after → `Abandoned`; interruption within last 3 messages → `Abandoned`; trailing error result with no later assistant text → `Failed`; empty session → `None`.
  work_type — first prompt "fix the failing auth test" → `Debugging`; "implement cursor pagination" → `Feature`; "refactor the parser module" → `Refactor`; "research whether turso fits" → `Research`; "configure the release pipeline" → `Ops`; no keyword + no file writes + reads>0 → `Research` (tool-mix fallback); no signal at all → `None`.
- [ ] **Step 2: run** → FAIL. **Step 3: implement** (priority order exactly as tests; keyword match = case-insensitive word predicates on the first User text block; enum `as_str()` lowercase labels). Wire into `upsert_session`: `let outcome = classify::outcome(&s, &facets); let work_type = classify::work_type(&s, &refs);` bound into the session INSERT.
- [ ] **Step 4: run** `cargo test -p decant-core` → PASS. **Step 5: commit** `feat(core): heuristic outcome + work-type classification at ingest`.

## Task 5: `stats::file_hotspots`

**Files:** Modify `crates/decant-core/src/stats.rs`.

- [ ] **Step 1: failing tests** seeding via the enriched fixtures: `group=path` returns `src/main.rs` with `reads=1, edits=1, sessions=1` ordered by total ops; `op=Some(Edit)` filters; `group=ext` rolls up by extension (`rs` row aggregating both sessions' rs ops).
- [ ] **Step 2: run** → FAIL. **Step 3: implement:**

```rust
#[derive(Debug, Serialize)]
pub struct FileStatRow {
    pub key: String,            // rel_path (or path fallback) | ext
    pub project: Option<String>,// NULL for group=ext
    pub reads: i64, pub edits: i64, pub writes: i64, pub deletes: i64,
    pub sessions: i64, pub last_touched_at: Option<String>,
}

pub enum FileGroup { Path, Ext }

pub fn file_hotspots(conn:&Connection, group:FileGroup, op:Option<enrich::Operation>, limit:i64) -> Result<Vec<FileStatRow>>
```

SQL (fixed-match group expr, injection-safe like `by_dimension`): group by `COALESCE(f.rel_path, f.path), p.path` (or `f.ext`), counting `SUM(operation='read')` etc., `COUNT(DISTINCT f.session_id)`, `MAX(f.timestamp)`, optional `WHERE operation = ?`, `ORDER BY (reads+edits+writes+deletes) DESC LIMIT ?`.
- [ ] **Step 4: run** → PASS. **Step 5: commit** `feat(core): file hotspot aggregation`.

## Task 6: daemon `/analytics/files` (+ OpenAPI)

**Files:** Modify `crates/decant-daemon/src/api/analytics.rs`, `api/mod.rs` (route), `docs/api/openapi.yaml`. Follow the existing `by-dimension` handler pattern exactly (envelope, filters, pagination, `with_read_conn`).

- [ ] Failing daemon test (existing api test pattern): seeded DB → `GET /api/v1/analytics/files?group=path` returns rows with the Task 5 shape inside the standard envelope; `group=ext` and `op=edit` variants; invalid `group` → 400 problem. Implement handler mapping query params → `stats::file_hotspots` with the standard `from/to/tool/project` filters applied via the daemon's `filters.rs` conventions (the daemon's own SQL variant if its filter layer can't wrap core's — mirror however `by-dimension` resolves this today). Document path + params + `FileStatRow` schema in `openapi.yaml`. Commit `feat(daemon): /analytics/files hotspot endpoint`.

## Task 7: facets in session payloads + filters (+ OpenAPI)

**Files:** Modify `crates/decant-daemon/src/api/sessions.rs`, `api/filters.rs`, `docs/api/openapi.yaml`.

- [ ] Failing daemon tests: session list/detail items include `facets` object (11 counters + `outcome` + `work_type`); `GET /sessions?outcome=abandoned` and `?work_type=debugging` filter (exact match, 400 on unknown value). Implement by extending the session row mapping + filter enum. OpenAPI: `SessionFacets` schema + two query params. Commit `feat(daemon): session facets + outcome/work_type filters`.

## Task 8: activity tracker + `/analytics/now` + SSE (+ OpenAPI)

**Files:** Create `crates/decant-daemon/src/activity.rs`; modify `watcher.rs` (report per-path events to the tracker), `events.rs` (new event type), `lib.rs` (construct/share tracker; sweep on the keep-alive tick), `api/analytics.rs` + `api/mod.rs` (endpoint), `docs/api/openapi.yaml`.

- [ ] **Tracker (TDD, injected clock):**

```rust
pub struct ActivityTracker { inner: Mutex<HashMap<PathBuf, ActiveEntry>> }
struct ActiveEntry { last_write: Instant, tool: Tool }
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(120);

impl ActivityTracker {
    /// Record a write; returns true when the path transitioned idle→active
    /// (caller broadcasts `session_activity{state:"active"}`).
    pub fn record_write(&self, path: &Path, tool: Tool, now: Instant) -> bool;
    /// Drop entries idle past the window; returns the expired paths
    /// (caller broadcasts `state:"idle"` per path).
    pub fn sweep(&self, now: Instant) -> Vec<(PathBuf, Tool)>;
    /// Current active paths for /analytics/now.
    pub fn active(&self, now: Instant) -> Vec<(PathBuf, Tool)>;
}
```

  Tests: record → active lists it; second record within window returns false (no re-announce); sweep after >120 s returns it once and empties.
- [ ] **Wiring:** watcher already receives per-path notify events before debouncing — pass each matching session-file path to `tracker.record_write` and broadcast on transition; sweep alongside the SSE keep-alive interval; broadcast idle transitions. SSE payload: `{"type":"session_activity","state":"active|idle","tool":"claude_code|codex","source_path":"…"}` (path stem only — no transcript content).
- [ ] **Endpoint:** `GET /api/v1/analytics/now` → today's totals (reuse the summary query with local-day bounds exactly as `/analytics/activity` computes them), `active_sessions` (tracker paths joined to `session` via `source_path` for title/project/model; unmatched paths emit `tool` + nulls), `last_sync_at` / `sync_in_progress` from the existing sync-status state. Daemon test with a fake-clock tracker seeded active. OpenAPI: path + schema + `session_activity` event documented under `/events`.
- [ ] Commit `feat(daemon): live-session activity tracker, /analytics/now, session_activity SSE`.

## Task 9: determinism-shifting recommendation signals

**Files:** Modify `crates/decant-core/src/recommendations.rs`.

- [ ] Failing tests seeding file_ref/facet data past each threshold, asserting key/tone/score shape: `signal:hot-context:<rel_path>` (read in ≥8 distinct sessions of a project within 30 days, ≤2 edit sessions), `signal:churn:<rel_path>` (edited in ≥6 distinct sessions), `signal:search-heavy` (Grep+Glob calls/session ≥ 8.0 over the window, min 20 sessions), `signal:abandoned-rate` (>25% abandoned, min 10 classified). Suggestions per spec §7 (AGENTS.md/skill distillation framing); scores scaled like existing signals; window = `started_at >= date('now','-30 days')`; respect the existing 12-signal cap/ranking. Commit `feat(core): determinism-shifting recommendation signals`.

## Task 10: CLI — `decant files` + facets in `show`

**Files:** `crates/decant-cli` (mirror an existing read command end-to-end: clap subcommand → core call → `output` table).

- [ ] `decant files [--group path|ext] [--op read|edit|write|delete] [--limit N]` printing KEY/PROJECT/READS/EDITS/WRITES/DELETES/SESSIONS/LAST; `decant show <id>` gains one facets block (turns, errors, interruptions, active `5m23s`-style, outcome, work type). Tests per existing CLI test pattern. Commit `feat(cli): files hotspot command + facets in show`.

## Task 11: threshold tuning + gates + real-data verification

- [ ] Rebuild scratch DB from real sources (`DECANT_DB=/tmp/decant-verify.db` + real source dirs, via the CLI's headless sync path); check: outcome/work-type distributions (acceptance: <30% NULL work_type, no label >60%, abandoned ∈ [5%,40%]); hotspot top-20 sanity (recognizable files; CLAUDE.md/AGENTS.md likely top reads); tune constants once if out of band (single commit `fix(core): tune classification thresholds against real archive`).
- [ ] Gates: `cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`. `web/` untouched → web gates not required (run `mix test` only if anything under `web/` changed).

## Self-review

Spec coverage: §3→T1, §4.1→T2/T3, §4.2→T4+T11, §5.1→T5/T6, §5.2→T8, §5.3→T7, §6→T8, §7→T9, §8→T10, §1/§9 informative. Type names consistent (`FileRef`/`Operation`/`Facets`/`FileStatRow`/`ActivityTracker` used identically across tasks). No TBDs.
