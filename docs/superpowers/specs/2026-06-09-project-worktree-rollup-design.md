# Project / Worktree Roll-up for "By Project" — Design

**Goal:** Make the Analytics "By Project" breakdown roll a git **worktree's** cost and
sessions up under its **root project**, while keeping each worktree individually
identifiable. Today every distinct working directory becomes its own `project`
row, so a repo worked on across several worktrees has its cost scattered across
many rows instead of summed under the repo it belongs to.

**Approach:** Resolve each project's *root* and *worktree* identity at ingest with a
single layered, idempotent pass (`decant_core::worktree::resolve_worktree_roots`),
ranked by confidence: git-authoritative (live dir) → in-tree path string → external
name-match → synthetic. Persist the result on the `project` row. "By Project" then
groups by the resolved `root_path`; drill-down includes every worktree; an inline
expand shows the per-worktree split.

**Tech stack:** Rust (`decant-core` new `worktree` module + schema migration V3;
`decant-daemon` query grouping, filter, and one new query param), `docs/api/openapi.yaml`
(the contract of record), and `web/` (`AnalyticsLive` render + expand). No new
dependencies; git resolution reads the `.git` pointer file directly (no `git`
subprocess), keeping `decant-core` dependency-free and offline.

---

## 1. Context and motivation

`decant` keys a `project` row by a session's `cwd` (full path; `name` = basename).
"By Project" is `GROUP BY COALESCE(p.path, '(none)')` in
`decant-daemon::api::query::by_dimension`. The project filter (row drill-down)
matches `s.project_id = (SELECT id FROM project WHERE path = ?)`.

A git **worktree** runs in a *different* working directory than the main checkout,
so it lands as its own `project` row and its cost never joins the repo's total.

The real footprint, from `~/.claude/projects` on this machine: **~50 of 67 project
dirs are worktrees**, produced by four different tools, in two structural families:

- **In-tree** — `…/dosu/dosu/.claude-worktrees/teedole-ops-39-docs-are-dead`. The
  root repo path *is* present in the string (the part before `/.claude-worktrees/`).
  Recoverable from the string alone, even after the worktree is deleted.
- **External / sibling** — `~/.warp-worktrees/dosu-agate-spire`,
  `~/.t3-worktrees/dosu-t3code-2d73eb17`, `~/conductor/workspaces/dosu-abuja`. The
  root path is **not** in the string; only a repo-name prefix (`dosu`) plus a random
  codename survives.

**Pivotal constraint:** nearly every worktree directory sampled is **already deleted
from disk**. Authoritative `git` resolution (reading the worktree's `.git` pointer)
only works while the worktree is *live*, so it fixes things going forward but cannot
retroactively recover deleted worktrees. Existing rows can only be rolled up with
path-string heuristics.

## 2. Decisions locked

Two product decisions were made up front:

1. **Scope = Both.** Authoritative git resolution for live worktrees at ingest
   (correct, forever) **plus** best-effort path-string heuristics to roll up the
   existing/deleted worktrees now. Historical external worktrees are heuristic and a
   few may attribute imperfectly; that tradeoff is accepted in exchange for fixing the
   currently-fragmented analytics immediately.
2. **UX = root rows + drill-down / expand.** "By Project" shows one rolled-up row per
   root project (worktree cost summed in) with a small `▸ N wt` marker. Row-click
   drills into all sessions for that root (worktrees included). Clicking the marker
   expands an inline, indented per-worktree split (`wt: <label> (<tool>)`).

*Approaches rejected:* **query-time-only** (string parsing in SQL is fragile, recomputed
every request, and cannot capture live-git truth) and **git-only / no heuristics**
(leaves the ~50 deleted worktree rows fragmented forever — fails the Both goal).

## 3. Data model

Five columns added to `project`. They go in `crates/decant-core/src/schema_v1.sql`
(so fresh DBs have them) and are added to existing DBs by migration V3 (§5), with the
`ALTER`s guarded by `PRAGMA table_info(project)` so applying V3 to a fresh DB that
already has the columns is a no-op.

| Column | Type | Meaning |
|---|---|---|
| `is_worktree` | `INTEGER NOT NULL DEFAULT 0` | 1 if this project is a worktree of another repo. |
| `root_path` | `TEXT` | Resolved root project key. For a non-worktree, equals `path`. For a worktree, the root repo path (real path when known) or a synthetic repo key. Defaults to `path` until resolved. |
| `worktree_label` | `TEXT` | Human label for the worktree (e.g. `agate-spire`, `ops-39-docs-are-dead`). Null for roots. |
| `worktree_tool` | `TEXT` | `warp` \| `t3` \| `conductor` \| `claude` \| `git`. Null for roots. |
| `root_source` | `TEXT` | `self` \| `git` \| `intree` \| `namematch` \| `synthetic`. Confidence tier and lock signal. |

`root_source = 'git'` rows are **locked**: later heuristic passes never overwrite an
authoritative result (the worktree may since have been deleted; we trust the earlier
live read).

The schema stays an internal detail of the daemon; the cross-process contract is the
HTTP API (§6).

## 4. Resolution algorithm

A new UI-agnostic module `crates/decant-core/src/worktree.rs`. Pure string logic is
isolated from filesystem access so it is unit-testable without a disk.

### 4.1 Pure classifier (no I/O)

```rust
pub enum RootSource { SelfRoot, Git, Intree, NameMatch, Synthetic } // serde → "self" | "git" | …

pub struct Resolution {
    pub is_worktree: bool,
    pub root_path: String,
    pub worktree_label: Option<String>,
    pub worktree_tool: Option<String>,
    pub source: RootSource,
}

pub struct KnownRoot { pub path: String, pub basename: String, pub sessions: i64, pub last_seen: Option<String> }

/// In-tree worktree? (`.worktrees` / `.claude-worktrees` segment). No I/O.
pub fn classify_intree(path: &str) -> Option<Resolution>;
/// External worktree container? Returns (tool, leaf) for warp/t3/conductor. No I/O.
pub fn external_container(path: &str) -> Option<(&'static str, String)>;
/// Resolve an external worktree leaf: name-match against known roots, else synthetic. No I/O.
pub fn classify_external(tool: &str, leaf: &str, path: &str, known_roots: &[KnownRoot]) -> Resolution;
```

The classifier is split into pieces because external name-matching needs the set of
roots discovered earlier in the pass (see §4.3). Rules, first match wins:

1. **In-tree** (`classify_intree`). If a path component is `.worktrees` (tool `git`) or
   `.claude-worktrees` (tool `claude`): `root_path` = the components before it joined as
   an absolute path; `worktree_label` = the components after it joined by `/`;
   `is_worktree = true`; `source = Intree`. Only dot-prefixed segment names match, to
   avoid false positives on a real directory literally named `worktrees`.
2. **External container** (`external_container` + `classify_external`). If the parent
   segment is `.warp-worktrees` (warp), `.t3-worktrees` (t3), or the two-segment
   `conductor/workspaces` (conductor), take the single `leaf` dir under it and:
   - **Name-match.** Among `known_roots`, keep those whose `basename` `B` satisfies
     `leaf == B || leaf.starts_with(&format!("{B}-"))`. Pick the **longest** `B`;
     tie-break by most `sessions`, then most-recent `last_seen`. On a hit:
     `root_path` = that root's `path`; `is_worktree = true`; `source = NameMatch`;
     `worktree_label` = `leaf` with the matched `B-` prefix stripped.
   - **Synthetic.** No name-match → strip the codename per tool to a best-effort repo
     key: t3 strips a trailing `-t3code-<hex>`; warp strips the trailing two
     `-`-tokens; conductor strips the trailing one `-`-token. `root_path` = that key
     (a bare repo name, not a path); `is_worktree = true`; `source = Synthetic`;
     `worktree_label` = the stripped codename. If stripping cannot produce a non-empty
     key (too few tokens), the leaf becomes its own unmerged root (`is_worktree =
     true`, `root_path = path`, `source = Synthetic`) — under-merge rather than
     mis-merge.
3. **Self.** Neither in-tree nor an external container → a normal project:
   `is_worktree = false`, `root_path = path`, `source = SelfRoot`.

Synthetic keys never collide with real roots: name-match runs first and only falls
through to synthetic when *no* known root for that repo exists. If a real root later
appears, the next resolve pass re-attributes the worktree to the real path and the
synthetic key empties out — self-correcting.

### 4.2 Git resolution (live dir only, filesystem)

```rust
/// Returns Some when `<dir>/.git` is a worktree pointer file.
pub fn resolve_git_root(dir: &Path) -> Option<GitResolution>;
```

Reads `<dir>/.git`. If it is a regular file containing `gitdir: <X>/.git/worktrees/<name>`,
then `root_path = <X>`, `worktree_label = <name>`, `is_worktree = true`,
`source = Git`. (`worktree_tool` is inferred from `dir`'s container, else `git`.) If
`<dir>/.git` is a directory, `dir` is a main checkout → root (`is_worktree = false`).
If `<dir>` does not exist or has no `.git`, returns `None` and the orchestrator falls
back to the string classifiers (`classify_intree` / `external_container`).

### 4.3 Orchestrator

```rust
pub fn resolve_worktree_roots(conn: &Connection) -> Result<()>;
```

Idempotent; operates only on the `project` table plus cheap FS stats:

1. Load every project (`id, path, root_source, session count, last_seen`).
2. **Pass A — git / in-tree / self.** For each project, in order:
   `resolve_git_root(path)` if not already locked (lock with `Git` on success) →
   else `classify_intree(path)` → else, if `external_container(path)` matches, mark it
   **deferred** → else `SelfRoot` (`root_path = path`). A project already locked at
   `root_source = 'git'` keeps its stored result.
3. Build `known_roots` from the **distinct `root_path` values produced in Pass A**
   (i.e. from every non-deferred project — self, git, and in-tree alike), each carrying
   its `basename`, summed `sessions`, and max `last_seen`. This means a root discovered
   only via an in-tree worktree (no standalone project row of its own) is still a valid
   name-match target.
4. **Pass B — external.** For each deferred project, call
   `classify_external(tool, leaf, path, &known_roots)` → `NameMatch` or `Synthetic`.
5. Write back the five columns for rows whose resolution changed.

A handful of FS stats per project per cycle is negligible.

### 4.4 Worked examples (real paths)

| `path` | `root_path` | label | tool | source |
|---|---|---|---|---|
| `/Users/onlydole/oss/decant` | `/Users/onlydole/oss/decant` | — | — | self |
| `/Users/onlydole/dosu/dosu/.claude-worktrees/teedole-ops-39-docs-are-dead` | `/Users/onlydole/dosu/dosu` | `teedole-ops-39-docs-are-dead` | claude | intree |
| `/Users/onlydole/.warp-worktrees/dosu-agate-spire` *(root `…/dosu/dosu` known)* | `/Users/onlydole/dosu/dosu` | `agate-spire` | warp | namematch |
| `/Users/onlydole/conductor/workspaces/dosu-abuja` *(root known)* | `/Users/onlydole/dosu/dosu` | `abuja` | conductor | namematch |
| `/Users/onlydole/.t3-worktrees/dosu-t3code-2d73eb17` *(no root known)* | `dosu` *(synthetic key)* | `t3code-2d73eb17` | t3 | synthetic |
| `/Users/onlydole/.warp-worktrees/youup-nighthawk-mirador` *(live at ingest)* | *(real root from `.git`)* | `nighthawk-mirador` | warp | git |

## 5. Ingest & migration flow

- **Migration V3** (`schema.rs`, `LATEST_VERSION` → 3): a programmatic step (not a pure
  SQL batch) that, for each new column, checks `PRAGMA table_info(project)` and `ALTER
  TABLE project ADD COLUMN …` if missing, then calls `resolve_worktree_roots(conn)`
  once to backfill existing rows. No full DB rebuild is needed — resolution reads only
  the `project` table and the filesystem, never the session files. (`migrate()` gains a
  small `if current < 3` branch alongside the existing SQL-batch helper.)
- **Per ingest cycle:** the daemon's ingest task calls `resolve_worktree_roots` after a
  batch commits, so newly-seen worktrees are linked and benefit from newly-seen roots.
  A `synthetic` row upgrades to `git` if that worktree is ever live during a later
  ingest.

This respects invariant 3 (compute-and-store, not query-time) and the "WAL, single
writer" invariant — resolution runs on the daemon's write connection inside the ingest
task.

## 6. Daemon API + OpenAPI

`crates/decant-daemon/src/api/query.rs` and `filters.rs`:

- **Rolled-up grouping.** `by_dimension(Project)` groups by
  `COALESCE(p.root_path, p.path, '(none)')` instead of `p.path`. Each row also returns
  `worktree_count = COUNT(DISTINCT CASE WHEN p.is_worktree = 1 THEN p.id END)`.
- **`DimRow` gains optional fields:** `worktree_count: Option<i64>` (rolled-up rows),
  `worktree_label: Option<String>` / `worktree_tool: Option<String>` (leaf rows). Other
  dimensions leave them `None`; serialization is additive and backward-compatible.
- **New `root` query param** on `GET /api/v1/analytics/by-dimension`. When set (with
  `dim=project`), the query scopes to `WHERE p.root_path = ?` and groups by leaf
  `p.path`, returning one row per worktree (root checkout + each worktree) with
  `worktree_label` / `worktree_tool` for display. This powers the inline expand.
- **Drill-down filter.** `Filters` project clause changes from `path = ?` to
  `s.project_id IN (SELECT id FROM project WHERE root_path = ? OR path = ?)` (same value
  bound twice), so clicking a root row includes all its worktrees; the `OR path = ?`
  is a safety net for any unresolved/null `root_path`.

`docs/api/openapi.yaml` (the contract of record, served at `/api/v1/openapi.yaml`):
document the new `root` query parameter and the three optional `DimRow` fields. The
existing openapi smoke test stays green.

## 7. Web UX (`AnalyticsLive`)

`web/lib/decant_web/live/analytics_live.ex` + `web/lib/decant/archive.ex` /
`daemon.ex`:

- **Rolled-up rows** keyed by `root_path`. Render the project key as
  `basename(root_path)` (full path still available on hover/title), with a `▸ N wt`
  marker when `worktree_count > 0`.
- **Row-click** navigates to `/?project=<root_path>` (the existing `Filters.url`
  mechanism), now filtering sessions to the whole root.
- **Expand** — clicking `▸` toggles an inline fetch of
  `Archive.by_dimension(:project, %{… root: root_path})` and renders indented
  `wt: <label> (<tool>)` sub-rows, matching the chosen preview. Expansion state lives in
  the LiveView assigns; rows collapse by default. The daemon client (`Decant.Daemon`)
  passes `root` through as a query param.

## 8. Testing (test-first)

- **Core (`worktree.rs`):** a table-driven test over the string classifiers
  (`classify_intree` / `external_container` / `classify_external`) covering: plain
  dir; `.worktrees` and `.claude-worktrees` in-tree; warp/t3/conductor leaves *with* and
  *without* a matching known root; longest-basename tie-break; same-basename collision
  tie-break (sessions then recency). A separate test for `resolve_git_root` builds a
  temp dir with a fake `.git` pointer file (and a `.git` directory) and asserts the
  parsed root/label. An integration test seeds a SQLite DB with worktree + root rows,
  runs `resolve_worktree_roots`, and asserts the five columns.
- **Daemon (`query.rs` / `api_test.rs`):** rolled-up `dim=project` grouping with
  `worktree_count`; `dim=project&root=<root>` leaf breakdown; and
  drill-down-includes-worktrees. Seed an in-test SQLite DB with a root plus two
  worktrees pointing at it.
- **Migration (`schema.rs`):** a V2→V3 test that the `ALTER`s land, are idempotent
  (re-running `migrate` is a no-op), and that backfill populates `root_path` for a
  pre-seeded worktree row.
- **Web (`analytics_live_test.exs`):** the rolled-up table renders the `▸ N wt` marker,
  row-click targets `/?project=<root_path>`, and the expand event renders worktree
  sub-rows. The daemon is mocked per the existing pattern (no live daemon).

## 9. Edge cases & limits

- `cwd = NULL` (some sessions) → no project row; unchanged `(none)` bucket.
- Non-git directory → `self`, `root_path = path`.
- Path normalization (trailing slash, etc.) is string-only so deleted paths resolve
  deterministically; the git path may additionally canonicalize via the filesystem.
- Same-basename collision (`/Users/onlydole/dosu` vs `/Users/onlydole/dosu/dosu`):
  name-match tie-breaks by session count then recency; the git path sidesteps it
  entirely for live worktrees. This is a documented heuristic limp.
- Synthetic external roots (no live dir, no known root) are best-effort; they merge
  per-tool worktrees of the same repo but cannot recover the real root path until a
  real root for that repo is seen.

## 10. Files touched

- `crates/decant-core/src/worktree.rs` (new), `lib.rs` (module export)
- `crates/decant-core/src/schema_v1.sql` (project columns), `schema.rs` (V3 migration + backfill)
- `crates/decant-daemon/src/api/query.rs` (grouping, `root` param, `DimRow` fields),
  `filters.rs` (project clause)
- `docs/api/openapi.yaml` (param + fields)
- `web/lib/decant/daemon.ex`, `web/lib/decant/archive.ex` (pass `root`), 
  `web/lib/decant_web/live/analytics_live.ex` (render + expand)
- Tests alongside each.

## 11. Out of scope

- Re-attributing **historical external** worktrees to their *real* root path (only the
  synthetic repo key is recoverable once the dir is deleted). Going-forward live
  worktrees get the real path via git.
- Worktree roll-up on dimensions other than project, and CLI `--by project` changes
  (the CLI uses core `stats::by_dimension`, which can adopt the same `root_path`
  grouping in a follow-up if desired).
- Editing or relocating worktrees; decant only reads.
