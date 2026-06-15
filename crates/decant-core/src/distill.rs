//! Deterministic distillation: turn recorded sessions into runnable artifacts.
//!
//! A pure extractor (`timeline`) reads the archive into a normalized,
//! success-tagged, secret-redacted op stream; renderers turn that into scripts,
//! replays, and skill files. No LLM, no network — output is a pure function of
//! (DB contents, decant version). UI-agnostic: returns data + `String`s only.

use crate::Result;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Command,
    FileWrite,
    FileEdit,
    FileDelete,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Setup,
    Build,
    Test,
    Lint,
    Run,
    Deploy,
    Vcs,
    Other,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Setup => "setup",
            Phase::Build => "build",
            Phase::Test => "test",
            Phase::Lint => "lint",
            Phase::Run => "run",
            Phase::Deploy => "deploy",
            Phase::Vcs => "vcs",
            Phase::Other => "other",
        }
    }
    /// Stable display order for grouping (determinism).
    pub fn order(self) -> u8 {
        match self {
            Phase::Setup => 0,
            Phase::Build => 1,
            Phase::Test => 2,
            Phase::Lint => 3,
            Phase::Run => 4,
            Phase::Deploy => 5,
            Phase::Vcs => 6,
            Phase::Other => 7,
        }
    }
    pub fn all() -> [Phase; 8] {
        [
            Phase::Setup,
            Phase::Build,
            Phase::Test,
            Phase::Lint,
            Phase::Run,
            Phase::Deploy,
            Phase::Vcs,
            Phase::Other,
        ]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Op {
    pub session_id: i64,
    pub ordinal: i64,
    pub kind: OpKind,
    pub raw: String,
    pub normalized: String,
    pub payload: Option<String>,
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

// ── Pure helpers ────────────────────────────────────────────────────────────

/// Decode a shell command from a `tool_call.input` payload. Claude `Bash` →
/// `command`; Codex `exec_command` → the JSON-string-encoded `cmd`. None for
/// non-command tools or unparseable input.
pub fn decode_command(tool_name: &str, input: Option<&str>) -> Option<String> {
    let input = input?;
    let mut v: serde_json::Value = serde_json::from_str(input).ok()?;
    // Codex stores arguments as a JSON-encoded string; unwrap one level.
    if let serde_json::Value::String(s) = v {
        v = serde_json::from_str(&s).ok()?;
    }
    let key = match tool_name {
        "Bash" => "command",
        "exec_command" | "shell" | "local_shell" => "cmd",
        _ => return None,
    };
    match v.get(key).or_else(|| v.get("command")) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(a)) => {
            // argv form (["bash","-lc","…"]) → shell-quote each element so the
            // rejoined string re-parses to the same tokens.
            let parts: Vec<String> = a
                .iter()
                .filter_map(|x| x.as_str())
                .map(shell_quote)
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        _ => None,
    }
}

/// Replace the project cwd with `$PROJECT_ROOT` (boundary-anchored, so a sibling
/// like `/a/b/cc` is never mangled) and collapse whitespace runs. Pure: depends
/// only on its inputs (no env reads — that would break the determinism contract).
pub fn normalize(raw: &str, cwd: Option<&str>) -> String {
    let s = match cwd {
        Some(c) => replace_path_prefix(raw, c.trim_end_matches('/'), "$PROJECT_ROOT"),
        None => raw.to_string(),
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replace `from` with `to`, but only where `from` is a whole path segment —
/// a boundary on BOTH sides — so neither a sibling (`/a/b/cc`) nor a
/// parent-embedded path (`/var/Users/dev/proj`) is mangled.
fn replace_path_prefix(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(pos) = s[i..].find(from) {
        let abs = i + pos;
        let after = abs + from.len();
        // Prefix boundary: start-of-string, or the preceding char can't be part
        // of a longer path token.
        let prefix_ok = abs == 0
            || !matches!(
                s[..abs].chars().next_back(),
                Some(c) if c.is_alphanumeric() || matches!(c, '/' | '_' | '-' | '.')
            );
        let suffix_ok = matches!(
            s[after..].chars().next(),
            None | Some('/' | ' ' | '\t' | '"' | '\'' | ')' | ':')
        );
        out.push_str(&s[i..abs]);
        out.push_str(if prefix_ok && suffix_ok { to } else { from });
        i = after;
    }
    out.push_str(&s[i..]);
    out
}

static REDACTORS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    // A secret VALUE: a single/double-quoted string (so spaces inside quotes are
    // fully masked, not just up to the first space) OR a bare non-whitespace run.
    const VAL: &str = r#"(?:'[^'\\]*(?:\\.[^'\\]*)*'|"[^"\\]*(?:\\.[^"\\]*)*"|\S+)"#;
    vec![
        (
            Regex::new(&format!(
                r"(?i)(--?(?:token|api[-_]?key|secret|password|passwd|pwd)[ =]){VAL}"
            ))
            .unwrap(),
            "${1}<REDACTED>",
        ),
        (
            Regex::new(&format!(r"(?i)(authorization:\s*bearer\s+){VAL}")).unwrap(),
            "${1}<REDACTED>",
        ),
        // API-key / auth-token request headers.
        (
            Regex::new(&format!(
                r"(?i)((?:x-api-key|x-auth-token|api-key):\s*){VAL}"
            ))
            .unwrap(),
            "${1}<REDACTED>",
        ),
        // Credentials embedded in a URL: scheme://user:PASSWORD@host (also covers
        // DATABASE_URL=postgres://u:pw@h, git clone https://u:pw@host, …).
        (
            Regex::new(r"(?i)([a-z][a-z0-9+.\-]*://[^/:@\s]+:)[^/@\s]+@").unwrap(),
            "${1}<REDACTED>@",
        ),
        (
            Regex::new(&format!(
                r"\b([A-Z0-9_]*(?:SECRET|TOKEN|KEY|PASSWORD)[A-Z0-9_]*=){VAL}"
            ))
            .unwrap(),
            "${1}<REDACTED>",
        ),
        (
            Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b").unwrap(),
            "<REDACTED>",
        ),
        (
            Regex::new(r"\bsk-[A-Za-z0-9]{20,}\b").unwrap(),
            "<REDACTED>",
        ),
        (Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(), "<REDACTED>"),
    ]
});

/// Mask obvious secret shapes. Best-effort; returns the masked string and
/// whether anything matched.
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

/// Heuristic phase bucket. Order is the heuristic: Setup before Run so `install`
/// wins; Lint before Test so `fmt --check` isn't read as a test.
pub fn classify_phase(cmd: &str) -> Phase {
    let c = cmd.to_lowercase();
    let has = |kw: &[&str]| kw.iter().any(|k| c.contains(*k));
    if has(&[
        "install",
        "deps.get",
        "setup",
        "bootstrap",
        "npm ci",
        "mix setup",
    ]) {
        Phase::Setup
    } else if has(&[
        "clippy", "fmt", "lint", "format", "rustfmt", "eslint", "prettier",
    ]) {
        Phase::Lint
    } else if has(&["test", "spec", "nextest"]) || (c.contains(" check") && !c.contains("checkout"))
    {
        Phase::Test
    } else if has(&["build", "compile"]) {
        Phase::Build
    } else if has(&["deploy", "release", "publish"]) {
        Phase::Deploy
    } else if has(&["git "]) {
        Phase::Vcs
    } else if has(&["run", "serve", "start", "phx.server"]) {
        Phase::Run
    } else {
        Phase::Other
    }
}

/// Destructive-command denylist → a static reason, or None. Matched on a
/// lowercased copy.
pub fn is_destructive(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_lowercase();
    const RULES: &[(&str, &str)] = &[
        ("rm -rf", "rm -rf"),
        ("rm -fr", "rm -rf"),
        ("git push --force", "force push"),
        ("git push -f", "force push"),
        ("git reset --hard", "hard reset"),
        ("git clean", "git clean"),
        ("git checkout -- ", "discard changes"),
        ("git checkout .", "discard changes"),
        ("git restore", "discard changes"),
        ("kubectl delete", "kubectl delete"),
        ("docker rm", "docker rm"),
        ("docker rmi", "docker rmi"),
        ("docker volume rm", "docker volume rm"),
        ("dropdb", "dropdb"),
        ("drop table", "drop table"),
        ("drop database", "drop database"),
        ("dd if=", "dd"),
        ("mkfs", "mkfs"),
        ("truncate -s 0", "truncate"),
        ("shred ", "shred"),
        (" -delete", "find -delete"),
        ("chmod -r 777", "chmod 777"),
        ("| sh", "pipe-to-shell"),
        ("| bash", "pipe-to-shell"),
    ];
    RULES
        .iter()
        .find(|(pat, _)| c.contains(pat))
        .map(|(_, reason)| *reason)
}

// ── Extractor ─────────────────────────────────────────────────────────────

/// Build the scope WHERE fragment (referencing `s` and `p` aliases) + bound
/// params. Values are always bound, never interpolated.
fn scope_clause(scope: &Scope) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value;
    let mut sql = String::new();
    let mut vals: Vec<Value> = Vec::new();
    if let Some(proj) = &scope.project {
        sql.push_str(" AND (p.name = ? OR p.path = ? OR COALESCE(p.root_path, p.path) = ?)");
        vals.push(Value::Text(proj.clone()));
        vals.push(Value::Text(proj.clone()));
        vals.push(Value::Text(proj.clone()));
    }
    if let Some(wt) = &scope.work_type {
        sql.push_str(" AND s.work_type = ?");
        vals.push(Value::Text(wt.clone()));
    }
    if let Some(sid) = scope.from_session {
        sql.push_str(" AND s.id = ?");
        vals.push(Value::Integer(sid));
    }
    (sql, vals)
}

fn scope_label(scope: &Scope) -> String {
    if let Some(sid) = scope.from_session {
        format!("session {sid}")
    } else if let Some(p) = &scope.project {
        format!("project {p:?}")
    } else if let Some(w) = &scope.work_type {
        format!("work_type {w:?}")
    } else {
        "all sessions".to_string()
    }
}

/// Extract the command op stream for the scoped sessions. Phase A: commands only.
pub fn timeline(conn: &Connection, scope: &Scope) -> Result<Distillation> {
    let (clause, vals) = scope_clause(scope);

    // Session count + date bounds for the scope (header provenance + frequency floor).
    let count_sql = format!(
        "SELECT COUNT(*), MIN(s.started_at), MAX(s.started_at) \
         FROM session s LEFT JOIN project p ON p.id = s.project_id WHERE 1=1{clause}"
    );
    let (session_count, date_from, date_to): (u32, Option<String>, Option<String>) = conn
        .query_row(&count_sql, rusqlite::params_from_iter(vals.iter()), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;

    let sql = format!(
        "SELECT tc.session_id, tc.ordinal, tc.tool_name, tc.input, tc.is_error, s.cwd \
         FROM tool_call tc \
         JOIN session s ON s.id = tc.session_id \
         LEFT JOIN project p ON p.id = s.project_id \
         WHERE 1=1{clause} \
         ORDER BY tc.session_id, tc.ordinal"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals.iter()), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<bool>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // First pass: decode, redact, normalize, classify. Redaction runs BEFORE
    // normalization so `raw` (pre-path-templating) is still secret-safe for JSON.
    struct Raw {
        session_id: i64,
        ordinal: i64,
        raw: String,
        normalized: String,
        redacted: bool,
        phase: Phase,
        is_error: bool,
    }
    let mut raws: Vec<Raw> = Vec::new();
    for (session_id, ordinal, tool_name, input, is_error, cwd) in rows {
        let Some(tool_name) = tool_name else { continue };
        let Some(cmd) = decode_command(&tool_name, input.as_deref()) else {
            continue;
        };
        let (red, redacted) = redact(&cmd);
        let normalized = normalize(&red, cwd.as_deref());
        let phase = classify_phase(&normalized);
        raws.push(Raw {
            session_id,
            ordinal,
            raw: red,
            normalized,
            redacted,
            phase,
            is_error: is_error == Some(true),
        });
    }

    // Frequency pass: per normalized command, distinct sessions + success rate.
    let mut agg: HashMap<&str, (HashSet<i64>, u32, u32)> = HashMap::new();
    for r in &raws {
        let e = agg
            .entry(r.normalized.as_str())
            .or_insert_with(|| (HashSet::new(), 0, 0));
        e.0.insert(r.session_id);
        e.1 += 1;
        if !r.is_error {
            e.2 += 1;
        }
    }

    let ops = raws
        .iter()
        .map(|r| {
            let (sessions, total, ok) = &agg[r.normalized.as_str()];
            Op {
                session_id: r.session_id,
                ordinal: r.ordinal,
                kind: OpKind::Command,
                raw: r.raw.clone(),
                normalized: r.normalized.clone(),
                payload: None,
                phase: r.phase,
                is_error: r.is_error,
                redacted: r.redacted,
                sessions_seen: sessions.len() as u32,
                success_rate: if *total == 0 {
                    0.0
                } else {
                    *ok as f32 / *total as f32
                },
            }
        })
        .collect();

    Ok(Distillation {
        scope_label: scope_label(scope),
        ops,
        session_count,
        date_from,
        date_to,
        generated_with: crate::version().to_string(),
    })
}

// ── Script renderer (S1 frequency + S3 exemplar) ────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFormat {
    Sh,
    Just,
    Make,
}

impl ScriptFormat {
    pub fn parse(s: &str) -> Option<ScriptFormat> {
        match s {
            "sh" | "bash" => Some(ScriptFormat::Sh),
            "just" => Some(ScriptFormat::Just),
            "make" => Some(ScriptFormat::Make),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptOpts {
    pub format: ScriptFormat,
    pub min_frequency: f32,
    pub exemplar: bool,
}

impl Default for ScriptOpts {
    fn default() -> Self {
        Self {
            format: ScriptFormat::Sh,
            min_frequency: 0.25,
            exemplar: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Line {
    phase: Phase,
    cmd: String,
    sessions_seen: u32,
    success_rate: f32,
    destructive: Option<&'static str>,
    order_key: (i64, i64),
}

/// Select + rank the command lines for a script. S1 dedupes by command and
/// applies a frequency floor; exemplar/single-session keeps source order.
fn select_lines(d: &Distillation, opts: &ScriptOpts) -> Vec<Line> {
    let exemplar = opts.exemplar || d.session_count <= 1;
    let mut lines: Vec<Line> = Vec::new();

    if exemplar {
        // Preserve source order; drop errored; dedupe consecutive duplicates.
        let mut last: Option<String> = None;
        for op in d.ops.iter().filter(|o| o.kind == OpKind::Command) {
            if op.is_error {
                continue;
            }
            if last.as_deref() == Some(op.normalized.as_str()) {
                continue;
            }
            last = Some(op.normalized.clone());
            lines.push(Line {
                phase: op.phase,
                cmd: op.normalized.clone(),
                sessions_seen: op.sessions_seen,
                success_rate: op.success_rate,
                destructive: is_destructive(&op.normalized),
                order_key: (op.session_id, op.ordinal),
            });
        }
        lines.sort_by_key(|l| l.order_key);
        return lines;
    }

    // S1: dedupe by command, apply floor (≥2 sessions and ≥min_frequency), require
    // at least one success.
    let mut seen: HashSet<&str> = HashSet::new();
    for op in d.ops.iter().filter(|o| o.kind == OpKind::Command) {
        if !seen.insert(op.normalized.as_str()) {
            continue;
        }
        let freq = op.sessions_seen as f32 / d.session_count.max(1) as f32;
        if op.success_rate <= 0.0 || op.sessions_seen < 2 || freq < opts.min_frequency {
            continue;
        }
        lines.push(Line {
            phase: op.phase,
            cmd: op.normalized.clone(),
            sessions_seen: op.sessions_seen,
            success_rate: op.success_rate,
            destructive: is_destructive(&op.normalized),
            order_key: (0, 0),
        });
    }
    // Total order: phase, then frequency desc, success desc, command lexical.
    lines.sort_by(|a, b| {
        a.phase
            .order()
            .cmp(&b.phase.order())
            .then(b.sessions_seen.cmp(&a.sessions_seen))
            .then(
                b.success_rate
                    .partial_cmp(&a.success_rate)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.cmd.cmp(&b.cmd))
    });
    lines
}

fn header_lines(d: &Distillation, opts: &ScriptOpts) -> Vec<String> {
    let day = |s: &Option<String>| {
        s.as_deref()
            .map(|t| t.chars().take(10).collect::<String>())
            .unwrap_or_else(|| "?".to_string())
    };
    let mode = if opts.exemplar || d.session_count <= 1 {
        "faithful single-session order"
    } else {
        "ranked by frequency × success"
    };
    vec![
        format!(
            "Distilled by decant {} from {}",
            d.generated_with, d.scope_label
        ),
        format!(
            "{} session(s) ({}..{}). Commands you actually ran, {}.",
            d.session_count,
            day(&d.date_from),
            day(&d.date_to),
            mode
        ),
        "REVIEW before running. Secrets are best-effort redacted.".to_string(),
    ]
}

pub fn render_script(d: &Distillation, opts: &ScriptOpts) -> String {
    let lines = select_lines(d, opts);
    match opts.format {
        ScriptFormat::Sh => emit_sh(d, opts, &lines),
        ScriptFormat::Just => emit_just(d, opts, &lines),
        ScriptFormat::Make => emit_make(d, opts, &lines),
    }
}

fn emit_sh(d: &Distillation, opts: &ScriptOpts, lines: &[Line]) -> String {
    let mut out = String::from("#!/usr/bin/env bash\n");
    for h in header_lines(d, opts) {
        out.push_str(&format!("# {h}\n"));
    }
    out.push_str("set -euo pipefail\n");
    out.push_str("PROJECT_ROOT=\"${PROJECT_ROOT:-$(git rev-parse --show-toplevel)}\"\n");
    if lines.is_empty() {
        out.push_str("\n# (no recurring commands met the frequency threshold)\n");
        return out;
    }
    let grouped = !opts.exemplar && d.session_count > 1;
    if grouped {
        for phase in Phase::all() {
            let group: Vec<&Line> = lines.iter().filter(|l| l.phase == phase).collect();
            if group.is_empty() {
                continue;
            }
            out.push_str(&format!("\n# {}\n", phase.as_str()));
            for l in group {
                emit_sh_line(&mut out, l, d.session_count);
            }
        }
    } else {
        out.push('\n');
        for l in lines {
            emit_sh_line(&mut out, l, d.session_count);
        }
    }
    out
}

fn emit_sh_line(out: &mut String, l: &Line, session_count: u32) {
    if let Some(reason) = l.destructive {
        out.push_str(&format!(
            "# REVIEW: destructive ({}), seen in {} session(s) — left commented\n# {}\n",
            reason, l.sessions_seen, l.cmd
        ));
    } else {
        if session_count > 1 {
            out.push_str(&format!(
                "# seen {}/{}, {:.0}% ok\n",
                l.sessions_seen,
                session_count,
                l.success_rate * 100.0
            ));
        }
        out.push_str(&format!("{}\n", l.cmd));
    }
}

fn emit_just(d: &Distillation, opts: &ScriptOpts, lines: &[Line]) -> String {
    let mut out = String::new();
    for h in header_lines(d, opts) {
        out.push_str(&format!("# {h}\n"));
    }
    out.push_str("\nexport PROJECT_ROOT := `git rev-parse --show-toplevel`\n");
    for phase in Phase::all() {
        let group: Vec<&Line> = lines.iter().filter(|l| l.phase == phase).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{}:\n", phase.as_str()));
        for l in group {
            if let Some(reason) = l.destructive {
                out.push_str(&format!("    # REVIEW destructive ({reason}): {}\n", l.cmd));
            } else {
                out.push_str(&format!("    {}\n", l.cmd));
            }
        }
    }
    out
}

fn emit_make(d: &Distillation, opts: &ScriptOpts, lines: &[Line]) -> String {
    let mut out = String::new();
    for h in header_lines(d, opts) {
        out.push_str(&format!("# {h}\n"));
    }
    let phases: Vec<Phase> = Phase::all()
        .into_iter()
        .filter(|p| lines.iter().any(|l| l.phase == *p))
        .collect();
    let names: Vec<&str> = phases.iter().map(|p| p.as_str()).collect();
    out.push_str(&format!("\n.PHONY: {}\n", names.join(" ")));
    for phase in phases {
        out.push_str(&format!("\n{}:\n", phase.as_str()));
        for l in lines.iter().filter(|l| l.phase == phase) {
            if let Some(reason) = l.destructive {
                out.push_str(&format!("\t# REVIEW destructive ({reason}): {}\n", l.cmd));
            } else {
                out.push_str(&format!("\t{}\n", l.cmd));
            }
        }
    }
    out
}

// ── Replay renderer (S3 faithful single session) ────────────────────────────

fn rel_to_root(path: &str, cwd: Option<&str>) -> String {
    if let Some(cwd) = cwd {
        let cwd = cwd.trim_end_matches('/');
        if let Some(stripped) = path.strip_prefix(&format!("{cwd}/")) {
            return stripped.to_string();
        }
    }
    path.to_string()
}

/// Heredoc delimiter for replayed file writes / patches.
const REPLAY_EOF: &str = "DECANT_EOF";

/// Single-quote a path for safe shell use (paths with spaces / metachars).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Emit a file-write block. Falls back to a SKIPPED comment if the content could
/// break out of the heredoc (contains the delimiter) — never risk injection.
fn write_block(path: &str, content: &str) -> String {
    let content = content.trim_end_matches('\n');
    if content.contains(REPLAY_EOF) || path.contains('\n') || path.contains(REPLAY_EOF) {
        return format!(
            "# SKIPPED write to {path:?}: content or path unsafe for a heredoc — recreate it manually.\n"
        );
    }
    let q = shell_quote(path);
    format!("mkdir -p \"$(dirname {q})\"\ncat > {q} <<'{REPLAY_EOF}'\n{content}\n{REPLAY_EOF}\n")
}

fn patch_block(patch: &str) -> String {
    let patch = patch.trim_end_matches('\n');
    if patch.contains(REPLAY_EOF) {
        "# SKIPPED patch: contains the heredoc delimiter — apply it manually.\n".to_string()
    } else {
        format!("git apply <<'{REPLAY_EOF}'\n{patch}\n{REPLAY_EOF}\n")
    }
}

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

/// Decode a non-command file op (Write/Edit/NotebookEdit/apply_patch) into an Op.
fn file_op(
    tool_name: &str,
    input: Option<&str>,
    cwd: Option<&str>,
    session_id: i64,
    ordinal: i64,
    is_error: bool,
) -> Option<Op> {
    let input = input?;
    let mk = |kind, raw: String, payload, redacted| Op {
        session_id,
        ordinal,
        kind,
        raw,
        normalized: String::new(),
        payload,
        phase: Phase::Other,
        is_error,
        redacted,
        sessions_seen: 1,
        success_rate: if is_error { 0.0 } else { 1.0 },
    };
    if tool_name == "apply_patch" {
        let v: serde_json::Value = serde_json::from_str(input).ok()?;
        let patch = match v {
            serde_json::Value::String(s) => s,
            _ => return None,
        };
        let (patch, redacted) = redact(&patch);
        return Some(mk(
            OpKind::Patch,
            "apply_patch".into(),
            Some(patch),
            redacted,
        ));
    }
    let mut v: serde_json::Value = serde_json::from_str(input).ok()?;
    if let serde_json::Value::String(s) = &v {
        v = serde_json::from_str(s).ok()?;
    }
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    match tool_name {
        "Write" => {
            let rel = rel_to_root(&get("file_path")?, cwd);
            let (content, redacted) = redact(&get("content").unwrap_or_default());
            Some(mk(OpKind::FileWrite, rel, Some(content), redacted))
        }
        "Edit" => {
            let rel = rel_to_root(&get("file_path")?, cwd);
            let old = one_line(&get("old_string").unwrap_or_default(), 60);
            let new = one_line(&get("new_string").unwrap_or_default(), 60);
            let (annot, redacted) = redact(&format!("replace `{old}` -> `{new}`"));
            Some(mk(OpKind::FileEdit, rel, Some(annot), redacted))
        }
        "NotebookEdit" => {
            let rel = rel_to_root(&get("notebook_path").or_else(|| get("file_path"))?, cwd);
            Some(mk(
                OpKind::FileEdit,
                rel,
                Some("notebook cell edit".into()),
                false,
            ))
        }
        _ => None,
    }
}

/// One session's ordered command + file-op stream for replay.
pub fn replay_ops(conn: &Connection, session_id: i64) -> Result<Vec<Op>> {
    let cwd: Option<String> = conn
        .query_row("SELECT cwd FROM session WHERE id = ?", [session_id], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten();
    let mut stmt = conn.prepare(
        "SELECT tc.ordinal, tc.tool_name, tc.input, tc.is_error \
         FROM tool_call tc WHERE tc.session_id = ? ORDER BY tc.ordinal",
    )?;
    let rows = stmt
        .query_map([session_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<bool>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut ops = Vec::new();
    for (ordinal, tool_name, input, is_error) in rows {
        let Some(tool_name) = tool_name else { continue };
        let is_error = is_error == Some(true);
        if let Some(cmd) = decode_command(&tool_name, input.as_deref()) {
            let (red, redacted) = redact(&cmd);
            let normalized = normalize(&red, cwd.as_deref());
            let phase = classify_phase(&normalized);
            ops.push(Op {
                session_id,
                ordinal,
                kind: OpKind::Command,
                raw: red,
                normalized,
                payload: None,
                phase,
                is_error,
                redacted,
                sessions_seen: 1,
                success_rate: if is_error { 0.0 } else { 1.0 },
            });
        } else if let Some(op) = file_op(
            &tool_name,
            input.as_deref(),
            cwd.as_deref(),
            session_id,
            ordinal,
            is_error,
        ) {
            ops.push(op);
        }
    }
    Ok(ops)
}

/// Render a best-effort replay script for one session. None if the id is unknown.
pub fn render_replay(
    conn: &Connection,
    session_id: i64,
    include_errors: bool,
) -> Result<Option<String>> {
    let meta: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT title, tool FROM session WHERE id = ?",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((title, tool)) = meta else {
        return Ok(None);
    };
    let ops = replay_ops(conn, session_id)?;
    let mut out = String::from("#!/usr/bin/env bash\n");
    out.push_str(&format!(
        "# Replay of {tool} session {session_id}: {}\n",
        title.as_deref().unwrap_or("(untitled)")
    ));
    out.push_str(&format!(
        "# Distilled by decant {}. Best-effort, REVIEW before running.\n",
        crate::version()
    ));
    out.push_str("# Assumes the same starting file state; secrets are best-effort redacted.\n");
    out.push_str("set -euo pipefail\n");
    out.push_str(
        "PROJECT_ROOT=\"${PROJECT_ROOT:-$(git rev-parse --show-toplevel)}\"\ncd \"$PROJECT_ROOT\"\n\n",
    );
    for op in &ops {
        match op.kind {
            OpKind::Command => {
                if op.is_error && !include_errors {
                    continue;
                }
                if op.is_error {
                    out.push_str(&format!(
                        "# (errored in original run)\n# {}\n",
                        op.normalized
                    ));
                } else if let Some(reason) = is_destructive(&op.normalized) {
                    out.push_str(&format!(
                        "# REVIEW: destructive ({reason})\n# {}\n",
                        op.normalized
                    ));
                } else {
                    out.push_str(&format!("{}\n", op.normalized));
                }
            }
            OpKind::FileWrite => {
                out.push_str(&write_block(&op.raw, op.payload.as_deref().unwrap_or("")));
            }
            OpKind::FileEdit => {
                out.push_str(&format!(
                    "# EDIT {}: {} (apply manually — v1 does not auto-apply edits)\n",
                    op.raw,
                    op.payload.as_deref().unwrap_or("")
                ));
            }
            OpKind::FileDelete => {
                out.push_str(&format!(
                    "# DELETE {} (review)\n# rm {}\n",
                    op.raw,
                    shell_quote(&op.raw)
                ));
            }
            OpKind::Patch => {
                out.push_str(&patch_block(op.payload.as_deref().unwrap_or("")));
            }
        }
    }
    Ok(Some(out))
}

// ── Skill / AGENTS.md / slash-command renderer ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HotFile {
    pub rel_path: String,
    pub reads: i64,
    pub edits: i64,
    pub sessions: i64,
}

/// Files read across many sessions but rarely edited (re-derived context).
pub fn hot_context(conn: &Connection, scope: &Scope, limit: i64) -> Result<Vec<HotFile>> {
    let (clause, mut vals) = scope_clause(scope);
    let sql = format!(
        "SELECT fr.rel_path, \
           SUM(CASE WHEN fr.operation='read' THEN 1 ELSE 0 END) reads, \
           SUM(CASE WHEN fr.operation IN ('edit','write') THEN 1 ELSE 0 END) edits, \
           COUNT(DISTINCT fr.session_id) sessions \
         FROM file_ref fr \
         JOIN session s ON s.id = fr.session_id \
         LEFT JOIN project p ON p.id = s.project_id \
         WHERE fr.rel_path IS NOT NULL{clause} \
         GROUP BY fr.rel_path HAVING reads > 0 \
         ORDER BY sessions DESC, reads DESC, fr.rel_path LIMIT ?"
    );
    vals.push(rusqlite::types::Value::Integer(limit));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals.iter()), |r| {
            Ok(HotFile {
                rel_path: r.get(0)?,
                reads: r.get(1)?,
                edits: r.get(2)?,
                sessions: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillKind {
    Skill,
    Agents,
    Command,
}

impl SkillKind {
    pub fn parse(s: &str) -> Option<SkillKind> {
        match s {
            "skill" => Some(SkillKind::Skill),
            "agents" => Some(SkillKind::Agents),
            "command" => Some(SkillKind::Command),
            _ => None,
        }
    }
}

fn commands_block(d: &Distillation) -> String {
    let lines: Vec<Line> = select_lines(d, &ScriptOpts::default())
        .into_iter()
        .filter(|l| l.destructive.is_none())
        .collect();
    if lines.is_empty() {
        return "_(no recurring commands found)_\n".to_string();
    }
    let mut out = String::from("```bash\n");
    for l in &lines {
        out.push_str(&format!("{}\n", l.cmd));
    }
    out.push_str("```\n");
    out
}

fn key_files_block(hot: &[HotFile]) -> String {
    if hot.is_empty() {
        return "_(no hot-context files found)_\n".to_string();
    }
    let mut out = String::new();
    for h in hot {
        out.push_str(&format!(
            "- `{}` — read in {} session(s){}\n",
            h.rel_path,
            h.sessions,
            if h.edits == 0 { ", rarely edited" } else { "" }
        ));
    }
    out
}

/// Render a SKILL.md / AGENTS.md section / slash-command from distilled evidence.
pub fn render_skill(d: &Distillation, hot: &[HotFile], kind: SkillKind, project: &str) -> String {
    let cmds = commands_block(d);
    let files = key_files_block(hot);
    let slug = project
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    match kind {
        SkillKind::Skill => format!(
            "---\nname: {slug}-workflow\ndescription: Distilled workflow for {project} — proven commands and the files agents re-read most. Use when working in {project}.\n---\n\n# {project} workflow\n\n_Distilled by decant {} from {} session(s). Review and edit before use._\n\n## When to use\n\nWorking in the {project} project — building, testing, or getting oriented.\n\n## Key files\n\n{files}\n## Commands\n\n{cmds}",
            d.generated_with, d.session_count
        ),
        SkillKind::Agents => format!(
            "## {project} — distilled workflow\n\n_decant {}, from {} session(s). Review before committing._\n\n### Commands\n\n{cmds}\n### Where things live\n\n{files}",
            d.generated_with, d.session_count
        ),
        SkillKind::Command => format!(
            "---\ndescription: Run the distilled {project} workflow\n---\n\nProven commands for {project} (decant, {} session(s)):\n\n{cmds}",
            d.session_count
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, schema, sources};

    #[test]
    fn phase_enum_labels_and_order() {
        assert_eq!(Phase::Build.as_str(), "build");
        assert_eq!(Phase::Other.as_str(), "other");
        assert!(Phase::Setup.order() < Phase::Other.order());
    }

    #[test]
    fn decode_claude_bash() {
        let input = r#"{"command":"cargo build","description":"build"}"#;
        assert_eq!(
            decode_command("Bash", Some(input)).as_deref(),
            Some("cargo build")
        );
    }

    #[test]
    fn decode_codex_exec_json_string() {
        let inner = r#"{"cmd":"cargo test","workdir":"/Users/dev/proj"}"#;
        let stored = serde_json::Value::String(inner.to_string()).to_string();
        assert_eq!(
            decode_command("exec_command", Some(&stored)).as_deref(),
            Some("cargo test")
        );
    }

    #[test]
    fn decode_non_command_is_none() {
        assert_eq!(decode_command("Read", Some(r#"{"file_path":"a"}"#)), None);
        assert_eq!(decode_command("Bash", None), None);
        assert_eq!(decode_command("Bash", Some("not json")), None);
    }

    #[test]
    fn normalize_strips_cwd_and_collapses_ws() {
        assert_eq!(
            normalize(
                "cd /Users/dev/proj/web && ls /Users/dev/proj",
                Some("/Users/dev/proj")
            ),
            "cd $PROJECT_ROOT/web && ls $PROJECT_ROOT"
        );
        assert_eq!(
            normalize("cargo    test   --workspace", None),
            "cargo test --workspace"
        );
    }

    #[test]
    fn redact_masks_and_flags() {
        let (s, hit) = redact("gh auth login --with-token=ghp_AAAAitsasecrettoken0000000000000000");
        assert!(hit && s.contains("<REDACTED>") && !s.contains("ghp_AAAAitsasecret"));
        let (s2, hit2) = redact("AWS_SECRET_ACCESS_KEY=abcd1234 aws s3 ls");
        assert!(hit2 && s2.contains("AWS_SECRET_ACCESS_KEY=<REDACTED>"));
        let (s3, hit3) = redact("cargo test --workspace");
        assert!(!hit3 && s3 == "cargo test --workspace");
    }

    #[test]
    fn phase_classification() {
        assert_eq!(classify_phase("cargo build --workspace"), Phase::Build);
        assert_eq!(classify_phase("cargo test --workspace"), Phase::Test);
        assert_eq!(classify_phase("cargo clippy -- -D warnings"), Phase::Lint);
        assert_eq!(classify_phase("mix deps.get"), Phase::Setup);
        assert_eq!(classify_phase("git push"), Phase::Vcs);
        assert_eq!(classify_phase("mix phx.server"), Phase::Run);
        assert_eq!(classify_phase("echo hi"), Phase::Other);
    }

    #[test]
    fn destructive_detection() {
        assert!(is_destructive("rm -rf target").is_some());
        assert!(is_destructive("git push --force origin main").is_some());
        assert!(is_destructive("kubectl delete pod x").is_some());
        assert_eq!(is_destructive("cargo test"), None);
    }

    /// Ingest the Claude distill fixture as `n` distinct sessions.
    fn seeded_n(n: usize) -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let claude = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/distill.jsonl"
        ))
        .unwrap();
        for i in 0..n {
            let p = sources::claude::parse_session(&format!("d-claude-{i}"), &claude);
            let tx = conn.unchecked_transaction().unwrap();
            ingest::upsert_session(&tx, &p, &format!("/x/d{i}.jsonl"), 1, 2, &format!("h{i}"))
                .unwrap();
            tx.commit().unwrap();
        }
        conn
    }

    #[test]
    fn timeline_extracts_commands_with_flags() {
        let conn = seeded_n(2);
        let d = timeline(&conn, &Scope::default()).unwrap();
        assert_eq!(d.session_count, 2);
        assert_eq!(d.generated_with, crate::version());
        let cmds: Vec<&str> = d.ops.iter().map(|o| o.normalized.as_str()).collect();
        assert!(cmds.iter().any(|c| c.contains("cargo build --workspace")));
        assert!(d
            .ops
            .iter()
            .any(|o| o.redacted && o.normalized.contains("<REDACTED>")));
        assert!(d.ops.iter().any(|o| o.is_error));
        // cargo build seen in both sessions.
        let build = d
            .ops
            .iter()
            .find(|o| o.normalized == "cargo build --workspace")
            .unwrap();
        assert_eq!(build.sessions_seen, 2);
        assert_eq!(build.success_rate, 1.0);
    }

    #[test]
    fn render_script_groups_ranks_and_flags() {
        let conn = seeded_n(2);
        let d = timeline(&conn, &Scope::default()).unwrap();
        let out = render_script(&d, &ScriptOpts::default());
        assert!(out.starts_with("#!/usr/bin/env bash"));
        assert!(out.contains("set -euo pipefail"));
        assert!(out.contains("cargo build --workspace"));
        assert!(out.contains("# test") || out.contains("# build"));
        assert!(out.contains("# REVIEW: destructive"));
        // secret never leaks; the failing-only command is excluded.
        assert!(!out.contains("ghp_AAAA"));
        assert!(!out.contains("--bad-flag"));
    }

    #[test]
    fn render_script_is_deterministic() {
        let conn = seeded_n(2);
        let d = timeline(&conn, &Scope::default()).unwrap();
        assert_eq!(
            render_script(&d, &ScriptOpts::default()),
            render_script(&d, &ScriptOpts::default())
        );
    }

    #[test]
    fn exemplar_mode_preserves_order_single_session() {
        let conn = seeded_n(1);
        let d = timeline(&conn, &Scope::default()).unwrap();
        let out = render_script(
            &d,
            &ScriptOpts {
                exemplar: true,
                ..Default::default()
            },
        );
        // single session → frequency floor bypassed, build appears before test.
        let b = out.find("cargo build --workspace").unwrap();
        let t = out.find("cargo test --workspace").unwrap();
        assert!(b < t);
    }

    #[test]
    fn codex_commands_extracted() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let codex = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/distill.jsonl"
        ))
        .unwrap();
        let p = sources::codex::parse_session("d-codex", &codex, &std::collections::HashMap::new());
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &p, "/x/c.jsonl", 1, 2, "hc").unwrap();
        tx.commit().unwrap();
        let d = timeline(&conn, &Scope::default()).unwrap();
        let cmds: Vec<&str> = d.ops.iter().map(|o| o.normalized.as_str()).collect();
        assert!(cmds.iter().any(|c| c.contains("cargo build --workspace")));
        assert!(cmds.iter().any(|c| c.contains("cargo test --workspace")));
    }

    fn seed_enriched_claude(conn: &Connection, n: usize) {
        let claude = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/enriched.jsonl"
        ))
        .unwrap();
        for i in 0..n {
            let p = sources::claude::parse_session(&format!("enr-{i}"), &claude);
            let tx = conn.unchecked_transaction().unwrap();
            ingest::upsert_session(&tx, &p, &format!("/y/e{i}.jsonl"), 1, 2, &format!("he{i}"))
                .unwrap();
            tx.commit().unwrap();
        }
    }

    fn first_session_id(conn: &Connection) -> i64 {
        conn.query_row("SELECT MIN(id) FROM session", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn replay_reproduces_commands_and_writes() {
        let conn = seeded_n(1);
        let id = first_session_id(&conn);
        let out = render_replay(&conn, id, false).unwrap().unwrap();
        assert!(out.starts_with("#!/usr/bin/env bash"));
        assert!(out.contains(&format!("session {id}")));
        assert!(out.contains("cargo build --workspace"));
        assert!(out.contains("cat > 'NOTES.md'")); // path shell-quoted
        assert!(out.contains("# EDIT src/main.rs"));
        // errored command dropped by default; secret redacted.
        assert!(!out.contains("--bad-flag"));
        assert!(!out.contains("ghp_"));
        assert!(out.contains("--with-token=<REDACTED>"));
    }

    #[test]
    fn replay_include_errors_keeps_them_commented() {
        let conn = seeded_n(1);
        let id = first_session_id(&conn);
        let out = render_replay(&conn, id, true).unwrap().unwrap();
        assert!(out.contains("# (errored in original run)"));
        assert!(out.contains("cargo build --bad-flag"));
    }

    #[test]
    fn replay_unknown_session_is_none() {
        let conn = seeded_n(1);
        assert!(render_replay(&conn, 9999, false).unwrap().is_none());
    }

    #[test]
    fn replay_codex_patch_via_git_apply() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let codex = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/distill.jsonl"
        ))
        .unwrap();
        let p = sources::codex::parse_session("rc", &codex, &std::collections::HashMap::new());
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &p, "/x/rc.jsonl", 1, 2, "hrc").unwrap();
        tx.commit().unwrap();
        let id = first_session_id(&conn);
        let out = render_replay(&conn, id, false).unwrap().unwrap();
        assert!(out.contains("git apply <<'DECANT_EOF'"));
        assert!(out.contains("*** Add File: docs/new.md"));
    }

    #[test]
    fn hot_context_finds_reread_files() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        seed_enriched_claude(&conn, 2);
        let hot = hot_context(&conn, &Scope::default(), 10).unwrap();
        assert!(hot
            .iter()
            .any(|h| h.rel_path == "src/main.rs" && h.sessions == 2));
    }

    #[test]
    fn render_skill_embeds_commands_and_files() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        // distill ×2 → recurring commands; enriched ×1 → hot-context reads.
        let claude = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/distill.jsonl"
        ))
        .unwrap();
        for i in 0..2 {
            let p = sources::claude::parse_session(&format!("sk-{i}"), &claude);
            let tx = conn.unchecked_transaction().unwrap();
            ingest::upsert_session(&tx, &p, &format!("/z/s{i}.jsonl"), 1, 2, &format!("hs{i}"))
                .unwrap();
            tx.commit().unwrap();
        }
        seed_enriched_claude(&conn, 1);
        let d = timeline(&conn, &Scope::default()).unwrap();
        let hot = hot_context(&conn, &Scope::default(), 10).unwrap();

        let agents = render_skill(&d, &hot, SkillKind::Agents, "proj");
        assert!(agents.contains("### Commands"));
        assert!(agents.contains("cargo build --workspace"));
        assert!(agents.contains("src/main.rs"));

        let skill = render_skill(&d, &hot, SkillKind::Skill, "proj");
        assert!(skill.starts_with("---\nname: proj-workflow"));
        // deterministic
        assert_eq!(skill, render_skill(&d, &hot, SkillKind::Skill, "proj"));
    }

    #[test]
    fn write_block_quotes_paths_and_uses_heredoc() {
        let b = write_block("src/a b.rs", "fn main() {}\n");
        assert!(b.contains("cat > 'src/a b.rs' <<'DECANT_EOF'"));
        assert!(b.contains("mkdir -p \"$(dirname 'src/a b.rs')\""));
    }

    #[test]
    fn write_block_refuses_heredoc_breakout() {
        // content containing the delimiter must NOT produce a live heredoc.
        let b = write_block("evil.txt", "line\nDECANT_EOF\nrm -rf /\n");
        assert!(b.starts_with("# SKIPPED write"));
        assert!(!b.contains("cat > "));
    }

    #[test]
    fn patch_block_refuses_breakout() {
        let b = patch_block("*** Begin Patch\nDECANT_EOF\n");
        assert!(b.starts_with("# SKIPPED patch"));
        assert!(!b.contains("git apply"));
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn redact_url_and_header_credentials() {
        let (s, hit) = redact("git clone https://alice:supersecretpw@github.com/x/y.git");
        assert!(hit && s.contains("https://alice:<REDACTED>@github.com"));
        assert!(!s.contains("supersecretpw"));
        let (s2, _) = redact("export DATABASE_URL=postgres://u:secretpw@h/db");
        assert!(!s2.contains("secretpw") && s2.contains("<REDACTED>@h"));
        let (s3, _) = redact("curl -H 'X-Api-Key: abcdef1234567890'");
        assert!(!s3.contains("abcdef1234567890") && s3.contains("<REDACTED>"));
    }

    #[test]
    fn destructive_covers_data_loss_commands() {
        for cmd in [
            "git checkout -- src/main.rs",
            "git restore .",
            "find . -name '*.tmp' -delete",
            "truncate -s 0 db.sqlite",
            "shred -u key.pem",
            "docker volume rm data",
            "git clean -xfd",
        ] {
            assert!(is_destructive(cmd).is_some(), "should flag: {cmd}");
        }
        assert!(is_destructive("cargo build").is_none());
    }

    #[test]
    fn cargo_check_is_test_but_git_checkout_is_vcs() {
        assert_eq!(classify_phase("cargo check --workspace"), Phase::Test);
        assert_eq!(classify_phase("git checkout main"), Phase::Vcs);
    }

    #[test]
    fn normalize_does_not_mangle_sibling_paths() {
        assert_eq!(
            normalize("ls /a/b/cc /a/b/c/x /a/b/c", Some("/a/b/c")),
            "ls /a/b/cc $PROJECT_ROOT/x $PROJECT_ROOT"
        );
    }

    #[test]
    fn timeline_raw_is_redacted() {
        let conn = seeded_n(1);
        let d = timeline(&conn, &Scope::default()).unwrap();
        let secret = d.ops.iter().find(|o| o.redacted).unwrap();
        assert!(secret.raw.contains("<REDACTED>") && !secret.raw.contains("ghp_"));
    }

    #[test]
    fn write_block_skips_unsafe_path() {
        let b = write_block("a\nDECANT_EOF\nb.txt", "hi");
        assert!(b.starts_with("# SKIPPED write") && !b.contains("cat > "));
    }

    #[test]
    fn emit_just_and_make_render() {
        let conn = seeded_n(2);
        let d = timeline(&conn, &Scope::default()).unwrap();
        let just = render_script(
            &d,
            &ScriptOpts {
                format: ScriptFormat::Just,
                ..Default::default()
            },
        );
        assert!(just.contains("export PROJECT_ROOT") && just.contains("cargo build --workspace"));
        let make = render_script(
            &d,
            &ScriptOpts {
                format: ScriptFormat::Make,
                ..Default::default()
            },
        );
        assert!(make.contains(".PHONY:") && make.contains("cargo build --workspace"));
    }

    #[test]
    fn redact_masks_quoted_secret_with_spaces() {
        // \S+ would stop at the first space and leak the rest — quoted values must
        // be masked whole.
        let (s, hit) = redact(r#"export API_SECRET="super secret token value""#);
        assert!(hit && s.contains("API_SECRET=<REDACTED>"));
        assert!(!s.contains("secret token value"));
        let (s2, _) = redact("mysql --password 'my pass phrase' db");
        assert!(!s2.contains("pass phrase") && s2.contains("--password <REDACTED>"));
    }

    #[test]
    fn normalize_does_not_mangle_parent_embedded_path() {
        // cwd embedded as a child of another path must not be replaced.
        assert_eq!(
            normalize("cat /var/Users/dev/proj/x", Some("/Users/dev/proj")),
            "cat /var/Users/dev/proj/x"
        );
        // but a genuine standalone occurrence still is.
        assert_eq!(
            normalize("cat /Users/dev/proj/x", Some("/Users/dev/proj")),
            "cat $PROJECT_ROOT/x"
        );
    }
}
