use axum::Json;
use serde_json::{json, Value};

/// Liveness + version. Intentionally unauthenticated so clients can probe readiness.
pub async fn health() -> Json<Value> {
    Json(json!({
        "data": {
            "api_version": crate::API_VERSION,
            "db_schema_version": decant_core::schema::LATEST_VERSION,
            "status": "ok"
        },
        "meta": { "timestamp": null },
        "errors": []
    }))
}
