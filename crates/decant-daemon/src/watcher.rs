//! Filesystem watcher over the Claude + Codex source dirs.
//!
//! Raw `notify` events are noisy (one logical save can emit several), so we
//! debounce: after the first event we wait for a quiet window
//! ([`DEFAULT_DEBOUNCE`]) and then emit a single "sync needed" `()` on the
//! trigger channel the ingest loop listens to. The watcher misses nothing
//! important because the ingest task also runs on a periodic fallback.

use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Quiet window collapsing a burst of file events into one sync.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(1500);

/// A running filesystem watcher. Dropping it stops watching and lets the
/// debounce thread exit.
pub struct SourceWatcher {
    _watcher: notify::RecommendedWatcher,
    _debounce: std::thread::JoinHandle<()>,
}

impl SourceWatcher {
    /// Watch `dirs` recursively, debouncing events into `trigger`.
    ///
    /// Returns an error only if `notify` cannot create the backend or watch a
    /// path. Missing paths should be filtered out by the caller
    /// (`ingest::watch_dirs`).
    pub fn start(
        dirs: Vec<PathBuf>,
        trigger: mpsc::Sender<()>,
        debounce: Duration,
    ) -> notify::Result<Self> {
        let (raw_tx, raw_rx) = std_mpsc::channel::<()>();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) if is_relevant(&event.kind) => {
                    let _ = raw_tx.send(());
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(error = %e, "watch error"),
            })?;

        for dir in &dirs {
            watcher.watch(dir, RecursiveMode::Recursive)?;
            tracing::info!(dir = %dir.display(), "watching source dir");
        }

        let debounce_handle = std::thread::spawn(move || debounce_loop(raw_rx, trigger, debounce));

        Ok(Self {
            _watcher: watcher,
            _debounce: debounce_handle,
        })
    }
}

/// Collapse bursts of raw events into single trigger sends. Exits when the raw
/// channel closes (watcher dropped) or the trigger channel closes (ingest loop
/// gone).
fn debounce_loop(raw_rx: std_mpsc::Receiver<()>, trigger: mpsc::Sender<()>, debounce: Duration) {
    while raw_rx.recv().is_ok() {
        // Got one event; drain the quiet window, restarting it on each new event.
        loop {
            match raw_rx.recv_timeout(debounce) {
                Ok(()) => continue,
                Err(std_mpsc::RecvTimeoutError::Timeout) => break,
                Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        if trigger.blocking_send(()).is_err() {
            // Ingest loop is gone; nothing left to notify.
            return;
        }
    }
}

/// Only data-changing events should trigger a sync. Access-time and metadata-
/// only events are ignored to cut churn.
fn is_relevant(kind: &notify::EventKind) -> bool {
    use notify::EventKind;
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevant_event_kinds() {
        use notify::event::{CreateKind, ModifyKind, RemoveKind};
        use notify::EventKind;
        assert!(is_relevant(&EventKind::Create(CreateKind::File)));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Any)));
        assert!(is_relevant(&EventKind::Remove(RemoveKind::File)));
        assert!(!is_relevant(&EventKind::Access(
            notify::event::AccessKind::Read
        )));
        assert!(!is_relevant(&EventKind::Any));
    }

    #[tokio::test]
    async fn watcher_emits_trigger_on_file_write() {
        let dir = tempfile::tempdir().unwrap();
        let watched = dir.path().join("projects");
        std::fs::create_dir_all(&watched).unwrap();

        let (tx, mut rx) = mpsc::channel::<()>(8);
        let _w = SourceWatcher::start(
            vec![watched.clone()],
            tx,
            Duration::from_millis(100), // short debounce for the test
        )
        .unwrap();

        // Write a file into the watched dir.
        std::fs::write(watched.join("sess.jsonl"), "{}\n").unwrap();

        // Expect a debounced trigger within a generous window.
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
        assert!(
            matches!(got, Ok(Some(()))),
            "expected a debounced sync trigger, got {got:?}"
        );
    }
}
