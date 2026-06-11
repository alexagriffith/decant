//! The ingest task: the daemon's **only** writer.
//!
//! It owns the single exclusive write connection and runs decant-core's
//! incremental sync — the exact code path `decant sync` uses
//! (`ingest::sync(&mut conn, &config)`) — on three triggers:
//!   1. once on boot,
//!   2. debounced filesystem events (from the watcher), and
//!   3. a periodic fallback interval (in case the watcher misses an event).
//!
//! A sync never panics the task out: errors are recorded in [`SyncStatusHandle`]
//! and the loop simply waits for the next trigger.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use decant_core::config::Config as CoreConfig;
use decant_core::ingest::{self as core_ingest, SyncReport};
use rusqlite::Connection;
use tokio::sync::mpsc;

use crate::db::WriteConn;
use crate::events::{self, ChangeEvent, ChangeSender};
use crate::sync_status::SyncStatusHandle;

/// Default fallback interval between syncs when no filesystem events arrive.
pub const DEFAULT_SYNC_INTERVAL: Duration = Duration::from_secs(45);

/// Resolve the decant-core config (DB + source dirs) the daemon should ingest
/// from. Source dirs follow decant-core/CLI resolution: explicit override >
/// `DECANT_CLAUDE_DIR`/`DECANT_CODEX_DIR` > platform defaults
/// (`~/.claude/projects`, `~/.codex`).
pub fn core_config_for(
    db_path: &Path,
    claude_override: Option<PathBuf>,
    codex_override: Option<PathBuf>,
) -> CoreConfig {
    CoreConfig::resolve(Some(db_path.to_path_buf()), claude_override, codex_override)
}

/// The source directories to watch for changes, in the same shape decant-core
/// discovers files: the Claude projects dir, the Codex `sessions` dir, and the
/// Codex `archived_sessions` dir. Only directories that exist are returned
/// (`notify` errors on watching a missing path).
pub fn watch_dirs(cfg: &CoreConfig) -> Vec<PathBuf> {
    [
        cfg.claude_dir.clone(),
        cfg.codex_dir.join("sessions"),
        cfg.codex_dir.join("archived_sessions"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect()
}

/// Run exactly one incremental sync against the write connection, updating
/// `status` around it. Marks `in_progress` while running and records success or
/// failure on completion. Returns the report (or the core error) for callers
/// and tests; the loop ignores the return after recording status.
pub fn run_sync_once(
    write: &mut Connection,
    cfg: &CoreConfig,
    status: &SyncStatusHandle,
    cancel: &std::sync::atomic::AtomicBool,
) -> decant_core::Result<SyncReport> {
    status.set_in_progress(true);
    match core_ingest::sync_cancellable(write, cfg, cancel) {
        Ok(report) => {
            let summary = format!(
                "scanned {}, ingested {}, skipped {}, issues {}, failed {}{}",
                report.scanned,
                report.ingested,
                report.skipped,
                report.issues,
                report.failed,
                if report.cancelled { ", cancelled" } else { "" }
            );
            tracing::info!(
                scanned = report.scanned,
                ingested = report.ingested,
                skipped = report.skipped,
                issues = report.issues,
                failed = report.failed,
                cancelled = report.cancelled,
                "sync complete"
            );
            // Regenerate recommendations off the same write connection now that
            // the ingest transaction has committed. This is best-effort: a
            // regeneration failure must NOT fail the sync (the archive is already
            // updated), so we log and carry on (spec §6). Skipped on a cancelled
            // sync: the daemon is shutting down.
            if !report.cancelled {
                regenerate_recommendations(write);
            }
            status.finish_ok(report.ingested, summary);
            Ok(report)
        }
        Err(e) => {
            tracing::error!(error = %e, "sync failed");
            status.finish_err(e.to_string());
            Err(e)
        }
    }
}

/// Regenerate the recommendation set from the freshly-synced archive. Best-
/// effort: errors are logged and swallowed so they never fail the sync.
fn regenerate_recommendations(write: &Connection) {
    if let Err(e) = decant_core::recommendations::regenerate(write) {
        tracing::error!(error = %e, "recommendation regeneration failed (sync still succeeded)");
    }
}

/// Run the ingest loop until `shutdown` resolves. Owns `write` for its lifetime.
///
/// `trigger` carries debounced "sync needed" signals from the watcher; a
/// periodic timer provides the fallback. The actual sync runs on a blocking
/// thread (`spawn_blocking`) because decant-core's sync is synchronous and CPU
/// + I/O bound (it parses files in a rayon pool).
///
/// `change_tx` broadcasts a [`ChangeEvent`] to SSE subscribers after each sync
/// that ingested new data; no-op syncs do not emit (spec §7).
#[allow(clippy::too_many_arguments)]
pub async fn run_loop(
    write: WriteConn,
    cfg: CoreConfig,
    status: SyncStatusHandle,
    change_tx: ChangeSender,
    mut trigger: mpsc::Receiver<()>,
    interval: Duration,
    shutdown: impl std::future::Future<Output = ()>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) {
    sync_blocking(&write, &cfg, &status, &change_tx, &cancel).await;

    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The first tick fires immediately; consume it so we don't double-sync at boot.
    tick.tick().await;

    // Once the watcher's senders are all dropped, `trigger.recv()` returns `None`
    // immediately forever; disable that arm so the loop keeps relying on the
    // periodic fallback without busy-spinning.
    let mut watcher_alive = true;

    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                tracing::info!("ingest task stopping");
                break;
            }
            _ = tick.tick() => {
                sync_blocking(&write, &cfg, &status, &change_tx, &cancel).await;
            }
            recv = trigger.recv(), if watcher_alive => {
                match recv {
                    Some(()) => {
                        // Coalesce any backlog of triggers into this one sync.
                        while trigger.try_recv().is_ok() {}
                        sync_blocking(&write, &cfg, &status, &change_tx, &cancel).await;
                    }
                    None => {
                        // All trigger senders dropped (watcher gone). Fall back to
                        // periodic-only syncs until shutdown.
                        tracing::debug!("watcher trigger channel closed; periodic sync only");
                        watcher_alive = false;
                    }
                }
            }
        }
    }
}

/// Run one sync on a blocking thread, locking the shared write connection for
/// the duration so the mark-implemented endpoint cannot write concurrently. A
/// panic inside the blocking sync is caught here and recorded — the task never
/// aborts. A poisoned lock (a prior panic mid-write) is recovered rather than
/// propagated, so the loop keeps running.
///
/// After a successful sync that ingested new data, broadcasts a [`ChangeEvent`]
/// to SSE subscribers; a no-op sync (nothing newly ingested) is silent.
async fn sync_blocking(
    write: &WriteConn,
    cfg: &CoreConfig,
    status: &SyncStatusHandle,
    change_tx: &ChangeSender,
    cancel: &Arc<std::sync::atomic::AtomicBool>,
) {
    let cfg_task = cfg.clone();
    let status_task = status.clone();
    let write_task = write.clone();
    let cancel_task = cancel.clone();
    let join = tokio::task::spawn_blocking(move || {
        // Recover from a poisoned mutex: a previous sync may have panicked while
        // holding the lock, but the connection itself is still usable.
        let mut guard = write_task
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        run_sync_once(&mut guard, &cfg_task, &status_task, &cancel_task)
    })
    .await;
    match join {
        Ok(report) => {
            maybe_emit_change(change_tx, status, &report);
        }
        Err(e) => {
            // The blocking sync panicked. The connection lives behind the Arc and
            // is recovered on the next `lock()` (see above), so there is nothing
            // to reopen — just record the failure and keep going.
            tracing::error!(error = %e, "sync task panicked");
            status.finish_err(format!("sync task panicked: {e}"));
        }
    }
}

/// Broadcast a change event iff the sync succeeded and ingested new data.
///
/// The timestamp matches what the sync just stored in `status` (`finish_ok`
/// stamps `last_sync_at`), so the SSE event and `/metadata/sync-status` agree.
/// `events::emit` ignores a "no subscribers" send error.
fn maybe_emit_change(
    change_tx: &ChangeSender,
    status: &SyncStatusHandle,
    report: &decant_core::Result<SyncReport>,
) {
    let Ok(report) = report else { return };
    if report.ingested == 0 {
        return; // no-op sync: nothing new, don't wake subscribers.
    }
    let last_sync_at = status
        .snapshot()
        .last_sync_at
        .unwrap_or_else(crate::api::envelope::now_rfc3339);
    events::emit(
        change_tx,
        ChangeEvent::archive_updated(report.ingested, last_sync_at),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_dirs_only_returns_existing() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude/projects");
        std::fs::create_dir_all(&claude).unwrap();
        let codex = dir.path().join("codex");
        std::fs::create_dir_all(codex.join("sessions")).unwrap();
        // archived_sessions intentionally absent.

        let cfg = CoreConfig {
            db_path: dir.path().join("d.db"),
            claude_dir: claude.clone(),
            codex_dir: codex.clone(),
        };
        let dirs = watch_dirs(&cfg);
        assert!(dirs.contains(&claude));
        assert!(dirs.contains(&codex.join("sessions")));
        assert!(!dirs.contains(&codex.join("archived_sessions")));
        assert_eq!(dirs.len(), 2);
    }

    #[test]
    fn core_config_for_uses_overrides() {
        let cfg = core_config_for(
            Path::new("/tmp/a.db"),
            Some(PathBuf::from("/c")),
            Some(PathBuf::from("/x")),
        );
        assert_eq!(cfg.db_path, PathBuf::from("/tmp/a.db"));
        assert_eq!(cfg.claude_dir, PathBuf::from("/c"));
        assert_eq!(cfg.codex_dir, PathBuf::from("/x"));
    }

    #[test]
    fn run_sync_once_records_error_when_sync_fails() {
        // Point at a real fixture so there is a file to ingest, but hand the sync
        // an UN-migrated connection: writing the parsed session fails because the
        // schema tables do not exist, driving run_sync_once's error branch.
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude/projects");
        let sess = claude_dir.join("proj/sess.jsonl");
        std::fs::create_dir_all(sess.parent().unwrap()).unwrap();
        let fixture = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sample.jsonl"
        ))
        .unwrap();
        std::fs::write(&sess, fixture).unwrap();

        let cfg = CoreConfig {
            db_path: dir.path().join("d.db"),
            claude_dir,
            codex_dir: dir.path().join("codex"),
        };

        // Open WITHOUT migrating: no schema tables.
        let mut conn = decant_core::db::open(&cfg.db_path).unwrap();
        let status = SyncStatusHandle::new();
        let result = run_sync_once(
            &mut conn,
            &cfg,
            &status,
            &std::sync::atomic::AtomicBool::new(false),
        );
        assert!(result.is_err(), "sync against an un-migrated DB must error");

        let snap = status.snapshot();
        assert!(!snap.in_progress, "failure clears in_progress");
        assert!(snap.last_error.is_some(), "the error is recorded in status");
        assert!(snap.last_sync_at.is_some());
    }
}
