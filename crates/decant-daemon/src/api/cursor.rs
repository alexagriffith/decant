//! Opaque pagination cursors: `base64(json({rowid, sort_key}))` (spec §5).
//!
//! A cursor carries the last row's tie-breaking `rowid` plus its `sort_key` (the
//! value of the column the list is ordered by — e.g. `started_at`). Keyset
//! pagination then resumes strictly after `(sort_key, rowid)`, so inserts and
//! deletes elsewhere in the result set never duplicate or skip a row.
//!
//! The encoding is URL-safe base64 (no padding) so cursors are safe in JSON and,
//! if a client ever puts one on a query string, in URLs too.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::envelope::ApiError;

/// The decoded cursor position. `sort_key` is the ordered column's value at the
/// boundary row; `rowid` breaks ties on equal `sort_key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub rowid: i64,
    /// The sort column's value for the boundary row. `None` when that column was
    /// SQL NULL (sorts last under our `started_at DESC` ordering).
    pub sort_key: Option<String>,
}

impl Cursor {
    pub fn new(rowid: i64, sort_key: Option<String>) -> Self {
        Cursor { rowid, sort_key }
    }

    /// Encode to the opaque base64 token.
    pub fn encode(&self) -> String {
        // serde_json on a tiny fixed struct never fails.
        let json = serde_json::to_vec(self).expect("cursor serializes");
        URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode an opaque token. A malformed cursor is a client error (400), not a
    /// 500 — it usually means a hand-edited or truncated value.
    pub fn decode(token: &str) -> Result<Cursor, ApiError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| invalid_cursor())?;
        serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())
    }
}

fn invalid_cursor() -> ApiError {
    ApiError::bad_request("invalid cursor")
        .with_hint("pass an unmodified `next_cursor` returned by a previous page, or omit it")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let c = Cursor::new(123, Some("2026-05-01T10:00:00Z".into()));
        let token = c.encode();
        let back = Cursor::decode(&token).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn round_trips_null_sort_key() {
        let c = Cursor::new(7, None);
        let back = Cursor::decode(&c.encode()).unwrap();
        assert_eq!(back, c);
        assert!(back.sort_key.is_none());
    }

    #[test]
    fn token_is_url_safe_and_unpadded() {
        let token = Cursor::new(999_999, Some("x/y+z".into())).encode();
        assert!(!token.contains('='), "no padding");
        assert!(!token.contains('+') && !token.contains('/'), "url-safe");
    }

    #[test]
    fn malformed_cursor_is_bad_request() {
        let err = Cursor::decode("not!base64!").unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        // valid base64 but not our json shape -> still 400
        let bogus = URL_SAFE_NO_PAD.encode(b"hello");
        assert_eq!(
            Cursor::decode(&bogus).unwrap_err().status,
            axum::http::StatusCode::BAD_REQUEST
        );
    }
}
