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
}

impl FilterParams {
    fn into_filters(self) -> Result<Filters, ApiError> {
        Filters::parse(self.from, self.to, self.tool, self.model, self.project)
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
