//! `/api/v1/metadata/*` handlers. Plan 2 adds only `sync-status`; the read
//! endpoints (date-bounds, etc.) arrive with Plan 3.

use axum::{extract::State, Json};
use serde_json::{json, Value};

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
