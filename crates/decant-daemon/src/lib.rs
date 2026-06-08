//! decant-daemon: the long-running service that owns the archive and serves the HTTP API.

pub mod auth;
pub mod config;
pub mod db;
pub mod health;
pub mod http;
pub mod ingest;
pub mod lock;
pub mod metadata;
pub mod middleware;
pub mod sync_status;
pub mod watcher;

/// HTTP API contract version, surfaced in the `X-Decant-API-Version` header and `/health`.
pub const API_VERSION: u32 = 1;

/// Smoke value used by the first test to prove the crate builds.
pub fn api_version() -> u32 {
    API_VERSION
}

/// Number of pooled read connections for HTTP handlers.
const READ_POOL_SIZE: u32 = 4;

/// Boot the daemon: acquire the single-instance lock, load the token, open the
/// private SQLite DB (one exclusive writer + a read pool), spawn the filesystem
/// watcher and ingest task, then serve until shutdown — at which point the
/// ingest task is asked to stop and awaited so its current transaction finishes.
pub async fn run(cfg: config::Config) -> anyhow::Result<()> {
    let _lock = lock::InstanceLock::acquire(&cfg.lock_path())
        .map_err(|e| anyhow::anyhow!("could not start: {e}"))?;
    let token = auth::load_or_create(&cfg.token_path())?;

    // Resolve the core config (DB path + source dirs). Source dirs honor
    // DECANT_CLAUDE_DIR / DECANT_CODEX_DIR, else platform defaults.
    let core_cfg = ingest::core_config_for(&cfg.db_path, None, None);
    if let Some(parent) = core_cfg.db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Single exclusive write connection (owned by the ingest task) + read pool.
    let write = db::open_write(&core_cfg.db_path)?;
    decant_core::schema::migrate(&write)?;
    let read_pool = db::read_pool(&core_cfg.db_path, READ_POOL_SIZE)
        .map_err(|e| anyhow::anyhow!("could not build read pool: {e}"))?;

    let sync_status = sync_status::SyncStatusHandle::new();

    // Shared shutdown: a watch channel fired by the OS-signal listener; both the
    // HTTP server and the ingest loop observe it.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        http::shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    // Filesystem watcher -> debounced trigger channel -> ingest loop.
    let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel::<()>(16);
    let watch_dirs = ingest::watch_dirs(&core_cfg);
    let _watcher = match watcher::SourceWatcher::start(
        watch_dirs,
        trigger_tx,
        watcher::DEFAULT_DEBOUNCE,
    ) {
        Ok(w) => Some(w),
        Err(e) => {
            // A watcher failure is non-fatal: the periodic fallback keeps ingest
            // running. Log loudly and continue.
            tracing::error!(error = %e, "filesystem watcher failed to start; relying on periodic sync");
            None
        }
    };

    // Spawn the ingest task (the only writer).
    let ingest_shutdown = wait_for_shutdown(shutdown_rx.clone());
    let ingest_handle = tokio::spawn(ingest::run_loop(
        write,
        core_cfg.clone(),
        sync_status.clone(),
        trigger_rx,
        ingest::DEFAULT_SYNC_INTERVAL,
        ingest_shutdown,
    ));

    let app = http::router(http::AppState {
        token,
        read_pool,
        sync_status,
    });

    // Serve until shutdown, then wind down the ingest task cleanly.
    let serve_shutdown = wait_for_shutdown(shutdown_rx);
    http::serve(&cfg.bind_addr(), app, serve_shutdown).await?;

    // Drop the watcher first so no new triggers arrive, then await the ingest
    // task so any in-flight sync transaction completes before we exit.
    drop(_watcher);
    if let Err(e) = ingest_handle.await {
        tracing::error!(error = %e, "ingest task did not shut down cleanly");
    }
    Ok(())
}

/// Resolve once the shared shutdown signal flips to `true`.
async fn wait_for_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    // If already set, return immediately; otherwise wait for the change.
    if *rx.borrow() {
        return;
    }
    let _ = rx.changed().await;
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_version_is_one() {
        assert_eq!(super::api_version(), 1);
    }
}
