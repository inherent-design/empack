use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn e2e_version_output() {
    Command::cargo_bin("empack")
        .unwrap()
        .env("NO_COLOR", "1")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.2.0-alpha.2"));
}

#[test]
fn e2e_help_exits_zero() {
    Command::cargo_bin("empack")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
