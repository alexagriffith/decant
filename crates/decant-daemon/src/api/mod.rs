//! The `/api/v1` read API: the `{data, meta, errors}` envelope, opaque cursor
//! pagination, filter parsing, the query layer, and the HTTP handlers.
//!
//! All handlers sit behind the auth/Host/Origin guard (mounted in `http::router`)
//! except `/health`. They borrow a pooled **read** connection and run the
//! (synchronous) rusqlite work on `tokio::task::spawn_blocking` so the async
//! runtime is never blocked.

pub mod cursor;
pub mod envelope;
pub mod filters;
pub mod query;

pub mod analytics;
pub mod search;
pub mod sessions;
pub mod tools;

use crate::db::ReadPool;
use envelope::ApiError;
use rusqlite::Connection;

/// Run `f` against a pooled read connection on a blocking thread.
///
/// rusqlite is synchronous; doing query work directly in an async handler would
/// stall the runtime. `spawn_blocking` moves it to the blocking pool. A pool
/// checkout failure (exhausted/timed out) or a panic inside `f` becomes a 500.
pub async fn with_read_conn<T, F>(pool: &ReadPool, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, ApiError> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool
            .get()
            .map_err(|e| ApiError::internal(format!("read pool exhausted: {e}")))?;
        f(&conn)
    })
    .await
    .map_err(|e| ApiError::internal(format!("query task failed: {e}")))?
}
