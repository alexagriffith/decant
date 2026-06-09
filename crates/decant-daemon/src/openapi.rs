//! `GET /api/v1/openapi.yaml` — serve the bundled API contract.
//!
//! The OpenAPI document at `docs/api/openapi.yaml` is the contract of record for
//! the daemon. We embed it at compile time with `include_str!` so the binary is
//! self-contained (no runtime file lookup, works from any install location), and
//! serve it unauthenticated alongside `/health`: it is a public schema, not data.

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;

/// The contract, embedded at build time relative to this source file
/// (`crates/decant-daemon/src/` → repo `docs/api/openapi.yaml`).
const OPENAPI_YAML: &str = include_str!("../../../docs/api/openapi.yaml");

/// Return the bundled OpenAPI document as `application/yaml`.
pub async fn spec() -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/yaml")], OPENAPI_YAML)
}

#[cfg(test)]
mod tests {
    use super::OPENAPI_YAML;

    #[test]
    fn bundled_spec_is_present_and_versioned() {
        assert!(
            OPENAPI_YAML.contains("openapi: 3.1.0"),
            "embedded OpenAPI doc should declare the 3.1.0 version"
        );
        assert!(
            OPENAPI_YAML.contains("decant daemon API"),
            "embedded OpenAPI doc should be the decant contract"
        );
    }
}
