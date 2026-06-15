//! Recommendations: data-derived **signals** + an evergreen **catalog** of
//! coding-agent enhancements, materialized with state into the `recommendation`
//! table at each sync.
//!
//! This is the Rust port of the web app's `Decant.Insights` (Elixir). The signal
//! rules and catalog entries (keys, titles, prompts, urls) are replicated
//! faithfully so the contract is identical whichever side computes them:
//!
//! - **error hotspots** — a tool with `calls >= 20` and `errors/calls >= 0.12`.
//! - **heavy MCP servers** — the top 3 servers, kept if `calls >= 50`.
//! - **heavy built-in tools** — the top 2 non-MCP tools, kept if `calls >= 200`.
//! - **cost concentration** — one model at `>= 40%` of total spend.
//!
//! `regenerate` UPSERTs the freshly-computed set by stable `key`, **preserving**
//! any existing `status`/`status_source`/`note`/`implemented_at` (an
//! `implemented` row is never clobbered back to `open`), and **auto-resolves**
//! activity-observable signals that no longer qualify (their error rate dropped,
//! etc.) to `implemented`/`activity`. Catalog entries are evergreen and are never
//! auto-resolved.
//!
//! UI-agnostic: this module computes and persists data; it never prints.

use crate::stats::{self, Dimension};
use crate::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

/// A single recommendation row: a signal or a catalog entry, with its display
/// fields and ranking `score`. State columns (status/source/note/timestamps)
/// live on the persisted row, not here — `current()` produces the freshly
/// computed *content*; `regenerate` merges it with stored state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recommendation {
    pub key: String,
    /// "signal" | "catalog".
    pub kind: String,
    pub category: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub suggestion: Option<String>,
    pub prompt: Option<String>,
    pub url: Option<String>,
    pub link_label: Option<String>,
    pub icon: Option<String>,
    pub tone: Option<String>,
    pub score: f64,
}

const SKILLS_URL: &str = "https://code.claude.com/docs/en/skills";

// Signals (ported from Decant.Insights). Highest-impact first; capped at 12.

/// Compute the data-derived signals from the archive, highest `score` first.
/// Mirrors `Decant.Insights.signals/1`: union the four rule families, sort by
/// score desc, take 12. Unlike the Elixir version the `score` is retained (the
/// table stores it for ranking); the order is identical.
pub fn signals(conn: &Connection) -> Result<Vec<Recommendation>> {
    let tools = stats::tool_usage(conn, false, 500)?;
    let mcp = stats::mcp_usage(conn, 500)?;
    let mut models = stats::by_dimension(conn, Dimension::Model)?;
    // Sort models by cost desc (Insights sorts the dimension rows by cost).
    models.sort_by(|a, b| {
        b.estimated_cost_usd
            .partial_cmp(&a.estimated_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out: Vec<Recommendation> = Vec::new();
    out.extend(error_hotspots(&tools));
    out.extend(heavy_servers(&mcp));
    out.extend(heavy_tools(&tools));
    out.extend(cost_concentration(&models));
    out.extend(hot_context_files(conn)?);
    out.extend(churn_files(conn)?);
    out.extend(search_heavy(conn)?);
    out.extend(abandoned_rate(conn)?);

    // Highest score first; stable so equal scores keep family/source order.
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(12);
    Ok(out)
}

fn server_suffix(server: Option<&str>) -> String {
    match server {
        Some(s) if !s.is_empty() => format!(" on {s}"),
        _ => String::new(),
    }
}

fn error_hotspots(tools: &[stats::ToolStatRow]) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for t in tools {
        if t.calls >= 20 && t.errors as f64 / t.calls as f64 >= 0.12 {
            let rate = t.errors as f64 / t.calls as f64;
            let pct = (rate * 100.0).round() as i64;
            let suffix = server_suffix(t.mcp_server.as_deref());
            out.push(Recommendation {
                key: format!("signal:error:{}", t.tool_name),
                kind: "signal".into(),
                category: None,
                title: format!("{} fails {}% of the time", t.tool_name, pct),
                detail: Some(format!(
                    "{} errors across {} calls{}.",
                    t.errors, t.calls, suffix
                )),
                suggestion: Some(
                    "Codify the recovery path as a Skill (or fix the call sites) so agents stop repeating this failure."
                        .into(),
                ),
                prompt: Some(format!(
                    "The {} tool is failing about {}% of the time ({} errors in {} calls){}. Investigate the common failure mode and codify a reusable Skill (or guardrail) so agents handle it consistently. Follow this repo's AGENTS.md and Skill conventions.",
                    t.tool_name, pct, t.errors, t.calls, suffix
                )),
                url: Some(SKILLS_URL.into()),
                link_label: Some("Skills guide".into()),
                icon: Some("hero-exclamation-triangle".into()),
                tone: Some("danger".into()),
                // score = error_rate * calls == errors (matches Insights: t.errors/t.calls * t.calls).
                score: rate * t.calls as f64,
            });
        }
    }
    out
}

fn heavy_servers(mcp: &[stats::McpStatRow]) -> Vec<Recommendation> {
    let mut out = Vec::new();
    for s in mcp.iter().take(3) {
        if s.calls >= 50 {
            out.push(Recommendation {
                key: format!("signal:heavy-server:{}", s.mcp_server),
                kind: "signal".into(),
                category: None,
                title: format!("Heavy reliance on the {} MCP server", s.mcp_server),
                detail: Some(format!("{} calls across {} tools.", s.calls, s.tools)),
                suggestion: Some(format!(
                    "Package the common {} workflows into a reusable Skill so agents use them consistently.",
                    s.mcp_server
                )),
                prompt: Some(format!(
                    "We rely heavily on the {} MCP server ({} calls across {} tools). Create a reusable Skill that packages our most common {} workflows so agents use them consistently. Follow this repo's Skill conventions.",
                    s.mcp_server, s.calls, s.tools, s.mcp_server
                )),
                url: Some(SKILLS_URL.into()),
                link_label: Some("Skills guide".into()),
                icon: Some("hero-cpu-chip".into()),
                tone: Some("accent".into()),
                score: s.calls as f64 / 2.0,
            });
        }
    }
    out
}

fn heavy_tools(tools: &[stats::ToolStatRow]) -> Vec<Recommendation> {
    // Built-in (non-MCP) tools only, in the most-called order `tool_usage` returns.
    let builtin: Vec<&stats::ToolStatRow> = tools.iter().filter(|t| t.tool_kind != "mcp").collect();
    let mut out = Vec::new();
    for t in builtin.into_iter().take(2) {
        if t.calls >= 200 {
            out.push(Recommendation {
                key: format!("signal:heavy-tool:{}", t.tool_name),
                kind: "signal".into(),
                category: None,
                title: format!("{} is one of your busiest tools", t.tool_name),
                detail: Some(format!("{} calls.", t.calls)),
                suggestion: Some(format!(
                    "High-frequency tools make good Skill candidates. Capture the patterns agents repeat around {}.",
                    t.tool_name
                )),
                prompt: Some(format!(
                    "We use the {} tool very frequently ({} calls). Identify the patterns we repeat around {} and codify them into a reusable Skill, following this repo's conventions.",
                    t.tool_name, t.calls, t.tool_name
                )),
                url: Some(SKILLS_URL.into()),
                link_label: Some("Skills guide".into()),
                icon: Some("hero-bolt".into()),
                tone: Some("info".into()),
                score: t.calls as f64 / 4.0,
            });
        }
    }
    out
}

fn cost_concentration(models: &[stats::DimRow]) -> Vec<Recommendation> {
    let Some(top) = models.first() else {
        return Vec::new();
    };
    let total: f64 = models.iter().map(|m| m.estimated_cost_usd).sum();
    if total > 0.0 && top.estimated_cost_usd / total >= 0.4 {
        let pct = (top.estimated_cost_usd / total * 100.0).round() as i64;
        vec![Recommendation {
            key: "signal:cost-concentration".into(),
            kind: "signal".into(),
            category: None,
            title: format!("{}% of spend is on {}", pct, top.key),
            detail: Some(format!(
                "{} of {} total.",
                fmt_usd(top.estimated_cost_usd),
                fmt_usd(total)
            )),
            suggestion: Some(
                "Consider routing routine sub-tasks to a cheaper model (sub-agents, simpler edits) to cut cost."
                    .into(),
            ),
            prompt: Some(format!(
                "About {}% of our agent spend is on {}. Propose and set up a model-routing strategy that uses cheaper models or subagents for routine work, and document it as guidance for this repo.",
                pct, top.key
            )),
            url: None,
            link_label: None,
            icon: Some("hero-currency-dollar".into()),
            tone: Some("warning".into()),
            score: 5.0,
        }]
    } else {
        Vec::new()
    }
}

/// `$X.XX` — matches Insights' `fmt_usd` (two decimals).
fn fmt_usd(n: f64) -> String {
    format!("${:.2}", n)
}

// Determinism-shifting signals (deterministic-enrichment spec §7): file-level
// evidence from `file_ref` + session facets, windowed to the last 30 days.
// The shared theme: when agents re-derive the same context every session,
// move it into AGENTS.md / a Skill — spend structure, not tokens.

const WINDOW: &str = "s.started_at >= date('now','-30 days')";
/// Distinct read sessions before a file counts as re-derived context.
const HOT_CONTEXT_MIN_SESSIONS: i64 = 8;
/// Edit sessions above which a "hot" file is churn, not stable context.
const HOT_CONTEXT_MAX_EDIT_SESSIONS: i64 = 2;
/// Distinct edit sessions before a file counts as a churn hotspot.
const CHURN_MIN_SESSIONS: i64 = 6;
/// Grep+Glob calls per session that flag discovery-heavy work, and the minimum
/// session count for the ratio to mean anything. (Tuned against the real
/// archive 2026-06-10: this archive averages 0.6 — agents here read directly —
/// so 5.0 keeps it quiet locally while tripping early for search-heavy repos.)
const SEARCH_HEAVY_RATIO: f64 = 5.0;
const SEARCH_HEAVY_MIN_SESSIONS: i64 = 20;
/// Abandoned share that flags stalling, over at least this many classified.
const ABANDONED_RATE: f64 = 0.25;
const ABANDONED_MIN_CLASSIFIED: i64 = 10;

/// Files read across many sessions but rarely edited: stable context agents
/// re-derive every time. Top 2 to keep the 12-signal budget balanced.
/// In-project files only (`rel_path IS NOT NULL`): out-of-project paths are
/// agent bookkeeping (memory indexes, automation notes) read by design, not a
/// distillation opportunity — validated against the real archive 2026-06-10.
fn hot_context_files(conn: &Connection) -> Result<Vec<Recommendation>> {
    let sql = format!(
        "SELECT f.rel_path AS key,
                COUNT(DISTINCT CASE WHEN f.operation = 'read' THEN f.session_id END) AS readers,
                COUNT(DISTINCT CASE WHEN f.operation IN ('edit','write','delete') THEN f.session_id END) AS editors
         FROM file_ref f JOIN session s ON s.id = f.session_id
         WHERE {WINDOW} AND f.rel_path IS NOT NULL
         GROUP BY key
         HAVING readers >= {HOT_CONTEXT_MIN_SESSIONS} AND editors <= {HOT_CONTEXT_MAX_EDIT_SESSIONS}
         ORDER BY readers DESC
         LIMIT 2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(path, readers)| Recommendation {
            key: format!("signal:hot-context:{path}"),
            kind: "signal".into(),
            category: None,
            title: format!("Agents re-read {path} in {readers} sessions this month"),
            detail: Some(format!(
                "{readers} distinct sessions read it in the last 30 days, with almost no edits — that's stable context being re-derived with tokens each time."
            )),
            suggestion: Some(format!(
                "Distill what agents need from {path} into AGENTS.md (or a Skill) so they stop re-reading and re-deriving it. Scaffold a starting point with `decant distill skill --kind agents` (scope with --project)."
            )),
            prompt: Some(format!(
                "Agents read {path} in {readers} separate sessions over the last 30 days while barely editing it. Scaffold a deterministic starting point with `decant distill skill --kind agents`, then read {path}, distill the parts agents actually need (contracts, invariants, gotchas) into AGENTS.md or a focused Skill, and keep the summary maintainable. Follow this repo's conventions."
            )),
            url: Some(SKILLS_URL.into()),
            link_label: Some("Skills guide".into()),
            icon: Some("hero-book-open".into()),
            tone: Some("accent".into()),
            score: readers as f64,
        })
        .collect())
}

/// Files edited across many sessions: complexity hotspots agents keep returning
/// to. Top 2.
fn churn_files(conn: &Connection) -> Result<Vec<Recommendation>> {
    let sql = format!(
        "SELECT f.rel_path AS key,
                COUNT(DISTINCT f.session_id) AS editors
         FROM file_ref f JOIN session s ON s.id = f.session_id
         WHERE {WINDOW} AND f.rel_path IS NOT NULL AND f.operation IN ('edit','write')
         GROUP BY key
         HAVING editors >= {CHURN_MIN_SESSIONS}
         ORDER BY editors DESC
         LIMIT 2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(path, editors)| Recommendation {
            key: format!("signal:churn:{path}"),
            kind: "signal".into(),
            category: None,
            title: format!("{path} keeps getting reworked"),
            detail: Some(format!(
                "Edited in {editors} distinct sessions over the last 30 days."
            )),
            suggestion: Some(format!(
                "A churn hotspot: consider a refactor, better tests, or clearer module docs so changes to {path} stop requiring agent archaeology."
            )),
            prompt: Some(format!(
                "{path} was edited in {editors} separate sessions in the last 30 days — a churn hotspot. Review it for unclear boundaries or missing tests, propose a focused refactor or documentation that makes future changes cheaper, and implement it following this repo's conventions."
            )),
            url: None,
            link_label: None,
            icon: Some("hero-fire".into()),
            tone: Some("warning".into()),
            score: editors as f64 * 1.5,
        })
        .collect())
}

/// Heavy Grep/Glob discovery per session: agents are searching for structure
/// the repo could just tell them.
fn search_heavy(conn: &Connection) -> Result<Vec<Recommendation>> {
    let sql = format!(
        "SELECT COUNT(*) AS searches, COUNT(DISTINCT s.id) AS sessions
         FROM tool_call tc JOIN session s ON s.id = tc.session_id
         WHERE {WINDOW} AND tc.tool_name IN ('Grep','Glob')"
    );
    let (searches, sessions): (i64, i64) =
        conn.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let in_window: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM session s WHERE {WINDOW}"),
        [],
        |r| r.get(0),
    )?;
    if in_window < SEARCH_HEAVY_MIN_SESSIONS || sessions == 0 {
        return Ok(Vec::new());
    }
    let ratio = searches as f64 / in_window as f64;
    if ratio < SEARCH_HEAVY_RATIO {
        return Ok(Vec::new());
    }
    Ok(vec![Recommendation {
        key: "signal:search-heavy".into(),
        kind: "signal".into(),
        category: None,
        title: format!("{ratio:.0} searches per session — discovery is expensive"),
        detail: Some(format!(
            "{searches} Grep/Glob calls across {in_window} sessions in the last 30 days."
        )),
        suggestion: Some(
            "Agents grep for structure the repo could state once: add a code map / module index to AGENTS.md so discovery is a read, not a search loop."
                .into(),
        ),
        prompt: Some(format!(
            "Our agents average {ratio:.0} Grep/Glob searches per session ({searches} over the last 30 days). Build a concise code map — key modules, their responsibilities, where common things live — and add it to AGENTS.md so agents can navigate by reading instead of searching."
        )),
        url: None,
        link_label: None,
        icon: Some("hero-magnifying-glass".into()),
        tone: Some("info".into()),
        score: ratio * 2.0,
    }])
}

/// Too many sessions classified abandoned: work is stalling before completion.
fn abandoned_rate(conn: &Connection) -> Result<Vec<Recommendation>> {
    let sql = format!(
        "SELECT COUNT(*) AS classified,
                SUM(s.outcome = 'abandoned') AS abandoned
         FROM session s
         WHERE {WINDOW} AND s.outcome IS NOT NULL"
    );
    let (classified, abandoned): (i64, Option<i64>) =
        conn.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let abandoned = abandoned.unwrap_or(0);
    if classified < ABANDONED_MIN_CLASSIFIED {
        return Ok(Vec::new());
    }
    let rate = abandoned as f64 / classified as f64;
    if rate <= ABANDONED_RATE {
        return Ok(Vec::new());
    }
    let pct = (rate * 100.0).round() as i64;
    Ok(vec![Recommendation {
        key: "signal:abandoned-rate".into(),
        kind: "signal".into(),
        category: None,
        title: format!("{pct}% of recent sessions end abandoned"),
        detail: Some(format!(
            "{abandoned} of {classified} classified sessions in the last 30 days ended mid-flight (interrupted, unanswered, or stopped between tool calls)."
        )),
        suggestion: Some(
            "Review where sessions stall: unclear prompts, missing context, or work that should be decomposed. Decant's session list filters by outcome=abandoned."
                .into(),
        ),
        prompt: Some(format!(
            "{pct}% of our last 30 days' agent sessions ended abandoned. List recent abandoned sessions (filter outcome=abandoned), find the common stall points, and propose concrete fixes — better AGENTS.md context, decomposed tasks, or skills for the repeated parts."
        )),
        url: None,
        link_label: None,
        icon: Some("hero-hand-raised".into()),
        tone: Some("danger".into()),
        score: pct as f64 / 3.0,
    }])
}

// Catalog (evergreen). Ported verbatim from Decant.Insights.catalog/0.
// Keys/titles/prompts/urls are part of the contract — do not change.

/// The curated, evergreen catalog of coding-agent enhancements. Keys are stable
/// (`catalog:<key>`) so state survives re-sync. `score` is 0.0 — catalog entries
/// are ordered by their fixed sequence, not ranked against signals.
pub fn catalog() -> Vec<Recommendation> {
    let cat = |key: &str,
               category: &str,
               icon: &str,
               title: &str,
               detail: &str,
               url: &str,
               link_label: &str,
               prompt: &str|
     -> Recommendation {
        Recommendation {
            key: format!("catalog:{key}"),
            kind: "catalog".into(),
            category: Some(category.into()),
            title: title.into(),
            detail: Some(detail.into()),
            suggestion: None,
            prompt: Some(prompt.into()),
            url: Some(url.into()),
            link_label: Some(link_label.into()),
            icon: Some(icon.into()),
            tone: None,
            score: 0.0,
        }
    };

    vec![
        cat(
            "agents-md",
            "Foundations",
            "hero-document-text",
            "AGENTS.md at the repo root",
            "One machine-readable contract of build and test commands, conventions, and boundaries that every agent reads first. Start here.",
            "https://agents.md",
            "agents.md standard",
            "Create a high-quality AGENTS.md at this repo root following the agents.md standard. Include the exact build, test, and lint commands, the conventions, and the boundaries. Keep it concise and command-first.",
        ),
        cat(
            "claude-md",
            "Foundations",
            "hero-book-open",
            "Project memory (CLAUDE.md)",
            "Persistent facts and conventions every session loads automatically so you stop re-explaining the same context.",
            "https://code.claude.com/docs/en/memory",
            "Memory guide",
            "Create a concise CLAUDE.md capturing this repo's durable facts and conventions (architecture, commands, gotchas), following the Claude Code memory guide.",
        ),
        cat(
            "skills",
            "Reusable workflows",
            "hero-sparkles",
            "Skills",
            "Capture a repeated procedure once as a SKILL.md. The agent loads it only when relevant and applies it the same way every time.",
            SKILLS_URL,
            "Skills guide",
            "Scaffold a reusable Skill (SKILL.md) for a workflow I repeat often in this repo, following the Agent Skills standard and the Claude Code Skills guide.",
        ),
        cat(
            "slash-commands",
            "Reusable workflows",
            "hero-command-line",
            "Custom slash commands",
            "Turn your most frequent multi-step requests into one-word commands your whole team can run.",
            "https://code.claude.com/docs/en/commands",
            "Commands reference",
            "Create custom slash commands for the requests I make most often in this repo, following the Claude Code commands reference.",
        ),
        cat(
            "subagents",
            "Reusable workflows",
            "hero-squares-2x2",
            "Subagent-driven development",
            "Fan out independent tasks to fresh subagents with isolated context, then review. Higher quality and faster iteration.",
            "https://code.claude.com/docs/en/sub-agents",
            "Subagents guide",
            "Set up a subagent-driven development workflow for this repo (a fresh subagent per task with a two-stage spec and quality review), following the Claude Code subagents guide.",
        ),
        cat(
            "mcp",
            "Connect and automate",
            "hero-cpu-chip",
            "MCP servers for your tools",
            "Typed, auditable access to GitHub, Linear, databases and more through the Model Context Protocol. No more pasting data into chat.",
            "https://code.claude.com/docs/en/mcp",
            "MCP setup guide",
            "Recommend MCP servers for the tools and services this project uses, then help me configure them following the Claude Code MCP guide.",
        ),
        cat(
            "hooks",
            "Connect and automate",
            "hero-bolt",
            "Hooks that keep the tree green",
            "Run format, lint, and tests automatically on agent events such as before each commit, so the working tree never drifts.",
            "https://code.claude.com/docs/en/hooks-guide",
            "Hooks guide",
            "Set up Claude Code hooks for this repo to run format, lint, and tests automatically (for example on file edits and pre-commit), following the hooks guide.",
        ),
    ]
}

/// The full freshly-computed recommendation set: signals (ranked) followed by
/// the evergreen catalog. This is the *content* set; persisted state is merged
/// in `regenerate`.
pub fn current(conn: &Connection) -> Result<Vec<Recommendation>> {
    let mut out = signals(conn)?;
    out.extend(catalog());
    Ok(out)
}

// Persistence: UPSERT preserving state + auto-resolve.

/// Regenerate the `recommendation` table from the archive's current state.
///
/// For every freshly-computed recommendation, UPSERT by `key`:
/// - **insert** new keys as `status='open'`, stamping `first_seen_at`/`updated_at`;
/// - **update** the display content + `score` of existing keys, refreshing
///   `updated_at` but **preserving** `status`, `status_source`, `note`,
///   `implemented_at` (an `implemented` row stays implemented, even if it
///   re-qualifies as a signal).
///
/// Then **auto-resolve**: any *signal* row currently `open` whose `key` is no
/// longer in the freshly-computed set (e.g. an error hotspot whose rate dropped
/// below threshold) is flipped to `status='implemented'`,
/// `status_source='activity'`, `implemented_at=now`. Catalog rows are evergreen
/// and never auto-resolved (they always regenerate, so they never fall out of
/// the set anyway, but the auto-resolve is scoped to `kind='signal'` for safety).
///
/// All writes run in one transaction so a partial regeneration is never visible.
pub fn regenerate(conn: &Connection) -> Result<()> {
    let recs = current(conn)?;
    let now = now_rfc3339(conn)?;

    let tx = conn.unchecked_transaction()?;

    // 1) UPSERT each computed recommendation, preserving state on conflict.
    {
        let mut stmt = tx.prepare(
            "INSERT INTO recommendation
               (key, kind, category, title, detail, suggestion, prompt, url,
                link_label, icon, tone, score, status, status_source, note,
                first_seen_at, updated_at, implemented_at)
             VALUES
               (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                'open', NULL, NULL, ?13, ?13, NULL)
             ON CONFLICT(key) DO UPDATE SET
               kind        = excluded.kind,
               category    = excluded.category,
               title       = excluded.title,
               detail      = excluded.detail,
               suggestion  = excluded.suggestion,
               prompt      = excluded.prompt,
               url         = excluded.url,
               link_label  = excluded.link_label,
               icon        = excluded.icon,
               tone        = excluded.tone,
               score       = excluded.score,
               updated_at  = excluded.updated_at",
        )?;
        for r in &recs {
            stmt.execute(params![
                r.key,
                r.kind,
                r.category,
                r.title,
                r.detail,
                r.suggestion,
                r.prompt,
                r.url,
                r.link_label,
                r.icon,
                r.tone,
                r.score,
                now,
            ])?;
        }
    }

    // 2) Auto-resolve open signals that no longer qualify. Build the set of
    // current signal keys; flip any open signal row not in it.
    let current_signal_keys: Vec<String> = recs
        .iter()
        .filter(|r| r.kind == "signal")
        .map(|r| r.key.clone())
        .collect();

    // NOT IN (...) with a dynamic placeholder list. When there are no current
    // signals, every open signal auto-resolves.
    let mut sql = String::from(
        "UPDATE recommendation
            SET status = 'implemented', status_source = 'activity',
                implemented_at = ?1, updated_at = ?1
          WHERE kind = 'signal' AND status = 'open'",
    );
    let mut bind: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Text(now.clone())];
    if !current_signal_keys.is_empty() {
        let placeholders: Vec<String> = (0..current_signal_keys.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        sql.push_str(&format!(" AND key NOT IN ({})", placeholders.join(", ")));
        for k in &current_signal_keys {
            bind.push(rusqlite::types::Value::Text(k.clone()));
        }
    }
    tx.execute(&sql, rusqlite::params_from_iter(bind.iter()))?;

    tx.commit()?;
    Ok(())
}

/// Mark a recommendation `implemented` by key (idempotent). Sets `status_source`
/// and an optional `note`; stamps `implemented_at`/`updated_at` only on the
/// first flip so re-marking an already-implemented key is a no-op for the
/// timestamp. Returns `true` if a row with that key exists (so callers can 404).
///
/// This is the write behind `POST /recommendations/mark-implemented`. It lives
/// in core (UI-agnostic) so the daemon and CLI share one implementation.
pub fn mark_implemented(
    conn: &Connection,
    key: &str,
    source: &str,
    note: Option<&str>,
) -> Result<bool> {
    let now = now_rfc3339(conn)?;
    let changed = conn.execute(
        "UPDATE recommendation
            SET status = 'implemented',
                status_source = ?2,
                note = COALESCE(?3, note),
                implemented_at = COALESCE(implemented_at, ?4),
                updated_at = ?4
          WHERE key = ?1",
        params![key, source, note, now],
    )?;
    Ok(changed > 0)
}

/// RFC3339 UTC, seconds precision (e.g. `2026-06-09T12:34:56Z`) — matches the
/// timestamps the daemon stamps elsewhere. Uses SQLite's own clock via the live
/// connection so core needs no extra time dependency.
fn now_rfc3339(conn: &Connection) -> Result<String> {
    Ok(
        conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |r| {
            r.get::<_, String>(0)
        })?,
    )
}

// Read side: list persisted recommendations with their state.

/// Which recommendations to return from [`list`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Open,
    Implemented,
    All,
}

impl StatusFilter {
    /// Parse the `?status=` query value. Defaults to `Open` for absent/unknown
    /// is the caller's job; this returns `None` on an unrecognized value so the
    /// caller can 400.
    pub fn parse(s: &str) -> Option<StatusFilter> {
        match s {
            "open" => Some(StatusFilter::Open),
            "implemented" => Some(StatusFilter::Implemented),
            "all" => Some(StatusFilter::All),
            _ => None,
        }
    }
}

/// A persisted recommendation row: the computed content plus its lifecycle
/// state. This is what the list endpoint serializes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredRecommendation {
    pub key: String,
    pub kind: String,
    pub category: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub suggestion: Option<String>,
    pub prompt: Option<String>,
    pub url: Option<String>,
    pub link_label: Option<String>,
    pub icon: Option<String>,
    pub tone: Option<String>,
    pub score: Option<f64>,
    pub status: String,
    pub status_source: Option<String>,
    pub note: Option<String>,
    pub first_seen_at: Option<String>,
    pub updated_at: Option<String>,
    pub implemented_at: Option<String>,
    pub memory_layer: Option<String>,
    pub promotion_target: Option<String>,
    pub trigger: Option<String>,
    pub evidence: Option<String>,
    pub action: Option<String>,
    pub success_metric: Option<String>,
}

/// List persisted recommendations filtered by `status`.
///
/// Ordering puts the most actionable first: open before implemented, then by
/// `score` desc (signals outrank the zero-scored catalog), with `key` as a
/// stable tiebreaker so the order is deterministic across calls.
pub fn list(conn: &Connection, status: StatusFilter) -> Result<Vec<StoredRecommendation>> {
    let where_c = match status {
        StatusFilter::Open => "WHERE status = 'open'",
        StatusFilter::Implemented => "WHERE status = 'implemented'",
        StatusFilter::All => "",
    };
    let sql = format!(
        "SELECT key, kind, category, title, detail, suggestion, prompt, url,
                link_label, icon, tone, score, status, status_source, note,
                first_seen_at, updated_at, implemented_at
         FROM recommendation
         {where_c}
         ORDER BY (status = 'open') DESC, score DESC, key ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            let mut rec = StoredRecommendation {
                key: r.get(0)?,
                kind: r.get(1)?,
                category: r.get(2)?,
                title: r.get(3)?,
                detail: r.get(4)?,
                suggestion: r.get(5)?,
                prompt: r.get(6)?,
                url: r.get(7)?,
                link_label: r.get(8)?,
                icon: r.get(9)?,
                tone: r.get(10)?,
                score: r.get(11)?,
                status: r.get(12)?,
                status_source: r.get(13)?,
                note: r.get(14)?,
                first_seen_at: r.get(15)?,
                updated_at: r.get(16)?,
                implemented_at: r.get(17)?,
                memory_layer: None,
                promotion_target: None,
                trigger: None,
                evidence: None,
                action: None,
                success_metric: None,
            };
            apply_promotion_card(&mut rec);
            Ok(rec)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn apply_promotion_card(rec: &mut StoredRecommendation) {
    let card = promotion_card(rec);
    rec.memory_layer = Some(card.memory_layer.into());
    rec.promotion_target = Some(card.promotion_target.into());
    rec.trigger = Some(card.trigger.into());
    rec.evidence = Some(card.evidence);
    rec.action = Some(card.action);
    rec.success_metric = Some(card.success_metric.into());
}

struct PromotionCard {
    memory_layer: &'static str,
    promotion_target: &'static str,
    trigger: &'static str,
    evidence: String,
    action: String,
    success_metric: &'static str,
}

fn promotion_card(rec: &StoredRecommendation) -> PromotionCard {
    let evidence = rec.detail.clone().unwrap_or_else(|| {
        if rec.kind == "catalog" {
            "Evergreen recommendation from Decant's coding-agent catalog.".to_string()
        } else {
            "Data-derived signal from the local session archive.".to_string()
        }
    });
    let action = rec
        .suggestion
        .as_ref()
        .or(rec.prompt.as_ref())
        .cloned()
        .unwrap_or_else(|| "Review the recommendation and promote the durable lesson.".to_string());

    if rec.key.starts_with("signal:error:") {
        return PromotionCard {
            memory_layer: "Procedural",
            promotion_target: "Skill or regression test",
            trigger: "Before future agents repeat this failing tool workflow.",
            evidence,
            action,
            success_metric: "Tool error rate falls below the signal threshold.",
        };
    }

    if rec.key.starts_with("signal:heavy-server:") || rec.key.starts_with("signal:heavy-tool:") {
        return PromotionCard {
            memory_layer: "Procedural",
            promotion_target: "Skill",
            trigger: "When future agents need this repeated tool or MCP workflow.",
            evidence,
            action,
            success_metric:
                "Repeated calls per session decline without reducing successful outcomes.",
        };
    }

    if rec.key == "signal:cost-concentration" {
        return PromotionCard {
            memory_layer: "Hot",
            promotion_target: "AGENTS.md model-routing rule",
            trigger: "Before routine work defaults to the most expensive model.",
            evidence,
            action,
            success_metric: "Spend concentration drops below 40 percent of archive cost.",
        };
    }

    if rec.key.starts_with("signal:hot-context:") {
        return PromotionCard {
            memory_layer: "Hot",
            promotion_target: "AGENTS.md or Skill",
            trigger: "At session start, before agents re-read stable context.",
            evidence,
            action,
            success_metric: "Repeat reads of the source file drop or the signal auto-resolves.",
        };
    }

    if rec.key.starts_with("signal:churn:") {
        return PromotionCard {
            memory_layer: "Cold",
            promotion_target: "Runbook or regression test",
            trigger: "Before editing a historically high-churn file.",
            evidence,
            action,
            success_metric: "Future edits land with fewer retries and stronger tests.",
        };
    }

    if rec.key == "signal:search-heavy" {
        return PromotionCard {
            memory_layer: "Hot",
            promotion_target: "AGENTS.md code map",
            trigger: "When agents need to navigate the repo structure.",
            evidence,
            action,
            success_metric: "Grep and Glob calls per session fall below the signal threshold.",
        };
    }

    if rec.key == "signal:abandoned-rate" {
        return PromotionCard {
            memory_layer: "Governance",
            promotion_target: "Planning checklist or Skill",
            trigger: "Before large, ambiguous tasks start.",
            evidence,
            action,
            success_metric: "Abandoned-session share drops below 25 percent.",
        };
    }

    match rec.key.as_str() {
        "catalog:agents-md" => PromotionCard {
            memory_layer: "Hot",
            promotion_target: "AGENTS.md",
            trigger: "Every coding-agent session in this repo.",
            evidence,
            action,
            success_metric:
                "Agents start with commands, boundaries, and invariants already loaded.",
        },
        "catalog:claude-md" => PromotionCard {
            memory_layer: "Hot",
            promotion_target: "Project memory",
            trigger: "Every Claude Code session for this repo.",
            evidence,
            action,
            success_metric: "Durable repo facts stop being re-explained in new sessions.",
        },
        "catalog:skills" => PromotionCard {
            memory_layer: "Procedural",
            promotion_target: "SKILL.md",
            trigger: "When a repeated workflow appears in a future task.",
            evidence,
            action,
            success_metric: "The workflow runs from a focused skill instead of being re-derived.",
        },
        "catalog:slash-commands" => PromotionCard {
            memory_layer: "Procedural",
            promotion_target: "Slash command",
            trigger: "When a frequent multi-step request recurs.",
            evidence,
            action,
            success_metric: "The repeated request becomes one consistent command.",
        },
        "catalog:subagents" => PromotionCard {
            memory_layer: "Governance",
            promotion_target: "Subagent workflow",
            trigger: "When work can be split into independent reviewable lanes.",
            evidence,
            action,
            success_metric:
                "Parallel lanes finish with explicit review and fewer context collisions.",
        },
        "catalog:mcp" => PromotionCard {
            memory_layer: "Cold",
            promotion_target: "MCP integration",
            trigger: "When agents need live tool data instead of pasted context.",
            evidence,
            action,
            success_metric: "Agents fetch source data directly and cite the actual tool result.",
        },
        "catalog:hooks" => PromotionCard {
            memory_layer: "Governance",
            promotion_target: "Hook or preflight gate",
            trigger: "Before changes drift away from the repo's validation contract.",
            evidence,
            action,
            success_metric: "Format, lint, and tests run earlier with fewer end-of-task surprises.",
        },
        _ => PromotionCard {
            memory_layer: "Cold",
            promotion_target: "Runbook",
            trigger: "When this recommendation becomes relevant again.",
            evidence,
            action,
            success_metric: "Future sessions can retrieve the lesson with cited evidence.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema};

    /// Insert `n` tool_call rows for one tool, `errors` of which are errors.
    /// `kind`/`server` set the classification. Each call belongs to session 1.
    fn seed_tool(
        conn: &Connection,
        name: &str,
        kind: &str,
        server: Option<&str>,
        n: i64,
        errors: i64,
    ) {
        for i in 0..n {
            let is_err = if i < errors { 1 } else { 0 };
            conn.execute(
                "INSERT INTO tool_call
                   (session_id, tool_kind, tool_name, mcp_server, is_error)
                 VALUES (1, ?1, ?2, ?3, ?4)",
                params![kind, name, server, is_err],
            )
            .unwrap();
        }
    }

    /// A migrated in-memory DB with one project + one session so `tool_call`'s
    /// FK to `session` is satisfiable. Sessions carry model + cost for the
    /// cost-concentration signal.
    fn base() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO project(id, path) VALUES (1, '/p');
             INSERT INTO session(id, tool, source_session_id, project_id, model, estimated_cost_usd)
                VALUES (1, 'claude_code', 's1', 1, 'claude-opus-4-7', 10.0);",
        )
        .unwrap();
        conn
    }

    fn keys(recs: &[Recommendation]) -> Vec<String> {
        recs.iter().map(|r| r.key.clone()).collect()
    }

    /// Insert `n` sessions started now, ids from `first_id`, each with one
    /// file_ref of `op` on `rel_path`. Drives the file-evidence signals.
    fn seed_file_sessions(conn: &Connection, first_id: i64, n: i64, rel_path: &str, op: &str) {
        for i in 0..n {
            let id = first_id + i;
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at)
                 VALUES (?1, 'claude_code', 'fs' || ?1, 1, datetime('now'))",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO file_ref(session_id, path, rel_path, ext, operation)
                 VALUES (?1, '/p/' || ?2, ?2, 'rs', ?3)",
                params![id, rel_path, op],
            )
            .unwrap();
        }
    }

    #[test]
    fn hot_context_signal_fires_for_read_only_hot_files() {
        let conn = base();
        seed_file_sessions(&conn, 100, 9, "AGENTS.md", "read");
        let recs = signals(&conn).unwrap();
        let hot = recs
            .iter()
            .find(|r| r.key == "signal:hot-context:AGENTS.md")
            .expect("hot-context signal must fire at 9 read sessions");
        assert_eq!(hot.tone.as_deref(), Some("accent"));
        assert!(hot.suggestion.as_deref().unwrap().contains("AGENTS.md"));
        // advice → runnable command (closes the loop with `decant distill`).
        assert!(hot
            .suggestion
            .as_deref()
            .unwrap()
            .contains("decant distill skill"));
        assert!(hot.score > 0.0);
    }

    #[test]
    fn hot_context_signal_suppressed_when_file_is_also_edited() {
        let conn = base();
        seed_file_sessions(&conn, 100, 9, "src/hot.rs", "read");
        seed_file_sessions(&conn, 200, 3, "src/hot.rs", "edit");
        let recs = signals(&conn).unwrap();
        assert!(
            !keys(&recs).contains(&"signal:hot-context:src/hot.rs".to_string()),
            "files under active churn are not stable context to distill"
        );
        // ...but 3 edit sessions is below the churn threshold too.
        assert!(!keys(&recs).contains(&"signal:churn:src/hot.rs".to_string()));
    }

    #[test]
    fn churn_signal_fires_for_repeatedly_edited_files() {
        let conn = base();
        seed_file_sessions(&conn, 100, 7, "src/parser.rs", "edit");
        let recs = signals(&conn).unwrap();
        let churn = recs
            .iter()
            .find(|r| r.key == "signal:churn:src/parser.rs")
            .expect("churn signal must fire at 7 edit sessions");
        assert_eq!(churn.tone.as_deref(), Some("warning"));
        assert!(churn.detail.as_deref().unwrap().contains("7"));
    }

    #[test]
    fn abandoned_rate_signal_fires_above_quarter() {
        let conn = base();
        // 12 classified sessions in-window: 4 abandoned (33%).
        for i in 0..12 {
            let outcome = if i < 4 { "abandoned" } else { "completed" };
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at, outcome)
                 VALUES (?1, 'claude_code', 'ab' || ?1, 1, datetime('now'), ?2)",
                params![300 + i, outcome],
            )
            .unwrap();
        }
        let recs = signals(&conn).unwrap();
        let ab = recs
            .iter()
            .find(|r| r.key == "signal:abandoned-rate")
            .expect("abandoned-rate must fire at 33%");
        assert!(ab.title.contains("33%"));
    }

    #[test]
    fn search_heavy_signal_fires_on_high_discovery_ratio() {
        let conn = base();
        // 20 in-window sessions, 8+ Grep/Glob calls per session on average.
        for i in 0..20 {
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at)
                 VALUES (?1, 'claude_code', 'sh' || ?1, 1, datetime('now'))",
                params![400 + i],
            )
            .unwrap();
            for _ in 0..9 {
                conn.execute(
                    "INSERT INTO tool_call(session_id, tool_kind, tool_name, timestamp)
                     VALUES (?1, 'builtin', 'Grep', datetime('now'))",
                    params![400 + i],
                )
                .unwrap();
            }
        }
        let recs = signals(&conn).unwrap();
        let sh = recs
            .iter()
            .find(|r| r.key == "signal:search-heavy")
            .expect("search-heavy must fire at 9 searches/session over 20 sessions");
        assert!(sh.suggestion.as_deref().unwrap().contains("AGENTS.md"));
    }

    #[test]
    fn file_signals_ignore_out_of_project_paths() {
        let conn = base();
        // 9 read sessions on an out-of-project (absolute, rel_path NULL) file —
        // agent bookkeeping like memory indexes, read by design.
        for i in 0..9 {
            let id = 500 + i;
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at)
                 VALUES (?1, 'claude_code', 'oop' || ?1, 1, datetime('now'))",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO file_ref(session_id, path, rel_path, ext, operation)
                 VALUES (?1, '/home/x/.claude/memory/MEMORY.md', NULL, 'md', 'read')",
                params![id],
            )
            .unwrap();
        }
        let recs = signals(&conn).unwrap();
        assert!(
            !keys(&recs)
                .iter()
                .any(|k| k.starts_with("signal:hot-context:")),
            "out-of-project bookkeeping must not produce distillation signals"
        );
    }

    #[test]
    fn file_signals_stay_quiet_below_thresholds() {
        let conn = base();
        seed_file_sessions(&conn, 100, 7, "calm.md", "read"); // < 8 readers
        seed_file_sessions(&conn, 200, 5, "calm.rs", "edit"); // < 6 editors
        let recs = signals(&conn).unwrap();
        let ks = keys(&recs);
        assert!(!ks.iter().any(|k| k.starts_with("signal:hot-context:")));
        assert!(!ks.iter().any(|k| k.starts_with("signal:churn:")));
        assert!(!ks.contains(&"signal:abandoned-rate".to_string()));
        assert!(!ks.contains(&"signal:search-heavy".to_string()));
    }

    #[test]
    fn catalog_keys_and_order_match_the_reference() {
        let cat = catalog();
        assert_eq!(
            keys(&cat),
            vec![
                "catalog:agents-md",
                "catalog:claude-md",
                "catalog:skills",
                "catalog:slash-commands",
                "catalog:subagents",
                "catalog:mcp",
                "catalog:hooks",
            ]
        );
        // Spot-check the spotlight entry's exact title/url/prompt (contract).
        let agents = &cat[0];
        assert_eq!(agents.title, "AGENTS.md at the repo root");
        assert_eq!(agents.url.as_deref(), Some("https://agents.md"));
        assert_eq!(agents.category.as_deref(), Some("Foundations"));
        assert!(agents
            .prompt
            .as_deref()
            .unwrap()
            .starts_with("Create a high-quality AGENTS.md"));
        // Every catalog entry is kind=catalog with score 0.
        assert!(cat.iter().all(|r| r.kind == "catalog" && r.score == 0.0));
    }

    #[test]
    fn error_hotspot_fires_at_threshold_and_carries_server_suffix() {
        let conn = base();
        // 25 calls, 5 errors = 20% >= 12%, and calls >= 20: fires.
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        let sig = signals(&conn).unwrap();
        let hot = sig
            .iter()
            .find(|r| r.key == "signal:error:fetch")
            .expect("error hotspot");
        assert_eq!(hot.kind, "signal");
        assert_eq!(hot.tone.as_deref(), Some("danger"));
        assert_eq!(hot.title, "fetch fails 20% of the time");
        assert_eq!(
            hot.detail.as_deref(),
            Some("5 errors across 25 calls on svc.")
        );
        assert!(hot.prompt.as_deref().unwrap().contains("on svc"));
        // score == errors (5)
        assert!((hot.score - 5.0).abs() < 1e-9);
    }

    #[test]
    fn error_hotspot_below_threshold_does_not_fire() {
        let conn = base();
        // 19 calls (< 20): no fire even at 100% error.
        seed_tool(&conn, "rare", "builtin", None, 19, 19);
        // 30 calls but only 3 errors = 10% (< 12%): no fire.
        seed_tool(&conn, "okay", "builtin", None, 30, 3);
        let sig = signals(&conn).unwrap();
        assert!(!keys(&sig).iter().any(|k| k.starts_with("signal:error:")));
    }

    #[test]
    fn heavy_server_fires_at_50_calls() {
        let conn = base();
        seed_tool(&conn, "mcp__svc__a", "mcp", Some("svc"), 60, 0);
        let sig = signals(&conn).unwrap();
        let s = sig
            .iter()
            .find(|r| r.key == "signal:heavy-server:svc")
            .expect("heavy server");
        assert_eq!(s.title, "Heavy reliance on the svc MCP server");
        assert_eq!(s.tone.as_deref(), Some("accent"));
        assert!((s.score - 30.0).abs() < 1e-9); // calls/2
    }

    #[test]
    fn heavy_tool_fires_at_200_builtin_calls() {
        let conn = base();
        seed_tool(&conn, "Bash", "builtin", None, 250, 0);
        let sig = signals(&conn).unwrap();
        let t = sig
            .iter()
            .find(|r| r.key == "signal:heavy-tool:Bash")
            .expect("heavy tool");
        assert_eq!(t.title, "Bash is one of your busiest tools");
        assert_eq!(t.tone.as_deref(), Some("info"));
        assert!((t.score - 62.5).abs() < 1e-9); // calls/4
                                                // MCP tools are excluded from heavy-tool even when very busy.
        seed_tool(&conn, "mcp__x__y", "mcp", Some("x"), 300, 0);
        let sig2 = signals(&conn).unwrap();
        assert!(!keys(&sig2)
            .iter()
            .any(|k| k == "signal:heavy-tool:mcp__x__y"));
    }

    #[test]
    fn cost_concentration_fires_when_one_model_dominates() {
        let conn = base();
        // session 1 already has model claude-opus-4-7 @ $10. Add a cheap session.
        conn.execute_batch(
            "INSERT INTO session(id, tool, source_session_id, project_id, model, estimated_cost_usd)
                VALUES (2, 'claude_code', 's2', 1, 'claude-haiku', 2.0);",
        )
        .unwrap();
        // $10 / $12 = 83% >= 40%: fires for opus.
        let sig = signals(&conn).unwrap();
        let c = sig
            .iter()
            .find(|r| r.key == "signal:cost-concentration")
            .expect("cost concentration");
        assert_eq!(c.title, "83% of spend is on claude-opus-4-7");
        assert_eq!(c.detail.as_deref(), Some("$10.00 of $12.00 total."));
        assert_eq!(c.tone.as_deref(), Some("warning"));
        assert!(c.url.is_none()); // cost signal has no url in the reference
    }

    #[test]
    fn signals_capped_at_twelve_and_ranked_by_score() {
        let conn = base();
        // Many error hotspots; scores = error counts so order is by errors desc.
        for i in 0..15 {
            seed_tool(&conn, &format!("t{i}"), "builtin", None, 100, 20 + i);
        }
        let sig = signals(&conn).unwrap();
        assert_eq!(sig.len(), 12, "capped at 12");
        // Descending by score.
        for w in sig.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn current_is_signals_then_catalog() {
        let conn = base();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        let cur = current(&conn).unwrap();
        // Catalog (7 evergreen) always present.
        for k in [
            "catalog:agents-md",
            "catalog:claude-md",
            "catalog:skills",
            "catalog:hooks",
        ] {
            assert!(keys(&cur).contains(&k.to_string()), "missing {k}");
        }
        // The signal precedes the catalog block.
        let first_catalog = cur.iter().position(|r| r.kind == "catalog").unwrap();
        let sig_pos = cur
            .iter()
            .position(|r| r.key == "signal:error:fetch")
            .unwrap();
        assert!(sig_pos < first_catalog);
    }

    #[test]
    fn regenerate_persists_and_is_idempotent() {
        let conn = base();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        regenerate(&conn).unwrap();
        let after_first: i64 = conn
            .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
            .unwrap();
        // 2 signals (error hotspot + the base session's single-model cost
        // concentration) + 7 catalog.
        assert_eq!(after_first, 9);

        // Capture first_seen_at for the catalog spotlight.
        let first_seen: String = conn
            .query_row(
                "SELECT first_seen_at FROM recommendation WHERE key = 'catalog:agents-md'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // A second run upserts the same keys (no duplicates) and preserves
        // first_seen_at.
        regenerate(&conn).unwrap();
        let after_second: i64 = conn
            .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after_second, 9, "idempotent: no duplicate keys");
        let first_seen_again: String = conn
            .query_row(
                "SELECT first_seen_at FROM recommendation WHERE key = 'catalog:agents-md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first_seen, first_seen_again, "first_seen_at preserved");
    }

    #[test]
    fn regenerate_preserves_implemented_status_across_runs() {
        let conn = base();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        regenerate(&conn).unwrap();

        // Mark a catalog entry implemented (catalog always regenerates, so this
        // proves the UPSERT does not clobber state even when the key recurs).
        assert!(mark_implemented(&conn, "catalog:agents-md", "manual", Some("did it")).unwrap());

        regenerate(&conn).unwrap();
        let (status, source, note): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, status_source, note FROM recommendation WHERE key = 'catalog:agents-md'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "implemented", "implemented survives re-sync");
        assert_eq!(source.as_deref(), Some("manual"));
        assert_eq!(note.as_deref(), Some("did it"));
    }

    #[test]
    fn auto_resolve_flips_no_longer_qualifying_signal_to_activity() {
        let conn = base();
        // First sync: error hotspot qualifies (20% error rate).
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        regenerate(&conn).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM recommendation WHERE key = 'signal:error:fetch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "open");

        // Now the error rate drops: many clean calls so rate < 12% and the
        // signal no longer regenerates. Clear and reseed with a healthy mix.
        conn.execute("DELETE FROM tool_call", []).unwrap();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 100, 2); // 2% < 12%
        regenerate(&conn).unwrap();

        let (status, source): (String, Option<String>) = conn
            .query_row(
                "SELECT status, status_source FROM recommendation WHERE key = 'signal:error:fetch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "implemented", "auto-resolved");
        assert_eq!(source.as_deref(), Some("activity"));
        assert!(conn
            .query_row(
                "SELECT implemented_at FROM recommendation WHERE key = 'signal:error:fetch'",
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn auto_resolve_does_not_touch_catalog_entries() {
        let conn = base();
        regenerate(&conn).unwrap(); // catalog only (no signals)
        regenerate(&conn).unwrap(); // a second run must not auto-resolve catalog
        let implemented: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recommendation WHERE status = 'implemented'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(implemented, 0, "evergreen catalog is never auto-resolved");
    }

    #[test]
    fn mark_implemented_is_idempotent_and_keeps_first_timestamp() {
        let conn = base();
        regenerate(&conn).unwrap();
        assert!(mark_implemented(&conn, "catalog:skills", "agent", None).unwrap());
        let first_at: String = conn
            .query_row(
                "SELECT implemented_at FROM recommendation WHERE key = 'catalog:skills'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // Mark again: still returns true (row exists), timestamp unchanged.
        assert!(mark_implemented(&conn, "catalog:skills", "manual", None).unwrap());
        let second_at: String = conn
            .query_row(
                "SELECT implemented_at FROM recommendation WHERE key = 'catalog:skills'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first_at, second_at, "implemented_at stamped once");
        // status_source updates to the latest caller's source.
        let source: String = conn
            .query_row(
                "SELECT status_source FROM recommendation WHERE key = 'catalog:skills'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual");
    }

    #[test]
    fn mark_implemented_unknown_key_returns_false() {
        let conn = base();
        regenerate(&conn).unwrap();
        assert!(!mark_implemented(&conn, "catalog:does-not-exist", "manual", None).unwrap());
    }

    #[test]
    fn list_filters_by_status_and_orders_open_first() {
        let conn = base();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        regenerate(&conn).unwrap();
        mark_implemented(&conn, "catalog:agents-md", "manual", None).unwrap();

        let open = list(&conn, StatusFilter::Open).unwrap();
        assert!(open.iter().all(|r| r.status == "open"));
        assert!(!open.iter().any(|r| r.key == "catalog:agents-md"));
        // Signals (score > 0) outrank the zero-scored catalog entries: every
        // signal precedes the first catalog row.
        let first_catalog = open.iter().position(|r| r.kind == "catalog").unwrap();
        assert!(open[..first_catalog].iter().all(|r| r.kind == "signal"));
        assert!(open[first_catalog..].iter().all(|r| r.kind == "catalog"));

        let implemented = list(&conn, StatusFilter::Implemented).unwrap();
        assert_eq!(implemented.len(), 1);
        assert_eq!(implemented[0].key, "catalog:agents-md");

        let all = list(&conn, StatusFilter::All).unwrap();
        // 2 signals + 7 catalog.
        assert_eq!(all.len(), 9);
        // Open rows come before implemented ones.
        let first_impl = all.iter().position(|r| r.status == "implemented").unwrap();
        assert!(all[..first_impl].iter().all(|r| r.status == "open"));
    }

    #[test]
    fn cost_concentration_quiet_when_no_models_or_no_dominance() {
        // No models at all → no signal (empty `models.first()`).
        assert!(cost_concentration(&[]).is_empty());

        // Two models, neither at 40% of total → no signal (the else branch).
        let split = vec![
            stats::DimRow {
                key: "a".into(),
                sessions: 1,
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 5.0,
            },
            stats::DimRow {
                key: "b".into(),
                sessions: 1,
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 5.0,
            },
        ];
        // Top is 50% here — that *would* fire. Make it not dominate:
        let balanced = vec![
            stats::DimRow {
                key: "a".into(),
                sessions: 1,
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 3.0,
            },
            stats::DimRow {
                key: "b".into(),
                sessions: 1,
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 3.0,
            },
            stats::DimRow {
                key: "c".into(),
                sessions: 1,
                input_tokens: 0,
                output_tokens: 0,
                estimated_cost_usd: 3.0,
            },
        ];
        assert!(
            cost_concentration(&balanced).is_empty(),
            "33% top share is below the 40% threshold"
        );
        // Zero total cost → no signal even with a top row.
        let _ = split; // (50% would fire; kept only to document the boundary)
        let zero = vec![stats::DimRow {
            key: "a".into(),
            sessions: 1,
            input_tokens: 0,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
        }];
        assert!(cost_concentration(&zero).is_empty(), "no spend, no signal");
    }

    #[test]
    fn search_heavy_quiet_when_ratio_below_threshold() {
        let conn = base();
        // 20 in-window sessions but only one Grep call total → ratio far below
        // SEARCH_HEAVY_RATIO, so the signal does not fire.
        for i in 0..20 {
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at)
                 VALUES (?1, 'claude_code', 'lo' || ?1, 1, datetime('now'))",
                params![600 + i],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO tool_call(session_id, tool_kind, tool_name, timestamp)
             VALUES (600, 'builtin', 'Grep', datetime('now'))",
            [],
        )
        .unwrap();
        let recs = signals(&conn).unwrap();
        assert!(!keys(&recs).contains(&"signal:search-heavy".to_string()));
    }

    #[test]
    fn abandoned_rate_quiet_at_or_below_threshold() {
        let conn = base();
        // 12 classified, 3 abandoned = 25% which is NOT > 25% → no signal.
        for i in 0..12 {
            let outcome = if i < 3 { "abandoned" } else { "completed" };
            conn.execute(
                "INSERT INTO session(id, tool, source_session_id, project_id, started_at, outcome)
                 VALUES (?1, 'claude_code', 'aq' || ?1, 1, datetime('now'), ?2)",
                params![700 + i, outcome],
            )
            .unwrap();
        }
        let recs = signals(&conn).unwrap();
        assert!(!keys(&recs).contains(&"signal:abandoned-rate".to_string()));
    }

    #[test]
    fn search_heavy_propagates_db_error() {
        // No `session` table -> the query_row `?` in `search_heavy` propagates.
        let bare = db::open_in_memory().unwrap();
        assert!(search_heavy(&bare).is_err());
    }

    #[test]
    fn mark_implemented_propagates_db_error() {
        // No `recommendation` table -> the UPDATE `?` in `mark_implemented`
        // propagates.
        let bare = db::open_in_memory().unwrap();
        assert!(mark_implemented(&bare, "k", "agent", None).is_err());
    }

    #[test]
    fn regenerate_propagates_insert_error() {
        // `current` succeeds (it produces signals from the seeded data) and the
        // INSERT statement prepares fine, but a BEFORE INSERT trigger aborts the
        // per-recommendation `stmt.execute` so the loop-body `?` propagates.
        let conn = base();
        seed_tool(&conn, "fetch", "mcp", Some("svc"), 25, 5);
        conn.execute_batch(
            "CREATE TRIGGER block_rec BEFORE INSERT ON recommendation
               BEGIN SELECT RAISE(ABORT, 'no insert'); END;",
        )
        .unwrap();
        assert!(regenerate(&conn).is_err());
    }

    #[test]
    fn list_adds_promotion_card_fields_without_schema_state() {
        let conn = base();
        seed_file_sessions(&conn, 100, 9, "AGENTS.md", "read");
        regenerate(&conn).unwrap();

        let rows = list(&conn, StatusFilter::All).unwrap();
        let hot = rows
            .iter()
            .find(|r| r.key == "signal:hot-context:AGENTS.md")
            .expect("hot-context recommendation");
        assert_eq!(hot.memory_layer.as_deref(), Some("Hot"));
        assert_eq!(hot.promotion_target.as_deref(), Some("AGENTS.md or Skill"));
        assert!(hot.trigger.as_deref().unwrap().contains("session start"));
        assert!(hot
            .evidence
            .as_deref()
            .unwrap()
            .contains("distinct sessions"));
        assert!(hot.action.as_deref().unwrap().contains("Distill"));
        assert!(hot
            .success_metric
            .as_deref()
            .unwrap()
            .contains("auto-resolves"));

        let hooks = rows
            .iter()
            .find(|r| r.key == "catalog:hooks")
            .expect("hooks catalog recommendation");
        assert_eq!(hooks.memory_layer.as_deref(), Some("Governance"));
        assert_eq!(
            hooks.promotion_target.as_deref(),
            Some("Hook or preflight gate")
        );
    }

    #[test]
    fn promotion_card_maps_every_key_family() {
        let cases = [
            (
                "signal:error:Read",
                "signal",
                "Procedural",
                "Skill or regression test",
            ),
            ("signal:heavy-server:Exa", "signal", "Procedural", "Skill"),
            ("signal:heavy-tool:Bash", "signal", "Procedural", "Skill"),
            (
                "signal:cost-concentration",
                "signal",
                "Hot",
                "AGENTS.md model-routing rule",
            ),
            (
                "signal:hot-context:AGENTS.md",
                "signal",
                "Hot",
                "AGENTS.md or Skill",
            ),
            (
                "signal:churn:src/lib.rs",
                "signal",
                "Cold",
                "Runbook or regression test",
            ),
            ("signal:search-heavy", "signal", "Hot", "AGENTS.md code map"),
            (
                "signal:abandoned-rate",
                "signal",
                "Governance",
                "Planning checklist or Skill",
            ),
            ("catalog:agents-md", "catalog", "Hot", "AGENTS.md"),
            ("catalog:claude-md", "catalog", "Hot", "Project memory"),
            ("catalog:skills", "catalog", "Procedural", "SKILL.md"),
            (
                "catalog:slash-commands",
                "catalog",
                "Procedural",
                "Slash command",
            ),
            (
                "catalog:subagents",
                "catalog",
                "Governance",
                "Subagent workflow",
            ),
            ("catalog:mcp", "catalog", "Cold", "MCP integration"),
            (
                "catalog:hooks",
                "catalog",
                "Governance",
                "Hook or preflight gate",
            ),
            ("signal:unknown", "signal", "Cold", "Runbook"),
        ];

        for (key, kind, layer, target) in cases {
            let card = promotion_card(&stored_rec(key, kind));
            assert_eq!(card.memory_layer, layer, "{key}");
            assert_eq!(card.promotion_target, target, "{key}");
        }

        let catalog = promotion_card(&stored_rec("catalog:unknown", "catalog"));
        assert_eq!(
            catalog.evidence,
            "Evergreen recommendation from Decant's coding-agent catalog."
        );
        assert_eq!(
            catalog.action,
            "Review the recommendation and promote the durable lesson."
        );

        let mut prompted = stored_rec("signal:unknown", "signal");
        prompted.prompt = Some("Use the prompt".to_string());
        let prompted = promotion_card(&prompted);
        assert_eq!(
            prompted.evidence,
            "Data-derived signal from the local session archive."
        );
        assert_eq!(prompted.action, "Use the prompt");
    }

    fn stored_rec(key: &str, kind: &str) -> StoredRecommendation {
        StoredRecommendation {
            key: key.to_string(),
            kind: kind.to_string(),
            category: None,
            title: "Title".to_string(),
            detail: None,
            suggestion: None,
            prompt: None,
            url: None,
            link_label: None,
            icon: None,
            tone: None,
            score: None,
            status: "open".to_string(),
            status_source: None,
            note: None,
            first_seen_at: None,
            updated_at: None,
            implemented_at: None,
            memory_layer: None,
            promotion_target: None,
            trigger: None,
            evidence: None,
            action: None,
            success_metric: None,
        }
    }

    #[test]
    fn status_filter_parse() {
        assert_eq!(StatusFilter::parse("open"), Some(StatusFilter::Open));
        assert_eq!(
            StatusFilter::parse("implemented"),
            Some(StatusFilter::Implemented)
        );
        assert_eq!(StatusFilter::parse("all"), Some(StatusFilter::All));
        assert_eq!(StatusFilter::parse("bogus"), None);
    }
}
