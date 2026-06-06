use assert_cmd::Command;
use predicates::prelude::*;

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
