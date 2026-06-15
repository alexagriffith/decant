//! Integration tests for the Plan 3 read API: boot the real router (with the
//! auth/Host/Origin guard) against a temp DB seeded from the repo's synthetic
//! fixtures, then assert every endpoint returns 200 with the `{data, meta,
//! errors}` envelope and sane values, plus the contract edge cases: cursor
//! pagination round-trips, unauth → 401, malformed search body → 400.

use decant_daemon::sync_status::SyncStatusHandle;

const TOKEN: &str = "secret-token";

fn claude_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/sample.jsonl"
    ))
    .unwrap()
}

fn codex_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/codex/sample.jsonl"
    ))
    .unwrap()
}

/// Boot the router against a temp DB seeded with both fixtures (one Claude
/// session 2026-05-01, one Codex session 2026-05-02) via the daemon's own ingest
/// path. Returns the base URL plus the `TempDir`; the caller holds the latter so
/// the DB outlives the test and is cleaned up on drop.
async fn spawn() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let claude_dir = root.join("claude/projects");
    let codex_dir = root.join("codex");
    let csess = claude_dir.join("proj/sess.jsonl");
    std::fs::create_dir_all(csess.parent().unwrap()).unwrap();
    std::fs::write(&csess, claude_fixture()).unwrap();
    let xsess = codex_dir.join("sessions/2026/05/02/rollout-x.jsonl");
    std::fs::create_dir_all(xsess.parent().unwrap()).unwrap();
    std::fs::write(&xsess, codex_fixture()).unwrap();

    let db_path = root.join("d.db");
    let core_cfg = decant_core::config::Config {
        db_path: db_path.clone(),
        claude_dir,
        codex_dir,
    };

    // Run one real sync to populate the DB (the same path the daemon uses). This
    // also regenerates recommendations, so the `recommendation` table is seeded.
    let mut conn = decant_core::db::open(&db_path).unwrap();
    decant_core::schema::migrate(&conn).unwrap();
    let status = SyncStatusHandle::new();
    let report = decant_daemon::ingest::run_sync_once(
        &mut conn,
        &core_cfg,
        &status,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.ingested, 2, "both fixtures must ingest");
    // Keep the connection alive as the shared writer for the mark-implemented
    // endpoint (the daemon shares this exact connection with the ingest task).
    let write = decant_daemon::db::shared_write(conn);

    let read_pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();
    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: TOKEN.to_string(),
        read_pool,
        write,
        sync_status: status,
        activity: std::sync::Arc::new(decant_daemon::activity::ActivityTracker::default()),
        events: decant_daemon::events::channel(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{}", addr.port()), dir)
}

/// Boot the router like [`spawn`], but drop `tool_call` from the file DB after
/// ingest so the tools handlers' read queries fail (exercising the
/// `with_read_conn(...).await?` error propagation -> 500).
async fn spawn_without_tool_call() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let claude_dir = root.join("claude/projects");
    let codex_dir = root.join("codex");
    let csess = claude_dir.join("proj/sess.jsonl");
    std::fs::create_dir_all(csess.parent().unwrap()).unwrap();
    std::fs::write(&csess, claude_fixture()).unwrap();

    let db_path = root.join("d.db");
    let core_cfg = decant_core::config::Config {
        db_path: db_path.clone(),
        claude_dir,
        codex_dir,
    };
    let mut conn = decant_core::db::open(&db_path).unwrap();
    decant_core::schema::migrate(&conn).unwrap();
    let status = SyncStatusHandle::new();
    decant_daemon::ingest::run_sync_once(
        &mut conn,
        &core_cfg,
        &status,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    // Break the schema the tools handlers read from.
    conn.execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE tool_call;")
        .unwrap();
    let write = decant_daemon::db::shared_write(conn);

    let read_pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();
    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: TOKEN.to_string(),
        read_pool,
        write,
        sync_status: status,
        activity: std::sync::Arc::new(decant_daemon::activity::ActivityTracker::default()),
        events: decant_daemon::events::channel(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{}", addr.port()), dir)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// GET with the bearer token; assert 200 and return the parsed JSON.
async fn get_ok(base: &str, path: &str) -> serde_json::Value {
    let r = client()
        .get(format!("{base}{path}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "GET {path} should be 200");
    r.json().await.unwrap()
}

/// Assert the standard envelope shape on a success body.
fn assert_envelope(body: &serde_json::Value) {
    assert!(body.get("data").is_some(), "envelope must have data");
    assert!(body.get("meta").is_some(), "envelope must have meta");
    assert_eq!(body["errors"], serde_json::json!([]), "errors must be []");
    assert!(
        body["meta"]["timestamp"].is_string(),
        "meta.timestamp must be set"
    );
}

#[tokio::test]
async fn sessions_list_returns_envelope_and_pagination() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/sessions").await;
    assert_envelope(&body);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    // Newest first: codex (05-02) before claude (05-01).
    assert_eq!(data[0]["tool"], "codex");
    assert_eq!(body["meta"]["pagination"]["total_count"], 2);
    assert_eq!(body["meta"]["pagination"]["has_more"], false);
    // Summaries carry cache token totals (spec §5).
    assert!(data[0]["total_cache_read_tokens"].is_number());
}

#[tokio::test]
async fn sessions_cursor_pagination_round_trips() {
    let (base, _dir) = spawn().await;
    let p1 = get_ok(&base, "/api/v1/sessions?limit=1").await;
    let d1 = p1["data"].as_array().unwrap();
    assert_eq!(d1.len(), 1);
    assert_eq!(p1["meta"]["pagination"]["has_more"], true);
    assert_eq!(p1["meta"]["pagination"]["total_count"], 2);
    let cursor = p1["meta"]["pagination"]["next_cursor"].as_str().unwrap();

    let p2 = get_ok(&base, &format!("/api/v1/sessions?limit=1&cursor={cursor}")).await;
    let d2 = p2["data"].as_array().unwrap();
    assert_eq!(d2.len(), 1);
    assert_ne!(d1[0]["id"], d2[0]["id"], "pages must not overlap");
    assert_eq!(p2["meta"]["pagination"]["total_count"], 2, "total stable");
    assert_eq!(p2["meta"]["pagination"]["has_more"], false);
    assert!(p2["meta"]["pagination"]["next_cursor"].is_null());
}

#[tokio::test]
async fn session_detail_has_stats_and_messages() {
    let (base, _dir) = spawn().await;
    let list = get_ok(&base, "/api/v1/sessions?tool=claude_code").await;
    let id = list["data"][0]["id"].as_i64().unwrap();

    let body = get_ok(&base, &format!("/api/v1/sessions/{id}")).await;
    assert_envelope(&body);
    assert!(body["data"]["summary"]["id"].as_i64() == Some(id));
    assert!(body["data"]["stats"]["cost_breakdown"]["total"].is_number());
    assert_eq!(body["data"]["stats"]["duration_seconds"], 10);
    let msgs = body["data"]["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 4);
    assert!(msgs
        .iter()
        .any(|m| !m["blocks"].as_array().unwrap().is_empty()));
}

#[tokio::test]
async fn session_detail_404_for_unknown_id() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/sessions/99999"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn search_returns_hits() {
    let (base, _dir) = spawn().await;
    let r = client()
        .post(format!("{base}/api/v1/search"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .json(&serde_json::json!({"q": "auth"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_envelope(&body);
    let hits = body["data"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected at least one hit for 'auth'");
    assert!(hits[0]["snippet"].is_string());
    assert!(hits[0]["block_id"].is_number());
}

#[tokio::test]
async fn search_malformed_body_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .post(format!("{base}/api/v1/search"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body("{ this is not json ")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["error"]["code"].is_string());
}

#[tokio::test]
async fn search_malformed_fts_query_is_400_not_500() {
    let (base, _dir) = spawn().await;
    let r = client()
        .post(format!("{base}/api/v1/search"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .json(&serde_json::json!({"q": "\"unbalanced"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "malformed FTS must be 400, never 500");
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_QUERY");
    assert!(body["error"]["hint"].is_string());
}

#[tokio::test]
async fn analytics_summary_scoped_to_filters() {
    let (base, _dir) = spawn().await;
    let all = get_ok(&base, "/api/v1/analytics/summary").await;
    assert_envelope(&all);
    assert_eq!(all["data"]["sessions"], 2);
    assert!(all["data"]["estimated_cost_usd"].is_number());

    let codex = get_ok(&base, "/api/v1/analytics/summary?tool=codex").await;
    assert_eq!(codex["data"]["sessions"], 1);
    assert_eq!(codex["meta"]["filters_applied"]["tool"], "codex");
}

#[tokio::test]
async fn analytics_by_dimension_ranked() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/analytics/by-dimension?dim=tool").await;
    assert_envelope(&body);
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(body["meta"]["pagination"]["total_count"], 2);
    let keys: Vec<&str> = rows.iter().map(|r| r["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"codex") && keys.contains(&"claude_code"));
}

#[tokio::test]
async fn analytics_by_dimension_bad_dim_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/analytics/by-dimension?dim=bogus"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_FILTER");
}

#[tokio::test]
async fn tools_usage_bad_filter_is_400() {
    // The `usage` handler's `Filters::parse` `?` rejects a malformed `from` date.
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/tools/usage?from=not-a-date"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
}

#[tokio::test]
async fn tools_mcp_usage_bad_filter_is_400() {
    // The `mcp_usage` handler's `Filters::parse` `?` rejects a malformed `from`.
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/tools/mcp-usage?from=not-a-date"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
}

#[tokio::test]
async fn tools_usage_db_error_is_500() {
    // The read query fails (no `tool_call` table) -> the `usage` handler's
    // `with_read_conn(...).await?` propagates a 500.
    let (base, _dir) = spawn_without_tool_call().await;
    let r = client()
        .get(format!("{base}/api/v1/tools/usage"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 500);
}

#[tokio::test]
async fn tools_mcp_usage_db_error_is_500() {
    // Same for the `mcp_usage` handler's `?`.
    let (base, _dir) = spawn_without_tool_call().await;
    let r = client()
        .get(format!("{base}/api/v1/tools/mcp-usage"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 500);
}

#[tokio::test]
async fn analytics_by_dimension_bad_filter_is_400() {
    // A valid `dim` but a malformed `from` date -> the handler's `Filters::parse`
    // `?` rejects the request with INVALID_FILTER.
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!(
            "{base}/api/v1/analytics/by-dimension?dim=tool&from=not-a-date"
        ))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_FILTER");
}

#[tokio::test]
async fn analytics_files_bad_filter_is_400() {
    // The `files` handler's `Filters::parse` `?` rejects a malformed `from` date.
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/analytics/files?from=not-a-date"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_FILTER");
}

#[tokio::test]
async fn analytics_activity_padded_arrays() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/analytics/activity").await;
    assert_envelope(&body);
    assert_eq!(body["data"]["by_hour"].as_array().unwrap().len(), 24);
    assert_eq!(body["data"]["by_weekday"].as_array().unwrap().len(), 7);
    assert!(body["data"]["timezone"].is_string());
    // Two sessions total across the hour histogram.
    let sum: i64 = body["data"]["by_hour"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .sum();
    assert_eq!(sum, 2);
}

#[tokio::test]
async fn analytics_model_sparklines_shared_axis() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/analytics/model-sparklines").await;
    assert_envelope(&body);
    let days = body["data"]["days"].as_array().unwrap();
    assert_eq!(days.len(), 2, "two distinct days across fixtures");
    let models = body["data"]["models"].as_object().unwrap();
    for (_model, counts) in models {
        assert_eq!(counts.as_array().unwrap().len(), 2, "aligned to day axis");
    }
}

#[tokio::test]
async fn tools_usage_and_mcp_usage() {
    let (base, _dir) = spawn().await;
    let usage = get_ok(&base, "/api/v1/tools/usage").await;
    assert_envelope(&usage);
    let rows = usage["data"].as_array().unwrap();
    assert!(!rows.is_empty(), "fixtures have tool calls");
    assert!(rows[0]["error_rate"].is_number());

    // errors_only filters down (fixtures have no errors -> empty).
    let only = get_ok(&base, "/api/v1/tools/usage?errors_only=true").await;
    assert!(only["data"].as_array().unwrap().is_empty());

    let mcp = get_ok(&base, "/api/v1/tools/mcp-usage").await;
    assert_envelope(&mcp);
    // No MCP tools in the fixtures.
    assert!(mcp["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn metadata_date_bounds() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/metadata/date-bounds").await;
    assert_envelope(&body);
    assert_eq!(body["data"]["min"], "2026-05-01");
    assert_eq!(body["data"]["max"], "2026-05-02");
}

#[tokio::test]
async fn unauthenticated_requests_are_401() {
    let (base, _dir) = spawn().await;
    for path in [
        "/api/v1/sessions",
        "/api/v1/analytics/summary",
        "/api/v1/tools/usage",
        "/api/v1/metadata/date-bounds",
    ] {
        let r = client().get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(r.status(), 401, "{path} must require auth");
    }
    let r = client()
        .post(format!("{base}/api/v1/search"))
        .json(&serde_json::json!({"q": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn invalid_date_filter_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/sessions?from=05-01-2026"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_FILTER");
}

#[tokio::test]
async fn responses_carry_api_version_header() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/sessions"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers().get("x-decant-api-version").unwrap(), "1");
}

/// POST mark-implemented with the bearer token; return (status, body).
async fn post_mark(
    base: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let r = client()
        .post(format!("{base}/api/v1/recommendations/mark-implemented"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status();
    (status, r.json().await.unwrap())
}

#[tokio::test]
async fn recommendations_list_returns_envelope_with_catalog() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/recommendations").await;
    assert_envelope(&body);
    let rows = body["data"].as_array().unwrap();
    // The evergreen catalog (7 entries) is always materialized by the sync's
    // regeneration; default status filter is `open`.
    let keys: Vec<&str> = rows.iter().map(|r| r["key"].as_str().unwrap()).collect();
    assert!(
        keys.contains(&"catalog:agents-md"),
        "catalog must be present"
    );
    assert!(keys.contains(&"catalog:hooks"));
    assert!(rows.iter().all(|r| r["status"] == "open"));
    assert_eq!(body["meta"]["filters_applied"]["status"], "open");
    let agents = rows
        .iter()
        .find(|r| r["key"] == "catalog:agents-md")
        .unwrap();
    assert_eq!(agents["kind"], "catalog");
    assert_eq!(agents["title"], "AGENTS.md at the repo root");
    assert_eq!(agents["memory_layer"], "Hot");
    assert_eq!(agents["promotion_target"], "AGENTS.md");
    assert!(agents["trigger"].as_str().unwrap().contains("Every"));
    assert!(agents["evidence"]
        .as_str()
        .unwrap()
        .contains("machine-readable"));
    assert!(agents["action"].as_str().unwrap().contains("AGENTS.md"));
    assert!(agents["success_metric"].is_string());
    assert!(agents["first_seen_at"].is_string());
}

#[tokio::test]
async fn mark_implemented_flips_status_and_is_visible_under_implemented_filter() {
    let (base, _dir) = spawn().await;
    let before = get_ok(&base, "/api/v1/recommendations?status=implemented").await;
    assert!(before["data"].as_array().unwrap().is_empty());

    let (status, body) = post_mark(
        &base,
        serde_json::json!({"key": "catalog:agents-md", "source": "agent", "note": "wired it up"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["key"], "catalog:agents-md");
    assert_eq!(body["data"]["status"], "implemented");
    assert_eq!(body["data"]["status_source"], "agent");

    let after = get_ok(&base, "/api/v1/recommendations?status=implemented").await;
    let impl_rows = after["data"].as_array().unwrap();
    assert_eq!(impl_rows.len(), 1);
    assert_eq!(impl_rows[0]["key"], "catalog:agents-md");
    assert_eq!(impl_rows[0]["status_source"], "agent");
    assert_eq!(impl_rows[0]["note"], "wired it up");
    assert!(impl_rows[0]["implemented_at"].is_string());

    let open = get_ok(&base, "/api/v1/recommendations?status=open").await;
    assert!(!open["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["key"] == "catalog:agents-md"));

    let all = get_ok(&base, "/api/v1/recommendations?status=all").await;
    assert!(all["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["key"] == "catalog:agents-md" && r["status"] == "implemented"));
}

#[tokio::test]
async fn mark_implemented_is_idempotent() {
    let (base, _dir) = spawn().await;
    let (s1, _) = post_mark(&base, serde_json::json!({"key": "catalog:skills"})).await;
    assert_eq!(s1, 200);
    let (s2, body2) = post_mark(
        &base,
        serde_json::json!({"key": "catalog:skills", "source": "manual"}),
    )
    .await;
    assert_eq!(s2, 200);
    assert_eq!(body2["data"]["status"], "implemented");
    let implemented = get_ok(&base, "/api/v1/recommendations?status=implemented").await;
    let n = implemented["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["key"] == "catalog:skills")
        .count();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn mark_implemented_unknown_key_is_404() {
    let (base, _dir) = spawn().await;
    let (status, body) = post_mark(&base, serde_json::json!({"key": "catalog:nope"})).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn recommendations_unknown_status_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/recommendations?status=bogus"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_FILTER");
}

#[tokio::test]
async fn recommendations_endpoints_require_auth() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!("{base}/api/v1/recommendations"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // POST without token -> 401 (never reaches the write).
    let r = client()
        .post(format!("{base}/api/v1/recommendations/mark-implemented"))
        .json(&serde_json::json!({"key": "catalog:agents-md"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn analytics_by_dimension_project_rollup_exposes_worktree_count_and_root_param() {
    let (base, _dir) = spawn().await;

    // Rolled-up project dimension: rows carry worktree_count (0 for the
    // worktree-free fixtures) — proves the DimRow change reaches HTTP.
    let body = get_ok(&base, "/api/v1/analytics/by-dimension?dim=project").await;
    let rows = body["data"].as_array().unwrap();
    assert!(!rows.is_empty(), "project rollup has rows");
    assert!(
        rows[0].get("worktree_count").is_some(),
        "rolled project rows expose worktree_count"
    );

    // The leaf breakdown for that root returns 200 with at least the root itself.
    let key = rows[0]["key"].as_str().unwrap();
    let enc = key.replace('/', "%2F");
    let leaf = get_ok(
        &base,
        &format!("/api/v1/analytics/by-dimension?dim=project&root={enc}"),
    )
    .await;
    let leaf_rows = leaf["data"].as_array().unwrap();
    assert!(!leaf_rows.is_empty());
    assert!(
        leaf_rows.iter().all(|r| r.get("worktree_count").is_none()),
        "leaf rows must not carry worktree_count (proves root param reached the query)"
    );

    // Empty root (?root=) must behave as absent — i.e. return the rolled-up view
    // (rows carry worktree_count), not leaf mode (which would return empty data
    // because no project has root_path = '').
    let rolled = get_ok(&base, "/api/v1/analytics/by-dimension?dim=project&root=").await;
    let rolled_rows = rolled["data"].as_array().unwrap();
    assert!(
        rolled_rows
            .first()
            .and_then(|r| r.get("worktree_count"))
            .is_some(),
        "empty root must behave as absent (rolled-up view)"
    );
}

#[tokio::test]
async fn analytics_files_returns_hotspots() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/analytics/files").await;
    assert_envelope(&body);
    let data = body["data"].as_array().unwrap();
    // Claude fixture has one Read of /Users/dev/proj/auth_test.py; codex sample
    // has no apply_patch -> exactly one hotspot row.
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["key"], "auth_test.py");
    assert_eq!(data[0]["project"], "/Users/dev/proj");
    assert_eq!(data[0]["reads"], 1);
    assert_eq!(data[0]["edits"], 0);
    assert_eq!(data[0]["sessions"], 1);
}

#[tokio::test]
async fn analytics_files_group_ext_and_op_filter() {
    let (base, _dir) = spawn().await;
    let ext = get_ok(&base, "/api/v1/analytics/files?group=ext").await;
    let data = ext["data"].as_array().unwrap();
    assert_eq!(data[0]["key"], "py");
    assert!(data[0]["project"].is_null());

    let edits = get_ok(&base, "/api/v1/analytics/files?op=edit").await;
    assert_eq!(edits["data"].as_array().unwrap().len(), 0);

    // Filters thread through: a date window before the fixture excludes it.
    let none = get_ok(&base, "/api/v1/analytics/files?to=2026-04-01").await;
    assert_eq!(none["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn analytics_files_rejects_unknown_group_and_op() {
    let (base, _dir) = spawn().await;
    for path in [
        "/api/v1/analytics/files?group=bogus",
        "/api/v1/analytics/files?op=bogus",
    ] {
        let r = client()
            .get(format!("{base}{path}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "GET {path} should be 400");
    }
}

#[tokio::test]
async fn sessions_carry_facets_and_classification() {
    let (base, _dir) = spawn().await;
    let body = get_ok(&base, "/api/v1/sessions").await;
    let data = body["data"].as_array().unwrap();
    let facets = &data[0]["facets"];
    assert!(facets.is_object(), "summary must embed a facets object");
    assert!(facets["turn_count"].is_number());
    assert!(facets["error_count"].is_number());
    assert!(facets["active_seconds"].is_number());
    // Both fixtures end with assistant output -> completed.
    assert_eq!(facets["outcome"], "completed");
    // Claude sample's first prompt is "Fix the failing auth test" -> debugging.
    let claude = data.iter().find(|s| s["tool"] == "claude_code").unwrap();
    assert_eq!(claude["facets"]["work_type"], "debugging");

    // Detail embeds the same summary shape.
    let id = data[0]["id"].as_i64().unwrap();
    let detail = get_ok(&base, &format!("/api/v1/sessions/{id}")).await;
    assert!(detail["data"]["summary"]["facets"].is_object());
}

#[tokio::test]
async fn sessions_filter_by_outcome_and_work_type() {
    let (base, _dir) = spawn().await;
    let abandoned = get_ok(&base, "/api/v1/sessions?outcome=abandoned").await;
    assert_eq!(abandoned["data"].as_array().unwrap().len(), 0);
    assert_eq!(abandoned["meta"]["pagination"]["total_count"], 0);

    let debugging = get_ok(&base, "/api/v1/sessions?work_type=debugging").await;
    let rows = debugging["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tool"], "claude_code");

    // Classification filters apply to analytics too (shared filter set).
    let summary = get_ok(&base, "/api/v1/analytics/summary?outcome=completed").await;
    assert_eq!(summary["data"]["sessions"], 2);

    for path in [
        "/api/v1/sessions?outcome=bogus",
        "/api/v1/sessions?work_type=bogus",
    ] {
        let r = client()
            .get(format!("{base}{path}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "GET {path} should be 400");
    }
}

#[tokio::test]
async fn analytics_now_reports_today_and_active_sessions() {
    // Boot with a pre-seeded tracker: one fixture source file marked active.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let claude_dir = root.join("claude/projects");
    let codex_dir = root.join("codex");
    let csess = claude_dir.join("proj/sess.jsonl");
    std::fs::create_dir_all(csess.parent().unwrap()).unwrap();
    std::fs::write(&csess, claude_fixture()).unwrap();

    let db_path = root.join("d.db");
    let core_cfg = decant_core::config::Config {
        db_path: db_path.clone(),
        claude_dir,
        codex_dir,
    };
    let mut conn = decant_core::db::open(&db_path).unwrap();
    decant_core::schema::migrate(&conn).unwrap();
    let status = decant_daemon::sync_status::SyncStatusHandle::new();
    decant_daemon::ingest::run_sync_once(
        &mut conn,
        &core_cfg,
        &status,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let write = decant_daemon::db::shared_write(conn);
    let read_pool = decant_daemon::db::read_pool(&db_path, 4).unwrap();

    let tracker = std::sync::Arc::new(decant_daemon::activity::ActivityTracker::default());
    tracker.record_write(&csess, "claude_code", std::time::Instant::now());

    let app = decant_daemon::http::router(decant_daemon::http::AppState {
        token: TOKEN.to_string(),
        read_pool,
        write,
        sync_status: status,
        activity: tracker,
        events: decant_daemon::events::channel(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://127.0.0.1:{}", addr.port());

    let body = get_ok(&base, "/api/v1/analytics/now").await;
    assert_envelope(&body);
    let data = &body["data"];
    // Fixtures are dated 2026-05; "today" totals are zero but present.
    assert_eq!(data["today"]["sessions"], 0);
    assert!(data["today"]["estimated_cost_usd"].is_number());
    // The seeded tracker entry joins back to the ingested session row.
    let active = data["active_sessions"].as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["tool"], "claude_code");
    assert_eq!(active[0]["title"], "Fix the failing auth test");
    assert_eq!(active[0]["project"], "/Users/dev/proj");
    assert!(active[0]["idle_seconds"].is_number());
    assert!(data["sync_in_progress"].is_boolean());
    assert!(data["last_sync_at"].is_string());
}

#[tokio::test]
async fn by_dimension_cursor_pagination_round_trips() {
    let (base, _dir) = spawn().await;
    // limit=1 over the two-tool fixture forces a second page and a cursor.
    let p1 = get_ok(&base, "/api/v1/analytics/by-dimension?dim=tool&limit=1").await;
    assert_eq!(p1["data"].as_array().unwrap().len(), 1);
    assert_eq!(p1["meta"]["pagination"]["has_more"], true);
    let cursor = p1["meta"]["pagination"]["next_cursor"].as_str().unwrap();

    // Page 2 decodes the cursor (analytics::by_dimension Some(cursor) arm).
    let p2 = get_ok(
        &base,
        &format!("/api/v1/analytics/by-dimension?dim=tool&limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(p2["data"].as_array().unwrap().len(), 1);
    assert_eq!(p2["meta"]["pagination"]["has_more"], false);
    assert_ne!(p1["data"][0]["key"], p2["data"][0]["key"]);
}

#[tokio::test]
async fn by_dimension_bad_cursor_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .get(format!(
            "{base}/api/v1/analytics/by-dimension?dim=tool&cursor=not-a-real-cursor"
        ))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "an undecodable cursor is a 400");
}

#[tokio::test]
async fn search_cursor_pagination_round_trips() {
    let (base, _dir) = spawn().await;
    // "auth" matches multiple blocks; limit=1 forces a cursor.
    let p1: serde_json::Value = client()
        .post(format!("{base}/api/v1/search"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .json(&serde_json::json!({"q": "the", "limit": 1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cursor = p1["meta"]["pagination"]["next_cursor"].as_str();
    if let Some(cursor) = cursor {
        // Page 2 exercises the search handler's Cursor::decode arm.
        let r = client()
            .post(format!("{base}/api/v1/search"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .json(&serde_json::json!({"q": "the", "limit": 1, "cursor": cursor}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }
}

#[tokio::test]
async fn search_bad_cursor_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .post(format!("{base}/api/v1/search"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .json(&serde_json::json!({"q": "auth", "cursor": "garbage-cursor"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "an undecodable search cursor is a 400");
}

#[tokio::test]
async fn tools_usage_errors_only_non_truthy_value_is_ignored() {
    let (base, _dir) = spawn().await;
    // A non-truthy errors_only value (truthy() Some(_) => false) means "do not
    // filter": all tools come back, same as omitting the param.
    let all = get_ok(&base, "/api/v1/tools/usage").await;
    let no = get_ok(&base, "/api/v1/tools/usage?errors_only=no").await;
    assert_eq!(
        all["data"].as_array().unwrap().len(),
        no["data"].as_array().unwrap().len()
    );
}

#[tokio::test]
async fn mark_implemented_malformed_body_is_400() {
    let (base, _dir) = spawn().await;
    let r = client()
        .post(format!("{base}/api/v1/recommendations/mark-implemented"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body("{ not valid json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["error"]["hint"].is_string());
}

#[tokio::test]
async fn mark_implemented_empty_key_is_400() {
    let (base, _dir) = spawn().await;
    let (status, body) = post_mark(&base, serde_json::json!({"key": "   "})).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"]["code"], "INVALID_REQUEST");
    assert!(body["error"]["hint"].is_string());
}

#[tokio::test]
async fn cross_origin_write_is_rejected() {
    let (base, _dir) = spawn().await;
    // A state-mutating request (POST) with a non-loopback Origin is rejected by
    // the middleware origin guard (403), even with a valid token.
    let r = client()
        .post(format!("{base}/api/v1/recommendations/mark-implemented"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("origin", "http://evil.example.com")
        .json(&serde_json::json!({"key": "catalog:agents-md"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "cross-origin write must be forbidden");
}

#[tokio::test]
async fn same_origin_loopback_write_is_allowed() {
    let (base, _dir) = spawn().await;
    // A loopback Origin on a write passes the origin guard (reaches the handler).
    let r = client()
        .post(format!("{base}/api/v1/recommendations/mark-implemented"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("origin", "http://localhost:4000")
        .json(&serde_json::json!({"key": "catalog:agents-md"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "loopback-origin write is allowed");
}
