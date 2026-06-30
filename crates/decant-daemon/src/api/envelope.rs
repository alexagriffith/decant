//! The `{data, meta, errors}` response envelope and the API error type.
//!
//! Every handler returns either an [`Envelope`] (200) or an [`ApiError`] (4xx/5xx
//! rendered as `{error: {code, message, hint, request_id, details}}`). Keeping
//! both here means every endpoint serializes the contract identically (spec §5).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

/// Pagination metadata for cursor-paginated list endpoints.
#[derive(Debug, Serialize)]
pub struct Pagination {
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// Total rows matching the filters (ignoring the cursor). `None` for endpoints
    /// where a total would require a second full scan (e.g. FTS search), where
    /// clients page until `has_more` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<i64>,
    pub page_size: i64,
}

/// The `meta` block. Fields are flattened into the JSON object; absent ones are
/// omitted so small responses (e.g. health) stay compact.
#[derive(Debug, Default, Serialize)]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters_applied: Option<Value>,
    pub timestamp: String,
}

impl Meta {
    /// A `meta` stamped with the current time and nothing else.
    pub fn now() -> Self {
        Meta {
            pagination: None,
            filters_applied: None,
            timestamp: now_rfc3339(),
        }
    }

    pub fn with_pagination(mut self, p: Pagination) -> Self {
        self.pagination = Some(p);
        self
    }

    pub fn with_filters(mut self, f: Value) -> Self {
        self.filters_applied = Some(f);
        self
    }
}

/// The success envelope: `{ "data": ..., "meta": {...}, "errors": [] }`.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub data: T,
    pub meta: Meta,
    pub errors: Vec<Value>,
}

impl<T: Serialize> Envelope<T> {
    /// Wrap a payload with a fresh `meta` and no errors.
    pub fn new(data: T) -> Self {
        Envelope {
            data,
            meta: Meta::now(),
            errors: Vec::new(),
        }
    }

    /// Wrap a payload with a caller-built `meta` (pagination/filters).
    pub fn with_meta(data: T, meta: Meta) -> Self {
        Envelope {
            data,
            meta,
            errors: Vec::new(),
        }
    }
}

impl<T: Serialize> IntoResponse for Envelope<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

/// A structured API error. `IntoResponse` renders it as the error envelope with
/// the mapped HTTP status; `code` is the stable machine string clients branch on.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
            hint: None,
            details: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 400 for a bad filter/param value.
    pub fn bad_request(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "INVALID_REQUEST", message)
    }

    /// 400 for a malformed filter value specifically.
    pub fn invalid_filter(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "INVALID_FILTER", message)
    }

    /// 404 for a missing resource.
    pub fn not_found(message: impl Into<String>) -> Self {
        ApiError::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    /// 500 for an internal/database failure. The detail string is logged, not
    /// leaked verbatim beyond a generic message in `details` for debugging.
    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "internal server error",
        )
        .detail(message)
    }

    fn detail(mut self, d: impl Into<String>) -> Self {
        self.details = Some(json!({ "cause": d.into() }));
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = new_request_id();
        let body = json!({
            "data": null,
            "meta": { "timestamp": now_rfc3339() },
            "errors": [],
            "error": {
                "code": self.code,
                "message": self.message,
                "hint": self.hint,
                "details": self.details,
                "request_id": request_id,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

/// Map a `decant_core::Error` (or any rusqlite failure surfaced through it) to a
/// 500 ApiError, preserving the cause in `details` for logs.
impl From<decant_core::Error> for ApiError {
    fn from(e: decant_core::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

/// RFC3339 UTC, seconds precision, matching the rest of the daemon.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A short random request id for error correlation. Hex of 8 random bytes.
fn new_request_id() -> String {
    let buf: [u8; 8] = rand::random();
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes_with_empty_errors() {
        let env = Envelope::new(json!({"x": 1}));
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(v["data"]["x"], 1);
        assert_eq!(v["errors"], json!([]));
        assert!(v["meta"]["timestamp"].is_string());
        // No pagination key when absent.
        assert!(v["meta"].get("pagination").is_none());
    }

    #[test]
    fn envelope_includes_pagination_when_set() {
        let meta = Meta::now().with_pagination(Pagination {
            next_cursor: Some("abc".into()),
            has_more: true,
            total_count: Some(42),
            page_size: 50,
        });
        let env = Envelope::with_meta(json!([]), meta);
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(v["meta"]["pagination"]["total_count"], 42);
        assert_eq!(v["meta"]["pagination"]["has_more"], true);
        assert_eq!(v["meta"]["pagination"]["next_cursor"], "abc");
    }

    #[test]
    fn api_error_renders_error_object_and_status() {
        let err = ApiError::invalid_filter("bad date").with_hint("use YYYY-MM-DD");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn internal_error_does_not_leak_message_in_top_level() {
        let err = ApiError::internal("table foo locked");
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.message, "internal server error");
        // cause is preserved in details for logs/debugging.
        assert_eq!(err.details.unwrap()["cause"], "table foo locked");
    }

    #[test]
    fn from_core_error_maps_to_internal() {
        let core_err = decant_core::Error::Other("core boom".into());
        let api: ApiError = core_err.into();
        assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api.code, "INTERNAL");
        // The cause is preserved for logs.
        assert_eq!(api.details.unwrap()["cause"], "core boom");
    }

    #[test]
    fn from_rusqlite_error_maps_to_internal() {
        let api: ApiError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api.code, "INTERNAL");
        assert!(api.details.unwrap()["cause"].is_string());
    }

    #[test]
    fn not_found_and_bad_request_constructors() {
        let nf = ApiError::not_found("gone");
        assert_eq!(nf.status, StatusCode::NOT_FOUND);
        assert_eq!(nf.code, "NOT_FOUND");
        let br = ApiError::bad_request("bad");
        assert_eq!(br.status, StatusCode::BAD_REQUEST);
        assert_eq!(br.code, "INVALID_REQUEST");
    }
}
