use decant_daemon::config::Config;

async fn spawn(token: &str) -> (String, tokio::task::JoinHandle<()>) {
    let cfg = Config::from_values(Some("0".into()), None, None); // port 0 = OS-assigned

    // A real (migrated) temp DB + read pool so AppState is fully built. Leak the
    // tempdir so the DB outlives the spawned server for the duration of the test.
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    let db_path = dir.path().join("d.db");
    {
        let conn = decant_core::db::open(&db_path).unwrap();
        decant_core::schema::migrate(&conn).unwrap();
    }
    let read_pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();

    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: token.to_string(),
        read_pool,
        sync_status: decant_daemon::sync_status::SyncStatusHandle::new(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _ = cfg;
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{}", addr.port()), handle)
}

#[tokio::test]
async fn health_is_open_and_reports_version() {
    let (base, _h) = spawn("secret-token").await;
    let res = reqwest::get(format!("{base}/api/v1/health")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["data"]["api_version"], 1);
}

#[tokio::test]
async fn protected_route_requires_token_and_good_host() {
    let (base, _h) = spawn("secret-token").await;
    let client = reqwest::Client::new();

    // Missing token -> 401
    let r = client
        .get(format!("{base}/api/v1/ping"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // Good token -> 200
    let r = client
        .get(format!("{base}/api/v1/ping"))
        .header("authorization", "Bearer secret-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // Bad Host header -> 403 (DNS-rebinding defense)
    let r = client
        .get(format!("{base}/api/v1/ping"))
        .header("authorization", "Bearer secret-token")
        .header("host", "evil.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
}
