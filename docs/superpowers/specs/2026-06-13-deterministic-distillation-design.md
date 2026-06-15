# Deterministic Distillation — Design

**Goal:** Make decant *pour the bottle*, not just inventory the cellar. Today the
archive is ~3:1 analytics-to-extraction and nothing it produces is *runnable*.
This tier turns the recorded history of **what actually worked** into concrete,
deterministic, **runnable artifacts** a human can use long after the session is
gone and an agent can consume mid-task: workflow **scripts** mined from real
command history, faithful session **replays**, and pre-filled **skill / AGENTS.md
/ slash-command** files. No LLM, no network — a pure function of the archive.

**Approach:** One pure extractor in `decant-core` (`distill`) turns a set of
sessions into a normalized, success-tagged, secret-redacted **operation stream**;
three thin renderers turn that stream into artifacts. Exposed as a `decant
distill {script,replay,skill}` CLI family that prints to stdout (composable),
emits `--json` (agent-consumable plan), or writes a file with `-o` (human use).
**No schema change** — it reads the existing `tool_call`, `file_ref`, `session`,
and `project` tables, so there is no migration and no backfill.

**Tech stack:** Rust only. `decant-core` (new `distill` module: extractor +
renderers, all pure, returning data structs) and `decant-cli` (new `distill`
command doing all printing/file I/O, opening the DB read-only via the existing
`--db`/`DECANT_DB` read-command path). `docs/api/openapi.yaml` is untouched in v1;
a daemon endpoint + web "Distill" affordance is a deferred follow-up (§10).

---

## 1. Why this, why now (audit, 2026-06-13)

A full inventory of decant's ~38 user-facing surfaces (CLI + HTTP API + web)
splits ~29 **analytics** (counts, costs, leaderboards, charts) vs ~9
**extraction** (search, transcript read, export, recommendations + the new
compounding memory cards). Findings that motivate this tier:

| Fact | Consequence |
|---|---|
| decant exposes **no agent-callable tool surface** (the `mcp` command is analytics *about* MCP servers used; zero JSON-RPC/MCP-protocol code in the workspace) | The stated north star — "make agents faster with structured context" — is unfulfilled. A `--json` plan + a callable CLI closes this without standing up an MCP server. |
| The good extractions (recommendations, memory cards) are consumed by **humans copy-pasting from the web `/insights` page**; the `recommendations` CLI can only `mark-implemented` | Extraction exists but is not *runnable* and barely reachable headless. |
| `tool_call` already stores `input` (raw command/patch text), `is_error`, `ordinal`, `timestamp`, `mcp_server`, joined to `session(cwd, project_id, work_type, outcome)` | An ordered, success-attributed op timeline per session is reconstructable **with no schema change**. |
| Enrichment probe: **13,683 Codex `exec_command` + thousands of Claude `Bash`** calls; **14,382 `file_ref`** rows; hot-context files read in ≥8 sessions | Enough real signal to mine proven commands and faithful replays deterministically. |

The north star is decant's own (`deterministic-enrichment-design.md` §2.1):
*"extract information and determinism, leave judgment to agents, and make them
faster with better structured context."* Distillation is the natural next step:
enrichment made the archive *mean* something; distillation makes it *do*
something.

## 2. Decisions locked

1. **Deterministic-first, no exceptions.** Output is a pure function of (DB
   contents, decant version). Re-running on the same DB yields **byte-identical**
   output (testable; see §9). No LLM, no network, honoring the local-first /
   no-network invariants. Normalization, redaction, and phase/destructive
   classification are versioned rule sets (like `classify.rs` heuristics).
2. **One extractor, three renderers** (§4). Replay and script are the same
   pipeline at different scopes; skill is that plus `file_ref` hot-context
   evidence. Rejected: three independent generators (more code, three places to
   get redaction/normalization wrong).
3. **Mining strategy = S1 + S3; defer S2.** `distill script` defaults to **S1**
   (frequency-ranked, phase-grouped proven commands); `--from-session <id>` and
   `distill replay` use **S3** (single-session, faithful order). **S2**
   (frequent ordered-subsequence mining across sessions) is deferred — higher
   complexity and overfitting risk; revisit with data, mirroring how enrichment
   deferred embeddings.
4. **"Binaries or functions" = runnable scripts + named callables in v1.** The
   pragmatic, deterministic deliverable is shell scripts, `justfile` recipes, and
   `make` targets (a recipe/target *is* a named callable function). Emitting a
   compiled standalone binary (codegen + a toolchain invocation) is **deferred**
   and noted as a possible future `--format` target (§10). Flagged for veto.
5. **Never execute.** decant only emits text; running is the human's choice.
   Generated scripts are review-before-run artifacts (§6).
6. **Ground every line in the archive.** Only commands/ops that actually appear
   are emitted, annotated with their counts. No invented steps — same discipline
   as recommendations.
7. **Scope by root project.** Aggregation rolls worktrees up to
   `project.root_path` (reusing the worktree-rollup notion) so a project's
   sessions across worktrees count together. `--project` matches `project.name`
   or `path`.
8. **No schema change.** Reads existing tables only. CLI opens the DB read-only
   (WAL allows concurrent readers alongside the daemon's writer), exactly like
   `stats`/`files`/`mcp` today. The daemon remains the only writer.

## 3. Data sources (existing schema, no migration)

- **`tool_call`** — `session_id, tool_kind, tool_name, mcp_server, input (TEXT),
  is_error, ordinal, timestamp, duration_ms`. The op backbone.
- **`session`** — `id, tool, project_id, cwd, git_branch, work_type, outcome,
  started_at, ended_at, source_session_id, title`. Scoping + normalization +
  exemplar selection.
- **`file_ref`** — `session_id, path, rel_path, ext, operation, timestamp`. File-op
  side of replay; hot-context evidence for skills.
- **`project`** — `id, path, name, root_path, is_worktree`. Scope resolution +
  worktree rollup.
- **`block`** — `tool_input, tool_result` available as a fallback for payloads not
  mirrored onto `tool_call.input`.

### 3.1 Per-source decoding (precedent: enrichment §1)

- **Claude `Bash`** — `input` is JSON `{"command": "...", ...}` → read `command`.
- **Claude `Edit`/`Write`/`NotebookEdit`** — `input` carries `file_path` +
  (`content` | `old_string`/`new_string`); reconstructs the edit for replay.
- **Codex `exec_command`** — `input` is a **JSON-encoded string** (decode the
  inner payload, then read `cmd`, a shell string). Verified 100% of sampled
  Codex inputs are `Value::String`.
- **Codex `apply_patch`** — `input` is raw patch text with `*** {Add,Update,
  Delete} File:` headers (526/526 parse). Reused for replay's file ops.
- **Grep/Glob/Read** — observation, not action: **excluded** from scripts and
  replay (reads aren't replayed). Search *volume* is out of scope here.

## 4. Core: `decant-core/src/distill.rs` (pure)

### 4.1 Extractor — `timeline(sessions, opts) -> Distillation`

Produces an ordered `Vec<Op>` for the selected sessions:

```rust
struct Op {
    session_id: i64,
    ordinal: i64,            // global order within a session (tool_call.ordinal / ts)
    kind: OpKind,            // Command | FileWrite | FileEdit | FileDelete | Patch
    raw: String,             // decoded, pre-normalization (for replay fidelity)
    normalized: String,      // cwd→$PROJECT_ROOT, $HOME, light placeholdering
    phase: Phase,            // Setup|Build|Test|Lint|Vcs|Deploy|Run|Other
    is_error: bool,
    redacted: bool,          // a secret pattern was masked in `normalized`
    sessions_seen: u32,      // distinct sessions containing this normalized op (S1)
    success_rate: f32,       // non-error calls / total calls for this normalized op
}

struct Distillation {
    scope: Scope,            // project / work_type / session(s), date bounds
    ops: Vec<Op>,
    session_count: u32,
    generated_with: String,  // decant version — part of the determinism contract
}
```

Pure helpers (all table-driven-testable):

- `normalize(raw, session.cwd) -> String` — strip the longest of (`cwd`, project
  `root_path`) → `$PROJECT_ROOT`; `$HOME`; collapse runs of whitespace. Volatile
  placeholdering (timestamps/uuids) is a documented knob, **off by default** in
  v1 (keep output faithful and explainable).
- `redact(s) -> (String, bool)` — deterministic secret denylist (§6). Returns the
  masked string and whether anything matched.
- `classify_phase(normalized) -> Phase` — keyword map
  (`build|compile`→Build; `test|spec`→Test; `lint|fmt|clippy|format`→Lint;
  `install|setup|init|bootstrap`→Setup; `deploy|release|publish`→Deploy;
  `git `→Vcs; `run|serve|start`→Run; else→Other). Versioned by code.
- `is_destructive(normalized) -> Option<Reason>` — deterministic denylist (§6).

### 4.2 Renderers (pure: `Distillation -> RenderedArtifact { filename, body }`)

- **`render_script(&Distillation, ScriptOpts)`** — **S1**: dedupe by `normalized`,
  rank by `sessions_seen` desc then `success_rate` desc then lexical (total
  order), keep ops above a frequency floor (default: seen in ≥25% of scope
  sessions, min 2), group by `Phase`, emit a `bash`/`just`/`make` artifact.
  Destructive ops emitted commented with `# REVIEW`. Header carries provenance
  (project, N sessions, date range, decant version) + `set -euo pipefail`.
  `--from-session <id>` switches to **S3** (one session, faithful order, no
  frequency filter) but still renders a **commands-only recipe** — it does *not*
  re-apply edits. (Reproducing edits is `distill replay`'s job; that is the
  script-vs-replay line: `script` = a reusable command recipe, `replay` =
  faithful reproduction including file edits.)
- **`render_replay(&Distillation)`** — **S3** over exactly one session: interleave
  commands and file ops by `ordinal`/timestamp; drop errored calls by default
  (`--include-errors` keeps them commented with the error preview). Faithfully
  re-applies what we have full payloads for — Write `content` via heredoc, Codex
  `apply_patch` re-emitted as `git apply` — and emits Claude in-place `Edit`s as
  **reviewable annotations** (auto-applying a substring replace isn't
  deterministic without guaranteeing base file state; v1 limitation, documented
  in the header). Reads skipped. Header states the starting-state assumption and
  "best-effort, review before running."
- **`render_skill(&Distillation, hot_context, SkillKind)`** — combine the S1
  recipe with hot-context files (file_ref read in ≥K distinct sessions, ≤2 edits —
  the existing `signal:hot-context` evidence) into:
  - `Skill` → a `SKILL.md` scaffold: frontmatter (`name`/`description` from
    project + dominant `work_type`), **When to use**, **Key files** (hot-context
    `rel_path`s with read counts), **Commands** (embedded recipe), **Notes**
    (left for the human/agent — judgment stays with them).
  - `Agents` → an `AGENTS.md` section: **Commands** + **Where things live**.
  - `Command` → a Claude Code slash-command markdown (frontmatter + body).

Renderers never touch I/O; they return text. The CLI writes/prints it.

## 5. CLI: `decant-cli/src/commands/distill.rs`

```
decant distill script  [--project P] [--work-type T] [--from-session ID]
                       [--format sh|just|make] [--min-frequency F] [-o FILE]
decant distill replay  <session-id> [--include-errors] [-o FILE]
decant distill skill   [--project P] [--kind skill|agents|command] [-o FILE]
```

- **Default → stdout** (composable; `decant distill script --project X > setup.sh`).
- **`--json` → the structured `Distillation`/plan** so agents and other tools
  consume the ops + provenance directly (the agent-callable affordance).
- **`-o FILE` → write the artifact** (human "use it later"); refuse to overwrite
  without `--force`.
- Opens the DB read-only via `db::open` + `schema::migrate` (read-path parity
  with `stats`/`files`/`mcp`); honors `--db`/`DECANT_DB`. All printing/coloring in
  the `output` module. Exit codes: `0` success, `1` no data in scope (with a clear
  message), standard error mapping otherwise.

This single shape satisfies "**agent-callable *and* human-usable after the
fact**": same command, three consumption modes.

### 5.1 Closing the loop with recommendations

The existing `signal:hot-context` / "distill this into AGENTS.md or a skill"
recommendation gains a concrete **action command** in its memory card
(`action`/`prompt` field): e.g. `decant distill skill --project decant --kind
agents`. Advice → generated artifact, in one copy-paste. No code change to the
recommendations engine beyond populating that string.

## 6. Safety (deterministic, non-negotiable)

- **Never execute** — emit text only.
- **Secret redaction** — regex denylist over `--token=`/`--api-key=`/`--password=`
  values, `Authorization: Bearer …`, `*_SECRET=`/`*_TOKEN=`/`*_KEY=`/`*_PASSWORD=`
  env-assignments, and known credential shapes (`AKIA[0-9A-Z]{16}`,
  `gh[pousr]_…`, `sk-…`). Masked → `<REDACTED>`; the op is flagged `redacted`.
  Best-effort, and the header says so.
- **Destructive flagging** — `rm -rf`, `git push --force`/`-f`, `git reset
  --hard`, `git clean -fd`, `kubectl delete`, `docker rm/rmi -f`, `dropdb`,
  `DROP TABLE|DATABASE`, `dd `, `mkfs`, `chmod -R 777`, pipe-to-shell
  (`curl … | sh`). Emitted **commented-out** with `# REVIEW: destructive
  (<reason>), seen in N sessions`. `sudo` is annotated, not auto-commented.
- **Provenance header** on every script: source project, session count, date
  range, decant version, and a "REVIEW before running" line.

## 7. Sample output (`decant distill script --project decant`)

```sh
#!/usr/bin/env bash
# Distilled by decant v0.x from 47 sessions in project "decant" (2026-05-01..06-13).
# Commands you actually ran, ranked by frequency × success. REVIEW before running.
set -euo pipefail
PROJECT_ROOT="${PROJECT_ROOT:-$(git rev-parse --show-toplevel)}"

# setup  (12/47, 100%)
cd "$PROJECT_ROOT/web" && mix deps.get
# build  (41/47, 96%)
cargo build --workspace
# test   (44/47, 98%)
cargo test --workspace
# lint   (38/47, 100%)
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# REVIEW: destructive (rm -rf), seen in 3 sessions — left commented
# rm -rf "$PROJECT_ROOT/target"
```

## 8. Decomposition (3 implementation plans, anchor first)

1. **Extractor + `distill script`** — `distill.rs` (extractor, normalize, redact,
   phase/destructive classifiers, `render_script` for S1 + S3), the `distill`
   CLI command (`script`, stdout/`--json`/`-o`), fixtures + golden tests. *This is
   the substance.*
2. **`distill replay`** — `render_replay` (single-session faithful, edit
   reconstruction from payloads), `replay` subcommand. Reuses the extractor.
3. **`distill skill`** — hot-context query (or reuse the recommendations signal),
   `render_skill` (3 kinds), `skill` subcommand; wire the action command into the
   `signal:hot-context` memory card.

## 9. Testing (test-first, per repo norms)

- **Pure-function unit tests** (table-driven): `normalize`, `redact` (each secret
  shape + a no-match case), `classify_phase`, `is_destructive`, S1 ranking/total
  order, frequency floor.
- **Golden-file tests** for each renderer (script/replay/skill) — assert exact
  body (the `decant version` line is pinned/masked so goldens survive version
  bumps). **Determinism test**: render twice from the same `Distillation`, assert
  byte-identical (the §2.1 contract).
- **New synthetic fixtures**: a small multi-session set with repeated commands, a
  secret-bearing command, a destructive command, a Codex `exec_command`
  (JSON-string-encoded), an `apply_patch`, and one clean `completed` exemplar for
  replay. Existing fixtures untouched.
- **CLI tests**: stdout vs `-o` vs `--json`; overwrite refusal; empty-scope exit
  code/message.
- **Real-archive sanity (not in CI)**: rebuild a scratch DB from `~/.claude`/
  `~/.codex` via `--db`; eyeball distilled scripts for top projects, a replay of a
  known session, and a generated AGENTS.md section — mirroring enrichment §10.

## 10. Explicitly deferred

- **S2 ordered-sequence mining** — consensus workflows with true order; revisit
  with data.
- **Daemon endpoint + web "Distill" affordance** — `POST /api/v1/distill/...`
  returning artifact text, surfaced as a button on `/insights`. The CLI + core is
  the substance; the web path is additive and goes through the daemon (the DB
  owner), documented in `openapi.yaml` when built.
- **Compiled standalone binary** `--format` target (§2.4).
- **Shell functions/aliases generator** (the 4th artifact type, not selected) —
  trivially layerable on the same extractor later.
- **Agent-assisted prose polish** for skill files — would mean a user-invoked
  agent flow POSTing back through the API; kept out to preserve determinism /
  no-network, exactly as enrichment deferred LLM enrichment.
