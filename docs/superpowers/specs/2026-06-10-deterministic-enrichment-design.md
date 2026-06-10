# Deterministic Enrichment — Design

**Goal:** Make the archive *mean* something without spending a token: extract file
hotspots (which files/operations dominate), per-session facets (errors,
interruptions, compactions, subagents, thinking share, active duration), and
heuristic outcome/work-type classification — all computed deterministically at
ingest. Surface them over the daemon API (including a glanceable `/analytics/now`
with live-session detection) and feed new "shift it to determinism"
recommendation signals: when agents re-derive the same context every session,
tell the user to move it into AGENTS.md/skills instead.

**Approach:** A new `file_ref` table + facet columns on `session` (schema v4),
populated by pure functions over the already-normalized session during
`upsert_session`. No LLM, no network, no new heavyweight dependencies — the whole
tier is recomputable by re-ingesting source files. The v4 migration invalidates
the ingest memo (`ingest_source.size = -1`) so the next sync backfills the entire
archive automatically.

**Tech stack:** Rust only for this plan: `decant-core` (new `enrich` and
`classify` modules, schema v4, stats queries), `decant-daemon` (two new endpoints,
an in-memory activity tracker, two new SSE event types), `decant-cli` (`decant
files`, facets in `show`), and `docs/api/openapi.yaml` (contract of record). The
web surfacing of hotspots is a follow-up plan consuming the same endpoints.

---

## 1. Evidence base (probed against the real archive, 2026-06-10)

1,528 sessions / 186,617 messages / 59,027 tool calls (1,204 Claude Code, 324
Codex). Findings that lock the design:

| Fact | Number | Consequence |
|---|---|---|
| Claude `Read`/`Edit`/`Write` carry `file_path` | 13,894 calls, ~100% coverage, 3,480 distinct paths | File ops extraction is exact, not heuristic |
| Codex `apply_patch` arguments = raw patch text | 526/526 calls parse via `*** Add/Update/Delete File:` headers (908 file ops) | Codex edit-side extraction is exact |
| Codex tool inputs are JSON-encoded **strings** | 100% of sampled calls | Extraction must decode the inner payload (`Value::String`) |
| Codex `exec_command` `cmd` is a shell string | 13,683 calls | Path mining from shell is heuristic → **deferred** (v2) |
| Sidechain messages (`isSidechain`) | 52,645 msgs across 983 sessions | Subagent share is a high-signal facet |
| Compaction markers (`subtype=compact_boundary`, `isCompactSummary`) | 21 each | Cheap, exact |
| Interruptions (`[Request interrupted by user`) | 50 blocks | Cheap, exact |
| Slash commands (`<command-name>` tags) | 201 blocks | Command tracking viable |
| `stop_reason` distribution | tool_use 46,630 / end_turn 2,581 / stop_sequence 86 | end-of-session shape supports outcome heuristic |

## 2. Decisions locked

1. **Deterministic-first, agents-second.** Everything in this plan is a pure
   function of the session file. Embeddings/semantic search and LLM-generated
   summaries are explicitly **out of scope** (deferred; see §9) — per the
   product direction: extract information and determinism, leave judgment to
   agents, and make *them* faster with better structured context.
2. **Grep/Glob/Bash do not produce `file_ref` rows.** Grep/Glob `path` is a
   directory scope, not file-level evidence; Bash/`exec_command` path mining is
   guesswork. Search *volume* still informs the `search-heavy` signal via
   existing `tool_call` stats.
3. **Operation taxonomy = `read | edit | write | delete`.** Claude `Read`→read,
   `Edit`→edit, `Write`→write, `NotebookEdit`→edit; Codex `Update File`→edit,
   `Add File`→write, `Delete File`→delete. Codex has no read-side coverage
   (reads happen via shell) — documented limitation.
4. **Hotspots aggregate by `(project, rel_path)`.** `rel_path` = path relative
   to the project root (strip `session.cwd`/project prefix); falls back to the
   absolute path when underivable. Claude paths are absolute; Codex patch paths
   are already workdir-relative.
5. **Outcome/work-type are heuristics, stored as plain columns, versioned by
   code.** Re-ingest recomputes them; no provenance machinery in v1. Labels:
   `outcome ∈ {completed, failed, abandoned}`,
   `work_type ∈ {debugging, feature, refactor, research, ops}` (NULL = no signal).
6. **Backfill via memo invalidation, not DB deletion.** Migration v4 sets
   `ingest_source.size = -1` so the next sync re-parses every file and fills
   enrichment for the whole archive. Same mechanism available for future
   heuristic revisions.
7. **Live-session detection is in-memory, not persisted.** The watcher already
   sees per-file events; the daemon tracks `source_path → last_write` and calls
   a session *active* if its file changed in the last 120 s. Restart loses only
   ephemeral "now" state — acceptable by definition.

## 3. Schema v4 (`decant-core/src/schema.rs` + `schema_v1.sql` for fresh DBs)

```sql
CREATE TABLE IF NOT EXISTS file_ref (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
  path TEXT NOT NULL,        -- as recorded by the tool
  rel_path TEXT,             -- project-relative aggregation key (NULL if underivable)
  ext TEXT,                  -- lowercased extension without dot, NULL if none
  operation TEXT NOT NULL,   -- 'read' | 'edit' | 'write' | 'delete'
  timestamp TEXT
);
CREATE INDEX idx_fileref_session ON file_ref(session_id);
CREATE INDEX idx_fileref_path ON file_ref(rel_path, operation);
```

New `session` columns (PRAGMA-guarded adds, mirroring v3's pattern), all
`INTEGER NOT NULL DEFAULT 0` unless noted:

`turn_count` (user→assistant exchanges), `error_count` (tool results with
`is_error`), `interruption_count`, `compaction_count`, `sidechain_message_count`,
`agent_spawn_count` (Agent/Task calls), `skill_count` (Skill calls),
`command_count` (`<command-name>` blocks), `thinking_block_count`,
`thinking_chars`, `active_seconds` (gap-capped wallclock, cap = 300 s),
`outcome TEXT`, `work_type TEXT` (both nullable).

`span_seconds` stays derivable (`ended_at - started_at`) — not stored.

## 4. Core modules

### 4.1 `enrich.rs` — facts from the normalized session (pure)

- `file_refs(&NormalizedSession) -> Vec<FileRef>`:
  - Claude: match builtin tool names on `ToolUse` blocks; read `file_path` /
    `notebook_path` from `tool_input` objects.
  - Codex: `tool_input` is `Value::String`; for `apply_patch` scan the raw text
    for `*** {Add,Update,Delete} File: ` headers (validated 100% parseable).
  - `rel_path`: strip the longest of (`session.cwd`, resolved project root) +
    `/`; `ext`: lowercased final extension of the leaf (None when absent).
- `facets(&NormalizedSession) -> Facets`: counts per §3, from block text
  prefixes (`[Request interrupted by user`, `<command-name>`), raw-JSON markers
  (`isSidechain`, `subtype == "compact_boundary"` or `isCompactSummary`), tool
  names (Agent/Task/Skill), thinking blocks, and timestamp deltas for
  `active_seconds` (sum of consecutive message gaps, each min(gap, 300 s)).

### 4.2 `classify.rs` — heuristics over facts (pure)

- `outcome(&NormalizedSession, &Facets) -> Option<Outcome>`:
  - `abandoned` — last meaningful message is a user turn, or the final
    assistant message stopped at `tool_use` (mid-flight), or an interruption
    occurs in the last 3 messages.
  - `failed` — not abandoned, and the last tool result in the session is an
    error with no later assistant text.
  - `completed` — otherwise, when the session ends with assistant
    text/`end_turn`.
- `work_type(&NormalizedSession, &[FileRef]) -> Option<WorkType>`:
  - First-user-prompt keyword pass (explicit intent wins): fix/bug/fail/error →
    debugging; implement/add/build/create → feature; refactor/rename/simplify/
    clean up → refactor; research/investigate/compare/"should we" → research;
    deploy/install/configure/ci/release → ops.
  - Fallback on tool-mix ratios: no file writes + heavy read/search → research;
    edits dominated by test files + error-heavy → debugging; etc. NULL when
    nothing clears threshold.
  - Thresholds tuned once against the real archive (task: distribution must be
    plausible and < 30% NULL over **primary** sessions; no label > 60%).
    Subagent transcripts — session files whose messages are all sidechain
    (Claude Code writes each subagent run as its own file) — carry a NULL
    outcome by design: there is no main thread to judge.

Both run inside `upsert_session` (same transaction as the session insert).

## 5. Stats + daemon API

### 5.1 `GET /api/v1/analytics/files`

Params: `group=path|ext` (default `path`), `op=read|edit|write|delete` (optional
filter), standard `from/to/tool/project` filters, `limit/cursor` pagination.
Row (group=path): `{ path, project, reads, edits, writes, deletes, sessions,
last_touched_at }`, ordered by total ops desc. `group=ext` returns the same
rollup keyed by extension (the language lens). Backed by a new
`stats::file_hotspots` in core (shared by CLI) following the existing
injection-safe fixed-match builder pattern.

### 5.2 `GET /api/v1/analytics/now`

One cheap call for glanceable clients (menu bar, web header):

```json
{ "today": { "sessions": N, "estimated_cost_usd": X, "input_tokens": …, "output_tokens": … },
  "active_sessions": [ { "tool", "project", "title", "model", "started_at", "last_activity_at" } ],
  "last_sync_at": "…", "sync_in_progress": false }
```

`today` uses local-time day bounds (same convention as `/analytics/activity`).
Active sessions come from the in-memory tracker (§6); metadata is joined from
the DB by `source_path` when the session is already ingested, else
`tool` + path-derived project only.

### 5.3 Sessions surface

`/sessions` and `/sessions/{id}` payloads gain a `facets` object (the §3
counters + `outcome` + `work_type`); `/sessions` accepts `outcome=` and
`work_type=` filters. Additive change, documented in `openapi.yaml`.

## 6. Daemon: activity tracker + SSE

New `activity.rs`: `ActivityTracker` (shared `Mutex<HashMap<PathBuf, Instant>>`)
fed by the existing watcher debounce path. Transitions:

- idle→active (first write after ≥ 120 s quiet): broadcast SSE
  `session_activity` `{ "type": "session_activity", "state": "active", "tool",
  "source_path_stem", "project" }`.
- active→idle (sweep finds last write > 120 s ago; sweep piggybacks the
  existing ~15 s keep-alive tick): same event with `"state": "idle"`.

`archive_updated`/`resync` are unchanged; unknown SSE types are ignored by the
existing web consumer (verify in implementation; the contract documents the new
type as optional). `/analytics/now` reads the same tracker.

## 7. Recommendations — determinism-shifting signals

New generators in `recommendations::signals`, same shape/scoring/cap rules as
existing ones (windowed to the last 30 days):

| Key | Trigger | Suggestion tenor |
|---|---|---|
| `signal:hot-context:<rel_path>` | file **read** in ≥ 8 distinct sessions of one project, ≤ 2 edits | "Agents re-read this every session — distill its contract into AGENTS.md or a skill so they stop re-deriving it." |
| `signal:churn:<rel_path>` | file **edited** in ≥ 6 distinct sessions | "Complexity hotspot — consider a refactor or tests; agents keep coming back here." |
| `signal:search-heavy` | archive-wide Grep+Glob calls / session above threshold with low read-follow-through | "Add a code map / AGENTS.md index to cut discovery loops." |
| `signal:abandoned-rate` | > 25% of the window's sessions classified `abandoned` (min 10) | "Sessions stall before completion — review interruption points." |

(Exact thresholds finalized against the real archive alongside §4.2 tuning.)

## 8. CLI

- `decant files [--group path|ext] [--op …] [--limit N]` — hotspots table.
- `decant show <id>` prints a facets line block (turns, errors, interruptions,
  active duration, outcome, work type).
All printing in `decant-cli`'s `output` module; core stays UI-agnostic.

## 9. Explicitly deferred

- **Embeddings/semantic search** (fastembed/model2vec + sqlite-vec): real value,
  but binary-size and ingest-cost tradeoffs deserve their own decision after
  this tier proves out. Determinism-first.
- **LLM enrichment** (summaries/decisions): would violate the daemon's
  no-network invariant; the design when wanted is a user-invoked agent flow
  POSTing back through the API.
- **Bash/`exec_command` path mining**: heuristic; revisit with data.
- **Web UI hotspots view**: follow-up plan consuming `/analytics/files` +
  `/analytics/now` (and the menu-bar app itself, which these endpoints unblock).

## 10. Validation results (real archive, 2026-06-10, 1,481 sessions re-ingested)

Full re-ingest: 1,529 files in 2m51s (release build), zero parse issues.

- **file_ref:** 14,382 rows over 3,615 distinct files. Hotspot top-N is
  immediately recognizable (writing-project drafts; `decant-core/src/ingest.rs`
  as top code churn; `notes.md`/`sources.md` read in 69/64 sessions with ~no
  edits — textbook hot-context).
- **Facets vs. the §1 probe:** interruptions 50 (exact), compaction markers 42
  (exact), skills 181, agent spawns 387, sessions-with-sidechain 938 — all in
  line with probe values given archive drift.
- **Active duration:** mean 13.5 min active vs 91.9 min span — the gap-capped
  metric avoids ~6.8× inflation.
- **Outcome (primary sessions):** completed 71.0%, abandoned 28.8%, failed
  ~0%, NULL 0.2%. 938+ subagent-transcript files are NULL by design (§4.2).
  (PR review hardening: `Failed` now requires the *trailing* result to be an
  error — every prior `failed` label was the recovered-error false positive.)
- **Work type (all sessions):** research 34.8%, feature 26.9%, debugging
  12.6%, refactor 3.4%, ops 3.0%, NULL 19.4% — within acceptance.
- **Signals:** hot-context and churn initially surfaced out-of-project agent
  bookkeeping (memory indexes, automation notes); both now require
  `rel_path IS NOT NULL` (in-project evidence only). `search-heavy` lowered
  8.0 → 5.0 (this archive averages 0.6 — correctly quiet). `abandoned-rate`
  fires at 34.3% over the 30-day window — a true, actionable signal.

## 11. Testing

- New synthetic fixtures: `fixtures/claude/enriched.jsonl` (interruption,
  compaction, sidechain, command tag, Skill/Agent calls, Edit/Write/Read paths),
  `fixtures/codex/enriched.jsonl` (apply_patch add/update/delete, string-encoded
  inputs). Existing fixtures untouched (existing count assertions stay valid).
- Unit: `enrich` and `classify` are pure → table-driven tests; migration v4
  idempotency + memo invalidation; `stats::file_hotspots` grouping/filters.
- Daemon: endpoint tests per existing api test pattern; tracker transition
  tests with injected clock; SSE event shape.
- Real-data verification (not in CI): rebuild a scratch DB from `~/.claude` /
  `~/.codex` via `--db`, then sanity-check hotspot top-N, facet distributions,
  outcome/work-type shares against the probe numbers in §1.
