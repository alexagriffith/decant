use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::db::ReadPool;
use crate::events::ChangeSender;
use crate::sync_status::SyncStatusHandle;

/// Shared state available to handlers.
#[derive(Clone)]
pub struct AppState {
    pub token: String,
    /// r2d2 pool of read connections for handlers (Plan 3 reads use this).
    pub read_pool: ReadPool,
    /// Live ingest status, written by the ingest task.
    pub sync_status: SyncStatusHandle,
    /// Broadcast sender for the SSE change-stream (Plan 4). The SSE handler
    /// subscribes a receiver per connection; the ingest task holds a clone.
    pub events: ChangeSender,
}

/// Build the router. `/api/v1/health` is public; everything else is gated by
/// the guard middleware.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route(
            "/api/v1/ping",
            get(|| async { axum::Json(serde_json::json!({})) }),
        )
        // Sessions
        .route("/api/v1/sessions", get(crate::api::sessions::list))
        .route("/api/v1/sessions/:id", get(crate::api::sessions::detail))
        // Search
        .route("/api/v1/search", post(crate::api::search::search))
        // Analytics
        .route(
            "/api/v1/analytics/summary",
            get(crate::api::analytics::summary),
        )
        .route(
            "/api/v1/analytics/by-dimension",
            get(crate::api::analytics::by_dimension),
        )
        .route(
            "/api/v1/analytics/activity",
            get(crate::api::analytics::activity),
        )
        .route(
            "/api/v1/analytics/model-sparklines",
            get(crate::api::analytics::model_sparklines),
        )
        // Tools
        .route("/api/v1/tools/usage", get(crate::api::tools::usage))
        .route("/api/v1/tools/mcp-usage", get(crate::api::tools::mcp_usage))
        // Metadata
        .route(
            "/api/v1/metadata/sync-status",
            get(crate::metadata::sync_status),
        )
        .route(
            "/api/v1/metadata/date-bounds",
            get(crate::metadata::date_bounds),
        )
        // SSE change-stream (Plan 4). Behind the same guard: Phoenix sends the
        // bearer token as a normal Authorization header on the SSE request.
        .route("/api/v1/events", get(crate::events::events))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::guard,
        ));

    Router::new()
        .route("/api/v1/health", get(crate::health::health))
        .merge(protected)
        .layer(axum::middleware::map_response(add_version_header))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn add_version_header(mut res: axum::response::Response) -> axum::response::Response {
    res.headers_mut().insert(
        "x-decant-api-version",
        axum::http::HeaderValue::from_static("1"),
    );
    res
}

/// Serve until `shutdown` resolves. The caller drives shutdown from a shared
/// signal so the ingest task and the HTTP server stop together (see `lib::run`).
pub async fn serve(
    addr: &str,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "decant-daemon listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Resolve when SIGINT or SIGTERM (Unix) / Ctrl-C is received.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    tracing::info!("decant-daemon shutting down");
}
