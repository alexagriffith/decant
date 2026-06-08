# decant Daemon Foundation — Implementation Plan (Plan 1 of 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a secure `decant-daemon` that boots on loopback, serves `GET /api/v1/health`, enforces Host + bearer-token + Origin checks, holds a single-instance lock, and shuts down gracefully — the foundation the rest of the service builds on.

**Architecture:** A new workspace crate `crates/decant-daemon` with a `tokio` runtime hosting an `axum` HTTP server bound to `127.0.0.1`. A tower middleware layer enforces defense-in-depth (the loopback is not a trust boundary). A bearer token is generated once into `~/.decant/daemon.token` (0600); a PID/advisory lock file prevents two daemons. No SQLite/watcher/ingest yet — those are Plan 2.

**Tech Stack:** Rust, `tokio` 1, `axum` 0.7, `tower` 0.5, `tower-http` 0.6, `serde`/`serde_json`, `rand` 0.8, `hex` 0.4, `fs2` 0.4, `tracing` + `tracing-subscriber`. Reuses `decant-core` for version constants only.

**Spec:** `docs/superpowers/specs/2026-06-08-decant-service-architecture-design.md` (§3 daemon, §4 security, §8 lifecycle).

**Pre-flight (not a code task):** This rewrite touches nearly every file. Before executing, ensure (a) the 1Password SSH agent is unlocked (commits are signed), and (b) the concurrent `docs/dosu-agent-session-ingestion-spec` work is in its own git worktree or paused — do not execute in a shared index.

---

## File structure

| Path | Responsibility |
|---|---|
| `Cargo.toml` (workspace) | add `crates/decant-daemon` member |
| `crates/decant-daemon/Cargo.toml` | daemon crate manifest + deps |
| `crates/decant-daemon/src/main.rs` | binary entrypoint: parse args, run |
| `crates/decant-daemon/src/lib.rs` | re-exports; `run(Config)` orchestrator |
| `crates/decant-daemon/src/config.rs` | `Config` from env/flags (port, dirs) |
| `crates/decant-daemon/src/auth.rs` | token load-or-create (0600) |
| `crates/decant-daemon/src/lock.rs` | single-instance advisory lock |
| `crates/decant-daemon/src/http.rs` | axum router, state, `serve()` + shutdown |
| `crates/decant-daemon/src/middleware.rs` | Host/token/Origin guard |
| `crates/decant-daemon/src/health.rs` | `GET /api/v1/health` handler |
| `crates/decant-daemon/tests/http_test.rs` | integration tests (spawn server) |
| `crates/decant-cli/src/...` | `decant daemon serve [--foreground] [--port N]` subcommand |

**Constant:** `API_VERSION: u32 = 1` (in `lib.rs`), surfaced in the `X-Decant-API-Version` header and `/health`.

---

### Task 1: Add the `decant-daemon` crate to the workspace

**Files:**
- Modify: `Cargo.toml` (workspace `members`)
- Create: `crates/decant-daemon/Cargo.toml`
- Create: `crates/decant-daemon/src/lib.rs`
- Create: `crates/decant-daemon/src/main.rs`

- [ ] **Step 1: Add the crate to the workspace members.** In the root `Cargo.toml` `[workspace] members = [...]`, add `"crates/decant-daemon"`.

- [ ] **Step 2: Create `crates/decant-daemon/Cargo.toml`.**

```toml
[package]
name = "decant-daemon"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "decant-daemon"
path = "src/main.rs"

[dependencies]
decant-core = { path = "../decant-core" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal", "net", "time", "sync"] }
axum = "0.7"
tower = "0.5"
tower-http = { version = "0.6", features = ["trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rand = "0.8"
hex = "0.4"
fs2 = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
reqwest = { version = "0.12", features = ["json"] }
tempfile = "3"
```

- [ ] **Step 3: Create `src/lib.rs` with the version constant and a placeholder run fn.**

```rust
//! decant-daemon: the long-running service that owns the archive and serves the HTTP API.
pub mod auth;
pub mod config;
pub mod health;
pub mod http;
pub mod lock;
pub mod middleware;

/// HTTP API contract version, surfaced in the `X-Decant-API-Version` header and `/health`.
pub const API_VERSION: u32 = 1;

/// Smoke value used by the first test to prove the crate builds.
pub fn api_version() -> u32 {
    API_VERSION
}
```

- [ ] **Step 4: Create a minimal `src/main.rs` so the binary builds.**

```rust
fn main() {
    eprintln!("decant-daemon {}", decant_daemon::API_VERSION);
}
```

- [ ] **Step 5: Add a unit test in `src/lib.rs` and run it.**

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn api_version_is_one() {
        assert_eq!(super::api_version(), 1);
    }
}
```

Run: `cargo test -p decant-daemon`
Expected: PASS (1 test). Also `cargo build --workspace` succeeds.

- [ ] **Step 6: Commit.**

```bash
git add Cargo.toml crates/decant-daemon
git commit -S -m "feat(daemon): scaffold decant-daemon crate"
```

---

### Task 2: Config from environment

**Files:**
- Create: `crates/decant-daemon/src/config.rs`
- Test: in the same file (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_env_absent() {
        let c = Config::from_values(None, None, None);
        assert_eq!(c.port, 4577);
        assert!(c.config_dir.ends_with(".decant"));
        assert!(c.bind_addr().starts_with("127.0.0.1:"));
    }

    #[test]
    fn port_override_parses() {
        let c = Config::from_values(Some("9000".into()), None, None);
        assert_eq!(c.port, 9000);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test -p decant-daemon config`
Expected: FAIL (`Config` not found).

- [ ] **Step 3: Implement `config.rs`.**

```rust
use std::path::PathBuf;

/// Daemon configuration, resolved from environment with sensible defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub config_dir: PathBuf,
    pub db_path: PathBuf,
}

impl Config {
    /// Resolve from process environment.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("DECANT_DAEMON_PORT").ok(),
            std::env::var("DECANT_CONFIG_DIR").ok(),
            std::env::var("DECANT_DB").ok(),
        )
    }

    /// Resolve from explicit optional values (testable).
    pub fn from_values(port: Option<String>, config_dir: Option<String>, db: Option<String>) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let config_dir = config_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".decant"));
        let db_path = db
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".decant").join("decant.db"));
        let port = port.and_then(|p| p.parse().ok()).unwrap_or(4577);
        Self { port, config_dir, db_path }
    }

    pub fn bind_addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn token_path(&self) -> PathBuf {
        self.config_dir.join("daemon.token")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.config_dir.join("daemon.lock")
    }
}
```

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p decant-daemon config`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit.**

```bash
git add crates/decant-daemon/src/config.rs crates/decant-daemon/src/lib.rs
git commit -S -m "feat(daemon): config from environment"
```

---

### Task 3: Bearer token (load-or-create, 0600)

**Files:**
- Create: `crates/decant-daemon/src/auth.rs`
- Test: in the same file

- [ ] **Step 1: Write the failing test.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reuses_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.token");

        let first = load_or_create(&path).unwrap();
        assert_eq!(first.len(), 64); // 32 bytes hex-encoded

        let second = load_or_create(&path).unwrap();
        assert_eq!(first, second); // stable across calls

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
```

Add `tempfile` is already a dev-dependency (Task 1).

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test -p decant-daemon auth`
Expected: FAIL (`load_or_create` not found).

- [ ] **Step 3: Implement `auth.rs`.**

```rust
use rand::RngCore;
use std::io;
use std::path::Path;

/// Read the bearer token from `path`, creating a fresh 32-byte (hex) token
/// with 0600 permissions if it does not yet exist.
pub fn load_or_create(path: &Path) -> io::Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    std::fs::write(path, &token)?;
    set_0600(path)?;
    Ok(token)
}

#[cfg(unix)]
fn set_0600(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_0600(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Constant-time comparison of a presented token against the expected one.
pub fn token_matches(expected: &str, presented: &str) -> bool {
    let a = expected.as_bytes();
    let b = presented.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
```

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p decant-daemon auth`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/decant-daemon/src/auth.rs
git commit -S -m "feat(daemon): bearer token load-or-create with 0600"
```

---

### Task 4: Single-instance lock

**Files:**
- Create: `crates/decant-daemon/src/lock.rs`
- Test: in the same file

- [ ] **Step 1: Write the failing test.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_while_first_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let held = InstanceLock::acquire(&path).expect("first lock");
        let second = InstanceLock::acquire(&path);
        assert!(second.is_err(), "second lock must fail while first is held");

        drop(held);
        let third = InstanceLock::acquire(&path);
        assert!(third.is_ok(), "lock available again after release");
    }
}
```

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test -p decant-daemon lock`
Expected: FAIL (`InstanceLock` not found).

- [ ] **Step 3: Implement `lock.rs` (advisory exclusive lock via fs2).**

```rust
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// An exclusive advisory lock proving this is the only running daemon.
/// Releases on drop (and the OS releases it if the process dies).
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).write(true).truncate(false).open(path)?;
        file.try_lock_exclusive()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "another decant daemon is running"))?;
        // Best-effort PID record for humans; not used for locking.
        let _ = file.set_len(0);
        let _ = write!(file, "{}", std::process::id());
        Ok(Self { _file: file })
    }
}
```

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p decant-daemon lock`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/decant-daemon/src/lock.rs
git commit -S -m "feat(daemon): single-instance advisory lock"
```

---

### Task 5: Health handler + router + app state

**Files:**
- Create: `crates/decant-daemon/src/health.rs`
- Create: `crates/decant-daemon/src/http.rs`
- Modify: `crates/decant-daemon/src/lib.rs` (already declares modules)

- [ ] **Step 1: Write the failing integration test.** Create `crates/decant-daemon/tests/http_test.rs`:

```rust
use decant_daemon::config::Config;

async fn spawn(token: &str) -> (String, tokio::task::JoinHandle<()>) {
    let cfg = Config::from_values(Some("0".into()), None, None); // port 0 = OS-assigned
    let app = decant_daemon::http::router(decant_daemon::http::AppState { token: token.to_string() });
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
```

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test -p decant-daemon --test http_test`
Expected: FAIL (`http::router` / `AppState` not found).

- [ ] **Step 3: Implement `health.rs`.**

```rust
use axum::Json;
use serde_json::{json, Value};

/// Liveness + version. Intentionally unauthenticated so clients can probe readiness.
pub async fn health() -> Json<Value> {
    Json(json!({
        "data": {
            "api_version": crate::API_VERSION,
            "db_schema_version": 1,
            "status": "ok"
        },
        "meta": { "timestamp": null },
        "errors": []
    }))
}
```

- [ ] **Step 4: Implement `http.rs` (router + state + serve with graceful shutdown).**

```rust
use axum::{routing::get, Router};

/// Shared state available to handlers.
#[derive(Clone)]
pub struct AppState {
    pub token: String,
}

/// Build the router. `/api/v1/health` is public; everything else is gated by
/// the guard middleware (added in Task 6).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(crate::health::health))
        .with_state(state)
}

/// Serve until a shutdown signal (SIGINT/SIGTERM) is received.
pub async fn serve(addr: &str, app: Router) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "decant-daemon listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    tracing::info!("decant-daemon shutting down");
}
```

- [ ] **Step 5: Run the test.**

Run: `cargo test -p decant-daemon --test http_test`
Expected: PASS (health returns 200 with `api_version: 1`).

- [ ] **Step 6: Commit.**

```bash
git add crates/decant-daemon/src/health.rs crates/decant-daemon/src/http.rs
git commit -S -m "feat(daemon): health endpoint, router, graceful serve"
```

---

### Task 6: Security guard middleware (Host + bearer token + Origin)

**Files:**
- Create: `crates/decant-daemon/src/middleware.rs`
- Modify: `crates/decant-daemon/src/http.rs` (apply the layer to non-health routes; add a protected probe route for the test)
- Modify: `crates/decant-daemon/tests/http_test.rs` (auth tests)

- [ ] **Step 1: Write the failing tests.** Append to `tests/http_test.rs`:

```rust
#[tokio::test]
async fn protected_route_requires_token_and_good_host() {
    let (base, _h) = spawn("secret-token").await;
    let client = reqwest::Client::new();

    // Missing token -> 401
    let r = client.get(format!("{base}/api/v1/ping")).send().await.unwrap();
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
```

Add a `/api/v1/ping` protected route (Step 4) returning 200 `{}` for the test.

- [ ] **Step 2: Run it to confirm it fails.**

Run: `cargo test -p decant-daemon --test http_test protected_route`
Expected: FAIL (route missing / no auth → wrong status).

- [ ] **Step 3: Implement `middleware.rs`.**

```rust
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::http::AppState;

/// Defense-in-depth guard for `/api/*`: validates Host against a loopback
/// allowlist (anti DNS-rebinding), requires a matching bearer token, and
/// rejects cross-origin writes. `/api/v1/health` is mounted outside this layer.
pub async fn guard(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1) Host allowlist.
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !host_allowed(host) {
        return Err(StatusCode::FORBIDDEN);
    }

    // 2) Bearer token.
    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if presented.is_empty() || !crate::auth::token_matches(&state.token, presented) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 3) Origin check on state-mutating methods.
    if !matches!(req.method(), &axum::http::Method::GET | &axum::http::Method::HEAD) {
        if let Some(origin) = req.headers().get(axum::http::header::ORIGIN).and_then(|h| h.to_str().ok()) {
            if !origin_allowed(origin) {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    Ok(next.run(req).await)
}

fn host_allowed(host: &str) -> bool {
    // Strip port; accept loopback names only.
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

fn origin_allowed(origin: &str) -> bool {
    origin.starts_with("http://127.0.0.1")
        || origin.starts_with("http://localhost")
        || origin.starts_with("https://127.0.0.1")
        || origin.starts_with("https://localhost")
}
```

- [ ] **Step 4: Wire the layer in `http.rs`.** Replace `router/1` so health is public and a protected group carries the guard + the version header:

```rust
use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/ping", get(|| async { axum::Json(serde_json::json!({})) }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::guard,
        ));

    Router::new()
        .route("/api/v1/health", get(crate::health::health))
        .merge(protected)
        .layer(axum::middleware::map_response(add_version_header))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn add_version_header(mut res: axum::response::Response) -> axum::response::Response {
    res.headers_mut().insert(
        "x-decant-api-version",
        axum::http::HeaderValue::from_static("1"),
    );
    res
}
```

(Keep `serve/1` and `shutdown_signal` from Task 5 unchanged.)

- [ ] **Step 5: Run the tests.**

Run: `cargo test -p decant-daemon`
Expected: PASS (health + auth tests; missing token → 401, good → 200, bad Host → 403).

- [ ] **Step 6: Commit.**

```bash
git add crates/decant-daemon/src/middleware.rs crates/decant-daemon/src/http.rs crates/decant-daemon/tests/http_test.rs
git commit -S -m "feat(daemon): Host/token/Origin guard middleware + version header"
```

---

### Task 7: Wire `run(Config)` and the `decant daemon serve` CLI

**Files:**
- Modify: `crates/decant-daemon/src/lib.rs` (add `run`)
- Modify: `crates/decant-daemon/src/main.rs` (call `run`)
- Modify: `crates/decant-cli/src/...` (add a `daemon serve` subcommand that execs the daemon)

- [ ] **Step 1: Implement `run` in `lib.rs`.**

```rust
/// Boot the daemon: acquire the single-instance lock, load/create the token,
/// build the app, and serve until shutdown.
pub async fn run(cfg: config::Config) -> anyhow::Result<()> {
    let _lock = lock::InstanceLock::acquire(&cfg.lock_path())
        .map_err(|e| anyhow::anyhow!("could not start: {e}"))?;
    let token = auth::load_or_create(&cfg.token_path())?;
    let app = http::router(http::AppState { token });
    http::serve(&cfg.bind_addr(), app).await?;
    Ok(())
}
```

Add `anyhow = "1"` to `[dependencies]` in `crates/decant-daemon/Cargo.toml`.

- [ ] **Step 2: Implement `main.rs`.**

```rust
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cfg = decant_daemon::config::Config::from_env();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(decant_daemon::run(cfg))
}
```

- [ ] **Step 3: Add the CLI subcommand.** In `crates/decant-cli` clap definitions, add a `Daemon { #[command(subcommand)] cmd: DaemonCmd }` variant with `DaemonCmd::Serve { #[arg(long)] foreground: bool, #[arg(long)] port: Option<u16> }`. Its handler sets `DECANT_DAEMON_PORT` when `port` is given and runs the daemon by delegating to `decant_daemon::run(Config::from_env())` inside a tokio runtime (add `decant-daemon` and `tokio` as deps of `decant-cli`, or shell to the `decant-daemon` binary; prefer the in-process call for a single distributable). Keep output via the existing `decant-cli` `output` module.

- [ ] **Step 4: Build and smoke-test manually.**

Run: `cargo build --workspace`
Then, in one shell: `DECANT_CONFIG_DIR=/tmp/decant-test cargo run -p decant-cli -- daemon serve --foreground --port 4577`
In another: `curl -s localhost:4577/api/v1/health` → JSON with `"api_version":1`; `curl -s localhost:4577/api/v1/ping` → `401`; with `-H "Authorization: Bearer $(cat /tmp/decant-test/daemon.token)"` → `{}`. Ctrl-C shuts it down cleanly.

- [ ] **Step 5: Run the full suite + lints.**

Run: `cargo test --workspace && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit.**

```bash
git add crates/decant-daemon crates/decant-cli
git commit -S -m "feat(daemon): run() orchestrator and decant daemon serve"
```

---

## Self-review

**Spec coverage (§3, §4, §8 foundation slice):** loopback bind ✓ (Task 5/7), Host allowlist ✓ (Task 6), bearer token 0600 ✓ (Tasks 3, 6), Origin-on-writes ✓ (Task 6), version header ✓ (Task 6), single-instance lock ✓ (Task 4), graceful shutdown ✓ (Task 5), config/port default 4577 ✓ (Task 2), `/health` ✓ (Task 5), `decant daemon serve` ✓ (Task 7). Watcher/ingest/SQLite/read-API/SSE/recommendations are **deliberately out of this plan** (Plans 2–6).

**Placeholder scan:** every code step has complete code; the only prose-only step is Task 7 Step 3 (CLI clap wiring) which names exact types/flags and the delegation call — acceptable as it depends on the existing clap layout in `decant-cli`.

**Type consistency:** `AppState { token }`, `Config::from_values/from_env`, `http::router`, `http::serve`, `auth::load_or_create`/`token_matches`, `lock::InstanceLock::acquire`, `API_VERSION` are used consistently across tasks and tests.

---

## The remaining plans (write each when we reach it, against the real code)

2. **Watcher + ingest in the daemon** — `notify` + debounce + sync task + write conn + r2d2 read pool + WAL/busy_timeout; parity with `decant sync`; `/api/v1/metadata/sync-status`.
3. **Read API** — sessions list/detail, `POST /search`, analytics (summary/by-dimension/activity/model-sparklines), tools + mcp usage, metadata; `{data, meta, errors}` envelope; cursor pagination; OpenAPI 3.1 + contract tests.
4. **SSE change-stream** — `GET /api/v1/events`, emitted on sync commit.
5. **Phoenix client swap** — `Decant.Daemon` (Req/Finch + token + version), `Decant.HealthCheck`, `Decant.DaemonEvents` (SSE→PubSub), migrate pages off `Decant.Archive` SQL, retire `AutoSync`.
6. **Recommendations** — core generation + `recommendation` table/migration + endpoints + `mark-implemented` + activity auto-resolve + Insights open/implemented UI + agent-handoff prompt/hook.
7. **Lifecycle + docs** — `decant daemon install/start/stop/status/logs`, macOS LaunchAgent, README architecture, `docs/api/openapi.yaml`.
