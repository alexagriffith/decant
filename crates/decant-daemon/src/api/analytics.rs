//! `GET /api/v1/analytics/*` handlers: summary, by-dimension, activity,
//! model-sparklines. All accept the standard filter set and return the envelope.

use axum::extract::{Query, State};
use serde::Deserialize;

use super::cursor::Cursor;
use super::envelope::{ApiError, Envelope, Meta, Pagination};
use super::filters::Filters;
use super::query;
use super::with_read_conn;
use crate::http::AppState;

/// Filter-only query params shared by summary/activity/sparklines.
#[derive(Debug, Deserialize)]
pub struct FilterParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub outcome: Option<String>,
    pub work_type: Option<String>,
}

impl FilterParams {
    fn into_filters(self) -> Result<Filters, ApiError> {
        Filters::parse(self.from, self.to, self.tool, self.model, self.project)?
            .with_classification(self.outcome, self.work_type)
    }
}

/// `GET /api/v1/analytics/summary` — totals scoped to the filters.
pub async fn summary(
    State(state): State<AppState>,
    Query(params): Query<FilterParams>,
) -> Result<Envelope<query::Totals>, ApiError> {
    let filters = params.into_filters()?;
    let filters_json = filters.as_json();
    let totals =
        with_read_conn(&state.read_pool, move |conn| query::totals(conn, &filters)).await?;
    Ok(Envelope::with_meta(
        totals,
        Meta::now().with_filters(filters_json),
    ))
}

/// Query params for by-dimension: filters + `dim` + pagination.
#[derive(Debug, Deserialize)]
pub struct ByDimensionParams {
    pub dim: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub root: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// `GET /api/v1/analytics/by-dimension?dim=tool|model|project|day` — ranked
/// rollups, paginated for high-cardinality dimensions.
pub async fn by_dimension(
    State(state): State<AppState>,
    Query(params): Query<ByDimensionParams>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let dim = query::parse_dimension(params.dim.as_deref())?;
    let root = params.root.filter(|s| !s.is_empty());
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

    let meta = Meta::now()
        .with_pagination(Pagination {
            next_cursor: page.next_cursor,
            has_more: page.has_more,
            total_count: Some(page.total_count),
            page_size: limit,
        })
        .with_filters(filters_json);
    Ok(Envelope::with_meta(serde_json::json!(page.rows), meta))
}

/// Query params for `/analytics/files`: filters + grouping + op + limit.
#[derive(Debug, Deserialize)]
pub struct FilesParams {
    pub group: Option<String>,
    pub op: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub outcome: Option<String>,
    pub work_type: Option<String>,
    pub limit: Option<i64>,
}

/// `GET /api/v1/analytics/files?group=path|ext&op=read|edit|write|delete` —
/// file hotspots (or the per-extension language lens), ordered by total
/// operations. Top-N by design; no cursor.
pub async fn files(
    State(state): State<AppState>,
    Query(params): Query<FilesParams>,
) -> Result<Envelope<Vec<decant_core::stats::FileStatRow>>, ApiError> {
    let group = query::parse_file_group(params.group.as_deref())?;
    let op = query::parse_file_op(params.op.as_deref())?;
    let filters = Filters::parse(
        params.from,
        params.to,
        params.tool,
        params.model,
        params.project,
    )?
    .with_classification(params.outcome, params.work_type)?;
    let limit = query::clamp_limit(params.limit);
    let filters_json = filters.as_json();
    let rows = with_read_conn(&state.read_pool, move |conn| {
        query::file_hotspots(conn, group, op, &filters, limit)
    })
    .await?;
    Ok(Envelope::with_meta(
        rows,
        Meta::now().with_filters(filters_json),
    ))
}

/// One currently-active session for `/analytics/now`: tracker state joined to
/// the archive row when the session has already been ingested.
#[derive(Debug, serde::Serialize)]
pub struct NowSession {
    pub tool: String,
    pub source_path: String,
    /// Seconds since the file last changed.
    pub idle_seconds: u64,
    pub title: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
}

/// `GET /api/v1/analytics/now` payload: one cheap call for glanceable clients
/// (menu bar, dashboard header).
#[derive(Debug, serde::Serialize)]
pub struct Now {
    pub today: query::Totals,
    pub active_sessions: Vec<NowSession>,
    pub last_sync_at: Option<String>,
    pub sync_in_progress: bool,
}

/// `GET /api/v1/analytics/now` — today's totals (local-time day bounds, same
/// convention as `/analytics/activity`), live sessions from the activity
/// tracker, and sync state.
pub async fn now(State(state): State<AppState>) -> Result<Envelope<Now>, ApiError> {
    let today = chrono::Local::now().date_naive().to_string();
    let filters = Filters::parse(Some(today.clone()), Some(today), None, None, None)?;

    let active = state.activity.active(std::time::Instant::now());
    let sync = state.sync_status.snapshot();

    let (totals, active_sessions) = with_read_conn(&state.read_pool, move |conn| {
        let totals = query::totals(conn, &filters)?;
        let mut sessions = Vec::with_capacity(active.len());
        for src in active {
            let source_path = src.path.to_string_lossy().to_string();
            let row = conn
                .query_row(
                    "SELECT s.title, p.path, s.model, s.started_at
                     FROM session s LEFT JOIN project p ON p.id = s.project_id
                     WHERE s.source_path = ?1",
                    [&source_path],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .ok();
            let (title, project, model, started_at) = row.unwrap_or((None, None, None, None));
            sessions.push(NowSession {
                tool: src.tool.to_string(),
                source_path,
                idle_seconds: src.idle.as_secs(),
                title,
                project,
                model,
                started_at,
            });
        }
        Ok((totals, sessions))
    })
    .await?;

    Ok(Envelope::new(Now {
        today: totals,
        active_sessions,
        last_sync_at: sync.last_sync_at,
        sync_in_progress: sync.in_progress,
    }))
}

/// `GET /api/v1/analytics/activity` — local-time hour/weekday histograms.
pub async fn activity(
    State(state): State<AppState>,
    Query(params): Query<FilterParams>,
) -> Result<Envelope<query::Activity>, ApiError> {
    let filters = params.into_filters()?;
    let filters_json = filters.as_json();
    let activity = with_read_conn(&state.read_pool, move |conn| {
        query::activity(conn, &filters)
    })
    .await?;
    Ok(Envelope::with_meta(
        activity,
        Meta::now().with_filters(filters_json),
    ))
}

/// `GET /api/v1/analytics/model-sparklines` — per-model daily counts on a shared
/// day axis.
pub async fn model_sparklines(
    State(state): State<AppState>,
    Query(params): Query<FilterParams>,
) -> Result<Envelope<query::ModelSparklines>, ApiError> {
    let filters = params.into_filters()?;
    let filters_json = filters.as_json();
    let sparks = with_read_conn(&state.read_pool, move |conn| {
        query::model_sparklines(conn, &filters)
    })
    .await?;
    Ok(Envelope::with_meta(
        sparks,
        Meta::now().with_filters(filters_json),
    ))
}
