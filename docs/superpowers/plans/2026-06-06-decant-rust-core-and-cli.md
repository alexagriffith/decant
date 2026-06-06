# decant — Rust Core + Foundational CLI (Plan 1 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `decant`, a Rust CLI that ingests Claude Code and Codex session logs into a normalized SQLite archive and lets you sync, list, read, and full-text-search sessions.

**Architecture:** A Cargo workspace with `decant-core` (all logic: DB, schema, parsers, normalization, tool/MCP classification, cost, idempotent parallel ingest, read queries — returning serde DTOs) and a thin `decant-cli` binary over it. SQLite (bundled, WAL, FTS5) is the single source of truth; the core API is UI-agnostic so the future Phoenix app and macOS app reuse it.

**Tech Stack:** Rust (edition 2021), `rusqlite` (bundled SQLite + FTS5), `serde`/`serde_json`, `rayon`, `walkdir`, `blake3`, `thiserror`, `directories`; CLI: `clap` (derive), `comfy-table`, `owo-colors`, `anyhow`; tests: `assert_cmd`, `predicates`, `tempfile`.

**This is Plan 1 of 3.** Plan 2 adds the full CLI DevX surface (`stats`, `mcp`, `tool`, `export`, `project`, `db`, `init`, completions, pagination, `--dry-run` everywhere, conformance test). Plan 3 is the Phoenix web app. Spec: `docs/superpowers/specs/2026-06-06-decant-design.md`.

---

## File Structure

```
Cargo.toml                              # workspace
crates/decant-core/
  Cargo.toml
  src/
    lib.rs            # module wiring + re-exports (the public API surface)
    error.rs          # Error enum + Result alias
    config.rs         # Config: db/claude/codex paths (env + platform defaults)
    db.rs             # open() / open_in_memory() with WAL pragmas
    schema.rs         # SCHEMA_V1 DDL + migrate()
    model.rs          # Tool/Role/BlockType/ToolKind enums + Normalized* structs
    tools.rs          # classify_tool() + tool-result extraction helpers
    cost.rs           # Pricing table + estimate_cost()
    sources/
      mod.rs
      claude.rs       # parse_session() for Claude Code JSONL
      codex.rs        # parse_session() for Codex rollout JSONL
    ingest.rs         # discover() + sync() + upsert_session()
    query.rs          # SessionSummary/SearchHit/SessionDetail + list/search/get
crates/decant-cli/
  Cargo.toml
  src/
    main.rs           # clap Cli, global flags, dispatch, exit codes
    output.rs         # OutputCtx (format/color/quiet), JSON + table helpers
    commands/
      mod.rs
      sync.rs
      session.rs      # ls + show
      search.rs
fixtures/
  claude/sample.jsonl
  codex/sample.jsonl
justfile
.gitignore
```

Each task is self-contained: write the failing test, see it fail, implement the minimum, see it pass, commit. Run `cargo test` from the repo root (workspace-wide) unless a more specific command is given.

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`, `crates/decant-core/Cargo.toml`, `crates/decant-core/src/lib.rs`, `crates/decant-cli/Cargo.toml`, `crates/decant-cli/src/main.rs`, `.gitignore`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/decant-core", "crates/decant-cli"]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT"
```

- [ ] **Step 2: Create `.gitignore`**

```gitignore
/target
**/*.rs.bk
*.db
*.db-shm
*.db-wal
```

- [ ] **Step 3: Scaffold `decant-core`**

Create `crates/decant-core/Cargo.toml`:

```toml
[package]
name = "decant-core"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]

[dev-dependencies]
tempfile = "3"
```

Create `crates/decant-core/src/lib.rs`:

```rust
//! decant-core: parse, normalize, store, and query AI coding-agent sessions.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonempty() {
        assert!(!version().is_empty());
    }
}
```

- [ ] **Step 4: Scaffold `decant-cli`**

Create `crates/decant-cli/Cargo.toml`:

```toml
[package]
name = "decant-cli"
edition.workspace = true
version.workspace = true
license.workspace = true

[[bin]]
name = "decant"
path = "src/main.rs"

[dependencies]
decant-core = { path = "../decant-core" }
```

Create `crates/decant-cli/src/main.rs`:

```rust
fn main() {
    println!("decant {}", decant_core::version());
}
```

- [ ] **Step 5: Verify it builds and the test passes**

Run: `cargo test`
Expected: compiles; `version_is_nonempty` passes.

Run: `cargo run -p decant-cli`
Expected: prints `decant 0.1.0`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore crates/
git commit -m "feat: scaffold decant cargo workspace (core + cli)"
```

---

### Task 2: Error type + database connection with WAL pragmas

**Files:**
- Create: `crates/decant-core/src/error.rs`, `crates/decant-core/src/db.rs`
- Modify: `crates/decant-core/src/lib.rs`, `crates/decant-core/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Run:
```bash
cargo add --package decant-core rusqlite --features bundled
cargo add --package decant-core thiserror
```
(`bundled` compiles SQLite into the binary with FTS5/JSON1 enabled — no system SQLite needed.)

- [ ] **Step 2: Write the failing test**

Append to `crates/decant-core/src/db.rs` (create the file with this content):

```rust
use crate::Result;
use rusqlite::Connection;
use std::path::Path;

/// Open (creating if needed) a decant database at `path` with WAL + sane pragmas.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// In-memory database (tests). Note: WAL is not used for `:memory:`.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_db_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let conn = open(&path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn foreign_keys_enabled() {
        let conn = open_in_memory().unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
```

Create `crates/decant-core/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Note: `WAL` is set inside the test via `PRAGMA journal_mode = WAL` (which returns the mode). `configure()` sets it for real connections too — add it there in Step 4.

- [ ] **Step 3: Wire modules + run test to verify it fails**

Replace `crates/decant-core/src/lib.rs` body with:

```rust
//! decant-core: parse, normalize, store, and query AI coding-agent sessions.

pub mod db;
pub mod error;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
```

You'll need `serde_json` for `error.rs` to compile. Run:
```bash
cargo add --package decant-core serde_json
```

Run: `cargo test -p decant-core db::`
Expected: FAIL — `file_db_uses_wal` fails because `configure()` does not yet set WAL (journal_mode defaults to `delete`/`memory`).

- [ ] **Step 4: Make WAL persistent in `configure()`**

Edit `configure()` in `crates/decant-core/src/db.rs` to set WAL:

```rust
fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;",
    )?;
    // journal_mode returns a row, so use query_row (file DBs become WAL).
    let _: String =
        conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    Ok(())
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p decant-core db::`
Expected: PASS (both tests).

- [ ] **Step 6: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): error type + WAL sqlite connection helpers"
```

---

### Task 3: Schema + migrations

**Files:**
- Create: `crates/decant-core/src/schema.rs`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/decant-core/src/schema.rs`:

```rust
use crate::Result;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = include_str!("schema_v1.sql");

/// Apply pending migrations. Idempotent.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations(
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    if current < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'))",
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    #[test]
    fn migrate_creates_core_tables_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // second run must not error or duplicate

        for t in [
            "project", "session", "message", "block", "tool_call",
            "block_fts", "ingest_source", "ingest_issue", "model_pricing",
        ] {
            assert!(table_exists(&conn, t), "missing table {t}");
        }

        let versions: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 1, "migration recorded exactly once");
    }

    #[test]
    fn fts_search_round_trips_through_triggers() {
        let conn = db::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO project(id, path) VALUES (1, '/p');
             INSERT INTO session(id, tool, source_session_id, project_id)
                VALUES (1, 'claude_code', 's1', 1);
             INSERT INTO message(id, session_id, seq, role, raw)
                VALUES (1, 1, 0, 'assistant', '{}');
             INSERT INTO block(id, message_id, session_id, ordinal, type, text)
                VALUES (1, 1, 1, 0, 'text', 'the quick brown fox');",
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH 'brown'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }
}
```

- [ ] **Step 2: Create the DDL file**

Create `crates/decant-core/src/schema_v1.sql`:

```sql
CREATE TABLE IF NOT EXISTS project (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  name TEXT,
  first_seen_at TEXT,
  last_seen_at TEXT
);

CREATE TABLE IF NOT EXISTS session (
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,
  source_session_id TEXT NOT NULL,
  project_id INTEGER REFERENCES project(id),
  title TEXT,
  cwd TEXT,
  git_branch TEXT,
  model TEXT,
  cli_version TEXT,
  started_at TEXT,
  ended_at TEXT,
  message_count INTEGER NOT NULL DEFAULT 0,
  total_input_tokens INTEGER NOT NULL DEFAULT 0,
  total_output_tokens INTEGER NOT NULL DEFAULT 0,
  total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  total_cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
  estimated_cost_usd REAL NOT NULL DEFAULT 0,
  is_archived INTEGER NOT NULL DEFAULT 0,
  source_path TEXT,
  raw_meta TEXT,
  ingested_at TEXT,
  source_mtime INTEGER,
  source_size INTEGER,
  source_hash TEXT,
  UNIQUE(tool, source_session_id)
);

CREATE TABLE IF NOT EXISTS message (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  source_uuid TEXT,
  parent_source_uuid TEXT,
  parent_id INTEGER REFERENCES message(id),
  role TEXT,
  model TEXT,
  stop_reason TEXT,
  timestamp TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_creation_tokens INTEGER,
  raw TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS block (
  id INTEGER PRIMARY KEY,
  message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  type TEXT,
  text TEXT,
  tool_name TEXT,
  tool_use_id TEXT,
  tool_input TEXT,
  tool_result TEXT
);

CREATE TABLE IF NOT EXISTS tool_call (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
  call_block_id INTEGER REFERENCES block(id),
  result_block_id INTEGER REFERENCES block(id),
  tool_kind TEXT,
  tool_name TEXT,
  mcp_server TEXT,
  tool_base_name TEXT,
  tool_use_id TEXT,
  input TEXT,
  is_error INTEGER,
  output_preview TEXT,
  output_bytes INTEGER,
  duration_ms INTEGER,
  ordinal INTEGER,
  timestamp TEXT
);

CREATE TABLE IF NOT EXISTS ingest_source (
  path TEXT PRIMARY KEY,
  tool TEXT,
  size INTEGER,
  mtime INTEGER,
  hash TEXT,
  session_id INTEGER REFERENCES session(id),
  line_count INTEGER,
  status TEXT,
  error TEXT,
  last_ingested_at TEXT
);

CREATE TABLE IF NOT EXISTS ingest_issue (
  id INTEGER PRIMARY KEY,
  source_path TEXT,
  line_no INTEGER,
  error TEXT,
  raw_line TEXT,
  created_at TEXT
);

CREATE TABLE IF NOT EXISTS model_pricing (
  model TEXT PRIMARY KEY,
  input_per_mtok REAL,
  output_per_mtok REAL,
  cache_read_per_mtok REAL,
  cache_write_per_mtok REAL,
  source TEXT,
  updated_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_session_project ON session(project_id);
CREATE INDEX IF NOT EXISTS idx_session_tool ON session(tool);
CREATE INDEX IF NOT EXISTS idx_session_started ON session(started_at);
CREATE INDEX IF NOT EXISTS idx_session_model ON session(model);
CREATE INDEX IF NOT EXISTS idx_message_session ON message(session_id, seq);
CREATE INDEX IF NOT EXISTS idx_block_session ON block(session_id);
CREATE INDEX IF NOT EXISTS idx_block_message ON block(message_id, ordinal);
CREATE INDEX IF NOT EXISTS idx_block_type ON block(type);
CREATE INDEX IF NOT EXISTS idx_block_tool ON block(tool_name);
CREATE INDEX IF NOT EXISTS idx_toolcall_session ON tool_call(session_id);
CREATE INDEX IF NOT EXISTS idx_toolcall_kind ON tool_call(tool_kind);
CREATE INDEX IF NOT EXISTS idx_toolcall_server ON tool_call(mcp_server);
CREATE INDEX IF NOT EXISTS idx_toolcall_name ON tool_call(tool_name);

CREATE VIRTUAL TABLE IF NOT EXISTS block_fts USING fts5(
  text, tool_name, tool_input,
  content='block', content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS block_ai AFTER INSERT ON block BEGIN
  INSERT INTO block_fts(rowid, text, tool_name, tool_input)
  VALUES (new.id, new.text, new.tool_name, new.tool_input);
END;
CREATE TRIGGER IF NOT EXISTS block_ad AFTER DELETE ON block BEGIN
  INSERT INTO block_fts(block_fts, rowid, text, tool_name, tool_input)
  VALUES ('delete', old.id, old.text, old.tool_name, old.tool_input);
END;
CREATE TRIGGER IF NOT EXISTS block_au AFTER UPDATE ON block BEGIN
  INSERT INTO block_fts(block_fts, rowid, text, tool_name, tool_input)
  VALUES ('delete', old.id, old.text, old.tool_name, old.tool_input);
  INSERT INTO block_fts(rowid, text, tool_name, tool_input)
  VALUES (new.id, new.text, new.tool_name, new.tool_input);
END;
```

- [ ] **Step 3: Wire module + run test to verify it fails, then passes**

Add `pub mod schema;` to `crates/decant-core/src/lib.rs` (under `pub mod db;`).

Run: `cargo test -p decant-core schema::`
Expected: PASS (the DDL exists, so this should pass on first compile; if a table name is misspelled the test catches it).

- [ ] **Step 4: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): v1 schema, migrations, and FTS5 triggers"
```

---

### Task 4: Domain model types

**Files:**
- Create: `crates/decant-core/src/model.rs`
- Modify: `crates/decant-core/src/lib.rs`, `crates/decant-core/Cargo.toml`

- [ ] **Step 1: Add serde**

Run: `cargo add --package decant-core serde --features derive`

- [ ] **Step 2: Write the failing test + types**

Create `crates/decant-core/src/model.rs`:

```rust
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    pub fn as_str(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude_code",
            Tool::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
    Other,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
            Role::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Text,
    Thinking,
    ToolUse,
    ToolResult,
    WebSearch,
    Image,
    Other,
}

impl BlockType {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockType::Text => "text",
            BlockType::Thinking => "thinking",
            BlockType::ToolUse => "tool_use",
            BlockType::ToolResult => "tool_result",
            BlockType::WebSearch => "web_search",
            BlockType::Image => "image",
            BlockType::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Builtin,
    Mcp,
    Custom,
    WebSearch,
    ToolSearch,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::Builtin => "builtin",
            ToolKind::Mcp => "mcp",
            ToolKind::Custom => "custom",
            ToolKind::WebSearch => "web_search",
            ToolKind::ToolSearch => "tool_search",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone)]
pub struct NormalizedBlock {
    pub ordinal: i64,
    pub block_type: BlockType,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_result: Option<String>,
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct NormalizedMessage {
    pub seq: i64,
    pub source_uuid: Option<String>,
    pub parent_source_uuid: Option<String>,
    pub role: Role,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub timestamp: Option<String>,
    pub usage: Option<TokenUsage>,
    pub raw: Value,
    pub blocks: Vec<NormalizedBlock>,
}

#[derive(Debug, Clone)]
pub struct NormalizedSession {
    pub tool: Tool,
    pub source_session_id: String,
    pub project_path: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub cli_version: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub is_archived: bool,
    pub raw_meta: Value,
    /// Session-level token totals (Codex sets these directly; Claude sums per-message).
    pub totals: TokenUsage,
    pub messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub line_no: usize,
    pub error: String,
    pub raw_line: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSession {
    pub session: NormalizedSession,
    pub issues: Vec<Issue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_strings_are_stable() {
        assert_eq!(Tool::ClaudeCode.as_str(), "claude_code");
        assert_eq!(Role::Tool.as_str(), "tool");
        assert_eq!(BlockType::ToolUse.as_str(), "tool_use");
        assert_eq!(ToolKind::Mcp.as_str(), "mcp");
    }
}
```

- [ ] **Step 3: Wire module + run test**

Add `pub mod model;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core model::`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): normalized domain model types"
```

---

### Task 5: Tool/MCP classification

**Files:**
- Create: `crates/decant-core/src/tools.rs`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `crates/decant-core/src/tools.rs`:

```rust
use crate::model::ToolKind;

/// Classify a logged tool name into (kind, mcp_server, base_name).
/// MCP convention: `mcp__<server>__<base>` (base may itself contain `__`).
pub fn classify_tool(name: &str) -> (ToolKind, Option<String>, String) {
    if let Some(rest) = name.strip_prefix("mcp__") {
        if let Some((server, base)) = rest.split_once("__") {
            return (ToolKind::Mcp, Some(server.to_string()), base.to_string());
        }
        return (ToolKind::Mcp, None, rest.to_string());
    }
    (ToolKind::Builtin, None, name.to_string())
}

/// First `max` chars of a string, with an ellipsis if truncated.
pub fn preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tool() {
        let (kind, server, base) = classify_tool("Bash");
        assert_eq!(kind, ToolKind::Builtin);
        assert_eq!(server, None);
        assert_eq!(base, "Bash");
    }

    #[test]
    fn simple_mcp_tool() {
        let (kind, server, base) = classify_tool("mcp__claude_ai_Linear__create_issue");
        assert_eq!(kind, ToolKind::Mcp);
        assert_eq!(server.as_deref(), Some("claude_ai_Linear"));
        assert_eq!(base, "create_issue");
    }

    #[test]
    fn nested_gateway_mcp_tool() {
        let (kind, server, base) = classify_tool("mcp__codex_apps__hubspot__create_deal");
        assert_eq!(kind, ToolKind::Mcp);
        assert_eq!(server.as_deref(), Some("codex_apps"));
        assert_eq!(base, "hubspot__create_deal");
    }

    #[test]
    fn preview_truncates() {
        assert_eq!(preview("abcdef", 3), "abc…");
        assert_eq!(preview("ab", 3), "ab");
    }
}
```

- [ ] **Step 2: Wire module + run test**

Add `pub mod tools;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core tools::`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): tool/MCP name classification"
```

---

### Task 6: Cost estimation

**Files:**
- Create: `crates/decant-core/src/cost.rs`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test + implementation**

Create `crates/decant-core/src/cost.rs`:

```rust
use crate::model::TokenUsage;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// Seed pricing (USD per million tokens). Estimates; editable in the DB later.
/// Unknown models estimate to 0.0 (surfaced as "unknown" in the UI).
pub fn default_pricing() -> HashMap<&'static str, Price> {
    let mut m = HashMap::new();
    // Claude (Anthropic) — representative published rates.
    m.insert("claude-opus-4-7", Price { input_per_mtok: 15.0, output_per_mtok: 75.0, cache_read_per_mtok: 1.5, cache_write_per_mtok: 18.75 });
    m.insert("claude-sonnet-4-6", Price { input_per_mtok: 3.0, output_per_mtok: 15.0, cache_read_per_mtok: 0.3, cache_write_per_mtok: 3.75 });
    m.insert("claude-haiku-4-5", Price { input_per_mtok: 1.0, output_per_mtok: 5.0, cache_read_per_mtok: 0.1, cache_write_per_mtok: 1.25 });
    m
}

/// Estimate cost in USD for one session's usage under a pricing table.
pub fn estimate_cost(model: Option<&str>, usage: &TokenUsage, pricing: &HashMap<&'static str, Price>) -> f64 {
    let Some(model) = model else { return 0.0 };
    let Some(p) = pricing.get(model) else { return 0.0 };
    let per = |tokens: i64, rate: f64| (tokens as f64) * rate / 1_000_000.0;
    per(usage.input, p.input_per_mtok)
        + per(usage.output, p.output_per_mtok)
        + per(usage.cache_read, p.cache_read_per_mtok)
        + per(usage.cache_creation, p.cache_write_per_mtok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_model_costs_add_up() {
        let pricing = default_pricing();
        let usage = TokenUsage { input: 1_000_000, output: 1_000_000, cache_read: 0, cache_creation: 0 };
        let cost = estimate_cost(Some("claude-opus-4-7"), &usage, &pricing);
        assert!((cost - 90.0).abs() < 1e-6, "got {cost}");
    }

    #[test]
    fn unknown_model_is_zero() {
        let pricing = default_pricing();
        let usage = TokenUsage { input: 5_000, output: 5_000, cache_read: 0, cache_creation: 0 };
        assert_eq!(estimate_cost(Some("gpt-5.4"), &usage, &pricing), 0.0);
        assert_eq!(estimate_cost(None, &usage, &pricing), 0.0);
    }
}
```

- [ ] **Step 2: Wire module + run test**

Add `pub mod cost;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core cost::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): model pricing + cost estimation"
```

---

### Task 7: Claude session parser

**Files:**
- Create: `crates/decant-core/src/sources/mod.rs`, `crates/decant-core/src/sources/claude.rs`, `fixtures/claude/sample.jsonl`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Create the fixture**

Create `fixtures/claude/sample.jsonl` (each line is one record; keep exactly these 4 lines):

```jsonl
{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"sess-claude-1","timestamp":"2026-05-01T10:00:00.000Z","cwd":"/Users/dev/proj","gitBranch":"main","version":"2.1.0","message":{"role":"user","content":"Fix the failing auth test"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"sess-claude-1","timestamp":"2026-05-01T10:00:05.000Z","cwd":"/Users/dev/proj","gitBranch":"main","version":"2.1.0","message":{"role":"assistant","model":"claude-opus-4-7","stop_reason":"tool_use","usage":{"input_tokens":1200,"output_tokens":340,"cache_read_input_tokens":5000,"cache_creation_input_tokens":800},"content":[{"type":"thinking","thinking":"I should read the test file first."},{"type":"text","text":"Let me read the test."},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/Users/dev/proj/auth_test.py"}}]}}
{"type":"user","uuid":"u2","parentUuid":"a1","sessionId":"sess-claude-1","timestamp":"2026-05-01T10:00:06.000Z","cwd":"/Users/dev/proj","gitBranch":"main","version":"2.1.0","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":false,"content":"def test_auth(): assert login()"}]}}
{"type":"assistant","uuid":"a2","parentUuid":"u2","sessionId":"sess-claude-1","timestamp":"2026-05-01T10:00:10.000Z","cwd":"/Users/dev/proj","gitBranch":"main","version":"2.1.0","message":{"role":"assistant","model":"claude-opus-4-7","stop_reason":"end_turn","usage":{"input_tokens":1500,"output_tokens":120,"cache_read_input_tokens":6000,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done — the test passes now."}]}}
```

- [ ] **Step 2: Write the failing test**

Create `crates/decant-core/src/sources/mod.rs`:

```rust
pub mod claude;
pub mod codex;
```

Create `crates/decant-core/src/sources/claude.rs` with the test first:

```rust
use crate::model::*;
use serde_json::Value;

/// Parse one Claude Code session file's contents (one session per file).
pub fn parse_session(source_session_id: &str, content: &str) -> ParsedSession {
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut title: Option<String> = None;
    let mut totals = TokenUsage::default();
    let mut seq: i64 = 0;

    const KNOWN_META: &[&str] = &[
        "summary", "ai-title", "last-prompt", "permission-mode",
        "attachment", "file-history-snapshot", "queue-operation",
    ];

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                issues.push(Issue { line_no: i + 1, error: e.to_string(), raw_line: line.to_string() });
                continue;
            }
        };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if started_at.is_none() { started_at = Some(ts.to_string()); }
            ended_at = Some(ts.to_string());
        }
        if cwd.is_none() { cwd = v.get("cwd").and_then(Value::as_str).map(String::from); }
        if git_branch.is_none() { git_branch = v.get("gitBranch").and_then(Value::as_str).map(String::from); }
        if cli_version.is_none() { cli_version = v.get("version").and_then(Value::as_str).map(String::from); }

        match typ {
            "user" => {
                let msg = parse_user(&v, seq);
                if title.is_none() && msg.role == Role::User {
                    title = first_text(&msg).map(|t| truncate(&t, 120));
                }
                messages.push(msg);
                seq += 1;
            }
            "assistant" => {
                let msg = parse_assistant(&v, seq, &mut totals);
                messages.push(msg);
                seq += 1;
            }
            "system" => {
                messages.push(simple_message(&v, Role::System, seq));
                seq += 1;
            }
            t if KNOWN_META.contains(&t) => {
                // Title hint from summary/ai-title; otherwise skip meta records.
                if title.is_none() {
                    if let Some(s) = v.get("summary").and_then(Value::as_str)
                        .or_else(|| v.get("title").and_then(Value::as_str)) {
                        title = Some(truncate(s, 120));
                    }
                }
            }
            _ => {
                // Unknown top-level type: keep it (lossless), never silently drop.
                messages.push(simple_message(&v, Role::Other, seq));
                seq += 1;
            }
        }
    }

    let session = NormalizedSession {
        tool: Tool::ClaudeCode,
        source_session_id: source_session_id.to_string(),
        project_path: cwd.clone(),
        title,
        cwd,
        git_branch,
        model: dominant_model(&messages),
        cli_version,
        started_at,
        ended_at,
        is_archived: false,
        raw_meta: Value::Null,
        totals,
        messages,
    };
    ParsedSession { session, issues }
}

fn truncate(s: &str, max: usize) -> String {
    crate::tools::preview(s.trim(), max)
}

fn first_text(msg: &NormalizedMessage) -> Option<String> {
    msg.blocks.iter().find(|b| b.block_type == BlockType::Text).and_then(|b| b.text.clone())
}

fn dominant_model(messages: &[NormalizedMessage]) -> Option<String> {
    messages.iter().filter_map(|m| m.model.clone()).next()
}

fn simple_message(v: &Value, role: Role, seq: i64) -> NormalizedMessage {
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role,
        model: None,
        stop_reason: None,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage: None,
        raw: v.clone(),
        blocks: Vec::new(),
    }
}

fn parse_user(v: &Value, seq: i64) -> NormalizedMessage {
    let mut blocks = Vec::new();
    let mut role = Role::User;
    let content = v.get("message").and_then(|m| m.get("content"));
    match content {
        Some(Value::String(s)) => {
            blocks.push(text_block(0, s));
        }
        Some(Value::Array(items)) => {
            for (ord, item) in items.iter().enumerate() {
                let bt = item.get("type").and_then(Value::as_str).unwrap_or("");
                match bt {
                    "text" => blocks.push(text_block(ord as i64, item.get("text").and_then(Value::as_str).unwrap_or(""))),
                    "tool_result" => {
                        role = Role::Tool;
                        blocks.push(NormalizedBlock {
                            ordinal: ord as i64,
                            block_type: BlockType::ToolResult,
                            text: None,
                            tool_name: None,
                            tool_use_id: item.get("tool_use_id").and_then(Value::as_str).map(String::from),
                            tool_input: None,
                            tool_result: Some(stringify_content(item.get("content"))),
                            is_error: item.get("is_error").and_then(Value::as_bool),
                        });
                    }
                    _ => blocks.push(other_block(ord as i64, item)),
                }
            }
        }
        _ => {}
    }
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role,
        model: None,
        stop_reason: None,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage: None,
        raw: v.clone(),
        blocks,
    }
}

fn parse_assistant(v: &Value, seq: i64, totals: &mut TokenUsage) -> NormalizedMessage {
    let m = v.get("message");
    let model = m.and_then(|m| m.get("model")).and_then(Value::as_str).map(String::from);
    let stop_reason = m.and_then(|m| m.get("stop_reason")).and_then(Value::as_str).map(String::from);
    let usage = m.and_then(|m| m.get("usage")).map(|u| {
        let g = |k: &str| u.get(k).and_then(Value::as_i64).unwrap_or(0);
        TokenUsage {
            input: g("input_tokens"),
            output: g("output_tokens"),
            cache_read: g("cache_read_input_tokens"),
            cache_creation: g("cache_creation_input_tokens"),
        }
    });
    if let Some(u) = &usage {
        totals.input += u.input;
        totals.output += u.output;
        totals.cache_read += u.cache_read;
        totals.cache_creation += u.cache_creation;
    }
    let mut blocks = Vec::new();
    if let Some(Value::Array(items)) = m.and_then(|m| m.get("content")) {
        for (ord, item) in items.iter().enumerate() {
            let bt = item.get("type").and_then(Value::as_str).unwrap_or("");
            match bt {
                "text" => blocks.push(text_block(ord as i64, item.get("text").and_then(Value::as_str).unwrap_or(""))),
                "thinking" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64, block_type: BlockType::Thinking,
                    text: item.get("thinking").and_then(Value::as_str).map(String::from),
                    tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
                }),
                "tool_use" => blocks.push(NormalizedBlock {
                    ordinal: ord as i64, block_type: BlockType::ToolUse,
                    text: None,
                    tool_name: item.get("name").and_then(Value::as_str).map(String::from),
                    tool_use_id: item.get("id").and_then(Value::as_str).map(String::from),
                    tool_input: item.get("input").cloned(),
                    tool_result: None, is_error: None,
                }),
                _ => blocks.push(other_block(ord as i64, item)),
            }
        }
    }
    NormalizedMessage {
        seq,
        source_uuid: v.get("uuid").and_then(Value::as_str).map(String::from),
        parent_source_uuid: v.get("parentUuid").and_then(Value::as_str).map(String::from),
        role: Role::Assistant,
        model, stop_reason,
        timestamp: v.get("timestamp").and_then(Value::as_str).map(String::from),
        usage,
        raw: v.clone(),
        blocks,
    }
}

fn text_block(ordinal: i64, text: &str) -> NormalizedBlock {
    NormalizedBlock {
        ordinal, block_type: BlockType::Text, text: Some(text.to_string()),
        tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
    }
}

fn other_block(ordinal: i64, item: &Value) -> NormalizedBlock {
    NormalizedBlock {
        ordinal, block_type: BlockType::Other,
        text: Some(item.to_string()),
        tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
    }
}

fn stringify_content(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap()
    }

    #[test]
    fn parses_messages_blocks_and_roles() {
        let parsed = parse_session("sess-claude-1", &fixture());
        let s = &parsed.session;
        assert!(parsed.issues.is_empty());
        assert_eq!(s.tool, Tool::ClaudeCode);
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[0].role, Role::User);
        assert_eq!(s.messages[1].role, Role::Assistant);
        assert_eq!(s.messages[2].role, Role::Tool); // tool_result
        // assistant turn 1 has thinking + text + tool_use
        let kinds: Vec<_> = s.messages[1].blocks.iter().map(|b| b.block_type).collect();
        assert_eq!(kinds, vec![BlockType::Thinking, BlockType::Text, BlockType::ToolUse]);
    }

    #[test]
    fn aggregates_tokens_and_picks_model_and_title() {
        let parsed = parse_session("sess-claude-1", &fixture());
        let s = &parsed.session;
        assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(s.title.as_deref(), Some("Fix the failing auth test"));
        assert_eq!(s.totals.input, 1200 + 1500);
        assert_eq!(s.totals.output, 340 + 120);
        assert_eq!(s.started_at.as_deref(), Some("2026-05-01T10:00:00.000Z"));
        assert_eq!(s.ended_at.as_deref(), Some("2026-05-01T10:00:10.000Z"));
    }
}
```

- [ ] **Step 3: Wire module + run test to verify it fails, then passes**

Add `pub mod sources;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core sources::claude`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/decant-core fixtures/claude
git commit -m "feat(core): Claude Code session parser + fixture"
```

---

### Task 8: Codex session parser

**Files:**
- Create: `crates/decant-core/src/sources/codex.rs`, `fixtures/codex/sample.jsonl`

- [ ] **Step 1: Create the fixture**

Create `fixtures/codex/sample.jsonl` (exactly these 7 lines):

```jsonl
{"type":"session_meta","timestamp":"2026-05-02T09:00:00.000Z","payload":{"id":"sess-codex-1","cwd":"/Users/dev/proj","originator":"codex_cli_rs","cli_version":"0.116.0","source":"cli","model_provider":"openai"}}
{"type":"turn_context","timestamp":"2026-05-02T09:00:01.000Z","payload":{"cwd":"/Users/dev/proj","model":"gpt-5.4","effort":"high","approval_policy":"on-request"}}
{"type":"response_item","timestamp":"2026-05-02T09:00:02.000Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"List the open TODOs"}]}}
{"type":"response_item","timestamp":"2026-05-02T09:00:05.000Z","payload":{"type":"function_call","name":"exec_command","call_id":"call_1","arguments":"{\"command\":\"rg TODO\"}"}}
{"type":"response_item","timestamp":"2026-05-02T09:00:06.000Z","payload":{"type":"function_call_output","call_id":"call_1","output":"src/a.rs: // TODO refactor"}}
{"type":"event_msg","timestamp":"2026-05-02T09:00:07.000Z","payload":{"type":"token_count","input_tokens":900,"output_tokens":150,"cached_input_tokens":400}}
{"type":"response_item","timestamp":"2026-05-02T09:00:08.000Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Found one TODO in src/a.rs"}]}}
```

- [ ] **Step 2: Write the failing test + implementation**

Create `crates/decant-core/src/sources/codex.rs`:

```rust
use crate::model::*;
use serde_json::Value;
use std::collections::HashMap;

/// Parse one Codex rollout file (one session per file). `titles` maps session id ->
/// thread name (from ~/.codex/session_index.jsonl); used as the preferred title.
pub fn parse_session(fallback_id: &str, content: &str, titles: &HashMap<String, String>) -> ParsedSession {
    let mut issues = Vec::new();
    let mut id = fallback_id.to_string();
    let mut cwd: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut model: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut title: Option<String> = None;
    let mut totals = TokenUsage::default();
    let mut raw_meta = Value::Null;
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut seq: i64 = 0;

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() { continue; }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => { issues.push(Issue { line_no: i + 1, error: e.to_string(), raw_line: line.to_string() }); continue; }
        };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            if started_at.is_none() { started_at = Some(ts.to_string()); }
            ended_at = Some(ts.to_string());
        }
        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
        match typ {
            "session_meta" => {
                if let Some(s) = payload.get("id").and_then(Value::as_str) { id = s.to_string(); }
                cwd = payload.get("cwd").and_then(Value::as_str).map(String::from);
                cli_version = payload.get("cli_version").and_then(Value::as_str).map(String::from);
                raw_meta = payload.clone();
            }
            "turn_context" => {
                if let Some(m) = payload.get("model").and_then(Value::as_str) { model = Some(m.to_string()); }
                if cwd.is_none() { cwd = payload.get("cwd").and_then(Value::as_str).map(String::from); }
            }
            "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                let g = |k: &str| payload.get(k).and_then(Value::as_i64).unwrap_or(0);
                // token_count is cumulative; keep the latest seen.
                totals = TokenUsage {
                    input: g("input_tokens"),
                    output: g("output_tokens"),
                    cache_read: g("cached_input_tokens"),
                    cache_creation: 0,
                };
            }
            "response_item" => {
                if let Some(msg) = parse_item(&v, &payload, seq, &mut title) {
                    messages.push(msg);
                    seq += 1;
                }
            }
            _ => {}
        }
    }

    if let Some(t) = titles.get(&id) { title = Some(t.clone()); }

    let session = NormalizedSession {
        tool: Tool::Codex,
        source_session_id: id,
        project_path: cwd.clone(),
        title,
        cwd,
        git_branch: None,
        model,
        cli_version,
        started_at,
        ended_at,
        is_archived: false,
        raw_meta,
        totals,
        messages,
    };
    ParsedSession { session, issues }
}

fn parse_item(line: &Value, payload: &Value, seq: i64, title: &mut Option<String>) -> Option<NormalizedMessage> {
    let ptyp = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let ts = line.get("timestamp").and_then(Value::as_str).map(String::from);
    let mk = |role: Role, block: NormalizedBlock| NormalizedMessage {
        seq, source_uuid: None, parent_source_uuid: None, role,
        model: None, stop_reason: None, timestamp: ts.clone(), usage: None,
        raw: line.clone(), blocks: vec![block],
    };
    match ptyp {
        "message" => {
            let role = match payload.get("role").and_then(Value::as_str) {
                Some("assistant") => Role::Assistant,
                Some("system") => Role::System,
                _ => Role::User,
            };
            let text = collect_text(payload.get("content"));
            if role == Role::User && title.is_none() && !text.is_empty() {
                *title = Some(crate::tools::preview(text.trim(), 120));
            }
            Some(mk(role, NormalizedBlock {
                ordinal: 0, block_type: BlockType::Text, text: Some(text),
                tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
            }))
        }
        "reasoning" => {
            let text = collect_text(payload.get("summary")).trim().to_string();
            let text = if text.is_empty() { collect_text(payload.get("content")) } else { text };
            Some(mk(Role::Assistant, NormalizedBlock {
                ordinal: 0, block_type: BlockType::Thinking, text: Some(text),
                tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
            }))
        }
        "function_call" | "custom_tool_call" | "tool_search_call" | "mcp_tool_call" => {
            let name = payload.get("name").and_then(Value::as_str).map(String::from);
            let args = payload.get("arguments").cloned()
                .or_else(|| payload.get("input").cloned());
            Some(mk(Role::Assistant, NormalizedBlock {
                ordinal: 0, block_type: BlockType::ToolUse, text: None,
                tool_name: name,
                tool_use_id: payload.get("call_id").and_then(Value::as_str).map(String::from),
                tool_input: args, tool_result: None, is_error: None,
            }))
        }
        "function_call_output" | "custom_tool_call_output" | "tool_search_output" => {
            Some(mk(Role::Tool, NormalizedBlock {
                ordinal: 0, block_type: BlockType::ToolResult, text: None,
                tool_name: None,
                tool_use_id: payload.get("call_id").and_then(Value::as_str).map(String::from),
                tool_input: None,
                tool_result: Some(stringify(payload.get("output"))),
                is_error: None,
            }))
        }
        "web_search_call" => Some(mk(Role::Assistant, NormalizedBlock {
            ordinal: 0, block_type: BlockType::WebSearch, text: None,
            tool_name: Some("web_search".to_string()),
            tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
        })),
        _ => Some(mk(Role::Other, NormalizedBlock {
            ordinal: 0, block_type: BlockType::Other, text: Some(payload.to_string()),
            tool_name: None, tool_use_id: None, tool_input: None, tool_result: None, is_error: None,
        })),
    }
}

fn collect_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|it| it.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn stringify(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/codex/sample.jsonl")).unwrap()
    }

    #[test]
    fn parses_meta_model_and_conversation() {
        let titles = HashMap::new();
        let parsed = parse_session("fallback", &fixture(), &titles);
        let s = &parsed.session;
        assert!(parsed.issues.is_empty());
        assert_eq!(s.tool, Tool::Codex);
        assert_eq!(s.source_session_id, "sess-codex-1");
        assert_eq!(s.cwd.as_deref(), Some("/Users/dev/proj"));
        assert_eq!(s.model.as_deref(), Some("gpt-5.4"));
        // user message, function_call, function_call_output, assistant message = 4
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[0].role, Role::User);
        assert_eq!(s.messages[1].blocks[0].block_type, BlockType::ToolUse);
        assert_eq!(s.messages[2].role, Role::Tool);
        assert_eq!(s.title.as_deref(), Some("List the open TODOs"));
    }

    #[test]
    fn cumulative_token_count_becomes_session_totals() {
        let parsed = parse_session("fallback", &fixture(), &HashMap::new());
        assert_eq!(parsed.session.totals.input, 900);
        assert_eq!(parsed.session.totals.output, 150);
        assert_eq!(parsed.session.totals.cache_read, 400);
    }

    #[test]
    fn session_index_title_overrides() {
        let mut titles = HashMap::new();
        titles.insert("sess-codex-1".to_string(), "TODO audit".to_string());
        let parsed = parse_session("fallback", &fixture(), &titles);
        assert_eq!(parsed.session.title.as_deref(), Some("TODO audit"));
    }
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p decant-core sources::codex`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/decant-core fixtures/codex
git commit -m "feat(core): Codex rollout parser + fixture"
```

---

### Task 9: Config — resolve DB/source paths

**Files:**
- Create: `crates/decant-core/src/config.rs`
- Modify: `crates/decant-core/src/lib.rs`, `crates/decant-core/Cargo.toml`

- [ ] **Step 1: Add path crate**

Run: `cargo add --package decant-core directories`

- [ ] **Step 2: Write the failing test + implementation**

Create `crates/decant-core/src/config.rs`:

```rust
use directories::{BaseDirs, ProjectDirs};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
}

impl Config {
    /// Resolve with precedence: explicit override > env > platform default.
    pub fn resolve(
        db_override: Option<PathBuf>,
        claude_override: Option<PathBuf>,
        codex_override: Option<PathBuf>,
    ) -> Config {
        let home = BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let data_dir = ProjectDirs::from("", "", "decant")
            .map(|p| p.data_dir().to_path_buf())
            .unwrap_or_else(|| home.join(".local/share/decant"));

        let db_path = db_override
            .or_else(|| std::env::var_os("DECANT_DB").map(PathBuf::from))
            .unwrap_or_else(|| data_dir.join("decant.db"));
        let claude_dir = claude_override
            .or_else(|| std::env::var_os("DECANT_CLAUDE_DIR").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".claude/projects"));
        let codex_dir = codex_override
            .or_else(|| std::env::var_os("DECANT_CODEX_DIR").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".codex"));

        Config { db_path, claude_dir, codex_dir }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        let c = Config::resolve(Some(PathBuf::from("/tmp/x.db")), None, None);
        assert_eq!(c.db_path, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn defaults_point_into_home() {
        let c = Config::resolve(None, None, None);
        assert!(c.claude_dir.ends_with(".claude/projects"));
        assert!(c.db_path.to_string_lossy().contains("decant"));
    }
}
```

- [ ] **Step 3: Wire module + run test**

Add `pub mod config;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core config::`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): config path resolution (override > env > default)"
```

---

### Task 10: Ingest — write a parsed session to the DB

**Files:**
- Create: `crates/decant-core/src/ingest.rs`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test + the writer**

Create `crates/decant-core/src/ingest.rs`:

```rust
use crate::cost::{default_pricing, estimate_cost};
use crate::model::*;
use crate::tools::classify_tool;
use crate::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Insert (or replace) one parsed session inside an open transaction.
/// Deletes any prior rows for the same (tool, source_session_id) first, so this
/// is idempotent. FTS is maintained by triggers.
pub fn upsert_session(conn: &Connection, parsed: &ParsedSession, source_path: &str, mtime: i64, size: i64, hash: &str) -> Result<i64> {
    let s = &parsed.session;

    // Project (by cwd path).
    let project_id: Option<i64> = if let Some(path) = &s.project_path {
        conn.execute(
            "INSERT INTO project(path, name, first_seen_at, last_seen_at)
             VALUES (?1, ?2, datetime('now'), datetime('now'))
             ON CONFLICT(path) DO UPDATE SET last_seen_at = datetime('now')",
            params![path, basename(path)],
        )?;
        Some(conn.query_row("SELECT id FROM project WHERE path = ?1", params![path], |r| r.get(0))?)
    } else {
        None
    };

    // Remove any existing session with the same identity (cascades to children).
    conn.execute(
        "DELETE FROM session WHERE tool = ?1 AND source_session_id = ?2",
        params![s.tool.as_str(), s.source_session_id],
    )?;

    let cost = estimate_cost(s.model.as_deref(), &s.totals, &default_pricing());
    conn.execute(
        "INSERT INTO session(
            tool, source_session_id, project_id, title, cwd, git_branch, model, cli_version,
            started_at, ended_at, message_count,
            total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_creation_tokens,
            estimated_cost_usd, is_archived, source_path, raw_meta,
            ingested_at, source_mtime, source_size, source_hash)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,datetime('now'),?20,?21,?22)",
        params![
            s.tool.as_str(), s.source_session_id, project_id, s.title, s.cwd, s.git_branch, s.model, s.cli_version,
            s.started_at, s.ended_at, s.messages.len() as i64,
            s.totals.input, s.totals.output, s.totals.cache_read, s.totals.cache_creation,
            cost, s.is_archived as i64, source_path, s.raw_meta.to_string(),
            mtime, size, hash,
        ],
    )?;
    let session_id = conn.last_insert_rowid();

    // call_id / tool_use_id -> (call_block_id, result_block_id) for tool_call pairing.
    let mut results: HashMap<String, i64> = HashMap::new();
    let mut result_errors: HashMap<String, Option<bool>> = HashMap::new();
    let mut tool_use_blocks: Vec<(i64, NormalizedBlock)> = Vec::new();

    for m in &s.messages {
        conn.execute(
            "INSERT INTO message(session_id, seq, source_uuid, parent_source_uuid, role, model, stop_reason,
                                 timestamp, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, raw)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                session_id, m.seq, m.source_uuid, m.parent_source_uuid, m.role.as_str(), m.model, m.stop_reason,
                m.timestamp,
                m.usage.as_ref().map(|u| u.input),
                m.usage.as_ref().map(|u| u.output),
                m.usage.as_ref().map(|u| u.cache_read),
                m.usage.as_ref().map(|u| u.cache_creation),
                m.raw.to_string(),
            ],
        )?;
        let message_id = conn.last_insert_rowid();

        for b in &m.blocks {
            conn.execute(
                "INSERT INTO block(message_id, session_id, ordinal, type, text, tool_name, tool_use_id, tool_input, tool_result)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    message_id, session_id, b.ordinal, b.block_type.as_str(), b.text, b.tool_name,
                    b.tool_use_id, b.tool_input.as_ref().map(|v| v.to_string()), b.tool_result,
                ],
            )?;
            let block_id = conn.last_insert_rowid();
            match b.block_type {
                BlockType::ToolUse => {
                    tool_use_blocks.push((block_id, b.clone()));
                }
                BlockType::ToolResult => {
                    if let Some(id) = &b.tool_use_id {
                        results.insert(id.clone(), block_id);
                        result_errors.insert(id.clone(), b.is_error);
                    }
                }
                _ => {}
            }
        }
    }

    // Build tool_call rows from tool_use blocks, pairing results by tool_use_id.
    for (call_block_id, b) in &tool_use_blocks {
        let name = b.tool_name.clone().unwrap_or_default();
        let (kind, server, base) = classify_tool(&name);
        let result_block_id = b.tool_use_id.as_ref().and_then(|id| results.get(id)).copied();
        let is_error: Option<bool> = b
            .tool_use_id
            .as_ref()
            .and_then(|id| result_errors.get(id))
            .copied()
            .flatten();
        conn.execute(
            "INSERT INTO tool_call(session_id, call_block_id, result_block_id, tool_kind, tool_name,
                                   mcp_server, tool_base_name, tool_use_id, input, is_error, ordinal, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                session_id, call_block_id, result_block_id, kind.as_str(), name,
                server, base, b.tool_use_id,
                b.tool_input.as_ref().map(|v| v.to_string()),
                is_error.map(|e| e as i64),
                b.ordinal, s.started_at,
            ],
        )?;
    }

    Ok(session_id)
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema, sources};

    fn claude_fixture() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap()
    }

    #[test]
    fn writes_session_messages_blocks_and_tool_calls() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &claude_fixture());
        let sid = upsert_session(&conn, &parsed, "/x/sample.jsonl", 1, 2, "h").unwrap();
        assert!(sid > 0);

        let (msgs, blocks, calls, mcount, tin): (i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM message),
                        (SELECT COUNT(*) FROM block),
                        (SELECT COUNT(*) FROM tool_call),
                        (SELECT message_count FROM session),
                        (SELECT total_input_tokens FROM session)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(msgs, 4);
        assert_eq!(mcount, 4);
        assert_eq!(blocks, 6); // thinking+text+tool_use + tool_result + user text + final text
        assert_eq!(calls, 1); // one tool_use (Read)
        assert_eq!(tin, 2700);

        let (kind, base): (String, String) = conn
            .query_row("SELECT tool_kind, tool_base_name FROM tool_call", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(kind, "builtin");
        assert_eq!(base, "Read");

        let fts: i64 = conn
            .query_row("SELECT COUNT(*) FROM block_fts WHERE block_fts MATCH 'auth'", [], |r| r.get(0))
            .unwrap();
        assert!(fts >= 1, "FTS should find 'auth'");
    }
}
```

- [ ] **Step 2: Wire module + run test**

Add `pub mod ingest;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core ingest::writes`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): write parsed sessions to sqlite (messages/blocks/tool_calls)"
```

---

### Task 11: Ingest — discover files + full sync (idempotent, parallel, issue-logging)

**Files:**
- Modify: `crates/decant-core/src/ingest.rs`, `crates/decant-core/Cargo.toml`

- [ ] **Step 1: Add deps**

Run:
```bash
cargo add --package decant-core walkdir blake3 rayon
```

- [ ] **Step 2: Write the failing test + sync engine**

Append to `crates/decant-core/src/ingest.rs` (above the `#[cfg(test)]` block):

```rust
use crate::config::Config;
use crate::sources;
use std::collections::HashMap as Map;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct SyncReport {
    pub scanned: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub issues: usize,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub tool: Tool,
    pub path: PathBuf,
    pub archived: bool,
}

/// Find all Claude and Codex session files under the configured roots.
pub fn discover(config: &Config) -> Vec<SourceFile> {
    let mut out = Vec::new();
    // Claude: <claude_dir>/<project>/<uuid>.jsonl
    collect(&config.claude_dir, Tool::ClaudeCode, false, |name| name.ends_with(".jsonl"), &mut out);
    // Codex rollouts under sessions/ and archived_sessions/
    collect(&config.codex_dir.join("sessions"), Tool::Codex, false, is_rollout, &mut out);
    collect(&config.codex_dir.join("archived_sessions"), Tool::Codex, true, is_rollout, &mut out);
    out
}

fn is_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn collect(root: &Path, tool: Tool, archived: bool, want: impl Fn(&str) -> bool, out: &mut Vec<SourceFile>) {
    if !root.exists() { return; }
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() { continue; }
        let name = entry.file_name().to_string_lossy();
        if want(&name) {
            out.push(SourceFile { tool, path: entry.path().to_path_buf(), archived });
        }
    }
}

/// Load Codex session-index titles (id -> thread_name), if present.
fn codex_titles(config: &Config) -> Map<String, String> {
    let mut titles = Map::new();
    let path = config.codex_dir.join("session_index.jsonl");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(name)) = (
                    v.get("id").and_then(|x| x.as_str()),
                    v.get("thread_name").and_then(|x| x.as_str()),
                ) {
                    titles.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    titles
}

struct Prepared {
    file: SourceFile,
    content: String,
    mtime: i64,
    size: i64,
    hash: String,
}

/// Full sync: discover, skip unchanged, parse in parallel, write serially. Idempotent.
pub fn sync(conn: &mut Connection, config: &Config) -> Result<SyncReport> {
    use rayon::prelude::*;

    let files = discover(config);
    let titles = codex_titles(config);
    let mut report = SyncReport { scanned: files.len(), ..Default::default() };

    // Decide which files changed (cheap, serial: stat + lookup).
    let mut to_read: Vec<SourceFile> = Vec::new();
    for f in files {
        let meta = match std::fs::metadata(&f.path) { Ok(m) => m, Err(_) => continue };
        let size = meta.len() as i64;
        let mtime = mtime_secs(&meta);
        let path_str = f.path.to_string_lossy().to_string();
        let prior: Option<(i64, i64)> = conn
            .query_row("SELECT size, mtime FROM ingest_source WHERE path = ?1", params![path_str], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok();
        if prior == Some((size, mtime)) {
            report.skipped += 1;
        } else {
            to_read.push(f);
        }
    }

    // Read + hash + parse in parallel.
    let prepared: Vec<(Prepared, ParsedSession)> = to_read
        .par_iter()
        .filter_map(|f| {
            let content = std::fs::read_to_string(&f.path).ok()?;
            let meta = std::fs::metadata(&f.path).ok()?;
            let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let parsed = match f.tool {
                Tool::ClaudeCode => sources::claude::parse_session(&stem, &content),
                Tool::Codex => sources::codex::parse_session(&stem, &content, &titles),
            };
            Some((Prepared { file: f.clone(), content, mtime: mtime_secs(&meta), size: meta.len() as i64, hash }, parsed))
        })
        .collect();

    // Write serially in one transaction (SQLite single-writer).
    let tx = conn.transaction()?;
    for (prep, mut parsed) in prepared {
        parsed.session.is_archived = prep.file.archived;
        let path_str = prep.file.path.to_string_lossy().to_string();
        let session_id = upsert_session(&tx, &parsed, &path_str, prep.mtime, prep.size, &prep.hash)?;
        for issue in &parsed.issues {
            tx.execute(
                "INSERT INTO ingest_issue(source_path, line_no, error, raw_line, created_at)
                 VALUES (?1,?2,?3,?4,datetime('now'))",
                params![path_str, issue.line_no as i64, issue.error, issue.raw_line],
            )?;
            report.issues += 1;
        }
        let status = if parsed.issues.is_empty() { "ok" } else { "ok_with_issues" };
        tx.execute(
            "INSERT INTO ingest_source(path, tool, size, mtime, hash, session_id, line_count, status, last_ingested_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))
             ON CONFLICT(path) DO UPDATE SET size=?3, mtime=?4, hash=?5, session_id=?6, line_count=?7, status=?8, last_ingested_at=datetime('now')",
            params![path_str, prep.file.tool.as_str(), prep.size, prep.mtime, prep.hash, session_id, prep.content.lines().count() as i64, status],
        )?;
        report.ingested += 1;
    }
    tx.commit()?;
    Ok(report)
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

Note: `upsert_session` is called with `&tx` — `Transaction` derefs to `Connection`, so the signature (`&Connection`) works unchanged.

- [ ] **Step 3: Write the integration test**

Append inside the `#[cfg(test)] mod tests` block in `ingest.rs`:

```rust
    use std::fs;

    fn write(path: &std::path::Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn sync_is_idempotent_and_logs_issues() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let codex_dir = dir.path().join("codex");
        // one good Claude session + one bad line appended
        let mut claude = claude_fixture();
        claude.push_str("\n{not valid json\n");
        write(&claude_dir.join("proj/sess.jsonl"), &claude);

        let config = Config { db_path: dir.path().join("d.db"), claude_dir, codex_dir };
        let mut conn = db::open(&config.db_path).unwrap();
        schema::migrate(&conn).unwrap();

        let r1 = sync(&mut conn, &config).unwrap();
        assert_eq!(r1.ingested, 1);
        assert_eq!(r1.issues, 1);

        let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1);

        // Second run: nothing changed -> skipped, no duplicates.
        let r2 = sync(&mut conn, &config).unwrap();
        assert_eq!(r2.ingested, 0);
        assert_eq!(r2.skipped, 1);
        let sessions2: i64 = conn.query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions2, 1);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p decant-core ingest::`
Expected: PASS (both `writes_...` and `sync_is_idempotent...`).

- [ ] **Step 5: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): idempotent parallel sync with discovery + issue logging"
```

---

### Task 12: Read query API (list, search, get)

**Files:**
- Create: `crates/decant-core/src/query.rs`
- Modify: `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test + queries**

Create `crates/decant-core/src/query.rs`:

```rust
use crate::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub tool: String,
    pub source_session_id: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub estimated_cost_usd: f64,
    pub is_archived: bool,
}

#[derive(Debug, Default)]
pub struct ListFilter {
    pub tool: Option<String>,
    pub limit: i64,
}

pub fn list_sessions(conn: &Connection, filter: &ListFilter) -> Result<Vec<SessionSummary>> {
    let limit = if filter.limit > 0 { filter.limit } else { 50 };
    let mut sql = String::from(
        "SELECT s.id, s.tool, s.source_session_id, s.title, p.path, s.model, s.started_at, s.ended_at,
                s.message_count, s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd, s.is_archived
         FROM session s LEFT JOIN project p ON p.id = s.project_id",
    );
    if filter.tool.is_some() {
        sql.push_str(" WHERE s.tool = ?1");
    }
    sql.push_str(" ORDER BY s.started_at DESC LIMIT ");
    sql.push_str(&limit.to_string());

    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row| {
        Ok(SessionSummary {
            id: r.get(0)?,
            tool: r.get(1)?,
            source_session_id: r.get(2)?,
            title: r.get(3)?,
            project_path: r.get(4)?,
            model: r.get(5)?,
            started_at: r.get(6)?,
            ended_at: r.get(7)?,
            message_count: r.get(8)?,
            total_input_tokens: r.get(9)?,
            total_output_tokens: r.get(10)?,
            estimated_cost_usd: r.get(11)?,
            is_archived: r.get::<_, i64>(12)? != 0,
        })
    };
    let rows = if let Some(tool) = &filter.tool {
        stmt.query_map(params![tool], map)?.collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub session_id: i64,
    pub session_title: Option<String>,
    pub tool: String,
    pub block_id: i64,
    pub snippet: String,
}

pub fn search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
    let limit = if limit > 0 { limit } else { 30 };
    let mut stmt = conn.prepare(
        "SELECT b.session_id, s.title, s.tool, b.id,
                snippet(block_fts, 0, '[', ']', '…', 12) AS snip
         FROM block_fts
         JOIN block b ON b.id = block_fts.rowid
         JOIN session s ON s.id = b.session_id
         WHERE block_fts MATCH ?1
         ORDER BY bm25(block_fts)
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |r| {
            Ok(SearchHit {
                session_id: r.get(0)?,
                session_title: r.get(1)?,
                tool: r.get(2)?,
                block_id: r.get(3)?,
                snippet: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct BlockView {
    pub block_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub tool_result: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub blocks: Vec<BlockView>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub messages: Vec<MessageView>,
}

pub fn get_session(conn: &Connection, id: i64) -> Result<Option<SessionDetail>> {
    let summaries = {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.tool, s.source_session_id, s.title, p.path, s.model, s.started_at, s.ended_at,
                    s.message_count, s.total_input_tokens, s.total_output_tokens, s.estimated_cost_usd, s.is_archived
             FROM session s LEFT JOIN project p ON p.id = s.project_id WHERE s.id = ?1",
        )?;
        stmt.query_map(params![id], |r| {
            Ok(SessionSummary {
                id: r.get(0)?, tool: r.get(1)?, source_session_id: r.get(2)?, title: r.get(3)?,
                project_path: r.get(4)?, model: r.get(5)?, started_at: r.get(6)?, ended_at: r.get(7)?,
                message_count: r.get(8)?, total_input_tokens: r.get(9)?, total_output_tokens: r.get(10)?,
                estimated_cost_usd: r.get(11)?, is_archived: r.get::<_, i64>(12)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let Some(summary) = summaries.into_iter().next() else { return Ok(None) };

    let mut stmt = conn.prepare(
        "SELECT m.id, m.role, m.timestamp, m.model FROM message m WHERE m.session_id = ?1 ORDER BY m.seq",
    )?;
    let msg_rows = stmt
        .query_map(params![id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut messages = Vec::new();
    for (mid, role, ts, model) in msg_rows {
        let mut bstmt = conn.prepare(
            "SELECT type, text, tool_name, tool_input, tool_result FROM block WHERE message_id = ?1 ORDER BY ordinal",
        )?;
        let blocks = bstmt
            .query_map(params![mid], |r| {
                Ok(BlockView {
                    block_type: r.get(0)?, text: r.get(1)?, tool_name: r.get(2)?,
                    tool_input: r.get(3)?, tool_result: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        messages.push(MessageView { role: role.unwrap_or_default(), timestamp: ts, model, blocks });
    }
    Ok(Some(SessionDetail { summary, messages }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, schema, sources};

    fn seeded() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &content);
        ingest::upsert_session(&conn, &parsed, "/x.jsonl", 1, 2, "h").unwrap();
        conn
    }

    #[test]
    fn list_and_get_and_search() {
        let conn = seeded();
        let list = list_sessions(&conn, &ListFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title.as_deref(), Some("Fix the failing auth test"));

        let detail = get_session(&conn, list[0].id).unwrap().unwrap();
        assert_eq!(detail.messages.len(), 4);

        let hits = search(&conn, "auth", 10).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].tool, "claude_code");
    }
}
```

- [ ] **Step 2: Wire module + run test**

Add `pub mod query;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core query::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/decant-core
git commit -m "feat(core): read query API (list/search/get) returning serde DTOs"
```

---

### Task 13: Verify the whole core compiles and tests pass

- [ ] **Step 1: Run the full core test suite**

Run: `cargo test -p decant-core`
Expected: PASS — all tests across db, schema, model, tools, cost, sources, config, ingest, query.

- [ ] **Step 2: Lint**

Run: `cargo clippy -p decant-core --all-targets` (if clippy installed; otherwise skip).
Expected: no errors (warnings acceptable for now).

- [ ] **Step 3: Commit (only if clippy changed anything)**

```bash
git add -A && git commit -m "chore(core): clippy cleanup" || echo "nothing to commit"
```

---

### Task 14: CLI skeleton — clap, global flags, output context, exit codes

**Files:**
- Modify: `crates/decant-cli/Cargo.toml`, `crates/decant-cli/src/main.rs`
- Create: `crates/decant-cli/src/output.rs`, `crates/decant-cli/src/commands/mod.rs`
- Create test: `crates/decant-cli/tests/cli.rs`

- [ ] **Step 1: Add deps**

Run:
```bash
cargo add --package decant-cli clap --features derive
cargo add --package decant-cli anyhow serde_json comfy-table
cargo add --package decant-cli --dev assert_cmd predicates tempfile
```

- [ ] **Step 2: Write the failing CLI test**

Create `crates/decant-cli/tests/cli.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_flag_works() {
    Command::cargo_bin("decant").unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("decant"));
}

#[test]
fn help_lists_core_commands() {
    Command::cargo_bin("decant").unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("session"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn unknown_command_exits_two() {
    Command::cargo_bin("decant").unwrap()
        .arg("frobnicate")
        .assert()
        .code(2);
}
```

- [ ] **Step 3: Implement the skeleton**

Create `crates/decant-cli/src/output.rs`:

```rust
use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
    Md,
}

#[derive(Debug, Clone, Copy)]
pub struct OutputCtx {
    pub format: Format,
    pub color: bool,
    pub quiet: bool,
}

impl OutputCtx {
    pub fn new(json: bool, format: Option<&str>, no_color: bool, quiet: bool) -> Self {
        let format = if json {
            Format::Json
        } else {
            match format {
                Some("json") => Format::Json,
                Some("md") => Format::Md,
                _ => Format::Table,
            }
        };
        let color = should_color(no_color);
        OutputCtx { format, color, quiet }
    }
}

/// Color only when: not disabled by flag, NO_COLOR unset, and stdout is a TTY.
pub fn should_color(no_color_flag: bool) -> bool {
    if no_color_flag || std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
```

Create `crates/decant-cli/src/commands/mod.rs`:

```rust
pub mod search;
pub mod session;
pub mod sync;
```

(You'll create `sync.rs`, `session.rs`, `search.rs` in Tasks 15–17. For now, stub them so the module compiles — create each file with a single `// implemented in a later task` line is NOT allowed; instead create them with real minimal handlers below.)

Replace `crates/decant-cli/src/main.rs`:

```rust
mod commands;
mod output;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// decant — extract, browse, and search your Claude Code & Codex sessions.
#[derive(Parser, Debug)]
#[command(name = "decant", version, about, long_about = None)]
pub struct Cli {
    /// Path to the decant SQLite database (overrides $DECANT_DB).
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Output format: table | json | md.
    #[arg(long, global = true)]
    pub format: Option<String>,

    /// Suppress non-essential output (and print bare IDs where applicable).
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color (also honors the NO_COLOR env var).
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan session directories and upsert new/changed sessions into the DB.
    Sync(commands::sync::SyncArgs),
    /// Inspect sessions (ls, show).
    #[command(subcommand)]
    Session(commands::session::SessionCmd),
    /// List sessions (alias for `session ls`).
    Ls(commands::session::LsArgs),
    /// Render a full transcript (alias for `session show`).
    Show(commands::session::ShowArgs),
    /// Full-text search across all sessions.
    Search(commands::search::SearchArgs),
}

impl Cli {
    pub fn output(&self) -> output::OutputCtx {
        output::OutputCtx::new(self.json, self.format.as_deref(), self.no_color, self.quiet)
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match run(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

/// Returns the process exit code (0 ok, 3 = completed with ingest issues).
fn run(cli: &Cli) -> anyhow::Result<i32> {
    match &cli.command {
        Commands::Sync(args) => commands::sync::run(cli, args),
        Commands::Session(cmd) => commands::session::run(cli, cmd),
        Commands::Ls(args) => commands::session::run_ls(cli, args),
        Commands::Show(args) => commands::session::run_show(cli, args),
        Commands::Search(args) => commands::search::run(cli, args),
    }
}
```

- [ ] **Step 4: Create real command stubs that compile**

These get fleshed out in Tasks 15–17, but must compile now. Create them minimally-real:

`crates/decant-cli/src/commands/sync.rs`:

```rust
use crate::Cli;
use clap::Args;

#[derive(Args, Debug)]
pub struct SyncArgs {}

pub fn run(_cli: &Cli, _args: &SyncArgs) -> anyhow::Result<i32> {
    anyhow::bail!("sync not implemented yet")
}
```

`crates/decant-cli/src/commands/session.rs`:

```rust
use crate::Cli;
use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// List sessions.
    Ls(LsArgs),
    /// Render a full transcript.
    Show(ShowArgs),
}

#[derive(Args, Debug)]
pub struct LsArgs {
    /// Only this tool: claude_code | codex.
    #[arg(long)]
    pub tool: Option<String>,
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Session id (integer from `session ls`).
    pub id: i64,
}

pub fn run(cli: &Cli, cmd: &SessionCmd) -> anyhow::Result<i32> {
    match cmd {
        SessionCmd::Ls(a) => run_ls(cli, a),
        SessionCmd::Show(a) => run_show(cli, a),
    }
}

pub fn run_ls(_cli: &Cli, _args: &LsArgs) -> anyhow::Result<i32> {
    anyhow::bail!("session ls not implemented yet")
}

pub fn run_show(_cli: &Cli, _args: &ShowArgs) -> anyhow::Result<i32> {
    anyhow::bail!("session show not implemented yet")
}
```

`crates/decant-cli/src/commands/search.rs`:

```rust
use crate::Cli;
use clap::Args;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Query string (FTS5 syntax supported).
    pub query: String,
    /// Max hits.
    #[arg(long, default_value_t = 30)]
    pub limit: i64,
}

pub fn run(_cli: &Cli, _args: &SearchArgs) -> anyhow::Result<i32> {
    anyhow::bail!("search not implemented yet")
}
```

Also add `mod output;` usage: `main.rs` already declares `mod output;`. Make `Cli` public so commands can import it — it is (`pub struct Cli`). Commands import via `use crate::Cli;`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p decant-cli`
Expected: PASS — `version_flag_works`, `help_lists_core_commands`, `unknown_command_exits_two` (clap exits 2 on unknown subcommand).

- [ ] **Step 6: Commit**

```bash
git add crates/decant-cli
git commit -m "feat(cli): clap skeleton, global flags, output context, exit codes"
```

---

### Task 15: CLI `sync` command

**Files:**
- Modify: `crates/decant-cli/src/commands/sync.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/decant-cli/tests/cli.rs`:

```rust
use std::fs;

fn write_fixture_tree(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let sample = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
    fs::write(claude_dir.join("sess.jsonl"), sample).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

#[test]
fn sync_then_reports_json() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ingested\""));
}
```

- [ ] **Step 2: Implement `sync`**

Replace `crates/decant-cli/src/commands/sync.rs`:

```rust
use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, ingest, schema};
use serde::Serialize;

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Override the Claude projects directory.
    #[arg(long)]
    pub claude_dir: Option<std::path::PathBuf>,
    /// Override the Codex home directory.
    #[arg(long)]
    pub codex_dir: Option<std::path::PathBuf>,
}

#[derive(Serialize)]
struct ReportJson {
    scanned: usize,
    ingested: usize,
    skipped: usize,
    issues: usize,
}

pub fn run(cli: &Cli, args: &SyncArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), args.claude_dir.clone(), args.codex_dir.clone());
    if let Some(parent) = config.db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let report = ingest::sync(&mut conn, &config)?;
    let out = cli.output();

    if matches!(out.format, crate::output::Format::Json) {
        crate::output::print_json(&ReportJson {
            scanned: report.scanned,
            ingested: report.ingested,
            skipped: report.skipped,
            issues: report.issues,
        })?;
    } else if !out.quiet {
        // Human summary goes to stderr (data-free command).
        eprintln!(
            "synced: {} scanned, {} ingested, {} skipped, {} issues",
            report.scanned, report.ingested, report.skipped, report.issues
        );
    }

    // Exit 3 if completed but with parse issues (CI can branch on this).
    Ok(if report.issues > 0 { 3 } else { 0 })
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p decant-cli sync_then_reports_json`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/decant-cli
git commit -m "feat(cli): sync command (JSON report, exit 3 on issues)"
```

---

### Task 16: CLI `session ls` (+ `ls` alias)

**Files:**
- Modify: `crates/decant-cli/src/commands/session.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/decant-cli/tests/cli.rs`:

```rust
#[test]
fn ls_json_lists_synced_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());

    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db)
        .args(["session", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"));
}
```

- [ ] **Step 2: Implement `run_ls`**

In `crates/decant-cli/src/commands/session.rs`, replace `run_ls` and add imports at the top:

```rust
use decant_core::{config::Config, db, query};
use decant_core::query::ListFilter;
```

```rust
pub fn run_ls(cli: &Cli, args: &LsArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    let filter = ListFilter { tool: args.tool.clone(), limit: args.limit };
    let sessions = query::list_sessions(&conn, &filter)?;
    let out = cli.output();

    match out.format {
        crate::output::Format::Json => crate::output::print_json(&sessions)?,
        _ => {
            if out.quiet {
                for s in &sessions {
                    println!("{}", s.id);
                }
            } else {
                use comfy_table::{Table, presets::UTF8_FULL};
                let mut table = Table::new();
                table.load_preset(UTF8_FULL);
                table.set_header(["ID", "TOOL", "TITLE", "MODEL", "MSGS", "COST$", "STARTED"]);
                for s in &sessions {
                    table.add_row([
                        s.id.to_string(),
                        s.tool.clone(),
                        s.title.clone().unwrap_or_default(),
                        s.model.clone().unwrap_or_default(),
                        s.message_count.to_string(),
                        format!("{:.2}", s.estimated_cost_usd),
                        s.started_at.clone().unwrap_or_default(),
                    ]);
                }
                println!("{table}");
            }
        }
    }
    Ok(0)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p decant-cli ls_json_lists_synced_sessions`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/decant-cli
git commit -m "feat(cli): session ls (table/json/-q) + ls alias"
```

---

### Task 17: CLI `session show` (+ `show` alias) and `search`

**Files:**
- Modify: `crates/decant-cli/src/commands/session.rs`, `crates/decant-cli/src/commands/search.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/decant-cli/tests/cli.rs`:

```rust
#[test]
fn show_renders_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"))
        .stdout(predicate::str::contains("Read"));
}

#[test]
fn search_finds_text() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).args(["search", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"session_id\""));
}
```

- [ ] **Step 2: Implement `run_show`**

In `crates/decant-cli/src/commands/session.rs`, replace `run_show`:

```rust
pub fn run_show(cli: &Cli, args: &ShowArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    let detail = match query::get_session(&conn, args.id)? {
        Some(d) => d,
        None => {
            eprintln!("error: no session with id {}", args.id);
            return Ok(1);
        }
    };
    let out = cli.output();
    if matches!(out.format, crate::output::Format::Json) {
        crate::output::print_json(&detail)?;
        return Ok(0);
    }

    let s = &detail.summary;
    println!("# {}", s.title.clone().unwrap_or_else(|| s.source_session_id.clone()));
    println!("{} · {} · {} msgs · ${:.2}",
        s.tool, s.model.clone().unwrap_or_default(), s.message_count, s.estimated_cost_usd);
    println!();
    for m in &detail.messages {
        println!("## {}", m.role.to_uppercase());
        for b in &m.blocks {
            match b.block_type.as_str() {
                "text" | "thinking" => {
                    if let Some(t) = &b.text {
                        if b.block_type == "thinking" { println!("_(thinking)_ {t}"); }
                        else { println!("{t}"); }
                    }
                }
                "tool_use" => {
                    println!("→ tool: {} {}",
                        b.tool_name.clone().unwrap_or_default(),
                        b.tool_input.clone().unwrap_or_default());
                }
                "tool_result" => {
                    println!("← result: {}", b.tool_result.clone().unwrap_or_default());
                }
                other => println!("[{other}]"),
            }
        }
        println!();
    }
    Ok(0)
}
```

- [ ] **Step 3: Implement `search`**

Replace `crates/decant-cli/src/commands/search.rs`:

```rust
use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, query};

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Query string (FTS5 syntax supported).
    pub query: String,
    /// Max hits.
    #[arg(long, default_value_t = 30)]
    pub limit: i64,
}

pub fn run(cli: &Cli, args: &SearchArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    let hits = query::search(&conn, &args.query, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&hits)?,
        _ => {
            if hits.is_empty() {
                eprintln!("no matches for {:?}", args.query);
            }
            for h in &hits {
                println!("[{}] {}  —  {}",
                    h.session_id,
                    h.session_title.clone().unwrap_or_default(),
                    h.snippet);
            }
        }
    }
    Ok(0)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p decant-cli`
Expected: PASS (all CLI tests, including `show_renders_transcript` and `search_finds_text`).

- [ ] **Step 5: Commit**

```bash
git add crates/decant-cli
git commit -m "feat(cli): session show + search commands"
```

---

### Task 18: End-to-end smoke, README, justfile

**Files:**
- Create: `justfile`, `README.md` (overwrite the stub)

- [ ] **Step 1: Full workspace test**

Run: `cargo test`
Expected: PASS across both crates.

- [ ] **Step 2: Real smoke test against your actual data**

Run:
```bash
cargo run -p decant-cli -- --db /tmp/decant-smoke.db sync
cargo run -p decant-cli -- --db /tmp/decant-smoke.db session ls --limit 10
cargo run -p decant-cli -- --db /tmp/decant-smoke.db search "error"
```
Expected: sync reports thousands of sessions; `session ls` shows a table; `search` returns hits. (This reads your real `~/.claude` and `~/.codex` — read-only.)

- [ ] **Step 3: Create `justfile`**

```just
# decant developer tasks

build:
    cargo build --release

test:
    cargo test

# Sync your real sessions into the default DB
sync:
    cargo run -p decant-cli -- sync

# List recent sessions
ls *ARGS:
    cargo run -p decant-cli -- session ls {{ARGS}}
```

- [ ] **Step 4: Overwrite `README.md`**

```markdown
# decant

Extract your Claude Code and Codex CLI sessions into a normalized, full-text-searchable
SQLite archive — then browse, search, and (soon) analyze them.

## Status

Plan 1 of 3 (Rust core + CLI). See `docs/superpowers/specs/2026-06-06-decant-design.md`.

## Quick start

```bash
cargo build --release
./target/release/decant sync                 # ingest ~/.claude + ~/.codex
./target/release/decant session ls           # list sessions
./target/release/decant search "auth bug"    # full-text search
./target/release/decant show 1               # read a transcript
./target/release/decant session ls --json    # machine-readable
```

Config via flags or env: `DECANT_DB`, `DECANT_CLAUDE_DIR`, `DECANT_CODEX_DIR`.
```

- [ ] **Step 5: Commit**

```bash
git add justfile README.md
git commit -m "docs: README quickstart + justfile; Plan 1 complete"
```

---

## Spec coverage (Plan 1)

Implemented here: workspace + hermetic SQLite (§2), schema `project/session/message/block/tool_call/block_fts/ingest_source/ingest_issue/model_pricing/schema_migrations` (§4), Claude + Codex parsers and normalization mapping (§3, §4), tool/MCP classification (§5), idempotent parallel sync with discovery + issue logging (§6), cost estimation (§4), UI-agnostic core API returning serde DTOs (§2), and foundational CLI `sync`/`session ls`/`session show`/`search` with `--json`/`-q`/`--no-color`/TTY + semantic exit codes (§7).

Deferred to **Plan 2** (full CLI DevX): `stats`, `mcp ls/stats`, `tool ls/stats`, `export`, `project`, `db migrate/info/vacuum`, `init`, completions, pagination/`$PAGER`, `--dry-run` on every mutating command, did-you-mean polish, parent-message-tree resolution (`message.parent_id`), per-message Codex usage, the clig.dev conformance test, and the `model_pricing` table seed/edit flow.

Deferred to **Plan 3**: the Phoenix LiveView web app (§8).
