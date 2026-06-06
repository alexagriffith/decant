# decant — CLI DevX Polish (Plan 2b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Round out the `decant` CLI's developer experience: validated `--format`, shell `completion`, `db info|vacuum|migrate`, `project ls`, and a clig.dev **conformance test** that keeps every command well-documented.

**Architecture:** Mostly thin CLI additions over existing `decant-core`, plus one new core read query (`list_projects`). No schema changes.

**Tech Stack:** Same as Plans 1–2; adds `clap_complete`.

**This is Plan 2b.** Builds on Plans 1–2 (merged to `main`). **Deferred to a later pass:** `decant init` + config-file (`~/.config/decant/config.toml`) support, pagination (`--page-size`/`--max-items`/`$PAGER`), `project show`. **Plan 3:** Phoenix LiveView web app. Spec: `docs/superpowers/specs/2026-06-06-decant-design.md`.

---

## File Structure

```
crates/decant-core/src/query.rs   # MODIFY: add ProjectSummary + list_projects
crates/decant-cli/src/
  output.rs                        # MODIFY: Format derives ValueEnum; OutputCtx::new takes Option<Format>
  main.rs                          # MODIFY: --format Option<Format>; add Completion/Db/Project; conformance tests
  commands/
    mod.rs                         # MODIFY: pub mod completion; db; project;
    completion.rs                  # NEW
    db.rs                          # NEW
    project.rs                     # NEW
  tests/cli.rs                     # MODIFY: append tests
```

All subagents: do NOT commit (controller commits signed). Run `cargo test` from repo root.

---

### Task 1: `--format` as a validated `ValueEnum`

**Files:** Modify `crates/decant-cli/src/output.rs`, `crates/decant-cli/src/main.rs`, `crates/decant-cli/tests/cli.rs`

- [ ] **Step 1: `output.rs` — derive `ValueEnum` on `Format`, change `OutputCtx::new`**

Replace the `Format` enum definition and the `OutputCtx::new` impl with:
```rust
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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
    pub fn new(json: bool, format: Option<Format>, no_color: bool, quiet: bool) -> Self {
        let format = if json { Format::Json } else { format.unwrap_or(Format::Table) };
        let color = should_color(no_color);
        OutputCtx { format, color, quiet }
    }
}
```
(Keep `should_color` and `print_json` as-is. Keep `use std::io::IsTerminal;` at the top.)

- [ ] **Step 2: `main.rs` — make the global `--format` an `Option<Format>`**

Change the `format` field on `Cli` to:
```rust
    /// Output format.
    #[arg(long, global = true, value_enum)]
    pub format: Option<output::Format>,
```
And change `Cli::output` to pass it by value:
```rust
    pub fn output(&self) -> output::OutputCtx {
        output::OutputCtx::new(self.json, self.format, self.no_color, self.quiet)
    }
```

- [ ] **Step 3: Test — invalid `--format` is rejected (exit 2)**

Append to `tests/cli.rs`:
```rust
#[test]
fn invalid_format_is_rejected() {
    Command::cargo_bin("decant").unwrap()
        .args(["--format", "bogus", "session", "ls"])
        .assert()
        .code(2);
}
```

- [ ] **Step 4: Run** `cargo test -p decant-cli` (all prior + this pass) and `cargo build` (clean). Report (no commit).

---

### Task 2: `decant completion <shell>`

**Files:** Create `crates/decant-cli/src/commands/completion.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: Add dep** — `cargo add --package decant-cli clap_complete`

- [ ] **Step 2: Create `crates/decant-cli/src/commands/completion.rs`**
```rust
use crate::Cli;
use clap::{Args, CommandFactory};
use clap_complete::Shell;

#[derive(Args, Debug)]
pub struct CompletionArgs {
    /// Shell to generate a completion script for.
    #[arg(value_enum)]
    pub shell: Shell,
}

pub fn run(_cli: &Cli, args: &CompletionArgs) -> anyhow::Result<i32> {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(0)
}
```

- [ ] **Step 3: Wire** — add `pub mod completion;` to `commands/mod.rs`; in `main.rs` `Commands`:
```rust
    /// Generate a shell completion script (bash|zsh|fish|...).
    Completion(commands::completion::CompletionArgs),
```
Dispatch arm:
```rust
        Commands::Completion(args) => commands::completion::run(cli, args),
```

- [ ] **Step 4: Test** — append to `tests/cli.rs`:
```rust
#[test]
fn completion_bash_generates_script() {
    Command::cargo_bin("decant").unwrap()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("decant"));
}
```

- [ ] **Step 5: Run** `cargo test -p decant-cli` + `cargo build` (clean). Report (no commit).

---

### Task 3: `decant db info | vacuum | migrate`

**Files:** Create `crates/decant-cli/src/commands/db.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: Create `crates/decant-cli/src/commands/db.rs`**
```rust
use crate::Cli;
use clap::Subcommand;
use decant_core::{config::Config, db, schema};

#[derive(Subcommand, Debug)]
pub enum DbCmd {
    /// Show DB path, size, schema version, and row counts.
    Info,
    /// Reclaim free space (VACUUM).
    Vacuum,
    /// Apply schema migrations explicitly (sync also does this automatically).
    Migrate,
}

pub fn run(cli: &Cli, cmd: &DbCmd) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    match cmd {
        DbCmd::Migrate => eprintln!("schema up to date at {}", config.db_path.display()),
        DbCmd::Vacuum => {
            conn.execute_batch("VACUUM;")?;
            eprintln!("vacuumed {}", config.db_path.display());
        }
        DbCmd::Info => {
            let size = std::fs::metadata(&config.db_path).map(|m| m.len()).unwrap_or(0);
            let version: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            let (sessions, messages, tool_calls): (i64, i64, i64) = conn.query_row(
                "SELECT (SELECT COUNT(*) FROM session),
                        (SELECT COUNT(*) FROM message),
                        (SELECT COUNT(*) FROM tool_call)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            println!("path:       {}", config.db_path.display());
            println!("size_bytes: {size}");
            println!("schema:     v{version}");
            println!("sessions:   {sessions}");
            println!("messages:   {messages}");
            println!("tool_calls: {tool_calls}");
        }
    }
    Ok(0)
}
```

- [ ] **Step 2: Wire** — `pub mod db;` in `commands/mod.rs`; in `main.rs` `Commands`:
```rust
    /// Database maintenance: `db info` / `db vacuum` / `db migrate`.
    #[command(subcommand)]
    Db(commands::db::DbCmd),
```
Dispatch arm:
```rust
        Commands::Db(cmd) => commands::db::run(cli, cmd),
```

- [ ] **Step 3: Test** — append to `tests/cli.rs`:
```rust
#[test]
fn db_info_reports_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).args(["db", "info"])
        .assert().success()
        .stdout(predicate::str::contains("schema:     v1"))
        .stdout(predicate::str::contains("sessions:   1"));
}
```

- [ ] **Step 4: Run** `cargo test -p decant-cli` + `cargo build` (clean). Report (no commit).

---

### Task 4: `project ls` (core `list_projects` + CLI)

**Files:** Modify `crates/decant-core/src/query.rs`; Create `crates/decant-cli/src/commands/project.rs`; Modify `commands/mod.rs`, `main.rs`, `tests/cli.rs`

- [ ] **Step 1: `query.rs` — add `ProjectSummary` + `list_projects`** (append above the `#[cfg(test)]` block)
```rust
#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
    pub sessions: i64,
    pub estimated_cost_usd: f64,
    pub last_seen_at: Option<String>,
}

pub fn list_projects(conn: &Connection) -> Result<Vec<ProjectSummary>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.path, p.name,
                COUNT(s.id) AS sessions,
                COALESCE(SUM(s.estimated_cost_usd), 0.0) AS cost,
                MAX(s.ended_at) AS last_seen
         FROM project p
         LEFT JOIN session s ON s.project_id = p.id
         GROUP BY p.id, p.path, p.name
         ORDER BY sessions DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProjectSummary {
                id: r.get(0)?,
                path: r.get(1)?,
                name: r.get(2)?,
                sessions: r.get(3)?,
                estimated_cost_usd: r.get(4)?,
                last_seen_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
```

- [ ] **Step 2: Add a core test** inside `query.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn list_projects_rolls_up() {
        let conn = seeded();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sessions, 1);
        assert_eq!(projects[0].path, "/Users/dev/proj");
    }
```

- [ ] **Step 3: Create `crates/decant-cli/src/commands/project.rs`**
```rust
use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, query, schema};

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    /// List projects with session counts and cost.
    Ls(ProjectArgs),
}

#[derive(Args, Debug)]
pub struct ProjectArgs {}

pub fn run(cli: &Cli, _cmd: &ProjectCmd) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = query::list_projects(&conn)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["ID", "PROJECT", "SESSIONS", "COST$", "LAST"]);
            for r in &rows {
                table.add_row([
                    r.id.to_string(),
                    r.path.clone(),
                    r.sessions.to_string(),
                    format!("{:.2}", r.estimated_cost_usd),
                    r.last_seen_at.clone().unwrap_or_default(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
```

- [ ] **Step 4: Wire** — `pub mod project;` in `commands/mod.rs`; in `main.rs` `Commands`:
```rust
    /// Projects (workspaces): `project ls`.
    #[command(subcommand)]
    Project(commands::project::ProjectCmd),
```
Dispatch arm:
```rust
        Commands::Project(cmd) => commands::project::run(cli, cmd),
```

- [ ] **Step 5: Test** — append to `tests/cli.rs`:
```rust
#[test]
fn project_ls_shows_project() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant").unwrap()
        .args(["--db"]).arg(&db).arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir).env("DECANT_CODEX_DIR", &codex_dir)
        .assert().success();

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db).args(["project", "ls"])
        .assert().success()
        .stdout(predicate::str::contains("/Users/dev/proj"));
}
```

- [ ] **Step 6: Run** `cargo test` (core project test + CLI test pass) + `cargo build` (clean). Report (no commit).

---

### Task 5: clig.dev conformance tests

**Files:** Modify `crates/decant-cli/src/main.rs`

- [ ] **Step 1: Add a conformance test module to `main.rs`** (at the end of the file)
```rust
#[cfg(test)]
mod conformance {
    use super::*;
    use clap::CommandFactory;

    /// Every (sub)command must carry help text (`about`).
    fn assert_has_help(cmd: &clap::Command) {
        for sub in cmd.get_subcommands() {
            assert!(
                sub.get_about().is_some(),
                "command `{}` is missing help/about text",
                sub.get_name()
            );
            assert_has_help(sub);
        }
    }

    #[test]
    fn every_command_has_help() {
        assert_has_help(&Cli::command());
    }

    #[test]
    fn global_flags_present() {
        let cmd = Cli::command();
        let ids: Vec<String> = cmd.get_arguments().map(|a| a.get_id().as_str().to_string()).collect();
        for flag in ["db", "json", "format", "quiet", "no_color"] {
            assert!(ids.contains(&flag.to_string()), "missing global flag --{flag}");
        }
    }
}
```

- [ ] **Step 2: Run** `cargo test -p decant-cli` — `every_command_has_help` and `global_flags_present` PASS (they enforce that every command we added has `about` text and the globals exist). If `every_command_has_help` fails, add the missing `///` doc comment to that command's enum variant.

- [ ] **Step 3: Report** (no commit).

---

### Task 6: Verify, smoke, README, commit

- [ ] **Step 1:** `cargo test` — all pass (core: 29 + 1 project = 30; CLI integration: prior 12 + 4 new = 16; bin conformance: 2).
- [ ] **Step 2:** `cargo clippy --all-targets` — clean.
- [ ] **Step 3: Real-data smoke** (controller):
```bash
cargo build --release
./target/release/decant --db /tmp/decant-smoke.db db info
./target/release/decant --db /tmp/decant-smoke.db project ls | head
./target/release/decant completion zsh | head -3
./target/release/decant --format bogus session ls   # expect clap error, exit 2
```
- [ ] **Step 4: README** — add under Quick start:
```markdown
./target/release/decant project ls           # projects by session count + cost
./target/release/decant db info               # db path, size, schema version, counts
./target/release/decant completion zsh        # shell completion script
```
- [ ] **Step 5: Commit** (controller, signed):
```bash
git add -A && git commit -S -m "feat(cli): --format validation, completion, db, project + conformance tests (Plan 2b)"
```

---

## Spec coverage (Plan 2b)

Implements the CLI DevX items from the spec §7: validated `--format` (`ValueEnum`), shell `completion` (clig.dev autocompletion), `db info|vacuum|migrate` (maintenance), `project ls` (browse by workspace), and a **conformance test** enforcing that every command is documented (clig.dev "detailed help") and the global flags exist.

**Deferred:** `decant init` + config-file support, pagination/`$PAGER`, `project show`, did-you-mean beyond clap's defaults. **Plan 3:** Phoenix LiveView web app.
