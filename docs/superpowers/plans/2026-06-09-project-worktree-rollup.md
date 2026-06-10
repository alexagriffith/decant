# Project / Worktree Roll-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Analytics "By Project" breakdown roll a git worktree's cost and sessions up under its root project, while keeping each worktree individually identifiable via an inline expand.

**Architecture:** A new UI-agnostic `decant_core::worktree` module resolves each project's root + worktree identity in a layered, idempotent pass (git-authoritative → in-tree string → external name-match → synthetic), persisted on new `project` columns at ingest and backfilled by a schema migration. The daemon groups "By Project" by the resolved `root_path`, exposes a per-worktree leaf breakdown via a new `root` query param, and widens the project drill-down filter to include worktrees. Phoenix renders rolled-up rows with a `▸ N wt` marker that expands inline.

**Tech Stack:** Rust (`rusqlite`, `decant-core`, `decant-daemon` with `axum`), OpenAPI 3.1 (`docs/api/openapi.yaml`), Elixir/Phoenix LiveView (`web/`). No new dependencies; git resolution reads the `.git` pointer file directly (no subprocess).

**Spec:** `docs/superpowers/specs/2026-06-09-project-worktree-rollup-design.md`

---

## File Structure

**Create:**
- `crates/decant-core/src/worktree.rs` — types, pure string classifiers, `resolve_git_root`, and the `resolve_worktree_roots` orchestrator. One clear responsibility: turn a project path (+ DB context) into a root/worktree resolution.

**Modify:**
- `crates/decant-core/src/lib.rs` — register the `worktree` module.
- `crates/decant-core/src/schema_v1.sql` — add five columns to `project`.
- `crates/decant-core/src/schema.rs` — V3 migration (PRAGMA-guarded ALTERs + backfill); bump `LATEST_VERSION`.
- `crates/decant-core/src/ingest.rs` — call `resolve_worktree_roots` at the end of `sync` when anything ingested.
- `crates/decant-daemon/src/api/query.rs` — `DimRow` fields + rolled-up/leaf `by_dimension` for the project dimension.
- `crates/decant-daemon/src/api/analytics.rs` — `root` query param plumbed into `by_dimension`.
- `crates/decant-daemon/src/api/filters.rs` — project drill-down filter widened to `root_path OR path`.
- `docs/api/openapi.yaml` — `Root` parameter + `DimensionRow` worktree fields.
- `web/lib/decant/archive.ex` — map worktree fields; pass `root` through `to_params`.
- `web/lib/decant_web/live/analytics_live.ex` — rolled-up render, `▸ N wt` marker, expand event + sub-rows.

---

## Task 1: Core — worktree types + pure string classifiers

**Files:**
- Create: `crates/decant-core/src/worktree.rs`
- Modify: `crates/decant-core/src/lib.rs:13` (add module after `pub mod tools;`)
- Test: in `crates/decant-core/src/worktree.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Register the module**

In `crates/decant-core/src/lib.rs`, add after line 13 (`pub mod tools;`):

```rust
pub mod worktree;
```

- [ ] **Step 2: Write `worktree.rs` with types + classifier signatures (stubs) + failing tests**

Create `crates/decant-core/src/worktree.rs`:

```rust
//! Identify a project's *root* repo and *worktree* identity from its path, so
//! analytics can roll worktree cost up under the repo it belongs to.
//!
//! Resolution is layered by confidence: git-authoritative (live dir) → in-tree
//! path string → external name-match → synthetic. The string logic here is pure
//! (no I/O) and unit-tested in isolation; `resolve_git_root` and the orchestrator
//! `resolve_worktree_roots` (added in later tasks) are the only parts that touch
//! the filesystem / DB.

/// Confidence tier of a root resolution; also the value stored in
/// `project.root_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    SelfRoot,
    Git,
    Intree,
    NameMatch,
    Synthetic,
}

impl RootSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RootSource::SelfRoot => "self",
            RootSource::Git => "git",
            RootSource::Intree => "intree",
            RootSource::NameMatch => "namematch",
            RootSource::Synthetic => "synthetic",
        }
    }
}

/// The resolved root + worktree identity for one project path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub is_worktree: bool,
    pub root_path: String,
    pub worktree_label: Option<String>,
    pub worktree_tool: Option<String>,
    pub source: RootSource,
}

/// A known root project, used as a name-match target for external worktrees.
#[derive(Debug, Clone)]
pub struct KnownRoot {
    pub path: String,
    pub basename: String,
    pub sessions: i64,
    pub last_seen: Option<String>,
}

/// Split an absolute-ish path into non-empty segments, remembering the leading `/`.
fn segments(path: &str) -> (bool, Vec<&str>) {
    let abs = path.starts_with('/');
    (abs, path.split('/').filter(|s| !s.is_empty()).collect())
}

/// Re-join segments back into a path, restoring the leading `/` when `abs`.
fn join(abs: bool, parts: &[&str]) -> String {
    let body = parts.join("/");
    if abs {
        format!("/{body}")
    } else {
        body
    }
}

/// Last non-empty path segment (e.g. basename of a root path or a synthetic key).
pub fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// In-tree worktree? Matches a `.worktrees` (tool `git`) or `.claude-worktrees`
/// (tool `claude`) segment and returns the root (everything before it) plus the
/// label (everything after it). No I/O.
pub fn classify_intree(path: &str) -> Option<Resolution> {
    let (abs, segs) = segments(path);
    let idx = segs
        .iter()
        .position(|s| *s == ".worktrees" || *s == ".claude-worktrees")?;
    if idx == 0 || idx + 1 >= segs.len() {
        return None; // need a root before and a label after
    }
    let tool = if segs[idx] == ".claude-worktrees" {
        "claude"
    } else {
        "git"
    };
    Some(Resolution {
        is_worktree: true,
        root_path: join(abs, &segs[..idx]),
        worktree_label: Some(segs[idx + 1..].join("/")),
        worktree_tool: Some(tool.to_string()),
        source: RootSource::Intree,
    })
}

/// External worktree container? Recognizes `.warp-worktrees`, `.t3-worktrees`
/// (parent segment) and `conductor/workspaces` (two segments). Returns
/// `(tool, leaf)` where `leaf` is the single directory under the container. No I/O.
pub fn external_container(path: &str) -> Option<(&'static str, String)> {
    let (_, segs) = segments(path);
    for (i, seg) in segs.iter().enumerate() {
        let tool = match *seg {
            ".warp-worktrees" => Some("warp"),
            ".t3-worktrees" => Some("t3"),
            "workspaces" if i > 0 && segs[i - 1] == "conductor" => Some("conductor"),
            _ => None,
        };
        if let Some(tool) = tool {
            if let Some(leaf) = segs.get(i + 1) {
                return Some((tool, leaf.to_string()));
            }
        }
    }
    None
}

/// Resolve an external worktree leaf: name-match against known roots, else a
/// best-effort synthetic repo key by stripping the per-tool codename. No I/O.
pub fn classify_external(
    tool: &str,
    leaf: &str,
    path: &str,
    known_roots: &[KnownRoot],
) -> Resolution {
    // Name-match: longest matching basename wins; tie-break sessions then recency.
    let best = known_roots
        .iter()
        .filter(|r| !r.basename.is_empty())
        .filter(|r| leaf == r.basename || leaf.starts_with(&format!("{}-", r.basename)))
        .max_by(|a, b| {
            a.basename
                .len()
                .cmp(&b.basename.len())
                .then(a.sessions.cmp(&b.sessions))
                .then(a.last_seen.cmp(&b.last_seen))
        });
    if let Some(root) = best {
        let label = leaf
            .strip_prefix(&format!("{}-", root.basename))
            .map(str::to_string);
        return Resolution {
            is_worktree: true,
            root_path: root.path.clone(),
            worktree_label: label,
            worktree_tool: Some(tool.to_string()),
            source: RootSource::NameMatch,
        };
    }

    // Synthetic: strip the codename to a bare repo key.
    let key = strip_codename(tool, leaf);
    let (root_path, label) = if key.is_empty() {
        (path.to_string(), None) // unmerged — under-merge rather than mis-merge
    } else {
        let label = leaf.strip_prefix(&format!("{key}-")).map(str::to_string);
        (key, label)
    };
    Resolution {
        is_worktree: true,
        root_path,
        worktree_label: label,
        worktree_tool: Some(tool.to_string()),
        source: RootSource::Synthetic,
    }
}

/// Best-effort repo key from an external worktree leaf, per tool convention:
/// t3 = `<repo>-t3code-<hash>`, warp = `<repo>-<two-word-codename>`,
/// conductor = `<repo>-<one-word-codename>`.
fn strip_codename(tool: &str, leaf: &str) -> String {
    match tool {
        "t3" => match leaf.find("-t3code-") {
            Some(idx) => leaf[..idx].to_string(),
            None => leaf.to_string(),
        },
        "warp" => {
            let toks: Vec<&str> = leaf.split('-').collect();
            if toks.len() >= 3 {
                toks[..toks.len() - 2].join("-")
            } else {
                leaf.to_string()
            }
        }
        "conductor" => {
            let toks: Vec<&str> = leaf.split('-').collect();
            if toks.len() >= 2 {
                toks[..toks.len() - 1].join("-")
            } else {
                leaf.to_string()
            }
        }
        _ => leaf.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intree_claude_worktree_recovers_root_and_label() {
        let r = classify_intree("/Users/onlydole/dosu/dosu/.claude-worktrees/teedole-ops-39")
            .unwrap();
        assert!(r.is_worktree);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("teedole-ops-39"));
        assert_eq!(r.worktree_tool.as_deref(), Some("claude"));
        assert_eq!(r.source, RootSource::Intree);
    }

    #[test]
    fn plain_path_is_not_intree() {
        assert!(classify_intree("/Users/onlydole/oss/decant").is_none());
    }

    #[test]
    fn external_container_detects_warp_and_conductor() {
        assert_eq!(
            external_container("/Users/onlydole/.warp-worktrees/dosu-agate-spire"),
            Some(("warp", "dosu-agate-spire".to_string()))
        );
        assert_eq!(
            external_container("/Users/onlydole/conductor/workspaces/dosu-abuja"),
            Some(("conductor", "dosu-abuja".to_string()))
        );
        assert_eq!(external_container("/Users/onlydole/oss/decant"), None);
    }

    #[test]
    fn external_namematches_known_root() {
        let roots = vec![KnownRoot {
            path: "/Users/onlydole/dosu/dosu".into(),
            basename: "dosu".into(),
            sessions: 10,
            last_seen: Some("2026-06-01".into()),
        }];
        let r = classify_external(
            "warp",
            "dosu-agate-spire",
            "/Users/onlydole/.warp-worktrees/dosu-agate-spire",
            &roots,
        );
        assert_eq!(r.source, RootSource::NameMatch);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
        assert_eq!(r.worktree_tool.as_deref(), Some("warp"));
        assert!(r.is_worktree);
    }

    #[test]
    fn external_namematch_tiebreaks_on_sessions() {
        let roots = vec![
            KnownRoot {
                path: "/Users/onlydole/dosu".into(),
                basename: "dosu".into(),
                sessions: 2,
                last_seen: Some("2026-06-01".into()),
            },
            KnownRoot {
                path: "/Users/onlydole/dosu/dosu".into(),
                basename: "dosu".into(),
                sessions: 40,
                last_seen: Some("2026-05-01".into()),
            },
        ];
        let r = classify_external("warp", "dosu-agate-spire", "/x", &roots);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu", "more sessions wins");
    }

    #[test]
    fn external_synthetic_strips_codename_per_tool() {
        assert_eq!(
            classify_external("warp", "dosu-agate-spire", "/x", &[]).root_path,
            "dosu"
        );
        assert_eq!(
            classify_external("t3", "dosu-t3code-2d73eb17", "/x", &[]).root_path,
            "dosu"
        );
        assert_eq!(
            classify_external("conductor", "dosu-abuja", "/x", &[]).root_path,
            "dosu"
        );
        let r = classify_external("warp", "dosu-agate-spire", "/x", &[]);
        assert_eq!(r.source, RootSource::Synthetic);
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p decant-core worktree::tests -- --nocapture`
Expected: all tests in the module PASS (the implementation is included above; this task ships the classifiers and their tests together).

- [ ] **Step 4: Lint + format**

Run: `cargo fmt --all && cargo clippy -p decant-core --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/decant-core/src/worktree.rs crates/decant-core/src/lib.rs
git commit -m "feat(core): worktree path classifiers (intree, external, synthetic)"
```

---

## Task 2: Core — `resolve_git_root` (authoritative, live dir)

**Files:**
- Modify: `crates/decant-core/src/worktree.rs`
- Test: same file's `tests` module

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/decant-core/src/worktree.rs`:

```rust
    #[test]
    fn git_pointer_file_resolves_authoritative_root() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("agate-spire");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            wt.join(".git"),
            "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
        )
        .unwrap();
        let r = resolve_git_root(&wt).unwrap();
        assert!(r.is_worktree);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
        assert_eq!(r.source, RootSource::Git);
    }

    #[test]
    fn git_directory_is_main_checkout_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        assert!(resolve_git_root(tmp.path()).is_none());
    }

    #[test]
    fn missing_or_non_worktree_git_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_git_root(tmp.path()).is_none()); // no .git at all
        // a submodule-style pointer is not a worktree we roll up
        std::fs::write(tmp.path().join(".git"), "gitdir: ../.git/modules/foo\n").unwrap();
        assert!(resolve_git_root(tmp.path()).is_none());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p decant-core worktree::tests::git -- --nocapture`
Expected: FAIL — `cannot find function resolve_git_root in this scope`.

- [ ] **Step 3: Implement `resolve_git_root` + tool inference**

Add to `crates/decant-core/src/worktree.rs` (after `classify_external`, before `#[cfg(test)]`). Add `use std::path::Path;` to the top of the file:

```rust
/// Authoritative resolution for a *live* worktree: if `<dir>/.git` is a regular
/// file pointing at `<root>/.git/worktrees/<name>`, return that root. Returns
/// `None` for a main checkout (`.git` is a directory), a missing dir, or a
/// non-worktree pointer (e.g. a submodule). Touches the filesystem.
pub fn resolve_git_root(dir: &Path) -> Option<Resolution> {
    let dotgit = dir.join(".git");
    if !dotgit.is_file() {
        return None; // .git dir = main checkout; absent = not a repo
    }
    let content = std::fs::read_to_string(&dotgit).ok()?;
    let target = content
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))?
        .trim();
    // git ≥ 2.48 can write relative pointers (worktree.useRelativePaths); a
    // relative root would be a junk grouping key, so fall back to the string
    // classifiers instead of locking it in as authoritative.
    if !Path::new(target).is_absolute() {
        return None;
    }
    let marker = "/.git/worktrees/";
    let idx = target.rfind(marker)?;
    let root_path = target[..idx].to_string();
    let name = target[idx + marker.len()..].trim_end_matches('/');
    if root_path.is_empty() || name.is_empty() {
        return None;
    }
    Some(Resolution {
        is_worktree: true,
        root_path,
        worktree_label: Some(name.to_string()),
        worktree_tool: Some(infer_tool(&dir.to_string_lossy()).to_string()),
        source: RootSource::Git,
    })
}

/// Infer the worktree tool from the worktree's own path (container or in-tree
/// segment), defaulting to plain `git`.
pub fn infer_tool(path: &str) -> &'static str {
    if let Some((tool, _)) = external_container(path) {
        return tool;
    }
    let (_, segs) = segments(path);
    if segs.iter().any(|s| *s == ".claude-worktrees") {
        "claude"
    } else {
        "git"
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p decant-core worktree::tests -- --nocapture`
Expected: PASS (all worktree tests).

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt --all && cargo clippy -p decant-core --all-targets -- -D warnings
git add crates/decant-core/src/worktree.rs
git commit -m "feat(core): authoritative git-pointer worktree resolution"
```

---

## Task 3: Core — schema columns + V3 migration

**Files:**
- Modify: `crates/decant-core/src/schema_v1.sql:1-7`
- Modify: `crates/decant-core/src/schema.rs:7` (LATEST_VERSION), add V3 branch + helper, update tests
- Test: `crates/decant-core/src/schema.rs` `tests` module

- [ ] **Step 1: Add the columns to the fresh-DB schema**

In `crates/decant-core/src/schema_v1.sql`, replace the `project` table (lines 1-7) with:

```sql
CREATE TABLE IF NOT EXISTS project (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  name TEXT,
  first_seen_at TEXT,
  last_seen_at TEXT,
  is_worktree INTEGER NOT NULL DEFAULT 0,
  root_path TEXT,
  worktree_label TEXT,
  worktree_tool TEXT,
  root_source TEXT
);
```

- [ ] **Step 2: Write the failing migration test**

In `crates/decant-core/src/schema.rs` `tests` module, add:

```rust
    #[test]
    fn v3_adds_project_worktree_columns_idempotently() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // idempotent

        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(project)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        for c in [
            "is_worktree",
            "root_path",
            "worktree_label",
            "worktree_tool",
            "root_source",
        ] {
            assert!(cols.contains(&c.to_string()), "missing column {c}");
        }
        let max: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(max, 3);
    }

    #[test]
    fn v3_backfills_existing_worktree_rows_on_a_v2_db() {
        // Model a DB at v2: project table WITHOUT the new columns + a minimal
        // session table for the resolve query's join.
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
             CREATE TABLE project(id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, name TEXT,
                                  first_seen_at TEXT, last_seen_at TEXT);
             CREATE TABLE session(id INTEGER PRIMARY KEY, project_id INTEGER, started_at TEXT);
             INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'));
             INSERT INTO project(path, name) VALUES ('/home/x/dosu/dosu', 'dosu');
             INSERT INTO project(path, name) VALUES ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let (is_wt, root): (i64, String) = conn
            .query_row(
                "SELECT is_worktree, root_path FROM project WHERE path = '/home/x/.warp-worktrees/dosu-agate-spire'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(is_wt, 1);
        assert_eq!(root, "/home/x/dosu/dosu", "warp worktree name-matched to root");
    }
```

Also update the two existing version assertions:
- `crates/decant-core/src/schema.rs:110` — change `assert_eq!(versions, 2, ...)` to `assert_eq!(versions, 3, "each migration recorded exactly once");`
- `crates/decant-core/src/schema.rs:156` — change `assert_eq!(max, 2);` to `assert_eq!(max, 3);`

- [ ] **Step 3: Run to verify the new tests fail**

Run: `cargo test -p decant-core schema::tests -- --nocapture`
Expected: FAIL — `v3_adds_project_worktree_columns_idempotently` (max is 2, columns missing) and the backfill test (no `root_path` column / no resolve).

- [ ] **Step 4: Implement the V3 migration**

In `crates/decant-core/src/schema.rs`:

1. Line 7: `pub const LATEST_VERSION: i64 = 3;`

2. In `migrate`, after the `if current < 2 { ... }` block (line 55), add:

```rust
    if current < 3 {
        apply_v3(conn)?;
    }
```

3. Add these functions after `apply` (after line 70):

```rust
/// v3: add worktree roll-up columns to `project` (ALTER lacks IF NOT EXISTS, so
/// each is PRAGMA-guarded — harmless on a fresh DB where `schema_v1.sql` already
/// created them), then backfill the resolution for existing rows. The backfill
/// runs after the column-add commits because it opens its own statements and
/// reads the filesystem.
fn apply_v3(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    add_column_if_missing(&tx, "is_worktree", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&tx, "root_path", "TEXT")?;
    add_column_if_missing(&tx, "worktree_label", "TEXT")?;
    add_column_if_missing(&tx, "worktree_tool", "TEXT")?;
    add_column_if_missing(&tx, "root_source", "TEXT")?;
    tx.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'))",
        [],
    )?;
    tx.commit()?;
    crate::worktree::resolve_worktree_roots(conn)?;
    Ok(())
}

/// Add a column to `project` only if it does not already exist.
fn add_column_if_missing(conn: &Connection, col: &str, decl: &str) -> Result<()> {
    let exists = conn
        .prepare("PRAGMA table_info(project)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|x| x.ok())
        .any(|name| name == col);
    if !exists {
        conn.execute(&format!("ALTER TABLE project ADD COLUMN {col} {decl}"), [])?;
    }
    Ok(())
}
```

> Note: this references `crate::worktree::resolve_worktree_roots`, implemented in Task 4. Implement Task 4 before running the backfill test in Step 5.

- [ ] **Step 5: After Task 4 exists, run to verify they pass**

Run: `cargo test -p decant-core schema::tests -- --nocapture`
Expected: PASS (columns added, idempotent, backfill links the warp worktree).

- [ ] **Step 6: Commit (after Task 4)**

```bash
git add crates/decant-core/src/schema_v1.sql crates/decant-core/src/schema.rs
git commit -m "feat(core): v3 migration adds project worktree columns + backfill"
```

---

## Task 4: Core — `resolve_worktree_roots` orchestrator

**Files:**
- Modify: `crates/decant-core/src/worktree.rs`
- Test: same file's `tests` module

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/decant-core/src/worktree.rs`:

```rust
    use crate::{db, schema};

    #[test]
    fn resolve_links_intree_and_external_worktrees_to_roots() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        // Synthetic, guaranteed-nonexistent paths so git resolution is skipped
        // and the string heuristics drive the result.
        conn.execute_batch(
            "INSERT INTO project(path, name) VALUES ('/home/x/dosu/dosu', 'dosu');
             INSERT INTO project(path, name) VALUES
               ('/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39', 'teedole-ops-39');
             INSERT INTO project(path, name) VALUES
               ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');",
        )
        .unwrap();

        resolve_worktree_roots(&conn).unwrap();

        let row = |path: &str| -> (i64, String, Option<String>, Option<String>) {
            conn.query_row(
                "SELECT is_worktree, root_path, worktree_tool, root_source FROM project WHERE path = ?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
        };

        assert_eq!(
            row("/home/x/dosu/dosu"),
            (0, "/home/x/dosu/dosu".into(), None, Some("self".into()))
        );
        let (iw, rp, tool, src) = row("/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39");
        assert_eq!((iw, rp.as_str(), tool.as_deref(), src.as_deref()),
                   (1, "/home/x/dosu/dosu", Some("claude"), Some("intree")));
        let (iw2, rp2, tool2, src2) = row("/home/x/.warp-worktrees/dosu-agate-spire");
        assert_eq!((iw2, rp2.as_str(), tool2.as_deref(), src2.as_deref()),
                   (1, "/home/x/dosu/dosu", Some("warp"), Some("namematch")));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p decant-core worktree::tests::resolve_links -- --nocapture`
Expected: FAIL — `cannot find function resolve_worktree_roots in this scope`.

- [ ] **Step 3: Implement the orchestrator**

Add to `crates/decant-core/src/worktree.rs` (after `infer_tool`). Add `use crate::Result;` and `use rusqlite::{params, Connection};` to the top of the file:

```rust
/// Resolve and persist root/worktree identity for every project. Idempotent;
/// operates on the `project` table plus cheap per-path filesystem stats. Rows
/// already locked at `root_source = 'git'` are left untouched (the worktree may
/// since have been deleted; we trust the earlier authoritative read).
pub fn resolve_worktree_roots(conn: &Connection) -> Result<()> {
    struct Proj {
        id: i64,
        path: String,
        is_worktree: i64,
        root_path: Option<String>,
        source: Option<String>,
        sessions: i64,
        last_seen: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT p.id, p.path, p.is_worktree, p.root_path, p.root_source,
                COUNT(s.id), MAX(s.started_at)
         FROM project p LEFT JOIN session s ON s.project_id = p.id
         GROUP BY p.id",
    )?;
    let projs: Vec<Proj> = stmt
        .query_map([], |r| {
            Ok(Proj {
                id: r.get(0)?,
                path: r.get(1)?,
                is_worktree: r.get(2)?,
                root_path: r.get(3)?,
                source: r.get(4)?,
                sessions: r.get(5)?,
                last_seen: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Pass A: git / intree / self; defer externals. Accumulate known roots from
    // every non-deferred project's resolved root_path (so a root discovered only
    // via an in-tree/git worktree is still a name-match target).
    let mut writes: Vec<(i64, Resolution)> = Vec::new();
    let mut deferred: Vec<(i64, String, &'static str, String, i64, Option<String>)> = Vec::new();
    let mut roots: std::collections::HashMap<String, (i64, Option<String>)> =
        std::collections::HashMap::new();
    let mut note_root = |rp: &str, sessions: i64, last: &Option<String>| {
        let e = roots.entry(rp.to_string()).or_insert((0, None));
        e.0 += sessions;
        if last > &e.1 {
            e.1 = last.clone();
        }
    };

    for p in &projs {
        // Locked authoritative result: contribute its root, never rewrite.
        if p.source.as_deref() == Some("git") {
            let rp = p.root_path.clone().unwrap_or_else(|| p.path.clone());
            note_root(&rp, p.sessions, &p.last_seen);
            continue;
        }
        if let Some(res) = resolve_git_root(Path::new(&p.path)) {
            note_root(&res.root_path, p.sessions, &p.last_seen);
            writes.push((p.id, res));
            continue;
        }
        if let Some(res) = classify_intree(&p.path) {
            note_root(&res.root_path, p.sessions, &p.last_seen);
            writes.push((p.id, res));
            continue;
        }
        if let Some((tool, leaf)) = external_container(&p.path) {
            deferred.push((p.id, p.path.clone(), tool, leaf, p.sessions, p.last_seen.clone()));
            continue;
        }
        // Self root.
        note_root(&p.path, p.sessions, &p.last_seen);
        writes.push((
            p.id,
            Resolution {
                is_worktree: false,
                root_path: p.path.clone(),
                worktree_label: None,
                worktree_tool: None,
                source: RootSource::SelfRoot,
            },
        ));
    }

    let known: Vec<KnownRoot> = roots
        .into_iter()
        .map(|(path, (sessions, last_seen))| KnownRoot {
            basename: basename(&path),
            path,
            sessions,
            last_seen,
        })
        .collect();

    // Pass B: external worktrees, now that roots are known.
    for (id, path, tool, leaf, _, _) in &deferred {
        writes.push((*id, classify_external(tool, leaf, path, &known)));
    }

    // Write back. Skip no-op writes to keep this cheap on steady-state DBs.
    let tx = conn.unchecked_transaction()?;
    for (id, res) in &writes {
        let p = projs.iter().find(|p| p.id == *id).unwrap();
        let unchanged = p.is_worktree == res.is_worktree as i64
            && p.root_path.as_deref() == Some(res.root_path.as_str())
            && p.source.as_deref() == Some(res.source.as_str());
        if unchanged {
            continue;
        }
        tx.execute(
            "UPDATE project
                SET is_worktree = ?2, root_path = ?3, worktree_label = ?4,
                    worktree_tool = ?5, root_source = ?6
              WHERE id = ?1",
            params![
                id,
                res.is_worktree as i64,
                res.root_path,
                res.worktree_label,
                res.worktree_tool,
                res.source.as_str(),
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p decant-core worktree::tests -- --nocapture`
Expected: PASS (orchestrator links intree → real root, warp → name-matched root, root stays self).

- [ ] **Step 5: Run Task 3's deferred tests + lint**

Run: `cargo test -p decant-core schema::tests -- --nocapture && cargo clippy -p decant-core --all-targets -- -D warnings`
Expected: PASS — Task 3's `v3_backfills_existing_worktree_rows_on_a_v2_db` now passes (the migration's `resolve_worktree_roots` call resolves). No clippy warnings.

- [ ] **Step 6: Commit (both core resolution + Task 3 migration)**

```bash
git add crates/decant-core/src/worktree.rs crates/decant-core/src/schema_v1.sql crates/decant-core/src/schema.rs
git commit -m "feat(core): resolve_worktree_roots orchestrator + wire v3 backfill"
```

---

## Task 5: Core — resolve at the end of every ingest

**Files:**
- Modify: `crates/decant-core/src/ingest.rs:362-364` (end of `sync`)
- Test: `crates/decant-core/src/ingest.rs` `tests` module

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/decant-core/src/ingest.rs`:

```rust
    #[test]
    fn sync_resolves_worktree_roots() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        write(&claude_dir.join("proj/sess.jsonl"), &claude_fixture());

        let config = Config {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir,
        };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();
        let r = sync(&mut conn, &config).unwrap();
        assert_eq!(r.ingested, 1);

        // Every project produced by ingest has a resolved root.
        let unresolved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project WHERE root_source IS NULL OR root_path IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unresolved, 0, "sync must resolve worktree roots for new projects");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p decant-core ingest::tests::sync_resolves_worktree_roots -- --nocapture`
Expected: FAIL — `unresolved` is 1 (the fixture project has NULL `root_source`).

- [ ] **Step 3: Call resolve at the end of `sync`**

In `crates/decant-core/src/ingest.rs`, change the end of `sync` (the `Ok(report)` at line 364) to:

```rust
    // Roll-up identity is data-derived and cheap; refresh it whenever new
    // projects/sessions landed so worktrees link to roots (and synthetic
    // attributions upgrade as real roots appear).
    if report.ingested > 0 {
        crate::worktree::resolve_worktree_roots(conn)?;
    }
    Ok(report)
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p decant-core ingest::tests -- --nocapture`
Expected: PASS (all ingest tests, including the new one and the existing idempotency/dup tests).

- [ ] **Step 5: Full core suite + lint + commit**

```bash
cargo test -p decant-core && cargo clippy -p decant-core --all-targets -- -D warnings
git add crates/decant-core/src/ingest.rs
git commit -m "feat(core): resolve worktree roots after each ingest cycle"
```

---

## Task 6: Daemon — rolled-up `by_dimension` + leaf mode + `DimRow` fields

**Files:**
- Modify: `crates/decant-daemon/src/api/query.rs:599-606` (`DimRow`), `:646-718` (`by_dimension`)
- Modify: `crates/decant-daemon/src/api/analytics.rs:79-81` (call site → pass `None`)
- Test: new `#[cfg(test)] mod tests` at the end of `crates/decant-daemon/src/api/query.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/decant-daemon/src/api/query.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use decant_core::{db, schema};

    fn seed() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO project(id, path, name, is_worktree, root_path, worktree_label, worktree_tool, root_source)
             VALUES
               (1, '/home/x/dosu/dosu', 'dosu', 0, '/home/x/dosu/dosu', NULL, NULL, 'self'),
               (2, '/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire', 1,
                   '/home/x/dosu/dosu', 'agate-spire', 'warp', 'namematch');
             INSERT INTO session(id, tool, source_session_id, project_id, started_at,
                                 total_input_tokens, total_output_tokens, estimated_cost_usd)
             VALUES
               (1, 'claude_code', 's1', 1, '2026-06-01', 10, 5, 1.0),
               (2, 'claude_code', 's2', 2, '2026-06-02', 20, 10, 2.0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn project_dim_rolls_worktrees_under_root() {
        let conn = seed();
        let page =
            by_dimension(&conn, Dimension::Project, &Filters::default(), 50, None, None).unwrap();
        assert_eq!(page.rows.len(), 1, "two paths collapse into one root row");
        let row = &page.rows[0];
        assert_eq!(row.key, "/home/x/dosu/dosu");
        assert_eq!(row.sessions, 2);
        assert!((row.estimated_cost_usd - 3.0).abs() < 1e-9);
        assert_eq!(row.worktree_count, Some(1));
        assert_eq!(row.worktree_label, None);
    }

    #[test]
    fn project_dim_root_param_lists_per_worktree_leaves() {
        let conn = seed();
        let page = by_dimension(
            &conn,
            Dimension::Project,
            &Filters::default(),
            50,
            None,
            Some("/home/x/dosu/dosu"),
        )
        .unwrap();
        assert_eq!(page.rows.len(), 2, "root checkout + one worktree");
        let wt = page
            .rows
            .iter()
            .find(|r| r.key.contains("agate-spire"))
            .unwrap();
        assert_eq!(wt.worktree_label.as_deref(), Some("agate-spire"));
        assert_eq!(wt.worktree_tool.as_deref(), Some("warp"));
        assert_eq!(wt.worktree_count, None);
    }

    #[test]
    fn non_project_dim_has_no_worktree_fields() {
        let conn = seed();
        let page =
            by_dimension(&conn, Dimension::Tool, &Filters::default(), 50, None, None).unwrap();
        assert!(page.rows.iter().all(|r| r.worktree_count.is_none()));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p decant-daemon --lib api::query::tests -- --nocapture`
Expected: FAIL — `by_dimension` takes 5 args (not 6) and `DimRow` has no `worktree_count` field.

- [ ] **Step 3: Extend `DimRow`**

In `crates/decant-daemon/src/api/query.rs`, replace the `DimRow` struct (lines 599-606) with:

```rust
#[derive(Debug, Serialize)]
pub struct DimRow {
    pub key: String,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
    /// Rolled-up project rows only: number of distinct worktrees folded in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_count: Option<i64>,
    /// Per-worktree leaf rows only (project dimension with `root` set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_tool: Option<String>,
}
```

- [ ] **Step 4: Rewrite `by_dimension` to add `root` + the two project modes**

In `crates/decant-daemon/src/api/query.rs`, replace the whole `by_dimension` function (lines 646-718) with:

```rust
pub fn by_dimension(
    conn: &Connection,
    dim: Dimension,
    filters: &Filters,
    limit: i64,
    cursor: Option<Cursor>,
    root: Option<&str>,
) -> Result<DimPage, ApiError> {
    // The project dimension has two modes: rolled up by `root_path` (default),
    // or — when `root` is set — the per-worktree leaf breakdown scoped to it.
    let project_leaf = matches!(dim, Dimension::Project) && root.is_some();

    // (group expr, join, three trailing select columns, optional leading predicate)
    let (expr, join, extra_cols, root_pred): (&str, &str, &str, &str) = match dim {
        Dimension::Tool => ("s.tool", "", "NULL, NULL, NULL", ""),
        Dimension::Model => ("COALESCE(s.model, '(unknown)')", "", "NULL, NULL, NULL", ""),
        Dimension::Project if project_leaf => (
            "COALESCE(p.path, '(none)')",
            "JOIN project p ON p.id = s.project_id",
            "NULL, MAX(p.worktree_label), MAX(p.worktree_tool)",
            "p.root_path = ? AND ",
        ),
        Dimension::Project => (
            "COALESCE(p.root_path, p.path, '(none)')",
            "LEFT JOIN project p ON p.id = s.project_id",
            "COUNT(DISTINCT CASE WHEN p.is_worktree = 1 THEN p.id END), NULL, NULL",
            "",
        ),
        Dimension::Day => ("substr(s.started_at, 1, 10)", "", "NULL, NULL, NULL", ""),
    };

    let where_c = filters.where_clause();
    let offset = cursor.as_ref().map(|c| c.rowid.max(0)).unwrap_or(0);

    let sql = format!(
        "SELECT {expr} AS k, COUNT(*) AS sessions, \
                COALESCE(SUM(s.total_input_tokens),0), \
                COALESCE(SUM(s.total_output_tokens),0), \
                COALESCE(SUM(s.estimated_cost_usd),0.0), \
                {extra_cols} \
         FROM session s {join} WHERE {root_pred}{} \
         GROUP BY k ORDER BY sessions DESC, k ASC LIMIT ? OFFSET ?",
        where_c.sql
    );

    let mut params: Vec<SqlValue> = Vec::new();
    if project_leaf {
        params.push(SqlValue::Text(root.unwrap().to_string()));
    }
    params.extend(where_c.params.clone());
    params.push(SqlValue::Integer(limit + 1));
    params.push(SqlValue::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<DimRow> = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| {
            Ok(DimRow {
                key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                sessions: r.get(1)?,
                input_tokens: r.get(2)?,
                output_tokens: r.get(3)?,
                estimated_cost_usd: r.get(4)?,
                worktree_count: r.get::<_, Option<i64>>(5)?,
                worktree_label: r.get::<_, Option<String>>(6)?,
                worktree_tool: r.get::<_, Option<String>>(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        Some(Cursor::new(offset + limit, None).encode())
    } else {
        None
    };

    // Distinct-group total for the dimension under the same scope.
    let total_sql = format!(
        "SELECT COUNT(*) FROM (SELECT {expr} AS k FROM session s {join} WHERE {root_pred}{} GROUP BY k)",
        where_c.sql
    );
    let mut total_params: Vec<SqlValue> = Vec::new();
    if project_leaf {
        total_params.push(SqlValue::Text(root.unwrap().to_string()));
    }
    total_params.extend(where_c.params.clone());
    let total_count: i64 = conn.query_row(
        &total_sql,
        rusqlite::params_from_iter(total_params.iter()),
        |r| r.get(0),
    )?;

    Ok(DimPage {
        rows,
        next_cursor,
        has_more,
        total_count,
    })
}
```

- [ ] **Step 5: Keep the call site compiling (pass `None` for now)**

In `crates/decant-daemon/src/api/analytics.rs`, change the call inside `by_dimension` (lines 79-81) to:

```rust
    let page = with_read_conn(&state.read_pool, move |conn| {
        query::by_dimension(conn, dim, &filters, limit, cursor, None)
    })
    .await?;
```

- [ ] **Step 6: Run to verify the tests pass**

Run: `cargo test -p decant-daemon --lib api::query::tests -- --nocapture`
Expected: PASS (rolled-up grouping, leaf breakdown, no worktree fields on tool dim).

- [ ] **Step 7: Lint + commit**

```bash
cargo fmt --all && cargo clippy -p decant-daemon --all-targets -- -D warnings
git add crates/decant-daemon/src/api/query.rs crates/decant-daemon/src/api/analytics.rs
git commit -m "feat(daemon): roll By Project up by root_path + per-worktree leaf mode"
```

---

## Task 7: Daemon — expose the `root` query param over HTTP

**Files:**
- Modify: `crates/decant-daemon/src/api/analytics.rs:45-93` (`ByDimensionParams` + handler)
- Test: `crates/decant-daemon/tests/api_test.rs`

- [ ] **Step 1: Write the failing integration test**

Add to `crates/decant-daemon/tests/api_test.rs` (uses the existing `spawn()` + `get_ok()` helpers):

```rust
#[tokio::test]
async fn analytics_by_dimension_project_rollup_exposes_worktree_count_and_root_param() {
    let base = spawn().await;

    // Rolled-up project dimension: rows carry worktree_count (0 for the
    // worktree-free fixtures) — proves the DimRow change reaches HTTP.
    let body = get_ok(&base, "/api/v1/analytics/by-dimension?dim=project").await;
    let rows = body["data"].as_array().unwrap();
    assert!(!rows.is_empty(), "project rollup has rows");
    assert!(
        rows[0].get("worktree_count").is_some(),
        "rolled project rows expose worktree_count"
    );

    // The leaf breakdown for that root returns 200 with at least the root itself.
    let key = rows[0]["key"].as_str().unwrap();
    let enc = key.replace('/', "%2F");
    let leaf = get_ok(
        &base,
        &format!("/api/v1/analytics/by-dimension?dim=project&root={enc}"),
    )
    .await;
    assert!(!leaf["data"].as_array().unwrap().is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p decant-daemon --test api_test analytics_by_dimension_project_rollup -- --nocapture`
Expected: FAIL — `worktree_count` absent (handler still passes `None`; `root` param ignored).

- [ ] **Step 3: Add `root` to params + plumb it through**

In `crates/decant-daemon/src/api/analytics.rs`:

1. Add a field to `ByDimensionParams` (after `pub project: Option<String>,`, line 53):

```rust
    pub root: Option<String>,
```

2. Replace the handler body's filter/call section (lines 64-82) with:

```rust
    let dim = query::parse_dimension(params.dim.as_deref())?;
    let root = params.root.clone();
    let filters = Filters::parse(
        params.from,
        params.to,
        params.tool,
        params.model,
        params.project,
    )?;
    let limit = query::clamp_limit(params.limit);
    let cursor = match params.cursor {
        Some(c) => Some(Cursor::decode(&c)?),
        None => None,
    };
    let filters_json = filters.as_json();

    let page = with_read_conn(&state.read_pool, move |conn| {
        query::by_dimension(conn, dim, &filters, limit, cursor, root.as_deref())
    })
    .await?;
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p decant-daemon --test api_test -- --nocapture`
Expected: PASS (new test + existing `analytics_by_dimension_ranked` and friends).

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt --all && cargo clippy -p decant-daemon --all-targets -- -D warnings
git add crates/decant-daemon/src/api/analytics.rs crates/decant-daemon/tests/api_test.rs
git commit -m "feat(daemon): accept ?root= on by-dimension for per-worktree breakdown"
```

---

## Task 8: Daemon — widen the project drill-down filter

**Files:**
- Modify: `crates/decant-daemon/src/api/filters.rs:76-79` (project clause), `:169-170` (test)
- Test: `crates/decant-daemon/src/api/filters.rs` `tests` module

- [ ] **Step 1: Update the failing test expectations**

In `crates/decant-daemon/src/api/filters.rs`, in `full_filters_build_clause_and_params`:
- Line 169: change `assert!(w.sql.contains("project WHERE path = ?"));` to:

```rust
        assert!(w.sql.contains("project WHERE root_path = ? OR path = ?"));
```

- Line 170: change `assert_eq!(w.params.len(), 5);` to `assert_eq!(w.params.len(), 6);`

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p decant-daemon --lib api::filters::tests::full_filters_build_clause_and_params -- --nocapture`
Expected: FAIL — current clause is `project WHERE path = ?` with 5 params.

- [ ] **Step 3: Widen the clause**

In `crates/decant-daemon/src/api/filters.rs`, replace the project block in `where_clause` (lines 76-79) with:

```rust
        if let Some(project) = &self.project {
            // A "project" value is a resolved root_path: match the root row and
            // every worktree that rolls up under it (plus a path fallback for any
            // not-yet-resolved row).
            clauses.push("s.project_id IN (SELECT id FROM project WHERE root_path = ? OR path = ?)");
            params.push(SqlValue::Text(project.clone()));
            params.push(SqlValue::Text(project.clone()));
        }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p decant-daemon --lib api::filters -- --nocapture`
Expected: PASS (all filter tests).

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt --all && cargo clippy -p decant-daemon --all-targets -- -D warnings
git add crates/decant-daemon/src/api/filters.rs
git commit -m "feat(daemon): project filter includes worktrees of the root"
```

---

## Task 9: Contract — update OpenAPI

**Files:**
- Modify: `docs/api/openapi.yaml:233-234` (param ref), `:578` (new Root param), `:810-820` (DimensionRow)

- [ ] **Step 1: Add the `Root` parameter reference to by-dimension**

In `docs/api/openapi.yaml`, in `/analytics/by-dimension` `parameters` (after line 233, `- $ref: "#/components/parameters/Project"`), add:

```yaml
        - $ref: "#/components/parameters/Root"
```

- [ ] **Step 2: Define the `Root` parameter**

In `docs/api/openapi.yaml`, in `components.parameters` after the `Project` parameter (after line 578), add:

```yaml
    Root:
      name: root
      in: query
      description: >
        Project dimension only. When set, scope the rollup to this resolved root
        project path and return one row per worktree (the root checkout plus each
        worktree) instead of the rolled-up root row.
      schema: { type: string }
```

- [ ] **Step 3: Extend `DimensionRow` with the worktree fields**

In `docs/api/openapi.yaml`, replace the `DimensionRow` schema (lines 810-820) with:

```yaml
    DimensionRow:
      type: object
      properties:
        key:
          type: string
          description: The dimension value (tool, model, day, or — for project — the resolved root path / per-worktree leaf path).
        sessions: { type: integer }
        input_tokens: { type: integer }
        output_tokens: { type: integer }
        estimated_cost_usd: { type: number }
        worktree_count:
          type: integer
          description: Project dimension rolled-up rows only — distinct worktrees folded into this root. Omitted for other dimensions.
        worktree_label:
          type: string
          description: Project dimension leaf rows only (with `root` set) — the worktree's label. Omitted otherwise.
        worktree_tool:
          type: string
          description: Project dimension leaf rows only — the worktree tool (warp|t3|conductor|claude|git). Omitted otherwise.
      required: [key, sessions, input_tokens, output_tokens, estimated_cost_usd]
```

- [ ] **Step 4: Verify the bundled spec still loads**

Run: `cargo test -p decant-daemon --lib openapi -- --nocapture`
Expected: PASS — the `openapi.rs` smoke test (`OPENAPI_YAML.contains("openapi: 3.1.0")`) stays green; the `include_str!` compiles the edited file in.

- [ ] **Step 5: Commit**

```bash
git add docs/api/openapi.yaml
git commit -m "docs(api): document ?root= param and DimensionRow worktree fields"
```

---

## Task 10: Web — map worktree fields + pass `root`

**Files:**
- Modify: `web/lib/decant/archive.ex:135-151` (`by_dimension` mapping), `:250-259` (`to_params`)
- Test: covered by Task 11's LiveView test (the mapping is exercised end-to-end there)

- [ ] **Step 1: Map the new fields in `by_dimension`**

In `web/lib/decant/archive.ex`, replace the `Enum.map` body in `by_dimension` (lines 138-146) with:

```elixir
        Enum.map(rows, fn r ->
          %{
            key: r["key"] || "",
            sessions: r["sessions"] || 0,
            input_tokens: r["input_tokens"] || 0,
            output_tokens: r["output_tokens"] || 0,
            cost: r["estimated_cost_usd"] || 0.0,
            worktree_count: r["worktree_count"],
            worktree_label: r["worktree_label"],
            worktree_tool: r["worktree_tool"]
          }
        end)
```

- [ ] **Step 2: Pass `root` through `to_params`**

In `web/lib/decant/archive.ex`, replace `to_params` (lines 250-259) with:

```elixir
  defp to_params(filters) do
    [
      from: iso_date(Map.get(filters, :from)),
      to: iso_date(Map.get(filters, :to)),
      tool: present(Map.get(filters, :tool)),
      model: present(Map.get(filters, :model)),
      project: present(Map.get(filters, :project)),
      root: present(Map.get(filters, :root))
    ]
    |> Enum.reject(fn {_k, v} -> is_nil(v) end)
  end
```

> `Decant.Daemon.by_dimension/2` already forwards `opts` as query params (`[{:dim, dim} | opts]`), so `root` reaches the daemon with no change there.

- [ ] **Step 3: Compile clean**

Run: `cd web && mix compile --warnings-as-errors`
Expected: compiles with no warnings.

- [ ] **Step 4: Commit**

```bash
git add web/lib/decant/archive.ex
git commit -m "feat(web): map worktree fields and forward the root param"
```

---

## Task 11: Web — rolled-up render + `▸ N wt` marker + inline expand

**Files:**
- Modify: `web/lib/decant_web/live/analytics_live.ex` — `mount` (add assigns), add `handle_event("toggle_project", ...)`, the "By project" `tbody` render, and a `basename/1` helper.
- Test: `web/test/decant_web/live/analytics_live_test.exs`

- [ ] **Step 1: Write the failing test**

Add to `web/test/decant_web/live/analytics_live_test.exs` (inside the module):

```elixir
  test "rolls projects up by root and expands to per-worktree rows", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :by_dimension, fn
      :project, opts ->
        if Keyword.has_key?(opts, :root) do
          {:ok,
           [
             %{"key" => "/home/x/dosu/dosu", "sessions" => 3, "input_tokens" => 0,
               "output_tokens" => 0, "estimated_cost_usd" => 1.0,
               "worktree_label" => nil, "worktree_tool" => nil},
             %{"key" => "/home/x/.warp-worktrees/dosu-agate-spire", "sessions" => 2,
               "input_tokens" => 0, "output_tokens" => 0, "estimated_cost_usd" => 2.0,
               "worktree_label" => "agate-spire", "worktree_tool" => "warp"}
           ], %{}}
        else
          {:ok,
           [
             %{"key" => "/home/x/dosu/dosu", "sessions" => 5, "input_tokens" => 0,
               "output_tokens" => 0, "estimated_cost_usd" => 3.0, "worktree_count" => 1}
           ], %{}}
        end

      _dim, _opts ->
        {:ok, [], %{}}
    end)

    {:ok, view, html} = live(conn, ~p"/analytics")

    assert html =~ "By project"
    assert html =~ "dosu"
    assert html =~ "1 wt"
    refute html =~ "agate-spire"

    html = render_click(view, "toggle_project", %{"key" => "/home/x/dosu/dosu"})
    assert html =~ "agate-spire"
    assert html =~ "warp"
  end
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && mix test test/decant_web/live/analytics_live_test.exs -o "rolls projects up"`
Expected: FAIL — no `1 wt` marker, and `toggle_project` is an unhandled event.

- [ ] **Step 3: Add expand state to `mount`**

In `web/lib/decant_web/live/analytics_live.ex`, replace the `assign(...)` in `mount` (lines 11-16) with:

```elixir
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Analytics",
       model_sort: {:cost, :desc},
       project_sort: {:cost, :desc},
       expanded: MapSet.new(),
       worktree_rows: %{}
     )}
```

- [ ] **Step 4: Add the `toggle_project` handler**

In `web/lib/decant_web/live/analytics_live.ex`, add after the existing `handle_event("sort", %{"table" => "project", ...})` clause (after line 106):

```elixir
  def handle_event("toggle_project", %{"key" => key}, socket) do
    if MapSet.member?(socket.assigns.expanded, key) do
      {:noreply,
       assign(socket,
         expanded: MapSet.delete(socket.assigns.expanded, key),
         worktree_rows: Map.delete(socket.assigns.worktree_rows, key)
       )}
    else
      rows = Archive.by_dimension(:project, Map.put(socket.assigns.filters, :root, key))

      {:noreply,
       assign(socket,
         expanded: MapSet.put(socket.assigns.expanded, key),
         worktree_rows: Map.put(socket.assigns.worktree_rows, key, rows)
       )}
    end
  end
```

- [ ] **Step 5: Add the `basename/1` helper**

In `web/lib/decant_web/live/analytics_live.ex`, add next to the other private helpers (after `defp weekday_label/1`, line 88):

```elixir
  # Display name for a project key: the last path segment, or the key itself for
  # synthetic (path-less) root keys.
  defp basename(key) do
    key |> to_string() |> String.trim_trailing("/") |> String.split("/") |> List.last()
  end
```

- [ ] **Step 6: Render rolled-up rows with the marker + sub-rows**

In `web/lib/decant_web/live/analytics_live.ex`, replace the "By project" `<tbody>` (lines 305-316) with:

```heex
              <tbody>
                <%= for r <- @by_project do %>
                  <tr
                    phx-click={JS.navigate(Filters.url(~p"/", Map.put(@filters, :project, r.key)))}
                    class="cursor-pointer border-b border-line/60 transition-colors hover:bg-elevated"
                  >
                    <td class="max-w-xl truncate px-4 py-2.5 font-mono text-xs text-fg" title={r.key}>
                      {basename(r.key)}
                      <button
                        :if={(r.worktree_count || 0) > 0}
                        type="button"
                        phx-click={JS.push("toggle_project", value: %{key: r.key})}
                        class="ml-2 rounded px-1 text-[10px] text-muted hover:text-fg"
                      >
                        ▸ {r.worktree_count} wt
                      </button>
                    </td>
                    <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.sessions)}</td>
                    <td class="px-4 py-2.5 text-right tabular-nums">{money(r.cost)}</td>
                  </tr>
                  <tr
                    :for={w <- Map.get(@worktree_rows, r.key, [])}
                    class="border-b border-line/40 bg-elevated/40"
                  >
                    <td class="truncate px-4 py-2 pl-10 font-mono text-xs text-muted">
                      wt: {w.worktree_label || basename(w.key)}<span :if={w.worktree_tool}>&nbsp;({w.worktree_tool})</span>
                    </td>
                    <td class="px-4 py-2 text-right tabular-nums text-muted">{int(w.sessions)}</td>
                    <td class="px-4 py-2 text-right tabular-nums text-muted">{money(w.cost)}</td>
                  </tr>
                <% end %>
              </tbody>
```

> As-built deviation (c9a3f13): the template's original `onclick="event.stopPropagation()"` line was removed — it prevents LiveView's delegated phx-click from ever firing; nested phx-click needs no propagation guard.

- [ ] **Step 7: Run to verify it passes**

Run: `cd web && mix test test/decant_web/live/analytics_live_test.exs`
Expected: PASS (the new roll-up/expand test and the existing totals/by-model tests).

- [ ] **Step 8: Format + compile clean + commit**

```bash
cd web && mix format && mix compile --warnings-as-errors
cd .. && git add web/lib/decant_web/live/analytics_live.ex web/test/decant_web/live/analytics_live_test.exs
git commit -m "feat(web): roll By Project up by root with an inline worktree expand"
```

---

## Final verification

- [ ] **Rust:** `cargo test --workspace && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings` — all green.
- [ ] **Web:** `cd web && mix test && mix format --check-formatted && mix compile --warnings-as-errors` — all green.
- [ ] **Manual smoke (optional):** delete `~/.decant/decant.db`, start the daemon (`cargo run -p decant-cli -- daemon serve`), let it re-ingest (recomputes roll-ups via the V3 migration backfill + ingest resolve), open `http://localhost:4000/analytics`, and confirm worktree-heavy repos (e.g. `dosu`) now show a single rolled-up row with a `▸ N wt` marker that expands.

---

## Self-Review

**Spec coverage:**
- §3 data model (5 columns) → Task 3 (schema_v1.sql + migration).
- §4.1 string classifiers → Task 1; §4.2 git resolution → Task 2; §4.3 orchestrator → Task 4.
- §5 ingest + migration flow → Task 3 (migration backfill) + Task 5 (sync hook).
- §6 daemon grouping + `worktree_count` + leaf `root` mode + `DimRow` fields → Task 6; `root` over HTTP → Task 7; drill-down filter → Task 8; openapi → Task 9.
- §7 web render + marker + expand → Tasks 10-11.
- §8 testing → tests embedded in every task.
- §9 edge cases: `cwd = NULL` (no project row) unaffected; non-git dir → self (Task 4 `SelfRoot`); same-basename tie-break (Task 1 `external_namematch_tiebreaks_on_sessions`); synthetic keys (Task 1 `external_synthetic_strips_codename_per_tool`).

**Type consistency:** `Resolution`/`RootSource`/`KnownRoot` defined in Task 1 and used unchanged in Tasks 2/4. `resolve_worktree_roots(&Connection)` defined in Task 4, called in Task 3 (migration) and Task 5 (sync). `by_dimension(conn, dim, &filters, limit, cursor, root)` — 6-arg signature defined in Task 6 and called with `None` (Task 6 call-site) then `root.as_deref()` (Task 7). `DimRow` worktree fields added in Task 6, serialized per Task 9's `DimensionRow`, mapped in Task 10, rendered in Task 11. Web assigns `expanded`/`worktree_rows` added in Task 11 `mount` and used in the same task's handler + render.

**Placeholder scan:** none — every code step contains complete code; every run step has an exact command and expected result.
