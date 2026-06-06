//! decant-core: parse, normalize, store, and query AI coding-agent sessions.

pub mod db;
pub mod error;
pub mod schema;

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
