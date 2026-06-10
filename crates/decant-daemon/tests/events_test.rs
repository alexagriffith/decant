//! Integration tests for the Plan 4 SSE change-stream (`GET /api/v1/events`).
//!
//! Boot the real router (with the auth/Host/Origin guard) over a temp DB, open a
//! streaming connection to `/api/v1/events` with a valid bearer token, publish a
//! `ChangeEvent` on the broadcast sender held in `AppState`, and assert the
//! client receives an `archive_updated` SSE frame carrying the expected JSON.
//! Also assert that a tokenless request is rejected with 401 before any stream
//! is established.

use std::time::Duration;

use decant_daemon::events::{ChangeEvent, ChangeSender};
use decant_daemon::sync_status::SyncStatusHandle;
use futures_util::StreamExt;

const TOKEN: &str = "secret-token";

/// Boot the router over a fresh migrated temp DB and return the base URL plus the
/// change-event sender so the test can publish events the SSE handler fans out.
async fn spawn() -> (String, ChangeSender) {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    let db_path = dir.path().join("d.db");
    {
        let conn = decant_core::db::open(&db_path).unwrap();
        decant_core::schema::migrate(&conn).unwrap();
    }
    let read_pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();
    let write = decant_daemon::db::shared_write(decant_daemon::db::open_write(&db_path).unwrap());

    let events = decant_daemon::events::channel();
    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: TOKEN.to_string(),
        read_pool,
        write,
        sync_status: SyncStatusHandle::new(),
        activity: std::sync::Arc::new(decant_daemon::activity::ActivityTracker::default()),
        events: events.clone(),
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{}", addr.port()), events)
}

#[tokio::test]
async fn events_requires_a_token() {
    let (base, _tx) = spawn().await;
    let client = reqwest::Client::new();

    // No Authorization header -> 401 from the guard, before any stream opens.
    let r = client
        .get(format!("{base}/api/v1/events"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn events_streams_archive_updated_frame() {
    let (base, tx) = spawn().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/v1/events"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ctype.starts_with("text/event-stream"),
        "expected SSE content-type, got {ctype:?}"
    );
    // The version header still rides on the streaming response.
    assert_eq!(
        resp.headers()
            .get("x-decant-api-version")
            .and_then(|v| v.to_str().ok()),
        Some("1")
    );

    // Publish events on a ticker from a background task. The handler subscribes
    // when it processes the request, which may land just after `send()` returns;
    // re-sending until the reader sees a frame closes that race without flakiness.
    let publisher = tokio::spawn(async move {
        for _ in 0..50 {
            tx.send(ChangeEvent::archive_updated(2, "2026-06-07T12:00:00Z").into())
                .ok();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let frame = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            buf.push_str(&String::from_utf8_lossy(&chunk));
            // SSE frames are terminated by a blank line.
            if buf.contains("event: archive_updated") && buf.contains("\n\n") {
                return buf.clone();
            }
        }
        panic!("stream ended before an archive_updated frame arrived; saw: {buf:?}");
    })
    .await
    .expect("timed out waiting for an SSE frame");

    publisher.abort();

    assert!(
        frame.contains("event: archive_updated"),
        "missing event name in frame: {frame:?}"
    );
    let data_line = frame
        .lines()
        .find(|l| l.starts_with("data:"))
        .unwrap_or_else(|| panic!("no data line in frame: {frame:?}"));
    let json: serde_json::Value =
        serde_json::from_str(data_line.trim_start_matches("data:").trim()).unwrap();
    assert_eq!(json["type"], "archive_updated");
    assert_eq!(json["ingested"], 2);
    assert_eq!(json["last_sync_at"], "2026-06-07T12:00:00Z");
}
