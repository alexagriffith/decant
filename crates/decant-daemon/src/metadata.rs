//! `/api/v1/metadata/*` handlers: `sync-status` (Plan 2) and `date-bounds`
//! (Plan 3, min/max session date for the date-range picker).

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::api::envelope::{ApiError, Envelope};
use crate::api::query;
use crate::api::with_read_conn;
use crate::http::AppState;

/// `GET /api/v1/metadata/sync-status` — current ingest status inside the
/// `{data, meta, errors}` envelope. Auth is enforced by the guard middleware
/// this route is mounted behind.
pub async fn sync_status(State(state): State<AppState>) -> Json<Value> {
    let status = state.sync_status.snapshot();
    let in_progress = status.in_progress;
    let last_sync_at = status.last_sync_at.clone();
    Json(json!({
        "data": status,
        "meta": {
            "sync": {
                "in_progress": in_progress,
                "last_sync_at": last_sync_at,
            },
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
        "errors": []
    }))
}

/// `GET /api/v1/metadata/date-bounds` — earliest/latest session date in the
/// archive, inside the `{data, meta, errors}` envelope.
pub async fn date_bounds(
    State(state): State<AppState>,
) -> Result<Envelope<query::DateBounds>, ApiError> {
    let bounds = with_read_conn(&state.read_pool, query::date_bounds).await?;
    Ok(Envelope::new(bounds))
}
