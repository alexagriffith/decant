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

// ---------------------------------------------------------------------------
// Signals (ported from Decant.Insights). Highest-impact first; capped at 12.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Catalog (evergreen). Ported verbatim from Decant.Insights.catalog/0.
// Keys/titles/prompts/urls are part of the contract — do not change.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Persistence: UPSERT preserving state + auto-resolve.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Read side: list persisted recommendations with their state.
// ---------------------------------------------------------------------------

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
            Ok(StoredRecommendation {
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
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
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
