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
use std::time::Duration;

use decant_core::config::Config as CoreConfig;
use decant_core::ingest::{self as core_ingest, SyncReport};
use rusqlite::Connection;
use tokio::sync::mpsc;

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
) -> decant_core::Result<SyncReport> {
    status.set_in_progress(true);
    match core_ingest::sync(write, cfg) {
        Ok(report) => {
            let summary = format!(
                "scanned {}, ingested {}, skipped {}, issues {}, failed {}",
                report.scanned, report.ingested, report.skipped, report.issues, report.failed
            );
            tracing::info!(
                scanned = report.scanned,
                ingested = report.ingested,
                skipped = report.skipped,
                issues = report.issues,
                failed = report.failed,
                "sync complete"
            );
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

/// Run the ingest loop until `shutdown` resolves. Owns `write` for its lifetime.
///
/// `trigger` carries debounced "sync needed" signals from the watcher; a
/// periodic timer provides the fallback. The actual sync runs on a blocking
/// thread (`spawn_blocking`) because decant-core's sync is synchronous and CPU
/// + I/O bound (it parses files in a rayon pool).
pub async fn run_loop(
    mut write: Connection,
    cfg: CoreConfig,
    status: SyncStatusHandle,
    mut trigger: mpsc::Receiver<()>,
    interval: Duration,
    shutdown: impl std::future::Future<Output = ()>,
) {
    // Initial sync on boot.
    write = sync_blocking(write, &cfg, &status).await;

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
                write = sync_blocking(write, &cfg, &status).await;
            }
            recv = trigger.recv(), if watcher_alive => {
                match recv {
                    Some(()) => {
                        // Coalesce any backlog of triggers into this one sync.
                        while trigger.try_recv().is_ok() {}
                        write = sync_blocking(write, &cfg, &status).await;
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

/// Run one sync on a blocking thread, handing ownership of the connection in and
/// back out so the loop keeps its single exclusive writer. A panic inside the
/// blocking sync is caught here and recorded — the task never aborts.
async fn sync_blocking(
    write: Connection,
    cfg: &CoreConfig,
    status: &SyncStatusHandle,
) -> Connection {
    let cfg_task = cfg.clone();
    let status_task = status.clone();
    let join = tokio::task::spawn_blocking(move || {
        let mut write = write;
        let _ = run_sync_once(&mut write, &cfg_task, &status_task);
        write
    })
    .await;
    match join {
        Ok(conn) => conn,
        Err(e) => {
            // The blocking sync panicked; the connection was moved into the
            // panicking task and is gone. Re-open a fresh write connection so
            // the loop can keep going.
            tracing::error!(error = %e, "sync task panicked; reopening write connection");
            status.finish_err(format!("sync task panicked: {e}"));
            reopen_write(&cfg.db_path, status)
        }
    }
}

/// Best-effort reopen of the write connection after a panic. Retries briefly;
/// if it cannot reopen it returns an in-memory connection so the loop stays
/// alive (subsequent syncs will fail and be recorded, but the daemon serves on).
fn reopen_write(db_path: &Path, status: &SyncStatusHandle) -> Connection {
    match crate::db::open_write(db_path) {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!(error = %e, "could not reopen write connection");
            status.finish_err(format!("could not reopen write connection: {e}"));
            // Fall back to an in-memory connection; never panic the loop.
            rusqlite::Connection::open_in_memory().expect("in-memory sqlite always opens")
        }
    }
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
}
