//! Coverage-focused integration tests: table/plain-text output branches,
//! `--quiet`, `--no-color`, error paths, the `db` subcommands, and the
//! `recommendations mark` HTTP command (against an in-process mock server).
//!
//! These complement `cli.rs` (which mostly exercises `--json`).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

/// Write the small `sample.jsonl` fixture under a temp claude tree.
/// Returns `(db, claude_projects_dir, codex_dir)`.
fn write_sample_tree(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let sample = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/sample.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("sess.jsonl"), sample).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

/// Write the richer `enriched.jsonl` fixture (thinking/tool_use/tool_result,
/// errors, interruptions) under a temp claude tree.
fn write_enriched_tree(
    root: &Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let enriched = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/enriched.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("enr.jsonl"), enriched).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

/// Sync a fixture tree into `db`. Panics on failure.
fn sync(db: &Path, claude_dir: &Path, codex_dir: &Path) {
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", claude_dir)
        .env("DECANT_CODEX_DIR", codex_dir)
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// session ls / show — table, quiet, and subcommand dispatch
// ---------------------------------------------------------------------------

#[test]
fn session_ls_table_and_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // SUBCOMMAND dispatch (`session ls`) + table output.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["session", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TITLE"))
        .stdout(predicate::str::contains("Fix the failing auth test"));

    // --quiet prints bare ids only.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--quiet", "--db"])
        .arg(&db)
        .args(["session", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1"))
        .stdout(predicate::str::contains("TITLE").not());
}

#[test]
fn session_show_subcommand_and_missing_id() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // SUBCOMMAND dispatch (`session show <id>`) + table render of blocks.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["session", "show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"))
        .stdout(predicate::str::contains("_(thinking)_"));

    // Missing id → error line on stderr, exit 1 (non-json path).
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["show", "99999"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no session with id 99999"));
}

#[test]
fn session_show_json_path() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"summary\""));
}

/// Build a synthetic claude transcript whose active duration exceeds one hour
/// and which carries a thinking-block-with-no-text plus an unknown block type,
/// to exercise the `1h..m` duration branch and the `[other]` block render.
fn write_long_session_tree(
    root: &Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();

    let mut lines = String::new();
    // First user turn.
    lines.push_str(
        r#"{"type":"user","uuid":"u0","parentUuid":null,"sessionId":"sess-long","timestamp":"2026-05-04T10:00:00.000Z","cwd":"/Users/dev/proj","message":{"role":"user","content":"long task"}}
"#,
    );
    // 15 assistant messages, each 300s after the previous (gap capped at 300s),
    // so active_seconds ~= 15 * 300 = 4500s (>= 1h). First assistant carries a
    // thinking block with no `thinking` field and an unknown `image` block.
    let base = 0; // minutes offset
    for i in 0..15 {
        let mins = base + (i + 1) * 5; // 5 min == 300s gaps
        let hh = 10 + mins / 60;
        let mm = mins % 60;
        let ts = format!("2026-05-04T{hh:02}:{mm:02}:00.000Z");
        let content = if i == 0 {
            r#"[{"type":"thinking"},{"type":"image","source":"x"},{"type":"text","text":"working"}]"#
        } else {
            r#"[{"type":"text","text":"working"}]"#
        };
        lines.push_str(&format!(
            r#"{{"type":"assistant","uuid":"a{i}","parentUuid":"u0","sessionId":"sess-long","timestamp":"{ts}","cwd":"/Users/dev/proj","message":{{"role":"assistant","model":"claude-opus-4-7","stop_reason":"end_turn","usage":{{"input_tokens":10,"output_tokens":5}},"content":{content}}}}}
"#
        ));
    }
    fs::write(claude_dir.join("long.jsonl"), lines).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

#[test]
fn session_show_hour_duration_and_other_block() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_long_session_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["show", "1"])
        .assert()
        .success()
        // hour branch of fmt_duration (e.g. "active 1h15m").
        .stdout(predicate::str::contains("active 1h"))
        // unknown block type renders as `[other]`.
        .stdout(predicate::str::contains("[other]"));
}

// ---------------------------------------------------------------------------
// stats — table mode (overall + by dimension)
// ---------------------------------------------------------------------------

#[test]
fn stats_table_overall_and_by_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("sessions:"))
        .stdout(predicate::str::contains("est. cost:"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["stats", "--by", "tool"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TOOL"))
        .stdout(predicate::str::contains("claude_code"));
}

// ---------------------------------------------------------------------------
// tool ls / stats — table mode
// ---------------------------------------------------------------------------

#[test]
fn tool_table_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["tool", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TOOL"))
        .stdout(predicate::str::contains("Read"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["tool", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CALLS"));
}

// ---------------------------------------------------------------------------
// mcp ls / stats — table mode + empty branch
// ---------------------------------------------------------------------------

#[test]
fn mcp_table_empty_branch() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // Fixture has no MCP calls -> empty branch prints to stderr, still exit 0.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["mcp", "stats"])
        .assert()
        .success()
        .stderr(predicate::str::contains("no MCP tool calls recorded"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["mcp", "ls"])
        .assert()
        .success()
        .stderr(predicate::str::contains("no MCP tool calls recorded"));
}

/// Write the `mcp.jsonl` fixture (a session with an `mcp__github__create_issue`
/// tool call) so `mcp ls`/`mcp stats` render TABLE rows.
fn write_mcp_tree(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let mcp = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/mcp.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("mcp.jsonl"), mcp).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

#[test]
fn mcp_table_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_mcp_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // `mcp ls` renders the per-server table with the github row.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["mcp", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SERVER"))
        .stdout(predicate::str::contains("github"));

    // `mcp stats` renders the same table branch.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["mcp", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CALLS"))
        .stdout(predicate::str::contains("github"));
}

// ---------------------------------------------------------------------------
// project ls — table mode
// ---------------------------------------------------------------------------

#[test]
fn project_ls_table_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["project", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PROJECT"))
        .stdout(predicate::str::contains("/Users/dev/proj"));
}

// ---------------------------------------------------------------------------
// search — text mode (match + no-match)
// ---------------------------------------------------------------------------

#[test]
fn search_text_mode_match_and_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["search", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[1]"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["search", "zzzznevermatcheszzzz"])
        .assert()
        .success()
        .stderr(predicate::str::contains("no matches for"));
}

// ---------------------------------------------------------------------------
// export — json to stdout, single to dir, --all json to dir
// ---------------------------------------------------------------------------

#[test]
fn export_json_stdout_and_to_dir() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // Single session as JSON to stdout (render json branch).
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["export", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"summary\""));

    // Single session to a directory (write-to-dir branch).
    let outdir = dir.path().join("single");
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "1", "--out"])
        .arg(&outdir)
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote"));
    assert!(outdir.join("1.md").exists());

    // --all as JSON to a directory.
    let alldir = dir.path().join("alljson");
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["export", "--all", "--out"])
        .arg(&alldir)
        .assert()
        .success();
    assert!(alldir.join("1.json").exists());

    // --all as markdown to a directory (non-json render branch + `n += 1`).
    let allmd = dir.path().join("allmd");
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "--all", "--out"])
        .arg(&allmd)
        .assert()
        .success()
        .stderr(predicate::str::contains("exported 1 sessions"));
    assert!(allmd.join("1.md").exists());

    // Single session as markdown to stdout (the `None => println!` branch).
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# "));
}

// ---------------------------------------------------------------------------
// db — migrate / vacuum / info (size branch)
// ---------------------------------------------------------------------------

#[test]
fn db_migrate_vacuum_info() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["db", "migrate"])
        .assert()
        .success()
        .stderr(predicate::str::contains("schema up to date"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["db", "vacuum"])
        .assert()
        .success()
        .stderr(predicate::str::contains("vacuumed"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["db", "info"])
        .assert()
        .success()
        .stdout(predicate::str::contains("size_bytes:"))
        .stdout(predicate::str::contains("schema:"));
}

// ---------------------------------------------------------------------------
// files — bad --op, valid --op branch
// ---------------------------------------------------------------------------

#[test]
fn files_bad_op_and_valid_op() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_enriched_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // Bad --op value -> exit 2.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["files", "--op", "bogus"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown --op value"));

    // Valid --op (Some branch) in table mode.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["files", "--op", "edit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("FILE"));

    // --group ext renders the EXT header (the FileGroup::Ext header branch).
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["files", "--group", "ext"])
        .assert()
        .success()
        .stdout(predicate::str::contains("EXT"));
}

// ---------------------------------------------------------------------------
// main.rs / output.rs — Err arm, Ls alias, --no-color
// ---------------------------------------------------------------------------

#[test]
fn run_err_arm_unopenable_db() {
    // Point --db at a directory so db::open() fails -> main prints `error:` & exits 1.
    let dir = tempfile::tempdir().unwrap();
    let badpath = dir.path(); // a directory, not a file
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(badpath)
        .args(["session", "ls"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("error:"));
}

#[test]
fn top_level_ls_alias_and_no_color() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_sample_tree(dir.path());
    sync(&db, &claude_dir, &codex_dir);

    // Top-level `ls` alias + --no-color (output::should_color early return).
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--no-color", "--db"])
        .arg(&db)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"));
}

// ---------------------------------------------------------------------------
// recommendations mark — daemon-unreachable, success, HTTP-error, flags, token
// ---------------------------------------------------------------------------

/// Bind a one-shot mock HTTP server on a free loopback port. The handler thread
/// reads the request line (so the client's write completes) and replies with the
/// supplied raw HTTP response. Returns the bound base URL.
fn spawn_mock_server(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read whatever the client sends until we have at least the headers;
            // a single read of a chunk is enough to let the client finish writing.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[test]
fn recommendations_mark_unreachable_text_and_json() {
    // Bind+drop a listener to obtain a port nothing is listening on.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");

    // Text mode.
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .args(["recommendations", "mark", "catalog:agents-md"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "could not reach the decant daemon",
        ));

    // JSON mode.
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .args(["--json", "recommendations", "mark", "catalog:agents-md"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"ok\": false"))
        .stdout(predicate::str::contains("could not reach the daemon"));
}

#[test]
fn recommendations_mark_success_text_json_and_flags() {
    // Text mode success.
    let url = spawn_mock_server("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .env("DECANT_DAEMON_TOKEN", "test-token")
        .args(["recommendations", "mark", "some:key"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Marked some:key as implemented"));

    // JSON mode success + --note/--source flags + token from env.
    let url2 = spawn_mock_server("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url2)
        .env("DECANT_DAEMON_TOKEN", "test-token")
        .args([
            "--json",
            "recommendations",
            "mark",
            "some:key",
            "--note",
            "did it",
            "--source",
            "human",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"))
        .stdout(predicate::str::contains("\"status\": \"implemented\""));
}

#[test]
fn recommendations_mark_uses_default_base_url_when_env_empty() {
    // An empty DECANT_DAEMON_URL falls through to the loopback default
    // (`base_url`'s `_ =>` arm). Nothing listens on 4577 in CI, so the request
    // fails to connect and we exit 1 — but the default-URL branch is exercised.
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", "")
        .args(["recommendations", "mark", "some:key"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("127.0.0.1:4577"));
}

#[test]
fn recommendations_mark_blank_token_env_falls_through() {
    // A whitespace-only DECANT_DAEMON_TOKEN takes the `!v.trim().is_empty()`
    // false arm in `token()` (so the env token is ignored). The mock server
    // accepts the unauthenticated request and the command succeeds.
    let url = spawn_mock_server("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .env("DECANT_DAEMON_TOKEN", "   ")
        .args(["recommendations", "mark", "some:key"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Marked some:key as implemented"));
}

#[test]
fn recommendations_mark_http_error_non_json_body() {
    // A non-JSON error body -> `error_detail`'s `Err(_) => String::new()` arm:
    // the failure line carries the status but no `: <message>` suffix.
    let resp = "HTTP/1.1 500 Internal Server Error\r\ncontent-type: text/plain\r\ncontent-length: 5\r\n\r\nboom!";
    let url = spawn_mock_server(resp);
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .env("DECANT_DAEMON_TOKEN", "  test-token  ")
        .args(["recommendations", "mark", "some:key"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("failed to mark some:key"));
}

#[test]
fn recommendations_mark_http_error_text_and_json() {
    // 409 with an error envelope -> error_detail picks up the message.
    let resp = "HTTP/1.1 409 Conflict\r\ncontent-type: application/json\r\ncontent-length: 39\r\n\r\n{\"errors\":[{\"message\":\"already done\"}]}";

    let url = spawn_mock_server(resp);
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url)
        .args(["recommendations", "mark", "some:key"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("failed to mark some:key"))
        .stderr(predicate::str::contains("already done"));

    let url2 = spawn_mock_server(resp);
    Command::cargo_bin("decant")
        .unwrap()
        .env("DECANT_DAEMON_URL", &url2)
        .args(["--json", "recommendations", "mark", "some:key"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"ok\": false"))
        .stdout(predicate::str::contains("\"http_status\": 409"))
        .stdout(predicate::str::contains("already done"));
}
