use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::db::ReadPool;
use crate::sync_status::SyncStatusHandle;

/// Shared state available to handlers.
#[derive(Clone)]
pub struct AppState {
    pub token: String,
    /// r2d2 pool of read connections for handlers (Plan 3 reads use this).
    pub read_pool: ReadPool,
    /// Live ingest status, written by the ingest task.
    pub sync_status: SyncStatusHandle,
}

/// Build the router. `/api/v1/health` is public; everything else is gated by
/// the guard middleware.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route(
            "/api/v1/ping",
            get(|| async { axum::Json(serde_json::json!({})) }),
        )
        .route(
            "/api/v1/metadata/sync-status",
            get(crate::metadata::sync_status),
        )
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
