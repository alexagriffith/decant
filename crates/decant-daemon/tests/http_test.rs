use decant_daemon::config::Config;

async fn spawn(token: &str) -> (String, tokio::task::JoinHandle<()>) {
    let cfg = Config::from_values(Some("0".into()), None, None); // port 0 = OS-assigned
    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: token.to_string(),
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
