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

// Cross-origin write guard. Match the Origin's host EXACTLY (any port) so an
// unanchored lookalike like "http://127.0.0.1.evil.com" cannot slip through.
// Absent Origin is intentionally allowed by the caller: non-browser clients
// (the launched coding agent and the CLI) send no Origin, and every write is
// already gated by the bearer token, which a cross-origin browser page can
// neither read nor make Phoenix attach.
fn origin_allowed(origin: &str) -> bool {
    let rest = match origin.split_once("://") {
        Some((scheme, rest)) if scheme == "http" || scheme == "https" => rest,
        _ => return false,
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""), // "[::1]:port" -> "::1"
        None => authority.split(':').next().unwrap_or(""), // "host:port" -> "host"
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_allows_loopback_any_port() {
        assert!(origin_allowed("http://localhost:4000"));
        assert!(origin_allowed("http://127.0.0.1:4000"));
        assert!(origin_allowed("https://localhost"));
        assert!(origin_allowed("http://[::1]:4000"));
    }

    #[test]
    fn origin_rejects_lookalikes_and_other_hosts() {
        assert!(!origin_allowed("http://127.0.0.1.evil.com"));
        assert!(!origin_allowed("http://localhost.evil.com"));
        assert!(!origin_allowed("http://evil.com"));
        assert!(!origin_allowed("ftp://localhost"));
        assert!(!origin_allowed("http://127.0.0.1@evil.com"));
        assert!(!origin_allowed(""));
    }

    #[test]
    fn host_allows_loopback_only() {
        assert!(host_allowed("127.0.0.1:4577"));
        assert!(host_allowed("localhost:4577"));
        assert!(host_allowed("[::1]:4577"));
        assert!(!host_allowed("evil.example.com"));
        assert!(!host_allowed("127.0.0.1.evil.com:4577"));
    }
}
