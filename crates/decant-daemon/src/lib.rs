//! decant-daemon: the long-running service that owns the archive and serves the HTTP API.

pub mod activity;
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod events;
pub mod health;
pub mod http;
pub mod ingest;
pub mod lock;
pub mod metadata;
pub mod middleware;
pub mod openapi;
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

    let core_cfg = ingest::core_config_for(&cfg.db_path, None, None);
    if let Some(parent) = core_cfg.db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Single exclusive write connection, shared behind a mutex: the ingest task
    // and the recommendations write endpoint each hold a clone, and the mutex
    // serializes them so there is only ever one writer. Plus the read pool.
    let write_conn = db::open_write(&core_cfg.db_path)?;
    decant_core::schema::migrate(&write_conn)?;
    let write = db::shared_write(write_conn);
    let read_pool = db::read_pool(&core_cfg.db_path, READ_POOL_SIZE)
        .map_err(|e| anyhow::anyhow!("could not build read pool: {e}"))?;

    let sync_status = sync_status::SyncStatusHandle::new();

    // Change-event broadcast channel (spec §7): the ingest task sends on it after
    // a sync that ingested new data; the SSE handler subscribes per connection.
    let change_tx = events::channel();

    // Shared shutdown: a watch channel fired by the OS-signal listener; both the
    // HTTP server and the ingest loop observe it. The cancel flag reaches into a
    // sync already running on the blocking thread, which can't see the channel.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let sync_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal_cancel = sync_cancel.clone();
    tokio::spawn(async move {
        http::shutdown_signal().await;
        signal_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = shutdown_tx.send(true);
    });

    // Live-session tracker: raw watcher events feed it (pre-debounce); a sweep
    // task expires quiet sessions; `/analytics/now` and SSE expose it.
    let tracker = std::sync::Arc::new(activity::ActivityTracker::default());

    // Filesystem watcher -> debounced trigger channel -> ingest loop.
    let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel::<()>(16);
    let watch_dirs = ingest::watch_dirs(&core_cfg);
    let path_hook: watcher::PathHook = {
        let tracker = tracker.clone();
        let tx = change_tx.clone();
        let claude_dir = core_cfg.claude_dir.clone();
        let codex_dir = core_cfg.codex_dir.clone();
        Box::new(move |path| {
            let Some(tool) = activity::classify_source_path(path, &claude_dir, &codex_dir) else {
                return;
            };
            if tracker.record_write(path, tool, std::time::Instant::now()) {
                events::emit(
                    &tx,
                    events::ActivityEvent::new("active", tool, path.to_string_lossy()),
                );
            }
        })
    };
    let _watcher = match watcher::SourceWatcher::start(
        watch_dirs,
        trigger_tx,
        watcher::DEFAULT_DEBOUNCE,
        Some(path_hook),
    ) {
        Ok(w) => Some(w),
        Err(e) => {
            // A watcher failure is non-fatal: the periodic fallback keeps ingest
            // running. Log loudly and continue.
            tracing::error!(error = %e, "filesystem watcher failed to start; relying on periodic sync");
            None
        }
    };

    // Sweep quiet sessions to idle on a steady tick (same cadence as the SSE
    // keep-alive); each expiry broadcasts one `session_activity{idle}` event.
    {
        let tracker = tracker.clone();
        let tx = change_tx.clone();
        let mut sweep_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        for (path, tool) in tracker.sweep(std::time::Instant::now()) {
                            events::emit(
                                &tx,
                                events::ActivityEvent::new("idle", tool, path.to_string_lossy()),
                            );
                        }
                    }
                    _ = sweep_shutdown.changed() => return,
                }
            }
        });
    }

    // Spawn the ingest task (the only writer). It gets a sender clone so each
    // sync that ingests new data broadcasts a change event.
    let ingest_shutdown = wait_for_shutdown(shutdown_rx.clone());
    let ingest_handle = tokio::spawn(ingest::run_loop(
        write.clone(),
        core_cfg.clone(),
        sync_status.clone(),
        change_tx.clone(),
        trigger_rx,
        ingest::DEFAULT_SYNC_INTERVAL,
        ingest_shutdown,
        sync_cancel,
    ));

    let app = http::router(http::AppState {
        token,
        read_pool,
        write,
        sync_status,
        events: change_tx,
        activity: tracker,
    });

    // Graceful shutdown waits for open connections, but SSE subscribers hold
    // theirs open indefinitely — bound the wait and then drop the server
    // (clients reconnect by design).
    let serve_shutdown = wait_for_shutdown(shutdown_rx.clone());
    let grace_elapsed = async {
        wait_for_shutdown(shutdown_rx).await;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    };
    let bind_addr = cfg.bind_addr();
    tokio::select! {
        res = http::serve(&bind_addr, app, serve_shutdown) => res?,
        _ = grace_elapsed => {
            tracing::warn!("shutdown grace period elapsed; dropping open connections");
        }
    }

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
