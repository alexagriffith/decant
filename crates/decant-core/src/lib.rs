//! decant-core: parse, normalize, store, and query AI coding-agent sessions.

pub mod config;
pub mod cost;
pub mod db;
pub mod error;
pub mod export;
pub mod ingest;
pub mod model;
pub mod query;
pub mod recommendations;
pub mod schema;
pub mod sources;
pub mod stats;
pub mod tools;
pub mod worktree;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonempty() {
        assert!(!version().is_empty());
    }
}
