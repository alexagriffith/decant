//! Shared ingest status, written by the ingest task and read by handlers.
//!
//! The state is small and updates are brief, so a plain `std::sync::RwLock`
//! behind an `Arc` is enough. Guards are never held across an `.await`.

use std::sync::{Arc, RwLock};

use serde::Serialize;

/// A point-in-time view of the ingest task. ISO8601/RFC3339 UTC timestamps.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    /// When the last sync *completed* (success or failure), RFC3339 UTC.
    pub last_sync_at: Option<String>,
    /// True while a sync is running.
    pub in_progress: bool,
    /// Human-readable summary of the last successful sync.
    pub last_report: Option<String>,
    /// Error message from the last failed sync, if the most recent run failed.
    pub last_error: Option<String>,
    /// Sessions ingested by the last successful sync.
    pub ingested_count: Option<usize>,
}

/// Cloneable handle to the shared [`SyncStatus`].
#[derive(Clone)]
pub struct SyncStatusHandle {
    inner: Arc<RwLock<SyncStatus>>,
}

impl SyncStatusHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SyncStatus::default())),
        }
    }

    /// Copy out the current status.
    pub fn snapshot(&self) -> SyncStatus {
        // A poisoned lock means a writer panicked mid-update; recover the inner
        // value rather than propagating the panic to readers.
        self.inner
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    /// Mark a sync as starting (or finished) without touching other fields.
    pub fn set_in_progress(&self, in_progress: bool) {
        self.with(|s| s.in_progress = in_progress);
    }

    /// Record a successful sync: stamp `last_sync_at`, clear the error, store the
    /// report and ingested count, and clear `in_progress`.
    pub fn finish_ok(&self, ingested: usize, report: impl Into<String>) {
        let now = now_rfc3339();
        self.with(|s| {
            s.in_progress = false;
            s.last_sync_at = Some(now);
            s.last_report = Some(report.into());
            s.ingested_count = Some(ingested);
            s.last_error = None;
        });
    }

    /// Record a failed sync: stamp `last_sync_at`, store the error, clear
    /// `in_progress`. Leaves the prior `last_report`/`ingested_count` intact.
    pub fn finish_err(&self, error: impl Into<String>) {
        let now = now_rfc3339();
        self.with(|s| {
            s.in_progress = false;
            s.last_sync_at = Some(now);
            s.last_error = Some(error.into());
        });
    }

    fn with(&self, f: impl FnOnce(&mut SyncStatus)) {
        match self.inner.write() {
            Ok(mut g) => f(&mut g),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }
}

impl Default for SyncStatusHandle {
    fn default() -> Self {
        Self::new()
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_ok_sets_fields_and_clears_error() {
        let h = SyncStatusHandle::new();
        h.set_in_progress(true);
        h.finish_err("boom"); // prior failure
        assert_eq!(h.snapshot().last_error.as_deref(), Some("boom"));

        h.set_in_progress(true);
        h.finish_ok(3, "ok: 3 ingested");
        let s = h.snapshot();
        assert!(!s.in_progress);
        assert_eq!(s.ingested_count, Some(3));
        assert_eq!(s.last_report.as_deref(), Some("ok: 3 ingested"));
        assert!(s.last_error.is_none(), "success clears prior error");
        assert!(s.last_sync_at.is_some());
        // Stamp is RFC3339.
        chrono::DateTime::parse_from_rfc3339(s.last_sync_at.as_deref().unwrap()).unwrap();
    }

    #[test]
    fn finish_err_keeps_prior_report() {
        let h = SyncStatusHandle::new();
        h.finish_ok(5, "ok: 5 ingested");
        h.finish_err("db locked");
        let s = h.snapshot();
        assert_eq!(s.last_error.as_deref(), Some("db locked"));
        assert_eq!(
            s.ingested_count,
            Some(5),
            "failure preserves last good count"
        );
        assert!(!s.in_progress);
    }
}
