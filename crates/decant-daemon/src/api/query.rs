//! Read queries backing the `/api/v1` endpoints.
//!
//! These mirror `web/lib/decant/archive.ex` (the contract the Phoenix app
//! enforced today) but add filter support across the board, keyset pagination,
//! and the daemon-only aggregations (activity, sparklines, date-bounds). They run
//! against a pooled **read** connection and return plain DTOs the handlers wrap
//! in the envelope.
//!
//! Why daemon-side and not in `decant-core`: core's `query`/`stats` functions are
//! the CLI's stable surface and don't carry the dashboard filter set or cursor
//! pagination. Reshaping them would churn the CLI. The aggregation SQL is small,
//! and core stays UI-agnostic. We do reuse core's `stats::Dimension`,
//! `tools::classify_tool`, and `cost` helpers.

use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;
use serde::Serialize;

use super::cursor::Cursor;
use super::envelope::ApiError;
use super::filters::Filters;
use decant_core::stats::Dimension;

/// Default and max page size for paginated endpoints.
pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 200;

/// Clamp a requested limit into `[1, MAX_LIMIT]`, defaulting when absent/<=0.
pub fn clamp_limit(requested: Option<i64>) -> i64 {
    match requested {
        Some(n) if n > 0 => n.min(MAX_LIMIT),
        _ => DEFAULT_LIMIT,
    }
}

/// A session row for list/detail. Includes cache token totals (the web summary
/// omitted them; the API contract in spec §5 includes "token totals incl. cache").
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub tool: String,
    pub source_session_id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub message_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cache_creation_tokens: i64,
    /// Codex `reasoning_output_tokens`, summed per session. A breakdown of
    /// `total_output_tokens` (not additive), so it is never priced separately.
    /// Claude reports none, so it is 0 there.
    pub total_reasoning_tokens: i64,
    /// Estimated reasoning tokens when the tool reports no exact count (Claude, by
    /// subtraction). Soft; pair with `reasoning_source` to know which to trust.
    pub est_reasoning_tokens: i64,
    /// `reported` (exact, Codex) | `inferred` (estimated, Claude) | `none`.
    pub reasoning_source: Option<String>,
    pub estimated_cost_usd: f64,
    pub is_archived: bool,
    pub facets: SessionFacets,
}

/// Deterministic per-session enrichment (schema v4): facet counters plus the
/// heuristic outcome / work-type labels. Embedded in every session summary.
#[derive(Debug, Serialize)]
pub struct SessionFacets {
    pub turn_count: i64,
    pub error_count: i64,
    pub interruption_count: i64,
    pub compaction_count: i64,
    pub sidechain_message_count: i64,
    pub agent_spawn_count: i64,
    pub skill_count: i64,
    pub command_count: i64,
    pub thinking_block_count: i64,
    pub thinking_chars: i64,
    pub active_seconds: i64,
    pub outcome: Option<String>,
    pub work_type: Option<String>,
}

const SESSION_COLS: &str = "s.id, s.tool, s.source_session_id, s.title, s.model, p.path, \
     s.started_at, s.ended_at, s.message_count, s.total_input_tokens, s.total_output_tokens, \
     s.total_cache_read_tokens, s.total_cache_creation_tokens, s.estimated_cost_usd, s.is_archived, \
     s.turn_count, s.error_count, s.interruption_count, s.compaction_count, \
     s.sidechain_message_count, s.agent_spawn_count, s.skill_count, s.command_count, \
     s.thinking_block_count, s.thinking_chars, s.active_seconds, s.outcome, s.work_type, \
     s.total_reasoning_tokens, s.est_reasoning_tokens, s.reasoning_source";

fn map_summary(r: &rusqlite::Row) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: r.get(0)?,
        tool: r.get(1)?,
        source_session_id: r.get(2)?,
        title: r.get(3)?,
        model: r.get(4)?,
        project: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
        message_count: r.get(8)?,
        total_input_tokens: r.get(9)?,
        total_output_tokens: r.get(10)?,
        total_cache_read_tokens: r.get(11)?,
        total_cache_creation_tokens: r.get(12)?,
        estimated_cost_usd: r.get(13)?,
        is_archived: r.get::<_, i64>(14)? != 0,
        // Appended to SESSION_COLS, so these land after the facet columns (15..27).
        total_reasoning_tokens: r.get(28)?,
        est_reasoning_tokens: r.get(29)?,
        reasoning_source: r.get(30)?,
        facets: SessionFacets {
            turn_count: r.get(15)?,
            error_count: r.get(16)?,
            interruption_count: r.get(17)?,
            compaction_count: r.get(18)?,
            sidechain_message_count: r.get(19)?,
            agent_spawn_count: r.get(20)?,
            skill_count: r.get(21)?,
            command_count: r.get(22)?,
            thinking_block_count: r.get(23)?,
            thinking_chars: r.get(24)?,
            active_seconds: r.get(25)?,
            outcome: r.get(26)?,
            work_type: r.get(27)?,
        },
    })
}

/// Supported sort orders for `/sessions`. Each maps to a `(sort column, direction)`
/// with `s.id` as the deterministic tie-breaker for keyset pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSort {
    StartedAtDesc,
    StartedAtAsc,
    CostDesc,
}

impl SessionSort {
    pub fn parse(s: Option<&str>) -> Result<SessionSort, ApiError> {
        match s.unwrap_or("started_at_desc") {
            "started_at_desc" => Ok(SessionSort::StartedAtDesc),
            "started_at_asc" => Ok(SessionSort::StartedAtAsc),
            "cost_desc" => Ok(SessionSort::CostDesc),
            other => Err(ApiError::bad_request(format!("unknown sort {other:?}"))
                .with_hint("one of: started_at_desc, started_at_asc, cost_desc")),
        }
    }

    /// The sort column expression (used in SELECT, ORDER BY, and the cursor key).
    fn column(self) -> &'static str {
        match self {
            SessionSort::StartedAtDesc | SessionSort::StartedAtAsc => "s.started_at",
            SessionSort::CostDesc => "s.estimated_cost_usd",
        }
    }

    /// True if descending.
    fn desc(self) -> bool {
        !matches!(self, SessionSort::StartedAtAsc)
    }
}

/// One page of sessions plus the total matching the filters.
#[derive(Debug)]
pub struct SessionPage {
    pub sessions: Vec<SessionSummary>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub total_count: i64,
}

/// List sessions matching `filters`, keyset-paginated. Fetches `limit + 1` rows to
/// detect a further page without a second count, and runs a scoped COUNT(*) for
/// `total_count`.
pub fn list_sessions(
    conn: &Connection,
    filters: &Filters,
    sort: SessionSort,
    limit: i64,
    cursor: Option<Cursor>,
) -> Result<SessionPage, ApiError> {
    let where_c = filters.where_clause();
    let col = sort.column();
    let dir = if sort.desc() { "DESC" } else { "ASC" };
    let cmp = if sort.desc() { "<" } else { ">" };

    // Bind params: filters first, then (optionally) the keyset boundary, then the
    // LIMIT. We build the vector to match the `?` order in the SQL exactly.
    let mut params: Vec<SqlValue> = where_c.params;

    // Keyset predicate: continue strictly after the cursor's (sort_key, id).
    // COALESCE(...,'') groups NULL sort keys with the empty string so the boundary
    // comparison is consistent with the ORDER BY below. The predicate references
    // the sort_key bind twice and the rowid once, so we push [sort_key, sort_key,
    // rowid] in that order.
    let keyset = if let Some(c) = &cursor {
        params.push(sort_key_to_sql(&c.sort_key));
        params.push(sort_key_to_sql(&c.sort_key));
        params.push(SqlValue::Integer(c.rowid));
        format!(
            " AND (COALESCE({col},'') {cmp} COALESCE(?,'') \
               OR (COALESCE({col},'') = COALESCE(?,'') AND s.id {cmp} ?))"
        )
    } else {
        String::new()
    };

    let sql = format!(
        "SELECT {SESSION_COLS} \
         FROM session s LEFT JOIN project p ON p.id = s.project_id \
         WHERE {}{keyset} \
         ORDER BY COALESCE({col},'') {dir}, s.id {dir} \
         LIMIT ?",
        where_c.sql
    );
    params.push(SqlValue::Integer(limit + 1));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<SessionSummary> = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), map_summary)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        rows.last().map(|s| {
            let sk = match sort {
                SessionSort::CostDesc => Some(s.estimated_cost_usd.to_string()),
                _ => s.started_at.clone(),
            };
            Cursor::new(s.id, sk).encode()
        })
    } else {
        None
    };

    // Scoped total (filters only, no keyset).
    let total_count = count_sessions(conn, filters)?;

    Ok(SessionPage {
        sessions: rows,
        next_cursor,
        has_more,
        total_count,
    })
}

fn count_sessions(conn: &Connection, filters: &Filters) -> Result<i64, ApiError> {
    let where_c = filters.where_clause();
    let sql = format!(
        "SELECT COUNT(*) FROM session s LEFT JOIN project p ON p.id = s.project_id WHERE {}",
        where_c.sql
    );
    let n: i64 = conn.query_row(
        &sql,
        rusqlite::params_from_iter(where_c.params.iter()),
        |r| r.get(0),
    )?;
    Ok(n)
}

fn sort_key_to_sql(sk: &Option<String>) -> SqlValue {
    match sk {
        Some(s) => SqlValue::Text(s.clone()),
        None => SqlValue::Null,
    }
}

#[derive(Debug, Serialize)]
pub struct CostBreakdown {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    pub total: f64,
}

#[derive(Debug, Serialize)]
pub struct SessionStats {
    pub duration_seconds: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Reasoning sub-component of `output_tokens` (Codex only; see
    /// [`SessionSummary::total_reasoning_tokens`]). Not priced separately.
    pub reasoning_tokens: i64,
    /// Estimated reasoning when no exact count exists (Claude). Soft; see
    /// `reasoning_source`.
    pub est_reasoning_tokens: i64,
    /// `reported` | `inferred` | `none` — provenance of the reasoning figures.
    pub reasoning_source: Option<String>,
    pub estimated_cost_usd: f64,
    pub cost_breakdown: CostBreakdown,
}

#[derive(Debug, Serialize)]
pub struct BlockView {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub tool_result: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub seq: i64,
    pub role: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub blocks: Vec<BlockView>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub stats: SessionStats,
    pub messages: Vec<MessageView>,
}

/// Full session detail: summary + computed stats (incl. cost breakdown) + ordered
/// messages with their blocks. `Ok(None)` if the id does not exist.
pub fn get_session(conn: &Connection, id: i64) -> Result<Option<SessionDetail>, ApiError> {
    let summary = {
        let sql = format!(
            "SELECT {SESSION_COLS} FROM session s LEFT JOIN project p ON p.id = s.project_id \
             WHERE s.id = ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map([id], map_summary)?;
        match rows.next() {
            Some(r) => r?,
            None => return Ok(None),
        }
    };

    let stats = session_stats(&summary);

    let mut stmt = conn.prepare(
        "SELECT m.id, m.seq, m.role, m.timestamp, m.model, m.input_tokens, m.output_tokens, \
                b.type, b.text, b.tool_name, b.tool_input, b.tool_result \
         FROM message m LEFT JOIN block b ON b.message_id = m.id \
         WHERE m.session_id = ? ORDER BY m.seq, b.ordinal",
    )?;
    type Row = (
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = stmt
        .query_map([id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut messages: Vec<MessageView> = Vec::new();
    let mut current: Option<i64> = None;
    for (mid, seq, role, ts, model, intok, outtok, btype, text, tname, tin, tres) in rows {
        if current != Some(mid) {
            current = Some(mid);
            messages.push(MessageView {
                seq,
                role: role.unwrap_or_else(|| "unknown".into()),
                timestamp: ts,
                model,
                input_tokens: intok,
                output_tokens: outtok,
                blocks: Vec::new(),
            });
        }
        if let Some(block_type) = btype {
            if let Some(last) = messages.last_mut() {
                last.blocks.push(BlockView {
                    block_type,
                    text,
                    tool_name: tname,
                    tool_input: tin,
                    tool_result: tres,
                });
            }
        }
    }

    Ok(Some(SessionDetail {
        summary,
        stats,
        messages,
    }))
}

/// Compute per-session stats from the (already loaded) summary: wall-clock
/// duration and a per-component cost breakdown using core's pricing.
fn session_stats(s: &SessionSummary) -> SessionStats {
    use decant_core::cost::{default_pricing, estimate_cost};
    use decant_core::model::TokenUsage;

    let pricing = default_pricing();
    let model = s.model.as_deref();
    let component = |usage: TokenUsage| estimate_cost(model, &usage, &pricing);

    let input = component(TokenUsage {
        input: s.total_input_tokens,
        ..Default::default()
    });
    let output = component(TokenUsage {
        output: s.total_output_tokens,
        ..Default::default()
    });
    let cache_read = component(TokenUsage {
        cache_read: s.total_cache_read_tokens,
        ..Default::default()
    });
    let cache_creation = component(TokenUsage {
        cache_creation: s.total_cache_creation_tokens,
        ..Default::default()
    });

    SessionStats {
        duration_seconds: duration_seconds(s.started_at.as_deref(), s.ended_at.as_deref()),
        input_tokens: s.total_input_tokens,
        output_tokens: s.total_output_tokens,
        cache_read_tokens: s.total_cache_read_tokens,
        cache_creation_tokens: s.total_cache_creation_tokens,
        // A breakdown of output, already priced as output — so it is reported
        // alongside the tokens but contributes nothing extra to cost_breakdown.
        reasoning_tokens: s.total_reasoning_tokens,
        est_reasoning_tokens: s.est_reasoning_tokens,
        reasoning_source: s.reasoning_source.clone(),
        estimated_cost_usd: s.estimated_cost_usd,
        cost_breakdown: CostBreakdown {
            input,
            output,
            cache_read,
            cache_creation,
            total: input + output + cache_read + cache_creation,
        },
    }
}

/// Seconds between two RFC3339 timestamps, clamped to >= 0. None if either is
/// absent/unparseable. Mirrors `Archive.duration_seconds/2`.
fn duration_seconds(a: Option<&str>, b: Option<&str>) -> Option<i64> {
    let (a, b) = (a?, b?);
    let da = chrono::DateTime::parse_from_rfc3339(a).ok()?;
    let db = chrono::DateTime::parse_from_rfc3339(b).ok()?;
    Some((db - da).num_seconds().max(0))
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub session_id: i64,
    pub session_title: Option<String>,
    pub tool: String,
    pub block_id: i64,
    pub snippet: String,
}

#[derive(Debug)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// FTS5 search over blocks, ranked by bm25, keyset-paginated by rank position.
///
/// Because bm25 rank is a float with no stable tie-breaker exposed as a column,
/// we paginate with a simple OFFSET-as-cursor: the cursor carries the count of
/// hits already returned. This is stable enough for search (results are
/// read-mostly) and keeps the contract's cursor shape (`{rowid, sort_key}` where
/// `rowid` is the offset). Malformed FTS queries surface as a 400 via
/// [`is_fts_syntax_error`].
pub fn search(
    conn: &Connection,
    query: &str,
    limit: i64,
    cursor: Option<Cursor>,
) -> Result<SearchPage, ApiError> {
    let q = query.trim();
    if q.is_empty() {
        return Err(ApiError::bad_request("`q` must not be empty")
            .with_hint("provide a search term, e.g. {\"q\": \"auth\"}"));
    }
    let offset = cursor.as_ref().map(|c| c.rowid.max(0)).unwrap_or(0);

    let sql = "SELECT b.session_id, s.title, s.tool, b.id, \
                COALESCE(snippet(block_fts, 0, '[', ']', '…', 12), \
                         snippet(block_fts, 1, '[', ']', '…', 12), \
                         snippet(block_fts, 2, '[', ']', '…', 12), '') AS snip \
         FROM block_fts \
         JOIN block b ON b.id = block_fts.rowid \
         JOIN session s ON s.id = b.session_id \
         WHERE block_fts MATCH ?1 \
         ORDER BY bm25(block_fts) \
         LIMIT ?2 OFFSET ?3";

    let mut stmt = conn.prepare(sql)?;
    let mapped = stmt.query_map(rusqlite::params![q, limit + 1, offset], |r| {
        Ok(SearchHit {
            session_id: r.get(0)?,
            session_title: r.get(1)?,
            tool: r.get(2)?,
            block_id: r.get(3)?,
            snippet: r.get(4)?,
        })
    });
    let mut hits: Vec<SearchHit> = match mapped {
        Ok(iter) => iter
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(map_fts_err)?,
        Err(e) => return Err(map_fts_err(e)),
    };

    let has_more = hits.len() as i64 > limit;
    if has_more {
        hits.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        Some(Cursor::new(offset + limit, None).encode())
    } else {
        None
    };

    Ok(SearchPage {
        hits,
        next_cursor,
        has_more,
    })
}

/// Map a rusqlite error to a 400 if it is an FTS5 syntax error, else a 500.
fn map_fts_err(e: rusqlite::Error) -> ApiError {
    if is_fts_syntax_error(&e) {
        ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "INVALID_QUERY",
            "malformed search query",
        )
        .with_hint(
            "FTS5 syntax: terms, \"quoted phrases\", AND/OR/NOT, prefix*; \
             avoid unbalanced quotes or a trailing operator",
        )
    } else {
        ApiError::internal(e.to_string())
    }
}

/// Detect FTS5 query syntax errors so they map to 400, not 500. SQLite raises
/// these as `SqliteFailure` with a message mentioning fts5 / MATCH / syntax.
pub fn is_fts_syntax_error(e: &rusqlite::Error) -> bool {
    if let rusqlite::Error::SqliteFailure(_, Some(msg)) = e {
        let m = msg.to_ascii_lowercase();
        m.contains("fts5")
            || m.contains("malformed match")
            || m.contains("unterminated string")
            || m.contains("no such column") && m.contains("match")
            || m.contains("syntax error") && m.contains("match")
    } else {
        false
    }
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub sessions: i64,
    pub messages: i64,
    pub tool_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Reasoning sub-component of `output_tokens`, exact (Codex only). Not additive.
    pub reasoning_tokens: i64,
    /// Estimated reasoning where no exact count exists (Claude). Reported
    /// separately so exact and estimated figures never mix in one number.
    pub est_reasoning_tokens: i64,
    pub estimated_cost_usd: f64,
}

/// Whole-archive rollup scoped to `filters` (spec §5 `/analytics/summary`).
pub fn totals(conn: &Connection, filters: &Filters) -> Result<Totals, ApiError> {
    let where_c = filters.where_clause();
    let p = || rusqlite::params_from_iter(where_c.params.iter());

    let (sessions, intok, outtok, cread, ccreate, reasoning, est_reasoning, cost) = conn
        .query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(s.total_input_tokens),0), \
                    COALESCE(SUM(s.total_output_tokens),0), \
                    COALESCE(SUM(s.total_cache_read_tokens),0), \
                    COALESCE(SUM(s.total_cache_creation_tokens),0), \
                    COALESCE(SUM(s.total_reasoning_tokens),0), \
                    COALESCE(SUM(s.est_reasoning_tokens),0), \
                    COALESCE(SUM(s.estimated_cost_usd),0.0) \
             FROM session s WHERE {}",
                where_c.sql
            ),
            p(),
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )?;

    let messages: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM message m JOIN session s ON s.id = m.session_id WHERE {}",
            where_c.sql
        ),
        p(),
        |r| r.get(0),
    )?;
    let tool_calls: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM tool_call tc JOIN session s ON s.id = tc.session_id WHERE {}",
            where_c.sql
        ),
        p(),
        |r| r.get(0),
    )?;

    Ok(Totals {
        sessions,
        messages,
        tool_calls,
        input_tokens: intok,
        output_tokens: outtok,
        cache_read_tokens: cread,
        cache_creation_tokens: ccreate,
        reasoning_tokens: reasoning,
        est_reasoning_tokens: est_reasoning,
        estimated_cost_usd: cost,
    })
}

#[derive(Debug, Serialize)]
pub struct DimRow {
    pub key: String,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Reasoning sub-component of `output_tokens`, exact (Codex only). Not additive.
    pub reasoning_tokens: i64,
    /// Estimated reasoning where no exact count exists (Claude).
    pub est_reasoning_tokens: i64,
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

#[derive(Debug)]
pub struct DimPage {
    pub rows: Vec<DimRow>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub total_count: i64,
}

/// Parse the `dim` query param into core's `Dimension`.
pub fn parse_dimension(s: Option<&str>) -> Result<Dimension, ApiError> {
    let s = s.ok_or_else(|| {
        ApiError::bad_request("missing `dim`").with_hint("dim=tool|model|project|day")
    })?;
    Dimension::parse(s).ok_or_else(|| {
        ApiError::invalid_filter(format!("unknown dim {s:?}"))
            .with_hint("one of: tool, model, project, day")
    })
}

/// Per-dimension rollup scoped to `filters`, ordered by session count desc, then
/// key asc as a stable tie-breaker for offset pagination of high-cardinality
/// dimensions (project/day/model). The cursor carries the offset in `rowid`.
///
/// The project dimension rolls worktrees up under their resolved `root_path` and
/// reports how many were folded in; passing `root` switches to the per-worktree
/// leaf breakdown scoped to that root (one row per project path under it).
pub fn by_dimension(
    conn: &Connection,
    dim: Dimension,
    filters: &Filters,
    limit: i64,
    cursor: Option<Cursor>,
    root: Option<&str>,
) -> Result<DimPage, ApiError> {
    let project_leaf = matches!(dim, Dimension::Project) && root.is_some();

    // (group expr, join, three trailing select columns, optional leading predicate)
    let (expr, join, extra_cols, root_pred): (&str, &str, &str, &str) = match dim {
        Dimension::Tool => ("s.tool", "", "NULL, NULL, NULL", ""),
        Dimension::Model => ("COALESCE(s.model, '(unknown)')", "", "NULL, NULL, NULL", ""),
        Dimension::Project if project_leaf => (
            "p.path",
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
                COALESCE(SUM(s.total_reasoning_tokens),0), \
                COALESCE(SUM(s.est_reasoning_tokens),0), \
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
                reasoning_tokens: r.get(4)?,
                est_reasoning_tokens: r.get(5)?,
                estimated_cost_usd: r.get(6)?,
                worktree_count: r.get::<_, Option<i64>>(7)?,
                worktree_label: r.get::<_, Option<String>>(8)?,
                worktree_tool: r.get::<_, Option<String>>(9)?,
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

#[derive(Debug, Serialize)]
pub struct Activity {
    /// 24 ints, index = local hour 0..23.
    pub by_hour: Vec<i64>,
    /// 7 ints, index = local weekday 0..6 (0 = Sunday).
    pub by_weekday: Vec<i64>,
    /// IANA-ish label for the server local zone (best-effort; "local" fallback).
    pub timezone: String,
    /// Index of the peak hour / weekday (None if no data).
    pub peak_hour: Option<usize>,
    pub peak_weekday: Option<usize>,
}

/// Session activity bucketed into local-time hour and weekday histograms,
/// matching `Archive.activity/1`: `strftime('%H'/'%w', started_at, 'localtime')`
/// over rows with a non-null `started_at`, padded to 24/7.
pub fn activity(conn: &Connection, filters: &Filters) -> Result<Activity, ApiError> {
    let by_hour = bucket(
        conn,
        "strftime('%H', s.started_at, 'localtime')",
        filters,
        24,
    )?;
    let by_weekday = bucket(
        conn,
        "strftime('%w', s.started_at, 'localtime')",
        filters,
        7,
    )?;
    Ok(Activity {
        peak_hour: peak(&by_hour),
        peak_weekday: peak(&by_weekday),
        by_hour,
        by_weekday,
        timezone: local_timezone(),
    })
}

/// Run one `GROUP BY strftime(...)` bucket and project into a `size`-length array
/// indexed by the integer bucket key (zero-filled).
fn bucket(
    conn: &Connection,
    expr: &str,
    filters: &Filters,
    size: usize,
) -> Result<Vec<i64>, ApiError> {
    let where_c = filters.where_clause();
    let sql = format!(
        "SELECT {expr} AS k, COUNT(*) FROM session s \
         WHERE {} AND s.started_at IS NOT NULL GROUP BY k",
        where_c.sql
    );
    let mut out = vec![0i64; size];
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(where_c.params.iter()), |r| {
        Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (key, count) = row?;
        if let Some(k) =
            key.and_then(|s| s.trim_start_matches('0').parse::<usize>().ok().or(Some(0)))
        {
            if k < size {
                out[k] = count;
            }
        }
    }
    Ok(out)
}

fn peak(counts: &[i64]) -> Option<usize> {
    counts
        .iter()
        .enumerate()
        .filter(|(_, &c)| c > 0)
        .max_by_key(|(_, &c)| c)
        .map(|(i, _)| i)
}

/// Best-effort local timezone label. chrono's `Local` doesn't expose an IANA
/// name portably, so we report the current UTC offset (e.g. "UTC-07:00").
fn local_timezone() -> String {
    use chrono::Offset;
    let offset = chrono::Local::now().offset().fix();
    let secs = offset.local_minus_utc();
    let sign = if secs < 0 { '-' } else { '+' };
    let secs = secs.abs();
    format!("UTC{sign}{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

#[derive(Debug, Serialize)]
pub struct ModelSparklines {
    /// model -> per-day session counts, aligned to `days`.
    pub models: std::collections::BTreeMap<String, Vec<i64>>,
    /// sorted distinct days (YYYY-MM-DD) forming the shared x-axis.
    pub days: Vec<String>,
}

/// Per-model daily session counts on a shared day axis, scoped to `filters`.
/// Mirrors `Archive.model_sparklines/1`.
pub fn model_sparklines(conn: &Connection, filters: &Filters) -> Result<ModelSparklines, ApiError> {
    let where_c = filters.where_clause();
    let sql = format!(
        "SELECT COALESCE(s.model,'(unknown)') k, substr(s.started_at,1,10) d, COUNT(*) c \
         FROM session s WHERE {} AND s.started_at IS NOT NULL GROUP BY k, d",
        where_c.sql
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map(rusqlite::params_from_iter(where_c.params.iter()), |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut day_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (_, d, _) in &rows {
        day_set.insert(d.clone());
    }
    let days: Vec<String> = day_set.into_iter().collect();
    let day_index: std::collections::HashMap<&str, usize> = days
        .iter()
        .enumerate()
        .map(|(i, d)| (d.as_str(), i))
        .collect();

    let mut models: std::collections::BTreeMap<String, Vec<i64>> =
        std::collections::BTreeMap::new();
    for (model, day, count) in &rows {
        let entry = models
            .entry(model.clone())
            .or_insert_with(|| vec![0i64; days.len()]);
        if let Some(&i) = day_index.get(day.as_str()) {
            entry[i] = *count;
        }
    }

    Ok(ModelSparklines { models, days })
}

#[derive(Debug, Serialize)]
pub struct ToolRow {
    pub tool_name: String,
    pub tool_kind: String,
    pub mcp_server: Option<String>,
    pub calls: i64,
    pub errors: i64,
    pub error_rate: f64,
}

/// Per-tool usage scoped to `filters`, most-called first. `errors_only` keeps
/// only tools with >0 errors. Mirrors `Archive.tool_usage/2` and computes
/// `error_rate = errors / calls`.
pub fn tool_usage(
    conn: &Connection,
    filters: &Filters,
    errors_only: bool,
    limit: i64,
) -> Result<Vec<ToolRow>, ApiError> {
    let where_c = filters.where_clause();
    let having = if errors_only { "HAVING errors > 0" } else { "" };
    let sql = format!(
        "SELECT tc.tool_name, tc.tool_kind, tc.mcp_server, COUNT(*) AS calls, \
                COALESCE(SUM(CASE WHEN tc.is_error = 1 THEN 1 ELSE 0 END), 0) AS errors \
         FROM tool_call tc JOIN session s ON s.id = tc.session_id WHERE {} \
         GROUP BY tc.tool_name, tc.tool_kind, tc.mcp_server {having} \
         ORDER BY calls DESC LIMIT ?",
        where_c.sql
    );
    let mut params = where_c.params.clone();
    params.push(SqlValue::Integer(limit));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| {
            let calls: i64 = r.get(3)?;
            let errors: i64 = r.get(4)?;
            Ok(ToolRow {
                tool_name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tool_kind: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                mcp_server: r.get(2)?,
                calls,
                errors,
                error_rate: error_rate(errors, calls),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct McpRow {
    pub mcp_server: String,
    pub tools: i64,
    pub calls: i64,
    pub errors: i64,
    pub error_rate: f64,
}

/// Per-MCP-server rollup scoped to `filters`, most-called first. Mirrors
/// `Archive.mcp_usage/2`.
pub fn mcp_usage(
    conn: &Connection,
    filters: &Filters,
    limit: i64,
) -> Result<Vec<McpRow>, ApiError> {
    let where_c = filters.where_clause();
    let sql = format!(
        "SELECT tc.mcp_server, COUNT(DISTINCT tc.tool_name) AS tools, COUNT(*) AS calls, \
                COALESCE(SUM(CASE WHEN tc.is_error = 1 THEN 1 ELSE 0 END), 0) AS errors \
         FROM tool_call tc JOIN session s ON s.id = tc.session_id \
         WHERE {} AND tc.tool_kind = 'mcp' AND tc.mcp_server IS NOT NULL \
         GROUP BY tc.mcp_server ORDER BY calls DESC LIMIT ?",
        where_c.sql
    );
    let mut params = where_c.params.clone();
    params.push(SqlValue::Integer(limit));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| {
            let calls: i64 = r.get(2)?;
            let errors: i64 = r.get(3)?;
            Ok(McpRow {
                mcp_server: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                tools: r.get(1)?,
                calls,
                errors,
                error_rate: error_rate(errors, calls),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn error_rate(errors: i64, calls: i64) -> f64 {
    if calls > 0 {
        errors as f64 / calls as f64
    } else {
        0.0
    }
}

#[derive(Debug, Serialize)]
pub struct DateBounds {
    pub min: Option<String>,
    pub max: Option<String>,
}

/// Min/max session date (YYYY-MM-DD). Mirrors `Archive.date_bounds/0`.
pub fn date_bounds(conn: &Connection) -> Result<DateBounds, ApiError> {
    let (min, max) = conn.query_row(
        "SELECT MIN(substr(started_at,1,10)), MAX(substr(started_at,1,10)) \
         FROM session WHERE started_at IS NOT NULL",
        [],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        },
    )?;
    Ok(DateBounds { min, max })
}

/// Parse the `group` query param for `/analytics/files`.
pub fn parse_file_group(s: Option<&str>) -> Result<decant_core::stats::FileGroup, ApiError> {
    match s {
        None => Ok(decant_core::stats::FileGroup::Path),
        Some(v) => decant_core::stats::FileGroup::parse(v).ok_or_else(|| {
            ApiError::invalid_filter(format!("unknown group {v:?}")).with_hint("one of: path, ext")
        }),
    }
}

/// Parse the `op` query param for `/analytics/files`.
pub fn parse_file_op(s: Option<&str>) -> Result<Option<decant_core::enrich::Operation>, ApiError> {
    match s {
        None => Ok(None),
        Some(v) => decant_core::enrich::Operation::parse(v)
            .map(Some)
            .ok_or_else(|| {
                ApiError::invalid_filter(format!("unknown op {v:?}"))
                    .with_hint("one of: read, edit, write, delete")
            }),
    }
}

/// File hotspots scoped to `filters` (spec §5.1 `/analytics/files`): which
/// files (group=path) or extensions (group=ext) agents touch most, split by
/// operation, ordered by total operations desc.
pub fn file_hotspots(
    conn: &Connection,
    group: decant_core::stats::FileGroup,
    op: Option<decant_core::enrich::Operation>,
    filters: &Filters,
    limit: i64,
) -> Result<Vec<decant_core::stats::FileStatRow>, ApiError> {
    use decant_core::stats::{FileGroup, FileStatRow};

    let where_c = filters.where_clause();
    let (key_expr, proj_expr, proj_join) = match group {
        FileGroup::Path => (
            "COALESCE(f.rel_path, f.path)",
            "p.path",
            "LEFT JOIN project p ON p.id = s.project_id",
        ),
        FileGroup::Ext => ("COALESCE(f.ext, '(none)')", "NULL", ""),
    };
    let mut params: Vec<SqlValue> = Vec::new();
    let op_clause = match op {
        Some(o) => {
            params.push(SqlValue::Text(o.as_str().to_string()));
            "f.operation = ?"
        }
        None => "1=1",
    };
    params.extend(where_c.params.iter().cloned());

    let sql = format!(
        "SELECT {key_expr} AS k, {proj_expr} AS proj,
                SUM(f.operation = 'read') AS reads,
                SUM(f.operation = 'edit') AS edits,
                SUM(f.operation = 'write') AS writes,
                SUM(f.operation = 'delete') AS deletes,
                COUNT(DISTINCT f.session_id) AS sessions,
                MAX(f.timestamp) AS last_touched
         FROM file_ref f
         JOIN session s ON s.id = f.session_id
         {proj_join}
         WHERE {op_clause} AND {where_sql}
         GROUP BY k, proj
         ORDER BY (reads + edits + writes + deletes) DESC, k ASC
         LIMIT {limit}",
        where_sql = where_c.sql,
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), |r| {
            Ok(FileStatRow {
                key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                project: r.get(1)?,
                reads: r.get(2)?,
                edits: r.get(3)?,
                writes: r.get(4)?,
                deletes: r.get(5)?,
                sessions: r.get(6)?,
                last_touched_at: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use decant_core::{db, ingest, schema, sources};

    /// Seed an in-memory DB with the Claude fixture (1 session, 4 messages, 1
    /// tool_use "Read") and the Codex fixture (1 session, 1 tool_call).
    fn seeded() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();

        let claude = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sample.jsonl"
        ))
        .unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &claude);
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &parsed, "/c.jsonl", 1, 2, "hc").unwrap();
        tx.commit().unwrap();

        let codex = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/codex/sample.jsonl"
        ))
        .unwrap();
        let titles = std::collections::HashMap::new();
        let parsed = sources::codex::parse_session("sess-codex-1", &codex, &titles);
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &parsed, "/x.jsonl", 3, 4, "hx").unwrap();
        tx.commit().unwrap();
        conn
    }

    #[test]
    fn lists_sessions_with_total() {
        let conn = seeded();
        let page = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::StartedAtDesc,
            50,
            None,
        )
        .unwrap();
        assert_eq!(page.total_count, 2);
        assert_eq!(page.sessions.len(), 2);
        // Newest first: codex (2026-05-02) before claude (2026-05-01).
        assert_eq!(page.sessions[0].tool, "codex");
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn keyset_pagination_round_trips_no_overlap() {
        let conn = seeded();
        let p1 = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::StartedAtDesc,
            1,
            None,
        )
        .unwrap();
        assert_eq!(p1.sessions.len(), 1);
        assert!(p1.has_more);
        let cur = Cursor::decode(p1.next_cursor.as_ref().unwrap()).unwrap();
        let p2 = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::StartedAtDesc,
            1,
            Some(cur),
        )
        .unwrap();
        assert_eq!(p2.sessions.len(), 1);
        // No overlap and total stays stable across pages.
        assert_ne!(p1.sessions[0].id, p2.sessions[0].id);
        assert_eq!(p1.total_count, p2.total_count);
        assert!(!p2.has_more);
    }

    #[test]
    fn tool_filter_scopes_list() {
        let conn = seeded();
        let f = Filters::parse(None, None, Some("codex".into()), None, None).unwrap();
        let page = list_sessions(&conn, &f, SessionSort::StartedAtDesc, 50, None).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.sessions[0].tool, "codex");
    }

    #[test]
    fn date_to_is_inclusive() {
        let conn = seeded();
        // to = the claude session's own day; it must be included.
        let f = Filters::parse(None, Some("2026-05-01".into()), None, None, None).unwrap();
        let page = list_sessions(&conn, &f, SessionSort::StartedAtDesc, 50, None).unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.sessions[0].tool, "claude_code");
    }

    #[test]
    fn session_detail_has_stats_and_blocks() {
        let conn = seeded();
        let list = list_sessions(
            &conn,
            &Filters::parse(None, None, Some("claude_code".into()), None, None).unwrap(),
            SessionSort::StartedAtDesc,
            10,
            None,
        )
        .unwrap();
        let id = list.sessions[0].id;
        let detail = get_session(&conn, id).unwrap().unwrap();
        assert_eq!(detail.messages.len(), 4);
        // Opus pricing -> nonzero cost breakdown that sums to total.
        let cb = &detail.stats.cost_breakdown;
        assert!(cb.total > 0.0);
        assert!(
            (cb.total - (cb.input + cb.output + cb.cache_read + cb.cache_creation)).abs() < 1e-9
        );
        // duration spans 10:00:00 -> 10:00:10.
        assert_eq!(detail.stats.duration_seconds, Some(10));
    }

    #[test]
    fn codex_session_surfaces_reasoning_without_inflating_cost() {
        // The codex sample's latest token_count reports reasoning=60 (a breakdown
        // of output=150). It must appear on both the summary and the stats, and
        // must not be double-counted into the cost breakdown.
        let conn = seeded();
        let list = list_sessions(
            &conn,
            &Filters::parse(None, None, Some("codex".into()), None, None).unwrap(),
            SessionSort::StartedAtDesc,
            10,
            None,
        )
        .unwrap();
        let summary = &list.sessions[0];
        assert_eq!(summary.total_reasoning_tokens, 60);
        assert!(summary.total_reasoning_tokens <= summary.total_output_tokens);
        // Codex reports exactly: source `reported`, nothing inferred.
        assert_eq!(summary.reasoning_source.as_deref(), Some("reported"));
        assert_eq!(summary.est_reasoning_tokens, 0);

        let detail = get_session(&conn, summary.id).unwrap().unwrap();
        assert_eq!(detail.stats.reasoning_tokens, 60);
        assert_eq!(detail.stats.reasoning_source.as_deref(), Some("reported"));
        // cost_breakdown is built only from input/output/cache — reasoning is
        // already inside output, so the components still sum to the total.
        let cb = &detail.stats.cost_breakdown;
        assert!(
            (cb.total - (cb.input + cb.output + cb.cache_read + cb.cache_creation)).abs() < 1e-9
        );
    }

    #[test]
    fn claude_inferred_reasoning_surfaces_through_the_api() {
        // Ingest a synthetic Claude turn that thinks (output cumulative 300 -> 400)
        // then emits a tool_use (`{"a":"bbbb"}` = 12 bytes). With no exact count,
        // the daemon surfaces est = 400 - 12/4 = 397 with source `inferred`, and
        // the exact reasoning stays 0 — across summary, stats, totals, by_dimension.
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = concat!(
            "{\"type\":\"assistant\",\"requestId\":\"q1\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"usage\":{\"input_tokens\":50,\"output_tokens\":300,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"content\":[{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"s\"}]}}\n",
            "{\"type\":\"assistant\",\"requestId\":\"q1\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"usage\":{\"input_tokens\":50,\"output_tokens\":400,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"content\":[{\"type\":\"tool_use\",\"name\":\"x\",\"id\":\"t1\",\"input\":{\"a\":\"bbbb\"}}]}}\n",
        );
        let parsed = sources::claude::parse_session("sess-think", content);
        let tx = conn.unchecked_transaction().unwrap();
        ingest::upsert_session(&tx, &parsed, "/t.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();

        let list = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::StartedAtDesc,
            10,
            None,
        )
        .unwrap();
        let summary = &list.sessions[0];
        assert_eq!(
            summary.total_reasoning_tokens, 0,
            "Claude has no exact count"
        );
        assert_eq!(summary.est_reasoning_tokens, 397);
        assert_eq!(summary.reasoning_source.as_deref(), Some("inferred"));
        assert!(summary.est_reasoning_tokens <= summary.total_output_tokens);

        let detail = get_session(&conn, summary.id).unwrap().unwrap();
        assert_eq!(detail.stats.est_reasoning_tokens, 397);
        assert_eq!(detail.stats.reasoning_source.as_deref(), Some("inferred"));

        let t = totals(&conn, &Filters::default()).unwrap();
        assert_eq!(t.reasoning_tokens, 0);
        assert_eq!(t.est_reasoning_tokens, 397);

        let page =
            by_dimension(&conn, Dimension::Tool, &Filters::default(), 50, None, None).unwrap();
        assert_eq!(page.rows[0].est_reasoning_tokens, 397);
    }

    #[test]
    fn get_missing_session_is_none() {
        let conn = seeded();
        assert!(get_session(&conn, 99999).unwrap().is_none());
    }

    #[test]
    fn search_finds_hits() {
        let conn = seeded();
        let page = search(&conn, "auth", 10, None).unwrap();
        assert!(!page.hits.is_empty());
    }

    #[test]
    fn search_malformed_query_is_400() {
        let conn = seeded();
        let err = search(&conn, "\"unbalanced", 10, None).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn search_empty_query_is_400() {
        let conn = seeded();
        let err = search(&conn, "   ", 10, None).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn totals_scoped_to_filters() {
        let conn = seeded();
        let all = totals(&conn, &Filters::default()).unwrap();
        assert_eq!(all.sessions, 2);
        // Only the codex sample reports reasoning (latest token_count = 60);
        // claude reports none, so the archive-wide reasoning total is 60.
        assert_eq!(all.reasoning_tokens, 60);
        let codex = totals(
            &conn,
            &Filters::parse(None, None, Some("codex".into()), None, None).unwrap(),
        )
        .unwrap();
        assert_eq!(codex.sessions, 1);
        assert!(codex.tool_calls >= 1);
        assert_eq!(codex.reasoning_tokens, 60);
        assert!(
            codex.reasoning_tokens <= codex.output_tokens,
            "reasoning is a breakdown of output"
        );
    }

    #[test]
    fn by_dimension_tool_rollup() {
        let conn = seeded();
        let page =
            by_dimension(&conn, Dimension::Tool, &Filters::default(), 50, None, None).unwrap();
        assert_eq!(page.total_count, 2);
        let keys: Vec<_> = page.rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"codex"));
        assert!(keys.contains(&"claude_code"));
        // Reasoning rolls up per tool: codex 60, claude 0.
        let codex = page.rows.iter().find(|r| r.key == "codex").unwrap();
        let claude = page.rows.iter().find(|r| r.key == "claude_code").unwrap();
        assert_eq!(codex.reasoning_tokens, 60);
        assert!(codex.reasoning_tokens <= codex.output_tokens);
        assert_eq!(claude.reasoning_tokens, 0);
    }

    #[test]
    fn activity_buckets_are_padded() {
        let conn = seeded();
        let a = activity(&conn, &Filters::default()).unwrap();
        assert_eq!(a.by_hour.len(), 24);
        assert_eq!(a.by_weekday.len(), 7);
        // Total sessions across hour buckets == 2.
        assert_eq!(a.by_hour.iter().sum::<i64>(), 2);
        assert!(a.peak_hour.is_some());
    }

    #[test]
    fn sparklines_share_day_axis() {
        let conn = seeded();
        let s = model_sparklines(&conn, &Filters::default()).unwrap();
        // Two days present (05-01, 05-02); every model vector has that length.
        assert_eq!(s.days.len(), 2);
        for v in s.models.values() {
            assert_eq!(v.len(), 2);
        }
    }

    #[test]
    fn tool_and_mcp_usage() {
        let conn = seeded();
        let tools = tool_usage(&conn, &Filters::default(), false, 50).unwrap();
        let read = tools.iter().find(|t| t.tool_name == "Read").unwrap();
        assert_eq!(read.calls, 1);
        assert_eq!(read.error_rate, 0.0);
        // No MCP tools in the fixtures.
        assert!(mcp_usage(&conn, &Filters::default(), 50)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn date_bounds_span_fixtures() {
        let conn = seeded();
        let b = date_bounds(&conn).unwrap();
        assert_eq!(b.min.as_deref(), Some("2026-05-01"));
        assert_eq!(b.max.as_deref(), Some("2026-05-02"));
    }

    #[test]
    fn count_sessions_propagates_db_error() {
        // The private count helper surfaces a query error on a table-less DB.
        let bare = decant_core::db::open_in_memory().unwrap();
        assert!(count_sessions(&bare, &Filters::default()).is_err());
    }

    #[test]
    fn totals_propagates_message_and_tool_call_query_errors() {
        // `totals` runs three queries in order: session COUNT, then message JOIN,
        // then tool_call JOIN. Dropping `message` lets the first succeed and the
        // message query's `?` fire; dropping `tool_call` instead fires the third.
        let conn = seeded();
        conn.execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE message;")
            .unwrap();
        assert!(totals(&conn, &Filters::default()).is_err());

        let conn2 = seeded();
        conn2
            .execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE tool_call;")
            .unwrap();
        assert!(totals(&conn2, &Filters::default()).is_err());
    }

    #[test]
    fn bucket_fills_indexed_counts_from_session_hours() {
        // The private `bucket` helper over seeded sessions (started at hour 10)
        // returns a zero-filled vector with the hour bucket populated.
        let conn = seeded();
        let hours = bucket(
            &conn,
            "strftime('%H', s.started_at, 'localtime')",
            &Filters::default(),
            24,
        )
        .unwrap();
        assert_eq!(hours.len(), 24);
        assert_eq!(hours.iter().sum::<i64>(), 2);
        // A table-less DB surfaces the prepare error.
        let bare = decant_core::db::open_in_memory().unwrap();
        assert!(bucket(&bare, "s.started_at", &Filters::default(), 24).is_err());
    }
}

#[cfg(test)]
mod worktree_tests {
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
        let page = by_dimension(
            &conn,
            Dimension::Project,
            &Filters::default(),
            50,
            None,
            None,
        )
        .unwrap();
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

#[cfg(test)]
mod branch_tests {
    use super::*;
    use decant_core::{db, schema};

    /// Seed several sessions with varying cost and one MCP tool call so the
    /// cost-sort, dimension, pagination, and MCP-usage branches are exercised.
    fn seed() -> Connection {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO project(id, path, name) VALUES (1, '/p', 'p');
             INSERT INTO session(id, tool, source_session_id, project_id, model, started_at, ended_at,
                                 message_count, total_input_tokens, total_output_tokens, estimated_cost_usd)
             VALUES
               (1, 'claude_code', 's1', 1, 'claude-opus-4', '2026-05-01T10:00:00Z', '2026-05-01T10:01:00Z', 1, 10, 5, 3.0),
               (2, 'codex',       's2', 1, 'gpt-5.5',       '2026-05-02T10:00:00Z', '2026-05-02T10:02:00Z', 1, 20, 7, 1.0),
               (3, 'claude_code', 's3', 1, 'claude-opus-4', '2026-05-03T10:00:00Z', '2026-05-03T10:03:00Z', 1, 30, 9, 2.0);
             -- A message with NO block rows: exercises the 'message without blocks' path.
             INSERT INTO message(id, session_id, seq, role, raw)
             VALUES (1, 1, 0, 'user', '{}');
             -- An MCP tool call so mcp_usage returns a row.
             INSERT INTO tool_call(id, session_id, tool_kind, tool_name, mcp_server, is_error)
             VALUES
               (1, 1, 'mcp', 'mcp__srv__do', 'srv', 0),
               (2, 1, 'mcp', 'mcp__srv__do', 'srv', 1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn session_sort_parse_variants() {
        assert_eq!(
            SessionSort::parse(Some("started_at_asc")).unwrap(),
            SessionSort::StartedAtAsc
        );
        assert_eq!(
            SessionSort::parse(Some("cost_desc")).unwrap(),
            SessionSort::CostDesc
        );
        assert_eq!(
            SessionSort::parse(None).unwrap(),
            SessionSort::StartedAtDesc
        );
        let err = SessionSort::parse(Some("bogus")).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.code, "INVALID_REQUEST");
    }

    #[test]
    fn cost_desc_sort_orders_and_paginates() {
        let conn = seed();
        // Page 1: highest cost first; a further page exists so a cost-keyed cursor
        // is emitted (the CostDesc branch in `next_cursor`).
        let p1 = list_sessions(&conn, &Filters::default(), SessionSort::CostDesc, 1, None).unwrap();
        assert_eq!(p1.sessions.len(), 1);
        assert_eq!(p1.sessions[0].id, 1); // cost 3.0 is the largest
        assert!(p1.has_more);
        let cur = Cursor::decode(p1.next_cursor.as_ref().unwrap()).unwrap();
        // The cursor carries the cost as a string sort_key (CostDesc branch).
        assert!(cur.sort_key.is_some());

        // Paging with the cost cursor exercises `sort_key_to_sql`'s Text arm and
        // the cost-column keyset; the boundary row must not repeat.
        let p2 = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::CostDesc,
            1,
            Some(cur),
        )
        .unwrap();
        // The keyset query runs cleanly with the cost cursor and returns a page.
        assert_eq!(p2.sessions.len(), 1);
    }

    #[test]
    fn cursor_with_null_sort_key_round_trips() {
        // A cursor carrying a None sort_key exercises `sort_key_to_sql`'s Null arm.
        // Use the asc order so the (NULL key, rowid 0) boundary returns rows.
        let conn = seed();
        let cur = Cursor::new(0, None);
        let page = list_sessions(
            &conn,
            &Filters::default(),
            SessionSort::StartedAtAsc,
            10,
            Some(cur),
        )
        .unwrap();
        // No panic; the Null sort-key boundary still returns matching rows.
        assert!(!page.sessions.is_empty());
    }

    #[test]
    fn get_session_handles_message_without_blocks() {
        let conn = seed();
        let detail = get_session(&conn, 1).unwrap().unwrap();
        assert_eq!(detail.messages.len(), 1);
        assert!(
            detail.messages[0].blocks.is_empty(),
            "the seeded message has no blocks"
        );
    }

    #[test]
    fn parse_dimension_missing_and_unknown() {
        let missing = parse_dimension(None).unwrap_err();
        assert_eq!(missing.status, axum::http::StatusCode::BAD_REQUEST);
        let unknown = parse_dimension(Some("galaxy")).unwrap_err();
        assert_eq!(unknown.code, "INVALID_FILTER");
        // The valid dims parse.
        assert!(parse_dimension(Some("model")).is_ok());
        assert!(parse_dimension(Some("day")).is_ok());
    }

    #[test]
    fn by_dimension_model_and_day_with_pagination() {
        let conn = seed();
        // Model dimension, limit 1 so has_more triggers and a cursor is returned.
        let m = by_dimension(&conn, Dimension::Model, &Filters::default(), 1, None, None).unwrap();
        assert_eq!(m.rows.len(), 1);
        assert!(m.has_more);
        let cur = Cursor::decode(m.next_cursor.as_ref().unwrap()).unwrap();
        let m2 = by_dimension(
            &conn,
            Dimension::Model,
            &Filters::default(),
            1,
            Some(cur),
            None,
        )
        .unwrap();
        assert_eq!(m2.rows.len(), 1);

        // Day dimension: three distinct days.
        let d = by_dimension(&conn, Dimension::Day, &Filters::default(), 50, None, None).unwrap();
        assert_eq!(d.total_count, 3);
    }

    #[test]
    fn search_paginates_with_offset_cursor() {
        // Seed FTS content across two blocks, then page one hit at a time.
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO session(id, tool, source_session_id) VALUES (1, 'claude_code', 's1');
             INSERT INTO message(id, session_id, seq, role, raw) VALUES (1, 1, 0, 'assistant', '{}');
             INSERT INTO block(id, message_id, session_id, ordinal, type, text)
             VALUES (1, 1, 1, 0, 'text', 'needle one alpha'),
                    (2, 1, 1, 1, 'text', 'needle two beta');",
        )
        .unwrap();
        let p1 = search(&conn, "needle", 1, None).unwrap();
        assert_eq!(p1.hits.len(), 1);
        assert!(p1.has_more);
        let cur = Cursor::decode(p1.next_cursor.as_ref().unwrap()).unwrap();
        let p2 = search(&conn, "needle", 1, Some(cur)).unwrap();
        assert_eq!(p2.hits.len(), 1);
        assert!(!p2.has_more);
        assert_ne!(p1.hits[0].block_id, p2.hits[0].block_id);
    }

    #[test]
    fn mcp_usage_returns_rows_with_error_rate() {
        let conn = seed();
        let rows = mcp_usage(&conn, &Filters::default(), 50).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.mcp_server, "srv");
        assert_eq!(r.calls, 2);
        assert_eq!(r.errors, 1);
        assert!((r.error_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn tool_usage_errors_only_filters_zero_error_tools() {
        let conn = seed();
        // errors_only keeps tools with >0 errors; the MCP tool has one error.
        let rows = tool_usage(&conn, &Filters::default(), true, 50).unwrap();
        assert!(rows.iter().all(|t| t.errors > 0));
        assert!(rows.iter().any(|t| t.tool_name == "mcp__srv__do"));
    }

    #[test]
    fn is_fts_syntax_error_recognizes_messages() {
        use rusqlite::ffi::{Error as FfiError, ErrorCode};
        let mk = |msg: &str| {
            rusqlite::Error::SqliteFailure(
                FfiError {
                    code: ErrorCode::Unknown,
                    extended_code: 1,
                },
                Some(msg.to_string()),
            )
        };
        assert!(is_fts_syntax_error(&mk("fts5: syntax error near \"x\"")));
        assert!(is_fts_syntax_error(&mk("malformed MATCH expression")));
        assert!(is_fts_syntax_error(&mk("unterminated string")));
        assert!(is_fts_syntax_error(&mk("no such column: foo in MATCH")));
        assert!(is_fts_syntax_error(&mk("syntax error in MATCH")));
        // A non-FTS message is not a syntax error.
        assert!(!is_fts_syntax_error(&mk("database is locked")));
        // A non-SqliteFailure error is never an FTS syntax error.
        assert!(!is_fts_syntax_error(&rusqlite::Error::QueryReturnedNoRows));
    }

    #[test]
    fn map_fts_err_internal_for_non_syntax() {
        // A non-FTS rusqlite error maps to a 500 INTERNAL, not a 400.
        let api = map_fts_err(rusqlite::Error::QueryReturnedNoRows);
        assert_eq!(api.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api.code, "INTERNAL");
    }

    #[test]
    fn query_functions_propagate_db_errors() {
        // An un-migrated DB has none of the tables; each function's `?` on its
        // first prepare/query_row propagates the SQLite error.
        let bare = decant_core::db::open_in_memory().unwrap();
        let f = Filters::default();
        assert!(list_sessions(&bare, &f, SessionSort::StartedAtDesc, 50, None).is_err());
        assert!(totals(&bare, &f).is_err());
        assert!(by_dimension(&bare, Dimension::Tool, &f, 50, None, None).is_err());
        assert!(activity(&bare, &f).is_err());
        assert!(model_sparklines(&bare, &f).is_err());
        assert!(tool_usage(&bare, &f, false, 50).is_err());
        assert!(mcp_usage(&bare, &f, 50).is_err());
        assert!(date_bounds(&bare).is_err());
        assert!(get_session(&bare, 1).is_err());
        assert!(search(&bare, "x", 50, None).is_err());
    }

    #[test]
    fn get_session_propagates_messages_query_error() {
        // Summary query succeeds (the session exists), then the messages query's
        // `prepare` fails because the `message` table is gone -> that `?` path.
        let conn = seed();
        // Session id 1 exists (summary query succeeds); dropping `block` makes the
        // messages query's prepare fail.
        conn.execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE block;")
            .unwrap();
        assert!(get_session(&conn, 1).is_err());
    }

    #[test]
    fn error_rate_zero_calls_is_zero() {
        // The `calls == 0` else arm returns 0.0.
        assert_eq!(error_rate(0, 0), 0.0);
        assert_eq!(error_rate(3, 0), 0.0);
        assert!((error_rate(1, 2) - 0.5).abs() < 1e-9);
    }
}
