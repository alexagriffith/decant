//! decant-daemon: the long-running service that owns the archive and serves the HTTP API.

pub mod auth;
pub mod config;
pub mod health;
pub mod http;
pub mod lock;
pub mod middleware;

/// HTTP API contract version, surfaced in the `X-Decant-API-Version` header and `/health`.
pub const API_VERSION: u32 = 1;

/// Smoke value used by the first test to prove the crate builds.
pub fn api_version() -> u32 {
    API_VERSION
}

/// Boot the daemon: acquire the single-instance lock, load/create the token,
/// build the app, and serve until shutdown.
pub async fn run(cfg: config::Config) -> anyhow::Result<()> {
    let _lock = lock::InstanceLock::acquire(&cfg.lock_path())
        .map_err(|e| anyhow::anyhow!("could not start: {e}"))?;
    let token = auth::load_or_create(&cfg.token_path())?;
    let app = http::router(http::AppState { token });
    http::serve(&cfg.bind_addr(), app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_version_is_one() {
        assert_eq!(super::api_version(), 1);
    }
}
