use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

#[test]
fn version_flag_works() {
    Command::cargo_bin("decant").unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("decant"));
}

#[test]
fn help_lists_core_commands() {
    Command::cargo_bin("decant").unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("session"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn unknown_command_exits_two() {
    Command::cargo_bin("decant").unwrap()
        .arg("frobnicate")
        .assert()
        .code(2);
}

fn write_fixture_tree(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let claude_dir = root.join("claude/projects/proj");
    let codex_dir = root.join("codex");
    let db = root.join("d.db");
    fs::create_dir_all(&claude_dir).unwrap();
    let sample = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
    fs::write(claude_dir.join("sess.jsonl"), sample).unwrap();
    (db, root.join("claude/projects"), codex_dir)
}

#[test]
fn sync_then_reports_json() {
    let dir = tempfile::tempdir().unwrap();
    let (db, claude_dir, codex_dir) = write_fixture_tree(dir.path());

    Command::cargo_bin("decant").unwrap()
        .args(["--json", "--db"]).arg(&db)
        .arg("sync")
        .env("DECANT_CLAUDE_DIR", &claude_dir)
        .env("DECANT_CODEX_DIR", &codex_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ingested\""));
}
