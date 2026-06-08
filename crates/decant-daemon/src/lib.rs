//! decant-daemon: the long-running service that owns the archive and serves the HTTP API.

pub mod auth;
pub mod config;

/// HTTP API contract version, surfaced in the `X-Decant-API-Version` header and `/health`.
pub const API_VERSION: u32 = 1;

/// Smoke value used by the first test to prove the crate builds.
pub fn api_version() -> u32 {
    API_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_version_is_one() {
        assert_eq!(super::api_version(), 1);
    }
}
