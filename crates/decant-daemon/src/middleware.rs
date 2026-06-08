use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::http::AppState;

/// Defense-in-depth guard for `/api/*`: validates Host against a loopback
/// allowlist (anti DNS-rebinding), requires a matching bearer token, and
/// rejects cross-origin writes. `/api/v1/health` is mounted outside this layer.
pub async fn guard(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1) Host allowlist.
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !host_allowed(host) {
        return Err(StatusCode::FORBIDDEN);
    }

    // 2) Bearer token.
    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if presented.is_empty() || !crate::auth::token_matches(&state.token, presented) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 3) Origin check on state-mutating methods.
    if !matches!(
        req.method(),
        &axum::http::Method::GET | &axum::http::Method::HEAD
    ) {
        if let Some(origin) = req
            .headers()
            .get(axum::http::header::ORIGIN)
            .and_then(|h| h.to_str().ok())
        {
            if !origin_allowed(origin) {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    Ok(next.run(req).await)
}

fn host_allowed(host: &str) -> bool {
    // Strip port; accept loopback names only.
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

fn origin_allowed(origin: &str) -> bool {
    origin.starts_with("http://127.0.0.1")
        || origin.starts_with("http://localhost")
        || origin.starts_with("https://127.0.0.1")
        || origin.starts_with("https://localhost")
}
