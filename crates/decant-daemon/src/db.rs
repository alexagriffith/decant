//! Daemon-owned SQLite access.
//!
//! The ingest task holds a single exclusive **write** connection (opened with
//! [`open_write`]); HTTP handlers borrow pooled **read** connections from an
//! `r2d2` pool ([`read_pool`]). Both go through `decant_core::db::open`, so WAL
//! mode and `busy_timeout` are configured identically to the CLI — there is one
//! writer and the readers tolerate the writer's transactions via `busy_timeout`.
//!
//! We implement `r2d2::ManageConnection` by hand instead of pulling in
//! `r2d2_sqlite`: no published `r2d2_sqlite` targets the `rusqlite` 0.40 that
//! `decant-core` pins, and adding one would link a second `libsqlite3-sys`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

/// The single exclusive write connection, shared behind a mutex.
///
/// There is exactly **one** write `Connection` for the whole daemon. The ingest
/// task holds a clone for its sync loop; the recommendations write endpoint
/// holds another clone. The mutex (not SQLite's file lock) serializes the two,
/// so we never introduce a second uncoordinated writer that could fight the WAL
/// lock — at most one task ever holds the connection at a time.
pub type WriteConn = Arc<Mutex<Connection>>;

/// Open the single exclusive write connection and wrap it for sharing.
/// Reuses decant-core's open/configure (WAL + `busy_timeout` + foreign keys).
pub fn open_write(path: &Path) -> decant_core::Result<Connection> {
    decant_core::db::open(path)
}

/// Wrap an owned write [`Connection`] as a shareable [`WriteConn`].
pub fn shared_write(conn: Connection) -> WriteConn {
    Arc::new(Mutex::new(conn))
}

/// An `r2d2` pool of read connections to the archive at `path`.
pub type ReadPool = r2d2::Pool<SqliteConnectionManager>;

/// Build a read pool of `size` connections over `path`.
pub fn read_pool(path: &Path, size: u32) -> Result<ReadPool, r2d2::Error> {
    r2d2::Pool::builder()
        .max_size(size)
        .build(SqliteConnectionManager::new(path))
}

/// `r2d2` connection manager that opens decant archive connections via
/// `decant_core::db::open`.
#[derive(Debug)]
pub struct SqliteConnectionManager {
    path: PathBuf,
}

impl SqliteConnectionManager {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        // Map decant-core's error back to rusqlite::Error for r2d2; the only
        // failure mode here is the underlying rusqlite open/configure.
        decant_core::db::open(&self.path).map_err(|e| match e {
            decant_core::Error::Sqlite(e) => e,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(
                other.to_string(),
            ))),
        })
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        conn.execute_batch("SELECT 1")
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_hands_out_usable_read_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.db");
        let conn = open_write(&path).unwrap();
        decant_core::schema::migrate(&conn).unwrap();
        drop(conn);

        let pool = read_pool(&path, 4).unwrap();
        let c = pool.get().unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        let mode: String = c
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
