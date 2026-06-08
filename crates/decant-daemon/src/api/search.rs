//! `POST /api/v1/search` — FTS5 over blocks.
//!
//! Search uses POST (the query can be long and may contain characters awkward in
//! a URL) with a JSON body `{q, limit, cursor}`. A malformed FTS5 query returns a
//! `400` with a hint, never a 500 (validated in `query::search`).

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use super::cursor::Cursor;
use super::envelope::{ApiError, Envelope, Meta, Pagination};
use super::query;
use super::with_read_conn;
use crate::http::AppState;

/// Request body for search.
#[derive(Debug, Deserialize)]
pub struct SearchBody {
    pub q: String,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// `POST /api/v1/search`.
///
/// A missing/invalid JSON body is rejected by axum's `Json` extractor before we
/// run; we additionally validate `q` (non-empty) and FTS5 syntax in the query
/// layer, both surfaced as 400s.
pub async fn search(
    State(state): State<AppState>,
    body: Result<Json<SearchBody>, axum::extract::rejection::JsonRejection>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    // Translate a malformed/missing body into our error envelope rather than
    // axum's default plain-text 400/422.
    let Json(body) = body.map_err(|e| {
        ApiError::bad_request(format!("invalid request body: {e}"))
            .with_hint("send JSON: {\"q\": \"<terms>\", \"limit\": 50}")
    })?;

    let limit = query::clamp_limit(body.limit);
    let cursor = match body.cursor {
        Some(c) => Some(Cursor::decode(&c)?),
        None => None,
    };
    let q = body.q.clone();

    let page = with_read_conn(&state.read_pool, move |conn| {
        query::search(conn, &q, limit, cursor)
    })
    .await?;

    let meta = Meta::now().with_pagination(Pagination {
        next_cursor: page.next_cursor,
        has_more: page.has_more,
        // FTS total is not cheaply available without a second full scan; the
        // contract allows omitting it (clients page until has_more is false).
        total_count: None,
        page_size: limit,
    });

    Ok(Envelope::with_meta(serde_json::json!(page.hits), meta))
}
