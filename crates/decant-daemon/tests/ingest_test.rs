//! Integration tests for the daemon data path (Plan 2): point the ingest task
//! at a temp DB + a temp source dir seeded with a synthetic fixture, run one
//! sync directly, and assert the DB has the expected rows AND that sync-status
//! reflects a recent, completed sync. Also exercises the `sync-status` endpoint
//! through the real router + guard middleware.

use std::path::PathBuf;

use decant_daemon::ingest;
use decant_daemon::sync_status::SyncStatusHandle;

fn claude_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/sample.jsonl"
    ))
    .unwrap()
}

/// A core config pointing entirely inside `dir`: a Claude session under
/// `claude/projects`, an (empty) codex home, and a DB at `d.db`.
fn seed_core_config(dir: &std::path::Path) -> decant_core::config::Config {
    let claude_dir = dir.join("claude/projects");
    let codex_dir = dir.join("codex");
    let sess = claude_dir.join("proj/sess.jsonl");
    std::fs::create_dir_all(sess.parent().unwrap()).unwrap();
    std::fs::write(&sess, claude_fixture()).unwrap();
    decant_core::config::Config {
        db_path: dir.join("d.db"),
        claude_dir,
        codex_dir,
    }
}

#[tokio::test]
async fn run_sync_once_ingests_fixture_and_updates_status() {
    let dir = tempfile::tempdir().unwrap();
    let core_cfg = seed_core_config(dir.path());

    // The ingest task owns the single exclusive write connection.
    let mut write = decant_core::db::open(&core_cfg.db_path).unwrap();
    decant_core::schema::migrate(&write).unwrap();

    let status = SyncStatusHandle::new();
    // Before any sync: never run.
    {
        let snap = status.snapshot();
        assert!(snap.last_sync_at.is_none());
        assert!(!snap.in_progress);
    }

    let report = ingest::run_sync_once(&mut write, &core_cfg, &status);
    assert!(report.is_ok(), "sync should not error: {report:?}");

    // DB now has exactly the one fixture session.
    let sessions: i64 = write
        .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1, "fixture session must be ingested");

    // Sync-status reflects a completed, recent sync.
    let snap = status.snapshot();
    assert!(!snap.in_progress, "sync finished -> not in progress");
    assert!(snap.last_sync_at.is_some(), "last_sync_at must be set");
    assert!(snap.last_error.is_none(), "no error expected");
    assert_eq!(snap.ingested_count, Some(1));
    // last_sync_at parses as RFC3339 and is recent (within the last minute).
    let ts = snap.last_sync_at.clone().unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(&ts)
        .unwrap_or_else(|e| panic!("last_sync_at {ts:?} must be RFC3339: {e}"));
    let age = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
    assert!(age.num_seconds() < 60, "last_sync_at must be recent: {ts}");
}

#[tokio::test]
async fn sync_status_endpoint_returns_envelope() {
    let token = "secret-token";
    let status = SyncStatusHandle::new();
    status.set_in_progress(true);
    status.finish_ok(7, "ok: 7 ingested");

    // Build the read pool over a real (migrated) temp DB.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("d.db");
    {
        let conn = decant_core::db::open(&db_path).unwrap();
        decant_core::schema::migrate(&conn).unwrap();
    }
    let pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();

    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: token.to_string(),
        read_pool: pool,
        sync_status: status.clone(),
        events: decant_daemon::events::channel(),
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://127.0.0.1:{}", addr.port());
    let client = reqwest::Client::new();

    // Unauthenticated -> 401 (behind the guard).
    let r = client
        .get(format!("{base}/api/v1/metadata/sync-status"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // Authenticated -> 200 with the {data, meta, errors} envelope.
    let r = client
        .get(format!("{base}/api/v1/metadata/sync-status"))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body.get("data").is_some(), "envelope must have data");
    assert!(body.get("meta").is_some(), "envelope must have meta");
    assert_eq!(body["errors"], serde_json::json!([]));
    assert_eq!(body["data"]["in_progress"], false);
    assert_eq!(body["data"]["ingested_count"], 7);
    assert!(body["data"]["last_sync_at"].is_string());
}

#[tokio::test]
async fn run_loop_syncs_then_stops_on_shutdown() {
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let core_cfg = seed_core_config(dir.path());

    let write = decant_core::db::open(&core_cfg.db_path).unwrap();
    decant_core::schema::migrate(&write).unwrap();

    let status = SyncStatusHandle::new();
    let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel::<()>(8);
    let (sd_tx, mut sd_rx) = tokio::sync::watch::channel(false);
    let shutdown = async move {
        // Resolve when the shutdown flag flips true.
        while !*sd_rx.borrow() {
            if sd_rx.changed().await.is_err() {
                break;
            }
        }
    };

    // Long periodic interval so the *trigger* (not the timer) drives the sync we
    // assert on.
    let loop_handle = tokio::spawn(ingest::run_loop(
        write,
        core_cfg.clone(),
        status.clone(),
        decant_daemon::events::channel(),
        trigger_rx,
        Duration::from_secs(3600),
        shutdown,
    ));

    // The boot sync should ingest the seeded fixture; poll until it lands.
    let read = decant_core::db::open(&core_cfg.db_path).unwrap();
    let mut sessions = 0i64;
    for _ in 0..50 {
        sessions = read
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        if sessions >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(sessions, 1, "boot sync must ingest the fixture");
    assert!(status.snapshot().last_sync_at.is_some());

    // Add a second session and fire a trigger; expect a re-sync to pick it up.
    let sess2 = core_cfg.claude_dir.join("proj2/sess2.jsonl");
    std::fs::create_dir_all(sess2.parent().unwrap()).unwrap();
    std::fs::write(&sess2, claude_fixture()).unwrap();
    trigger_tx.send(()).await.unwrap();

    let mut sessions2 = sessions;
    for _ in 0..50 {
        sessions2 = read
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        if sessions2 >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(sessions2, 2, "triggered sync must ingest the new session");

    // Shutdown: the loop must observe it and the task must join cleanly.
    sd_tx.send(true).unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), loop_handle).await;
    assert!(joined.is_ok(), "ingest loop must stop on shutdown");
    joined.unwrap().unwrap();
}

#[tokio::test]
async fn run_loop_broadcasts_change_event_on_ingest_but_not_on_noop() {
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let core_cfg = seed_core_config(dir.path());

    let write = decant_core::db::open(&core_cfg.db_path).unwrap();
    decant_core::schema::migrate(&write).unwrap();

    let status = SyncStatusHandle::new();
    // Subscribe BEFORE booting the loop so the boot sync's event is captured.
    let change_tx = decant_daemon::events::channel();
    let mut rx = change_tx.subscribe();

    let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel::<()>(8);
    let (sd_tx, mut sd_rx) = tokio::sync::watch::channel(false);
    let shutdown = async move {
        while !*sd_rx.borrow() {
            if sd_rx.changed().await.is_err() {
                break;
            }
        }
    };

    let loop_handle = tokio::spawn(ingest::run_loop(
        write,
        core_cfg.clone(),
        status.clone(),
        change_tx.clone(),
        trigger_rx,
        Duration::from_secs(3600),
        shutdown,
    ));

    // The boot sync ingests the seeded fixture, so it must broadcast one event.
    let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("boot sync must broadcast a change event within 5s")
        .expect("channel open");
    assert_eq!(ev.kind, "archive_updated");
    assert_eq!(ev.ingested, 1, "boot sync ingested the one fixture session");

    // Fire a trigger with NO new files: the sync is a no-op (nothing ingested),
    // so it must NOT broadcast. Give the loop time to run that sync.
    trigger_tx.send(()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    match rx.try_recv() {
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {} // expected
        other => panic!("no-op sync must not broadcast a change event, got {other:?}"),
    }

    sd_tx.send(true).unwrap();
    let joined = tokio::time::timeout(Duration::from_secs(5), loop_handle).await;
    assert!(joined.is_ok(), "ingest loop must stop on shutdown");
    joined.unwrap().unwrap();
}

#[test]
fn core_config_from_daemon_resolves_source_dirs() {
    // Daemon resolves the same source dirs decant-core/CLI use, honoring the
    // env overrides. We pass overrides explicitly to keep the test hermetic.
    let cfg = ingest::core_config_for(
        &PathBuf::from("/tmp/x.db"),
        Some(PathBuf::from("/tmp/claude")),
        Some(PathBuf::from("/tmp/codex")),
    );
    assert_eq!(cfg.db_path, PathBuf::from("/tmp/x.db"));
    assert_eq!(cfg.claude_dir, PathBuf::from("/tmp/claude"));
    assert_eq!(cfg.codex_dir, PathBuf::from("/tmp/codex"));
}
