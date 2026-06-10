//! `GET /api/v1/sessions` and `GET /api/v1/sessions/{id}`.

use axum::extract::{Path, Query, State};
use serde::Deserialize;

use super::cursor::Cursor;
use super::envelope::{ApiError, Envelope, Meta, Pagination};
use super::filters::Filters;
use super::query::{self, SessionSort};
use super::with_read_conn;
use crate::http::AppState;

/// Query params for the session list.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub outcome: Option<String>,
    pub work_type: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// `GET /api/v1/sessions` — filtered, sorted, cursor-paginated session summaries.
pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let filters = Filters::parse(
        params.from,
        params.to,
        params.tool,
        params.model,
        params.project,
    )?
    .with_classification(params.outcome, params.work_type)?;
    let sort = SessionSort::parse(params.sort.as_deref())?;
    let limit = query::clamp_limit(params.limit);
    let cursor = match params.cursor {
        Some(c) => Some(Cursor::decode(&c)?),
        None => None,
    };

    let filters_json = filters.as_json();
    let page = with_read_conn(&state.read_pool, move |conn| {
        query::list_sessions(conn, &filters, sort, limit, cursor)
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

    Ok(Envelope::with_meta(serde_json::json!(page.sessions), meta))
}

/// `GET /api/v1/sessions/{id}` — summary + stats + messages/blocks.
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Envelope<query::SessionDetail>, ApiError> {
    let detail = with_read_conn(&state.read_pool, move |conn| query::get_session(conn, id))
        .await?
        .ok_or_else(|| ApiError::not_found(format!("session {id} not found")))?;
    Ok(Envelope::new(detail))
}
