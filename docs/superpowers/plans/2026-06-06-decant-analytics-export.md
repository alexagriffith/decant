# decant — Analytics & Export CLI (Plan 2 of N) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add usage/cost analytics and export to the `decant` CLI: `stats`, `tool`, `mcp`, and `export`, backed by new UI-agnostic aggregation + render functions in `decant-core`.

**Architecture:** New `decant-core` modules `stats` (aggregation queries → serde DTOs) and `export` (render a `SessionDetail` to Markdown). Thin `decant-cli` commands over them, reusing the existing `OutputCtx` (table/json/quiet). No schema changes.

**Tech Stack:** Same as Plan 1 — Rust, `rusqlite`, `serde`, `clap`, `comfy-table`, `assert_cmd`.

**This is Plan 2.** It builds on Plan 1 (merged to `main`: core ingest + sync/ls/show/search). **Deferred to Plan 2b (DevX polish):** `project`, `db migrate|info|vacuum`, `init`, shell `completion`, pagination/`$PAGER`, `--dry-run` everywhere, `--format` `ValueEnum` validation, did-you-mean, and the clig.dev conformance test. **Deferred refinements:** cross-file session dedup/merge, Codex `web_search`/`custom`/`tool_search` tool-kind granularity. Spec: `docs/superpowers/specs/2026-06-06-decant-design.md`.

---

## File Structure

```
crates/decant-core/src/
  stats.rs          # NEW: Totals, by_dimension, tool_usage, mcp_usage (aggregation DTOs)
  export.rs         # NEW: to_markdown(&SessionDetail) -> String
  lib.rs            # MODIFY: pub mod stats; pub mod export;
crates/decant-cli/src/
  commands/
    mod.rs          # MODIFY: pub mod stats; pub mod tool; pub mod mcp; pub mod export;
    stats.rs        # NEW
    tool.rs         # NEW
    mcp.rs          # NEW
    export.rs       # NEW
  main.rs           # MODIFY: add Stats/Tool/Mcp/Export to Commands enum + dispatch
  tests/cli.rs      # MODIFY: append tests
```

Run `cargo test` from the repo root unless a more specific command is given. All subagents: do NOT commit (the controller commits signed); implement, run tests, self-review, report.

---

### Task 1: Core — overview stats (`Totals`, `by_dimension`)

**Files:** Create `crates/decant-core/src/stats.rs`; Modify `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Create `crates/decant-core/src/stats.rs`**

```rust
use crate::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct Totals {
    pub sessions: i64,
    pub messages: i64,
    pub tool_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub estimated_cost_usd: f64,
}

/// Whole-archive rollup.
pub fn totals(conn: &Connection) -> Result<Totals> {
    let t = conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM session),
           (SELECT COUNT(*) FROM message),
           (SELECT COUNT(*) FROM tool_call),
           (SELECT COALESCE(SUM(total_input_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_output_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_cache_read_tokens),0) FROM session),
           (SELECT COALESCE(SUM(total_cache_creation_tokens),0) FROM session),
           (SELECT COALESCE(SUM(estimated_cost_usd),0.0) FROM session)",
        [],
        |r| {
            Ok(Totals {
                sessions: r.get(0)?,
                messages: r.get(1)?,
                tool_calls: r.get(2)?,
                input_tokens: r.get(3)?,
                output_tokens: r.get(4)?,
                cache_read_tokens: r.get(5)?,
                cache_creation_tokens: r.get(6)?,
                estimated_cost_usd: r.get(7)?,
            })
        },
    )?;
    Ok(t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Tool,
    Model,
    Project,
    Day,
}

impl Dimension {
    /// Parse the `--by` value. Returns None for unknown (caller reports a clear error).
    pub fn parse(s: &str) -> Option<Dimension> {
        match s {
            "tool" => Some(Dimension::Tool),
            "model" => Some(Dimension::Model),
            "project" => Some(Dimension::Project),
            "day" => Some(Dimension::Day),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DimRow {
    pub key: String,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
}

/// Per-dimension rollup, ordered by session count desc. The grouping expression is
/// chosen from a fixed match (never user text) so the format! is injection-safe.
pub fn by_dimension(conn: &Connection, dim: Dimension) -> Result<Vec<DimRow>> {
    let (group_expr, join) = match dim {
        Dimension::Tool => ("s.tool", ""),
        Dimension::Model => ("COALESCE(s.model, '(unknown)')", ""),
        Dimension::Project => (
            "COALESCE(p.path, '(none)')",
            "LEFT JOIN project p ON p.id = s.project_id",
        ),
        Dimension::Day => ("substr(s.started_at, 1, 10)", ""),
    };
    let sql = format!(
        "SELECT {group_expr} AS k,
                COUNT(*) AS sessions,
                COALESCE(SUM(s.total_input_tokens),0),
                COALESCE(SUM(s.total_output_tokens),0),
                COALESCE(SUM(s.estimated_cost_usd),0.0)
         FROM session s {join}
         GROUP BY k
         ORDER BY sessions DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DimRow {
                key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                sessions: r.get(1)?,
                input_tokens: r.get(2)?,
                output_tokens: r.get(3)?,
                estimated_cost_usd: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
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
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &parsed, "/x.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();
        conn
    }

    #[test]
    fn totals_rollup() {
        let conn = seeded();
        let t = totals(&conn).unwrap();
        assert_eq!(t.sessions, 1);
        assert_eq!(t.messages, 4);
        assert_eq!(t.tool_calls, 1);
        assert_eq!(t.input_tokens, 2700);
    }

    #[test]
    fn by_tool_and_day() {
        let conn = seeded();
        let by_tool = by_dimension(&conn, Dimension::Tool).unwrap();
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool[0].key, "claude_code");
        assert_eq!(by_tool[0].sessions, 1);

        let by_day = by_dimension(&conn, Dimension::Day).unwrap();
        assert_eq!(by_day[0].key, "2026-05-01");
    }

    #[test]
    fn dimension_parse() {
        assert_eq!(Dimension::parse("model"), Some(Dimension::Model));
        assert_eq!(Dimension::parse("nope"), None);
    }
}
```

- [ ] **Step 2: Wire module + run tests**

Add `pub mod stats;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core stats::`
Expected: 3 tests PASS (`totals_rollup`, `by_tool_and_day`, `dimension_parse`).

- [ ] **Step 3: Report** (no commit) — status, test output, files changed.

---

### Task 2: Core — tool & MCP usage aggregations

**Files:** Modify `crates/decant-core/src/stats.rs`

- [ ] **Step 1: Append to `crates/decant-core/src/stats.rs`** (above the `#[cfg(test)]` block)

```rust
#[derive(Debug, Serialize)]
pub struct ToolStatRow {
    pub tool_name: String,
    pub tool_kind: String,
    pub mcp_server: Option<String>,
    pub calls: i64,
    pub errors: i64,
}

/// Per-tool usage, most-called first. `errors_only` keeps only tools with >0 errors.
pub fn tool_usage(conn: &Connection, errors_only: bool, limit: i64) -> Result<Vec<ToolStatRow>> {
    let limit = if limit > 0 { limit } else { 50 };
    let having = if errors_only { "HAVING errors > 0" } else { "" };
    let sql = format!(
        "SELECT tool_name, tool_kind, mcp_server,
                COUNT(*) AS calls,
                COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
         FROM tool_call
         GROUP BY tool_name, tool_kind, mcp_server
         {having}
         ORDER BY calls DESC
         LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ToolStatRow {
                tool_name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tool_kind: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                mcp_server: r.get(2)?,
                calls: r.get(3)?,
                errors: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct McpStatRow {
    pub mcp_server: String,
    pub tools: i64,
    pub calls: i64,
    pub errors: i64,
}

/// Per-MCP-server usage, most-called first.
pub fn mcp_usage(conn: &Connection, limit: i64) -> Result<Vec<McpStatRow>> {
    let limit = if limit > 0 { limit } else { 50 };
    let mut stmt = conn.prepare(
        "SELECT mcp_server,
                COUNT(DISTINCT tool_name) AS tools,
                COUNT(*) AS calls,
                COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0) AS errors
         FROM tool_call
         WHERE tool_kind = 'mcp' AND mcp_server IS NOT NULL
         GROUP BY mcp_server
         ORDER BY calls DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(McpStatRow {
                mcp_server: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tools: r.get(1)?,
                calls: r.get(2)?,
                errors: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
```

- [ ] **Step 2: Add tests inside the existing `#[cfg(test)] mod tests` block in `stats.rs`**

```rust
    #[test]
    fn tool_usage_counts_builtin_call() {
        let conn = seeded();
        let tools = tool_usage(&conn, false, 50).unwrap();
        // fixture has one tool_use: builtin "Read"
        let read = tools.iter().find(|t| t.tool_name == "Read").unwrap();
        assert_eq!(read.tool_kind, "builtin");
        assert_eq!(read.calls, 1);
        assert_eq!(read.errors, 0);

        // errors_only filters it out (no errors in the fixture)
        assert!(tool_usage(&conn, true, 50).unwrap().is_empty());
    }

    #[test]
    fn mcp_usage_empty_for_builtin_only_fixture() {
        let conn = seeded();
        assert!(mcp_usage(&conn, 50).unwrap().is_empty());
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p decant-core stats::`
Expected: 5 tests PASS (3 from Task 1 + the 2 new).

- [ ] **Step 4: Report** (no commit).

---

### Task 3: Core — Markdown export renderer

**Files:** Create `crates/decant-core/src/export.rs`; Modify `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Create `crates/decant-core/src/export.rs`**

```rust
use crate::query::SessionDetail;
use std::fmt::Write;

/// Render a full session transcript to Markdown. UI-agnostic (no IO), so the CLI
/// and the future web/macOS apps can all reuse it.
pub fn to_markdown(detail: &SessionDetail) -> String {
    let s = &detail.summary;
    let mut out = String::new();
    let title = s.title.clone().unwrap_or_else(|| s.source_session_id.clone());
    let _ = writeln!(out, "# {title}\n");
    let _ = writeln!(
        out,
        "- **tool:** {}\n- **model:** {}\n- **messages:** {}\n- **est. cost:** ${:.2}\n- **started:** {}\n",
        s.tool,
        s.model.clone().unwrap_or_default(),
        s.message_count,
        s.estimated_cost_usd,
        s.started_at.clone().unwrap_or_default(),
    );

    for m in &detail.messages {
        let _ = writeln!(out, "## {}\n", m.role.to_uppercase());
        for b in &m.blocks {
            match b.block_type.as_str() {
                "text" => {
                    if let Some(t) = &b.text {
                        let _ = writeln!(out, "{t}\n");
                    }
                }
                "thinking" => {
                    if let Some(t) = &b.text {
                        let _ = writeln!(out, "> _thinking:_ {t}\n");
                    }
                }
                "tool_use" => {
                    let _ = writeln!(
                        out,
                        "**\u{2192} {}**\n\n```json\n{}\n```\n",
                        b.tool_name.clone().unwrap_or_default(),
                        b.tool_input.clone().unwrap_or_default(),
                    );
                }
                "tool_result" => {
                    let _ = writeln!(out, "```\n{}\n```\n", b.tool_result.clone().unwrap_or_default());
                }
                other => {
                    let _ = writeln!(out, "_[{other}]_\n");
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, query, schema, sources};

    #[test]
    fn renders_markdown_transcript() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &content);
        let tx = conn.unchecked_transaction().unwrap();
        let id = ingest::upsert_session(&tx, &parsed, "/x.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();

        let detail = query::get_session(&conn, id).unwrap().unwrap();
        let md = to_markdown(&detail);
        assert!(md.starts_with("# Fix the failing auth test"));
        assert!(md.contains("## USER"));
        assert!(md.contains("Read"));
        assert!(md.contains("_thinking:_"));
    }
}
```

- [ ] **Step 2: Wire module + run tests**

Add `pub mod export;` to `crates/decant-core/src/lib.rs`.

Run: `cargo test -p decant-core export::`
Expected: `renders_markdown_transcript` PASSES. Also `cargo test -p decant-core` (all green).

- [ ] **Step 3: Report** (no commit).

---

### Task 4: CLI — `stats` command

**Files:** Modify `crates/decant-cli/src/commands/mod.rs`, `crates/decant-cli/src/main.rs`; Create `crates/decant-cli/src/commands/stats.rs`; Modify `tests/cli.rs`

- [ ] **Step 1: Create `crates/decant-cli/src/commands/stats.rs`**

```rust
use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, schema, stats};

#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Break down by: tool | model | project | day. Omit for the overall rollup.
    #[arg(long)]
    pub by: Option<String>,
}

pub fn run(cli: &Cli, args: &StatsArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let out = cli.output();

    if let Some(by) = &args.by {
        let dim = match stats::Dimension::parse(by) {
            Some(d) => d,
            None => {
                eprintln!("error: unknown --by value {by:?} (expected: tool | model | project | day)");
                return Ok(2);
            }
        };
        let rows = stats::by_dimension(&conn, dim)?;
        match out.format {
            crate::output::Format::Json => crate::output::print_json(&rows)?,
            _ => {
                use comfy_table::{presets::UTF8_FULL, Table};
                let mut table = Table::new();
                table.load_preset(UTF8_FULL);
                table.set_header([by.to_uppercase().as_str(), "SESSIONS", "IN_TOK", "OUT_TOK", "COST$"]);
                for r in &rows {
                    table.add_row([
                        r.key.clone(),
                        r.sessions.to_string(),
                        r.input_tokens.to_string(),
                        r.output_tokens.to_string(),
                        format!("{:.2}", r.estimated_cost_usd),
                    ]);
                }
                println!("{table}");
            }
        }
    } else {
        let t = stats::totals(&conn)?;
        match out.format {
            crate::output::Format::Json => crate::output::print_json(&t)?,
            _ => {
                println!("sessions:   {}", t.sessions);
                println!("messages:   {}", t.messages);
                println!("tool calls: {}", t.tool_calls);
                println!("input tok:  {}", t.input_tokens);
                println!("output tok: {}", t.output_tokens);
                println!("est. cost:  ${:.2}", t.estimated_cost_usd);
            }
        }
    }
    Ok(0)
}
```

- [ ] **Step 2: Wire into the command tree**

Add `pub mod stats;` to `crates/decant-cli/src/commands/mod.rs`.

In `crates/decant-cli/src/main.rs`, add a variant to the `Commands` enum:
```rust
    /// Usage & cost rollups (overall, or --by tool|model|project|day).
    Stats(commands::stats::StatsArgs),
```
and a dispatch arm in `run`:
```rust
        Commands::Stats(args) => commands::stats::run(cli, args),
```

- [ ] **Step 3: Add test to `crates/decant-cli/tests/cli.rs`** (reuse `write_fixture_tree`)

```rust
#[test]
fn stats_overall_and_by_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).arg("stats")
        .assert().success()
        .stdout(predicate::str::contains("\"sessions\""));

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).args(["stats", "--by", "tool"])
        .assert().success()
        .stdout(predicate::str::contains("claude_code"));

    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).args(["stats", "--by", "bogus"])
        .assert().code(2);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p decant-cli`
Expected: prior CLI tests + `stats_overall_and_by_tool` PASS.

- [ ] **Step 5: Report** (no commit).

---

### Task 5: CLI — `tool` command (`tool ls` / `tool stats`)

**Files:** Create `crates/decant-cli/src/commands/tool.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: Create `crates/decant-cli/src/commands/tool.rs`**

```rust
use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, schema, stats};

#[derive(Subcommand, Debug)]
pub enum ToolCmd {
    /// List tools by call count (alias of `stats`).
    Ls(ToolArgs),
    /// Tool usage stats (calls, errors), most-used first.
    Stats(ToolArgs),
}

#[derive(Args, Debug)]
pub struct ToolArgs {
    /// Only tools that have at least one error.
    #[arg(long)]
    pub errors_only: bool,
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

pub fn run(cli: &Cli, cmd: &ToolCmd) -> anyhow::Result<i32> {
    let args = match cmd {
        ToolCmd::Ls(a) | ToolCmd::Stats(a) => a,
    };
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = stats::tool_usage(&conn, args.errors_only, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["TOOL", "KIND", "SERVER", "CALLS", "ERRORS"]);
            for r in &rows {
                table.add_row([
                    r.tool_name.clone(),
                    r.tool_kind.clone(),
                    r.mcp_server.clone().unwrap_or_default(),
                    r.calls.to_string(),
                    r.errors.to_string(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
```

- [ ] **Step 2: Wire into the command tree**

Add `pub mod tool;` to `commands/mod.rs`. In `main.rs` `Commands`:
```rust
    /// Tool usage (built-in vs MCP): `tool ls` / `tool stats`.
    #[command(subcommand)]
    Tool(commands::tool::ToolCmd),
```
Dispatch arm:
```rust
        Commands::Tool(cmd) => commands::tool::run(cli, cmd),
```

- [ ] **Step 3: Add test to `tests/cli.rs`**

```rust
#[test]
fn tool_stats_lists_read() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).args(["tool", "stats"])
        .assert().success()
        .stdout(predicate::str::contains("Read"))
        .stdout(predicate::str::contains("\"calls\""));
}
```

- [ ] **Step 4: Run tests** — `cargo test -p decant-cli` (new test passes).
- [ ] **Step 5: Report** (no commit).

---

### Task 6: CLI — `mcp` command (`mcp ls` / `mcp stats`)

**Files:** Create `crates/decant-cli/src/commands/mcp.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: Create `crates/decant-cli/src/commands/mcp.rs`**

```rust
use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, schema, stats};

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// List MCP servers by call count.
    Ls(McpArgs),
    /// MCP server stats (tools, calls, errors), most-used first.
    Stats(McpArgs),
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

pub fn run(cli: &Cli, cmd: &McpCmd) -> anyhow::Result<i32> {
    let args = match cmd {
        McpCmd::Ls(a) | McpCmd::Stats(a) => a,
    };
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = stats::mcp_usage(&conn, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            if rows.is_empty() {
                eprintln!("no MCP tool calls recorded");
            }
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["SERVER", "TOOLS", "CALLS", "ERRORS"]);
            for r in &rows {
                table.add_row([
                    r.mcp_server.clone(),
                    r.tools.to_string(),
                    r.calls.to_string(),
                    r.errors.to_string(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
```

- [ ] **Step 2: Wire into the command tree**

Add `pub mod mcp;` to `commands/mod.rs`. In `main.rs` `Commands`:
```rust
    /// MCP server usage: `mcp ls` / `mcp stats`.
    #[command(subcommand)]
    Mcp(commands::mcp::McpCmd),
```
Dispatch arm:
```rust
        Commands::Mcp(cmd) => commands::mcp::run(cli, cmd),
```

- [ ] **Step 3: Add test to `tests/cli.rs`**

```rust
#[test]
fn mcp_stats_runs_and_is_json_array() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    // Fixture has no MCP calls -> empty JSON array, still exit 0.
    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).args(["mcp", "stats"])
        .assert().success()
        .stdout(predicate::str::contains("[]"));
}
```

- [ ] **Step 4: Run tests** — `cargo test -p decant-cli` (new test passes).
- [ ] **Step 5: Report** (no commit).

---

### Task 7: CLI — `export` command

**Files:** Create `crates/decant-cli/src/commands/export.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: Create `crates/decant-cli/src/commands/export.rs`**

```rust
use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, export, query, schema};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Session id to export. Omit with --all to export everything.
    pub id: Option<i64>,
    /// Export every session.
    #[arg(long)]
    pub all: bool,
    /// Output directory (required for --all). For a single session, omit to write to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(cli: &Cli, args: &ExportArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let json = matches!(cli.output().format, crate::output::Format::Json);
    let ext = if json { "json" } else { "md" };

    let render = |detail: &query::SessionDetail| -> anyhow::Result<String> {
        Ok(if json {
            serde_json::to_string_pretty(detail)?
        } else {
            export::to_markdown(detail)
        })
    };

    if args.all {
        let dir = match &args.out {
            Some(d) => d.clone(),
            None => {
                eprintln!("error: --all requires --out <dir>");
                return Ok(2);
            }
        };
        std::fs::create_dir_all(&dir)?;
        let summaries = query::list_sessions(&conn, &query::ListFilter { tool: None, limit: i64::MAX })?;
        let mut n = 0;
        for s in &summaries {
            if let Some(detail) = query::get_session(&conn, s.id)? {
                let path = dir.join(format!("{}.{}", s.id, ext));
                std::fs::write(&path, render(&detail)?)?;
                n += 1;
            }
        }
        eprintln!("exported {n} sessions to {}", dir.display());
        return Ok(0);
    }

    let id = match args.id {
        Some(id) => id,
        None => {
            eprintln!("error: provide a session id, or --all --out <dir>");
            return Ok(2);
        }
    };
    let detail = match query::get_session(&conn, id)? {
        Some(d) => d,
        None => {
            eprintln!("error: no session with id {id}");
            return Ok(1);
        }
    };
    let content = render(&detail)?;
    match &args.out {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("{id}.{ext}"));
            std::fs::write(&path, content)?;
            eprintln!("wrote {}", path.display());
        }
        None => println!("{content}"),
    }
    Ok(0)
}
```

- [ ] **Step 2: Wire into the command tree**

Add `pub mod export;` to `commands/mod.rs`. In `main.rs` `Commands`:
```rust
    /// Export a session (or --all) to Markdown or JSON.
    Export(commands::export::ExportArgs),
```
Dispatch arm:
```rust
        Commands::Export(args) => commands::export::run(cli, args),
```
Note: `query::ListFilter` and `query::SessionDetail`/`list_sessions`/`get_session` are already public from Plan 1.

- [ ] **Step 3: Add test to `tests/cli.rs`**

```rust
#[test]
fn export_session_to_markdown_stdout_and_all_to_dir() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    // single session -> markdown to stdout
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).args(["export", "1"])
        .assert().success()
        .stdout(predicate::str::contains("# Fix the failing auth test"));

    // --all -> files in a dir
    let outdir = dir.path().join("export");
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).args(["export", "--all", "--out"]).arg(&outdir)
        .assert().success();
    assert!(outdir.join("1.md").exists());
}
```

- [ ] **Step 4: Run tests** — `cargo test -p decant-cli` (new test passes).
- [ ] **Step 5: Report** (no commit).

---

### Task 8: Verify, real-data smoke, README, commit

**Files:** Modify `README.md`

- [ ] **Step 1: Full workspace test**

Run: `cargo test` — all pass (core: 5 stats + 1 export + prior 23 = 29; CLI: prior 7 + 4 new = 11).

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets` — no warnings (fix any that appear).

- [ ] **Step 3: Real-data smoke** (controller runs against the synced smoke DB)

```bash
cargo build --release
./target/release/decant --db /tmp/decant-smoke.db sync   # if not already populated
./target/release/decant --db /tmp/decant-smoke.db stats
./target/release/decant --db /tmp/decant-smoke.db stats --by model
./target/release/decant --db /tmp/decant-smoke.db mcp stats
./target/release/decant --db /tmp/decant-smoke.db tool stats --errors-only
./target/release/decant --db /tmp/decant-smoke.db export 1 | head
```
Expected: rollups + an MCP-server leaderboard + tool error rates over real data.

- [ ] **Step 4: Update `README.md`** — add the new commands under Quick start:
```markdown
./target/release/decant stats                # usage & cost rollup
./target/release/decant stats --by model     # break down by tool|model|project|day
./target/release/decant mcp stats            # MCP server leaderboard
./target/release/decant tool stats           # tool usage (built-in vs MCP) + error counts
./target/release/decant export 1 > s1.md     # export a transcript (Markdown or --json)
```

- [ ] **Step 5: Commit** (controller, signed)

```bash
git add -A
git commit -S -m "feat: analytics (stats/tool/mcp) + export CLI (Plan 2)"
```

---

## Spec coverage (Plan 2)

Implements the **usage & cost analytics** and **export** use-cases at the CLI (spec §1, §7, §8 CLI portions): `stats` (overall + by tool/model/project/day), `tool ls|stats` (built-in vs MCP, error counts), `mcp ls|stats` (server leaderboard, tools-per-server, errors), `export` (Markdown/JSON, single or `--all`). New UI-agnostic `decant-core` `stats` + `export` modules keep the analytics reusable by the future Phoenix/macOS surfaces.

**Deferred to Plan 2b:** `project`, `db migrate|info|vacuum`, `init`, shell `completion`, pagination/`$PAGER`, `--dry-run` on all mutating commands, `--format` `ValueEnum` validation + did-you-mean, the clig.dev conformance test. **Refinements:** cross-file session dedup/merge, Codex `web_search`/`custom`/`tool_search` tool-kind granularity. **Plan 3:** Phoenix LiveView web app.
