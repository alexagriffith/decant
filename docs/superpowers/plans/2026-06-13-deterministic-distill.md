# Deterministic Distillation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate runnable artifacts (workflow scripts, session replays, skill/AGENTS.md files) deterministically from the session archive, via a `decant distill` command family.

**Architecture:** One pure extractor in `decant-core` (`distill`) turns selected sessions into a normalized, success-tagged, secret-redacted `Vec<Op>`; three renderers turn that into text artifacts. The `decant-cli` `distill` command does all I/O (stdout / `--json` / `-o FILE`), opening the DB read-only like `stats`/`files`. No schema change, no LLM, no network.

**Tech Stack:** Rust (rusqlite, serde, clap, comfy_table not needed — text output). Spec: `docs/superpowers/specs/2026-06-13-deterministic-distillation-design.md`.

---

## File Structure

- **Create** `crates/decant-core/src/distill.rs` — extractor + pure helpers (`decode_command`, `normalize`, `redact`, `classify_phase`, `is_destructive`), `timeline`, `render_script`, `render_replay`, `render_skill`, `hot_context`. All pure / DB-read; returns data + `String` artifacts. No printing.
- **Modify** `crates/decant-core/src/lib.rs` — `pub mod distill;`
- **Create** `crates/decant-cli/src/commands/distill.rs` — clap `DistillCmd` (`script`/`replay`/`skill`), `run`, stdout/`--json`/`-o`.
- **Modify** `crates/decant-cli/src/commands/mod.rs` — `pub mod distill;`
- **Modify** `crates/decant-cli/src/main.rs` — add `Distill` to `Commands` enum + dispatch.
- **Modify** `crates/decant-core/src/recommendations.rs` — populate the `signal:hot-context` card `action` with a `decant distill skill …` command (Task C4).
- **Create** `fixtures/claude/distill.jsonl`, `fixtures/codex/distill.jsonl` — synthetic multi-op sessions for tests.

**Conventions to follow:** core fns take `&Connection`, return `crate::Result<T>`; fixed-match SQL via `format!` (injection-safe, see `stats::file_hotspots`); comments terse (one line, only for non-obvious constraints); every CLI subcommand needs `about` text (conformance test in `main.rs`).

---

## Phase A — Extractor + `distill script` (anchor)

### Task A1: Test fixtures

**Files:** Create `fixtures/claude/distill.jsonl`, `fixtures/codex/distill.jsonl`

- [ ] **Step 1: Create the Claude fixture** — one session with: two `Bash` calls that recur (`cargo build`, `cargo test`), one `Bash` carrying a secret (`gh auth login --with-token=ghp_AAAitsasecrettokenvalue000000000000`), one destructive `Bash` (`rm -rf target`), one `Write` (file_path + content), one `Edit` (file_path + old_string/new_string), one errored `Bash` (tool_result `is_error:true`). Mirror the structure of `fixtures/claude/enriched.jsonl` (use it as a template). Set a stable `cwd` (`/Users/dev/proj`) via the session's first record.

- [ ] **Step 2: Create the Codex fixture** — one session with two `exec_command` calls (`arguments` is a JSON string: `"{\"cmd\":\"cargo build\",\"workdir\":\"/Users/dev/proj\"}"`), and one `apply_patch` (`arguments` = a `*** Begin Patch / *** Add File: foo.txt … *** End Patch`). Mirror `fixtures/codex/enriched.jsonl`.

- [ ] **Step 3: Commit**
```bash
git add fixtures/claude/distill.jsonl fixtures/codex/distill.jsonl
git commit -m "test(distill): synthetic fixtures for distillation"
```

### Task A2: Module skeleton + types

**Files:** Create `crates/decant-core/src/distill.rs`; Modify `crates/decant-core/src/lib.rs`

- [ ] **Step 1: Write the failing test** (in `distill.rs` `#[cfg(test)] mod tests`)
```rust
#[test]
fn phase_enum_serializes_lowercase_label() {
    assert_eq!(Phase::Build.as_str(), "build");
    assert_eq!(Phase::Other.as_str(), "other");
}
```

- [ ] **Step 2: Run, expect fail** — `cargo test -p decant-core distill::tests::phase_enum` → FAIL (module not found).

- [ ] **Step 3: Implement types**
```rust
use crate::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind { Command, FileWrite, FileEdit, FileDelete, Patch }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase { Setup, Build, Test, Lint, Vcs, Deploy, Run, Other }

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Setup => "setup", Phase::Build => "build", Phase::Test => "test",
            Phase::Lint => "lint", Phase::Vcs => "vcs", Phase::Deploy => "deploy",
            Phase::Run => "run", Phase::Other => "other",
        }
    }
    /// Stable display order for grouping.
    pub fn order(self) -> u8 {
        match self {
            Phase::Setup=>0, Phase::Build=>1, Phase::Test=>2, Phase::Lint=>3,
            Phase::Run=>4, Phase::Deploy=>5, Phase::Vcs=>6, Phase::Other=>7,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Op {
    pub session_id: i64,
    pub ordinal: i64,
    pub kind: OpKind,
    pub raw: String,           // decoded command, or rel_path for file ops
    pub normalized: String,    // cwd→$PROJECT_ROOT, secrets redacted
    pub payload: Option<String>, // file content / patch (file ops); None for commands
    pub phase: Phase,
    pub is_error: bool,
    pub redacted: bool,
    pub sessions_seen: u32,
    pub success_rate: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Scope {
    pub project: Option<String>,
    pub work_type: Option<String>,
    pub from_session: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Distillation {
    pub scope_label: String,
    pub ops: Vec<Op>,
    pub session_count: u32,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub generated_with: String,
}
```
Then `lib.rs`: add `pub mod distill;` (alphabetical, after `db`).

- [ ] **Step 4: Run, expect pass** — `cargo test -p decant-core distill::tests::phase_enum` → PASS.

- [ ] **Step 5: Commit** — `git commit -am "feat(distill): core module skeleton + types"`

### Task A3: `decode_command` (per-source)

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn decode_claude_bash() {
    let input = r#"{"command":"cargo build","description":"build"}"#;
    assert_eq!(decode_command("Bash", Some(input)).as_deref(), Some("cargo build"));
}
#[test]
fn decode_codex_exec_json_string() {
    // ingest stores Value::String(...).to_string() → a quoted JSON string literal
    let inner = r#"{"cmd":"cargo test","workdir":"/Users/dev/proj"}"#;
    let stored = serde_json::Value::String(inner.to_string()).to_string();
    assert_eq!(decode_command("exec_command", Some(&stored)).as_deref(), Some("cargo test"));
}
#[test]
fn decode_non_command_is_none() {
    assert_eq!(decode_command("Read", Some(r#"{"file_path":"a"}"#)), None);
    assert_eq!(decode_command("Bash", None), None);
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement**
```rust
/// Decode a shell command from a tool_call.input payload. Claude `Bash` →
/// `command`; Codex `exec_command` → the JSON-string-encoded `cmd`. None for
/// non-command tools or unparseable input.
pub fn decode_command(tool_name: &str, input: Option<&str>) -> Option<String> {
    let input = input?;
    let mut v: serde_json::Value = serde_json::from_str(input).ok()?;
    if let serde_json::Value::String(s) = v {          // Codex: unwrap one JSON-string level
        v = serde_json::from_str(&s).ok()?;
    }
    let key = match tool_name {
        "Bash" => "command",
        "exec_command" | "shell" | "local_shell" => "cmd",
        _ => return None,
    };
    match v.get(key).or_else(|| v.get("command")) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(a)) => {          // defensive: ["bash","-lc","…"]
            let parts: Vec<&str> = a.iter().filter_map(|x| x.as_str()).collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): decode_command for Claude Bash + Codex exec_command"`

### Task A4: `normalize`

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn normalize_strips_cwd_and_home() {
    let s = normalize("cd /Users/dev/proj/web && ls /Users/dev/proj", Some("/Users/dev/proj"));
    assert_eq!(s, "cd $PROJECT_ROOT/web && ls $PROJECT_ROOT");
}
#[test]
fn normalize_collapses_whitespace() {
    assert_eq!(normalize("cargo    test   --workspace", None), "cargo test --workspace");
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement**
```rust
/// Replace the project cwd with $PROJECT_ROOT and collapse whitespace runs.
/// Deterministic; longest-prefix first so cwd wins over $HOME.
pub fn normalize(raw: &str, cwd: Option<&str>) -> String {
    let mut s = raw.to_string();
    if let Some(cwd) = cwd {
        let cwd = cwd.trim_end_matches('/');
        if !cwd.is_empty() {
            s = s.replace(cwd, "$PROJECT_ROOT");
        }
    }
    if let Some(home) = std::env::var_os("HOME").and_then(|h| h.into_string().ok()) {
        let home = home.trim_end_matches('/');
        if !home.is_empty() && home != "$PROJECT_ROOT" {
            s = s.replace(home, "$HOME");
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
```
> Note: `$HOME` substitution uses the *runtime* `$HOME`. For determinism in tests, fixtures use `/Users/dev/proj` paths (not the test runner's home), so the `$HOME` branch is inert in CI. Document this in a one-line comment.

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): path/whitespace normalization"`

### Task A5: `redact`

**Files:** Modify `crates/decant-core/src/distill.rs` (add `regex` if not present — check `crates/decant-core/Cargo.toml`; `regex` is commonly already a dep via transitive, but add explicitly if missing).

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn redact_token_flags_and_masks() {
    let (s, hit) = redact("gh auth login --with-token=ghp_AAAitsasecret0000000000000000000000");
    assert!(hit);
    assert!(s.contains("<REDACTED>"));
    assert!(!s.contains("ghp_AAAitsasecret"));
}
#[test]
fn redact_env_assignment() {
    let (s, hit) = redact("AWS_SECRET_ACCESS_KEY=abcd1234 aws s3 ls");
    assert!(hit && s.contains("AWS_SECRET_ACCESS_KEY=<REDACTED>"));
}
#[test]
fn redact_clean_command_untouched() {
    let (s, hit) = redact("cargo test --workspace");
    assert!(!hit && s == "cargo test --workspace");
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement** — a `static` set of compiled patterns (use `std::sync::LazyLock<Vec<Regex>>`, stable on current toolchain). Patterns:
  - `(?i)(--?(?:token|api[-_]?key|secret|password|passwd|pwd)[ =])\S+` → `${1}<REDACTED>`
  - `(?i)(authorization:\s*bearer\s+)\S+` → `${1}<REDACTED>`
  - `\b([A-Z0-9_]*(?:SECRET|TOKEN|KEY|PASSWORD)[A-Z0-9_]*=)\S+` → `${1}<REDACTED>`
  - `\b(gh[pousr]_[A-Za-z0-9]{20,})\b` → `<REDACTED>`
  - `\b(sk-[A-Za-z0-9]{20,})\b` → `<REDACTED>`
  - `\b(AKIA[0-9A-Z]{16})\b` → `<REDACTED>`
```rust
pub fn redact(s: &str) -> (String, bool) {
    let mut out = s.to_string();
    let mut hit = false;
    for (re, rep) in REDACTORS.iter() {
        if re.is_match(&out) {
            hit = true;
            out = re.replace_all(&out, *rep).into_owned();
        }
    }
    (out, hit)
}
```
> If `regex` is not already in `decant-core/Cargo.toml`, add `regex = "1"`; confirm with `cargo tree -p decant-core | grep regex` first to avoid a needless dep.

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): deterministic secret redaction"`

### Task A6: `classify_phase`

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn phase_classification() {
    assert_eq!(classify_phase("cargo build --workspace"), Phase::Build);
    assert_eq!(classify_phase("cargo test"), Phase::Test);
    assert_eq!(classify_phase("cargo clippy -- -D warnings"), Phase::Lint);
    assert_eq!(classify_phase("mix deps.get"), Phase::Setup);
    assert_eq!(classify_phase("git push"), Phase::Vcs);
    assert_eq!(classify_phase("mix phx.server"), Phase::Run);
    assert_eq!(classify_phase("echo hi"), Phase::Other);
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement** — ordered keyword scan (first match wins), case-insensitive on a lowercased copy:
```rust
pub fn classify_phase(cmd: &str) -> Phase {
    let c = cmd.to_lowercase();
    let has = |kw: &[&str]| kw.iter().any(|k| c.contains(*k));
    if has(&["install", "deps.get", "setup", "init ", "bootstrap", "npm ci"]) { Phase::Setup }
    else if has(&["clippy", "fmt", "lint", "format", "rustfmt", "eslint"]) { Phase::Lint }
    else if has(&["test", "spec", " check"]) { Phase::Test }
    else if has(&["build", "compile", "cargo b"]) { Phase::Build }
    else if has(&["deploy", "release", "publish"]) { Phase::Deploy }
    else if has(&["git "]) { Phase::Vcs }
    else if has(&["run", "serve", "start", "phx.server"]) { Phase::Run }
    else { Phase::Other }
}
```
> Lint before Test so `cargo fmt --check` isn't miscategorized; Setup first so `npm install` wins over `run`. Order is the heuristic — document in one line.

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): phase classification"`

### Task A7: `is_destructive`

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn destructive_detection() {
    assert!(is_destructive("rm -rf target").is_some());
    assert!(is_destructive("git push --force origin main").is_some());
    assert!(is_destructive("kubectl delete pod x").is_some());
    assert_eq!(is_destructive("cargo test"), None);
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement** — substring/prefix denylist returning a static reason:
```rust
pub fn is_destructive(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_lowercase();
    const RULES: &[(&str, &str)] = &[
        ("rm -rf", "rm -rf"), ("rm -fr", "rm -rf"), ("git push --force", "force push"),
        ("git push -f", "force push"), ("git reset --hard", "hard reset"),
        ("git clean -fd", "git clean"), ("kubectl delete", "kubectl delete"),
        ("docker rm", "docker rm"), ("docker rmi", "docker rmi"), ("dropdb", "dropdb"),
        ("drop table", "drop table"), ("drop database", "drop database"), ("dd if=", "dd"),
        ("mkfs", "mkfs"), ("chmod -r 777", "chmod 777"), ("| sh", "pipe-to-shell"),
        ("| bash", "pipe-to-shell"),
    ];
    RULES.iter().find(|(pat, _)| c.contains(pat)).map(|(_, reason)| *reason)
}
```

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): destructive-command denylist"`

### Task A8: `timeline` extractor

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing test** (seeds the fixtures, then extracts)
```rust
fn seeded() -> Connection {
    use crate::{db, ingest, schema, sources};
    let conn = db::open_in_memory().unwrap();
    schema::migrate(&conn).unwrap();
    let claude = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/distill.jsonl")).unwrap();
    let p = sources::claude::parse_session("d-claude", &claude);
    let tx = conn.unchecked_transaction().unwrap();
    ingest::upsert_session(&tx, &p, "/x/d.jsonl", 1, 2, "h").unwrap();
    tx.commit().unwrap();
    conn
}
#[test]
fn timeline_extracts_commands_with_success_and_frequency() {
    let conn = seeded();
    let d = timeline(&conn, &Scope::default()).unwrap();
    let cmds: Vec<&str> = d.ops.iter().filter(|o| o.kind == OpKind::Command).map(|o| o.normalized.as_str()).collect();
    assert!(cmds.iter().any(|c| c.contains("cargo build")));
    // the secret-bearing command is redacted
    assert!(d.ops.iter().any(|o| o.redacted && o.normalized.contains("<REDACTED>")));
    // errored command is flagged
    assert!(d.ops.iter().any(|o| o.is_error));
    assert_eq!(d.generated_with, crate::version());
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement** — query `tool_call` joined to `session` (+ `project` for scope/root rollup), ordered by `(session_id, ordinal)`; decode commands; normalize+redact; classify; compute `sessions_seen`/`success_rate` per normalized command across the scope. SQL is fixed (no user-interpolated columns); scope filters bind params. Compute frequency in Rust over the fetched rows (the scope is bounded; aggregate in a `HashMap<String,(HashSet<i64>, used, errs)>`). Set `scope_label`, `date_from/to`, `session_count`, `generated_with = crate::version()`. Sort `ops` by `(session_id, ordinal)` (total order). Detail in code; key points:
  - `WHERE` clauses for `--project` match `p.name = ?1 OR p.path = ?1`, rolled to `COALESCE(p.root_path,p.path)`; `--work-type` matches `s.work_type = ?`.
  - Only rows where `decode_command(tool_name, input)` is `Some` become `Command` ops in Phase A (file ops added in Phase B via a separate path).

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): timeline extractor over tool_call"`

### Task A9: `render_script` (S1) + determinism

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing tests**
```rust
#[test]
fn render_script_groups_ranks_and_flags() {
    let conn = seeded();
    let d = timeline(&conn, &Scope::default()).unwrap();
    let out = render_script(&d, &ScriptOpts::default());
    assert!(out.starts_with("#!/usr/bin/env bash"));
    assert!(out.contains("set -euo pipefail"));
    assert!(out.contains("# test") || out.contains("# build"));
    assert!(out.contains("cargo build"));
    // destructive line is commented with REVIEW
    assert!(out.contains("# REVIEW: destructive"));
    assert!(out.lines().any(|l| l.trim_start().starts_with("# rm -rf") || l.contains("# rm -rf")));
    // secret never leaks
    assert!(!out.contains("ghp_AAAitsasecret"));
}
#[test]
fn render_script_is_deterministic() {
    let conn = seeded();
    let d = timeline(&conn, &Scope::default()).unwrap();
    assert_eq!(render_script(&d, &ScriptOpts::default()), render_script(&d, &ScriptOpts::default()));
}
```

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement**
```rust
#[derive(Debug, Clone)]
pub struct ScriptOpts { pub format: ScriptFormat, pub min_frequency: f32, pub exemplar: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ScriptFormat { Sh, Just, Make }
impl Default for ScriptOpts { fn default() -> Self { Self { format: ScriptFormat::Sh, min_frequency: 0.25, exemplar: false } } }

pub fn render_script(d: &Distillation, opts: &ScriptOpts) -> String {
    // 1. keep Command ops; dedupe by `normalized`.
    // 2. exemplar mode (S3): keep order, no freq filter. else (S1): keep ops with
    //    sessions_seen as f32 / session_count >= min_frequency (min 2 sessions),
    //    or always-keep when session_count == 1.
    // 3. sort kept: by phase.order(), then sessions_seen desc, then success_rate desc,
    //    then normalized lexical (TOTAL order → determinism).
    // 4. emit header (version via d.generated_with, MASKED in goldens), set -euo pipefail,
    //    PROJECT_ROOT line; per phase a "# <phase>" comment; each cmd a "# (seen N/M, R%)"
    //    line then the command; destructive → commented with "# REVIEW: destructive (<reason>), seen in N sessions".
    // Just/Make formats wrap the same body in a recipe/target.
}
```
Use `d.generated_with` in the header; the golden test masks the version line (see Task A9 test uses `contains`, not full-body equality — full-body goldens added once output stabilizes).

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): render_script (S1) with phasing, ranking, safety"`

### Task A10: `render_script` exemplar (S3) via `--from-session`

- [ ] **Step 1: Failing test** — seed, get the session id, `Scope{from_session:Some(id),..}`; assert the script preserves source order and skips the frequency filter (single session → all proven commands present).
- [ ] **Step 2–4:** wire `Scope.from_session` through `timeline` (restrict to that session id, `session_count = 1`) and `ScriptOpts.exemplar = true`. Verify.
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): exemplar (single-session) script mode"`

### Task A11: CLI `distill script`

**Files:** Create `crates/decant-cli/src/commands/distill.rs`; Modify `commands/mod.rs`, `main.rs`

- [ ] **Step 1: Failing CLI test** (in `distill.rs` under `#[cfg(test)]`, or an integration test mirroring existing CLI tests) — build a temp DB via core, run the `script` subcommand against it with `--db`, assert stdout contains `#!/usr/bin/env bash`; assert `--json` emits a parseable object with an `ops` array; assert empty-scope exits non-zero with a clear message.

- [ ] **Step 2: Run, expect fail.**

- [ ] **Step 3: Implement** — clap:
```rust
#[derive(Subcommand, Debug)]
pub enum DistillCmd {
    /// Workflow script from your real command history (frequency-ranked).
    Script(ScriptArgs),
    /// Reproduce one session's commands + file writes as a script.
    Replay(ReplayArgs),
    /// Generate a SKILL.md / AGENTS.md section / slash-command from a project.
    Skill(SkillArgs),
}
```
`ScriptArgs { project: Option<String>, work_type: Option<String>, from_session: Option<i64>, format: String /*sh|just|make*/, min_frequency: Option<f32>, out: Option<PathBuf> }`. `run` opens DB (`db::open` + `schema::migrate` like `files.rs`), builds `Scope`, calls `timeline`, then `render_script`; if `--json` print `print_json(&distillation)`, else write to `-o` (refuse overwrite without `--force`) or stdout. Empty `ops` → `eprintln!` + `Ok(1)`. Register `Distill(commands::distill::DistillCmd)` in `Commands` with about text + dispatch. (Replay/Skill arms can `todo!()`-free stub returning `Ok(0)` with "not yet implemented" until Phases B/C — OR gate them in later tasks. Prefer: implement `script` arm now; add `replay`/`skill` arms in B3/C3.)

- [ ] **Step 4: Run, expect pass** — also `cargo test -p decant-cli` (conformance: every command has about text).
- [ ] **Step 5: Commit** — `git commit -am "feat(cli): decant distill script"`

**Phase A gate:** `cargo test --workspace && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`.

---

## Phase B — `distill replay`

### Task B1: `reconstruct_edit` / payload decode

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing tests** — Claude `Write` input `{"file_path":"a.txt","content":"hello\n"}` → a heredoc block writing `a.txt`; Codex `apply_patch` arguments → the raw patch text extracted (mirror `enrich.rs` apply_patch handling). Claude `Edit` `{"file_path","old_string","new_string"}` → a **reviewable annotation** string (v1 does not auto-apply substring edits — documented limitation).

- [ ] **Step 2–4:** implement `fn replay_ops(conn, session_id) -> Result<Vec<Op>>` returning the interleaved command + file-op stream for one session (commands via `decode_command`; file ops via `block.tool_input` for Claude Write/Edit and `tool_call.input` for Codex apply_patch), ordered by `ordinal`. `payload` carries content/patch. Pure render helpers turn each into emitted text.

- [ ] **Step 5: Commit** — `git commit -am "feat(distill): replay op extraction + edit reconstruction"`

### Task B2: `render_replay`

- [ ] **Step 1: Failing test** — `render_replay(&conn, id, false)` starts with the bash header naming the session, contains the `cargo build` command and a heredoc for the `Write`, drops errored commands by default, and notes the starting-state assumption. `--include-errors` keeps errored commands commented.
- [ ] **Step 2–4:** implement; commands as-is (errored dropped unless `include_errors`), Write → `mkdir -p` + `cat > path <<'DECANT_EOF'…DECANT_EOF`, Codex patch → `git apply <<'DECANT_EOF'…`, Claude Edit → commented annotation. Header documents best-effort + review.
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): render_replay (faithful single-session)"`

### Task B3: CLI `distill replay`

- [ ] **Step 1: Failing CLI test** — `replay <id>` against a temp DB prints a bash script; missing id → clear error + non-zero exit.
- [ ] **Step 2–4:** implement the `Replay` arm (`ReplayArgs { session_id: i64, include_errors: bool, out: Option<PathBuf> }`).
- [ ] **Step 5: Commit** — `git commit -am "feat(cli): decant distill replay"`

**Phase B gate:** full `cargo test --workspace && fmt && clippy`.

---

## Phase C — `distill skill`

### Task C1: `hot_context` query

**Files:** Modify `crates/decant-core/src/distill.rs`

- [ ] **Step 1: Failing test** — seed a DB where a file is read across sessions; `hot_context(&conn, scope, 10)` returns it with read/edit counts. (Reuse the `file_ref` aggregation shape from `stats::file_hotspots`; restrict to `operation='read'`, in-project `rel_path IS NOT NULL`.)
- [ ] **Step 2–4:** implement `pub struct HotFile { rel_path, reads, edits, sessions }` + `pub fn hot_context(conn, scope, limit) -> Result<Vec<HotFile>>`.
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): hot-context file query"`

### Task C2: `render_skill` (3 kinds)

- [ ] **Step 1: Failing tests** — `render_skill(kind=Skill, project, recipe_ops, hot_files)` → markdown with YAML frontmatter (`name`, `description`), `## When to use`, `## Key files` (lists hot rel_paths + read counts), `## Commands` (the recipe). `kind=Agents` → an AGENTS.md section (no frontmatter). `kind=Command` → slash-command frontmatter + body. Determinism test (render twice equal).
- [ ] **Step 2–4:** implement `pub enum SkillKind { Skill, Agents, Command }` + `pub fn render_skill(d: &Distillation, hot: &[HotFile], kind: SkillKind, project: &str) -> String`. Reuse `render_script` body (or a shared `recipe_lines` helper) for the Commands section.
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): render_skill (skill/agents/command)"`

### Task C3: CLI `distill skill`

- [ ] **Step 1: Failing CLI test** — `skill --project P --kind agents` prints a markdown section with a `## Commands` block; unknown `--kind` → exit 2.
- [ ] **Step 2–4:** implement the `Skill` arm (`SkillArgs { project: Option<String>, kind: String, out: Option<PathBuf> }`); resolve project label; call `timeline` + `hot_context` + `render_skill`.
- [ ] **Step 5: Commit** — `git commit -am "feat(cli): decant distill skill"`

### Task C4: Close the loop with the hot-context recommendation

**Files:** Modify `crates/decant-core/src/recommendations.rs`

- [ ] **Step 1: Failing test** — the `signal:hot-context:<path>` recommendation's `action` (or `prompt`) field contains `decant distill skill`. Find the existing hot-context card builder (search `hot-context` / `promotion_card`) and assert the new action string.
- [ ] **Step 2–4:** populate the card `action` with `decant distill skill --project <project> --kind agents` (project from the signal's scope). No engine/shape change beyond the string.
- [ ] **Step 5: Commit** — `git commit -am "feat(distill): wire distill skill into the hot-context recommendation"`

**Phase C gate:** full `cargo test --workspace && fmt && clippy`.

---

## Docs (after Phase C)

- [ ] Update `README.md` Features + Quick start: add a "Distill" bullet (turn history into runnable scripts/skills) and 2–3 `decant distill …` examples; nudge framing toward extraction.
- [ ] Update `AGENTS.md` Commands section with the `distill` family.
- [ ] Commit — `docs: document the decant distill family`.

## Self-Review (completed)

- **Spec coverage:** extractor (§4.1) → A2–A8; render_script S1+S3 (§4.2, §2.3) → A9–A10; safety (§6) → A5/A7/A9; CLI stdout/json/-o (§5) → A11/B3/C3; replay (§4.2) → B1–B3 (Edit-as-annotation v1 limitation flagged — **spec §4.2 to be aligned**); skill (§4.2) → C1–C3; recommendations loop (§5.1) → C4; tests/determinism (§9) → throughout; docs → final section.
- **Placeholders:** none — pure-fn code is complete; renderer/CLI bodies specify exact behavior + key code (acceptable given the implementer has full context this session).
- **Type consistency:** `Op`/`Phase`/`OpKind`/`Scope`/`Distillation`/`ScriptOpts`/`SkillKind`/`HotFile` defined once (A2/A9/C1/C2), reused consistently; `decode_command(tool_name, input)`, `normalize(raw, cwd)`, `redact(s)->(String,bool)`, `classify_phase(cmd)`, `is_destructive(cmd)->Option<&str>`, `timeline(conn,scope)`, `render_script(d,opts)`, `render_replay(conn,id,include_errors)`, `render_skill(d,hot,kind,project)` signatures stable across tasks.
