use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

#[test]
fn version_flag_works() {
    Command::cargo_bin("decant")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("decant"));
}

#[test]
fn help_lists_core_commands() {
    Command::cargo_bin("decant")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("session"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn unknown_command_exits_two() {
    Command::cargo_bin("decant")
        .unwrap()
        .arg("frobnicate")
        .assert()
        .code(2);
}

fn write_fixture_tree(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
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

#[test]
fn sync_then_reports_json() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ingested\""));
}

#[test]
fn ls_json_lists_synced_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["session", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"));
}

#[test]
fn show_renders_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fix the failing auth test"))
        .stdout(predicate::str::contains("Read"));
}

#[test]
fn search_finds_text() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["search", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"session_id\""));
}

#[test]
fn stats_overall_and_by_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"sessions\""));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["stats", "--by", "tool"])
        .assert()
        .success()
        .stdout(predicate::str::contains("claude_code"));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["stats", "--by", "bogus"])
        .assert()
        .code(2);
}

#[test]
fn tool_stats_lists_read() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["tool", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Read"))
        .stdout(predicate::str::contains("\"calls\""));
}

#[test]
fn mcp_stats_runs_and_is_json_array() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    // Fixture has no MCP calls -> empty JSON array, still exit 0.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["mcp", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[test]
fn export_session_to_markdown_stdout_and_all_to_dir() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Fix the failing auth test"));

    let outdir = dir.path().join("export");
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "--all", "--out"])
        .arg(&outdir)
        .assert()
        .success();
    assert!(outdir.join("1.md").exists());
}

#[test]
fn export_error_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "--all"])
        .assert()
        .code(2);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("export")
        .assert()
        .code(2);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["export", "99999"])
        .assert()
        .code(1);
}

#[test]
fn invalid_format_is_rejected() {
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--format", "bogus", "session", "ls"])
        .assert()
        .code(2);
}

#[test]
fn completion_bash_generates_script() {
    Command::cargo_bin("decant")
        .unwrap()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("decant"));
}

#[test]
fn db_info_reports_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["db", "info"])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema:     v2"))
        .stdout(predicate::str::contains("sessions:   1"));
}

#[test]
fn project_ls_shows_project() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["project", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("/Users/dev/proj"));
}
