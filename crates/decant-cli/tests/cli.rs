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
        .stdout(predicate::str::contains("schema:     v6"))
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

#[test]
fn files_lists_hotspots_from_enriched_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude/projects/proj");
    let codex_dir = dir.path().join("codex");
    let db = dir.path().join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let enriched = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/enriched.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("enr.jsonl"), enriched).unwrap();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", dir.path().join("claude/projects"))
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("README.md"));

    // Language lens + op filter + JSON output.
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["files", "--group", "ext", "--op", "edit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"rs\""));

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["files", "--group", "bogus"])
        .assert()
        .code(2);
}

#[test]
fn show_prints_facets_line() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude/projects/proj");
    let codex_dir = dir.path().join("codex");
    let db = dir.path().join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let enriched = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/enriched.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("enr.jsonl"), enriched).unwrap();

    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", dir.path().join("claude/projects"))
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();

    // enriched fixture: 1 turn, 1 error, active 8m10s (490s), 1 interruption,
    // refactor work type ("Refactor the auth module").
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1 turns · 1 errors · active 8m10s",
        ))
        .stdout(predicate::str::contains("1 interruptions"))
        .stdout(predicate::str::contains("refactor"));
}

/// Build a temp archive whose one Claude session has Bash commands + a Write +
/// an Edit + a secret + a destructive command (the distill fixture).
fn write_distill_tree(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let sample = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/distill.jsonl"
    ))
    .unwrap();
    fs::write(claude_dir.join("sess.jsonl"), sample).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

fn synced_distill_db(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let (db, claude_dir, codex_dir) = write_distill_tree(dir.path());
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success();
    db
}

#[test]
fn distill_script_emits_safe_bash() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "script"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#!/usr/bin/env bash"))
        .stdout(predicate::str::contains("cargo build --workspace"))
        .stdout(predicate::str::contains("# REVIEW: destructive"))
        .stdout(predicate::str::contains("<REDACTED>"))
        .stdout(predicate::str::contains("ghp_").not());
}

#[test]
fn distill_script_json_has_ops_and_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--json", "--db"])
        .arg(&db)
        .args(["distill", "script"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\""))
        .stdout(predicate::str::contains("\"artifact\""));
}

#[test]
fn distill_replay_reproduces_commands_and_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "replay", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cat > 'NOTES.md'"))
        .stdout(predicate::str::contains("# EDIT src/main.rs"));
}

#[test]
fn distill_skill_agents_section() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "skill", "--kind", "agents", "--project", "proj"])
        .assert()
        .success()
        .stdout(predicate::str::contains("### Commands"))
        .stdout(predicate::str::contains("cargo build --workspace"));
}

#[test]
fn distill_bad_format_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "script", "--as", "bogus"])
        .assert()
        .code(2);
}

#[test]
fn distill_replay_unknown_session_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "replay", "999"])
        .assert()
        .code(1);
}

#[test]
fn distill_script_rejects_bad_min_frequency() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "script", "--min-frequency", "nan"])
        .assert()
        .code(2);
}

#[test]
fn distill_skill_empty_scope_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = synced_distill_db(&dir);
    Command::cargo_bin("decant")
        .unwrap()
        .args(["--db"])
        .arg(&db)
        .args(["distill", "skill", "--project", "no-such-project-xyz"])
        .assert()
        .code(1);
}
