//! `GET /api/v1/recommendations` (list) and
//! `POST /api/v1/recommendations/mark-implemented` (state write).
//!
//! The list is a read (pooled read connection). Mark-implemented is the daemon's
//! only API write: it locks the shared single write connection — the same one
//! the ingest task syncs on — so the mutex serializes it against a concurrent
//! sync instead of racing the WAL lock. The actual work lives in
//! `decant_core::recommendations` (UI-agnostic); these handlers just parse,
//! coordinate the connection, and wrap the result in the envelope.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use super::envelope::{ApiError, Envelope, Meta};
use super::with_read_conn;
use crate::http::AppState;
use decant_core::recommendations::{self, StatusFilter};

/// Query params for the list endpoint: `?status=open|implemented|all`.
#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub status: Option<String>,
}

/// `GET /api/v1/recommendations?status=open|implemented|all` (default `open`).
///
/// Returns the persisted recommendations (signals + catalog) with their
/// lifecycle state, in the `{data, meta, errors}` envelope. An unrecognized
/// `status` is a 400 (never a silent default).
pub async fn list(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let status = match params.status.as_deref() {
        None | Some("") => StatusFilter::Open,
        Some(s) => StatusFilter::parse(s).ok_or_else(|| {
            ApiError::invalid_filter(format!("unknown status {s:?}"))
                .with_hint("status must be one of: open, implemented, all")
        })?,
    };

    let rows = with_read_conn(&state.read_pool, move |conn| {
        recommendations::list(conn, status).map_err(ApiError::from)
    })
    .await?;

    Ok(Envelope::with_meta(
        serde_json::json!(rows),
        Meta::now().with_filters(serde_json::json!({ "status": status_label(status) })),
    ))
}

fn status_label(status: StatusFilter) -> &'static str {
    match status {
        StatusFilter::Open => "open",
        StatusFilter::Implemented => "implemented",
        StatusFilter::All => "all",
    }
}

/// Request body for marking a recommendation implemented. `key` is in the body
/// (not the path) because keys contain `:` (e.g. `signal:error:Bash`). `source`
/// defaults to `manual`; `note` is optional.
#[derive(Debug, Deserialize)]
pub struct MarkBody {
    pub key: String,
    pub source: Option<String>,
    pub note: Option<String>,
}

/// `POST /api/v1/recommendations/mark-implemented`.
///
/// Idempotent: marking an already-implemented key returns 200. Locks the shared
/// write connection (recovering a poisoned lock) and runs the synchronous write
/// on the blocking pool. An unknown key is a 404.
pub async fn mark_implemented(
    State(state): State<AppState>,
    body: Result<Json<MarkBody>, axum::extract::rejection::JsonRejection>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let Json(body) = body.map_err(|e| {
        ApiError::bad_request(format!("invalid request body: {e}"))
            .with_hint("send JSON: {\"key\": \"catalog:agents-md\", \"source\": \"agent\"}")
    })?;

    if body.key.trim().is_empty() {
        return Err(
            ApiError::bad_request("`key` is required").with_hint("send a recommendation key")
        );
    }

    let key = body.key.clone();
    let source = body.source.clone().unwrap_or_else(|| "manual".to_string());
    let note = body.note.clone();
    let write = state.write.clone();

    // The write is synchronous rusqlite; run it on the blocking pool. Lock the
    // shared connection there (recovering a poisoned mutex), so a concurrent sync
    // and this write never overlap.
    let found = tokio::task::spawn_blocking(move || {
        let guard = write.lock().unwrap_or_else(|p| p.into_inner());
        recommendations::mark_implemented(&guard, &key, &source, note.as_deref())
    })
    .await
    .map_err(|e| ApiError::internal(format!("write task failed: {e}")))?
    .map_err(ApiError::from)?;

    if !found {
        return Err(ApiError::not_found(format!(
            "no recommendation with key {:?}",
            body.key
        )));
    }

    Ok(Envelope::new(serde_json::json!({
        "key": body.key,
        "status": "implemented",
        "status_source": body.source.unwrap_or_else(|| "manual".to_string()),
    })))
}
