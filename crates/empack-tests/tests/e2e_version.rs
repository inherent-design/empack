use empack_tests::e2e::{empack_assert_cmd, TestProject};
use predicates::prelude::*;

#[test]
fn e2e_version_output() {
    empack_assert_cmd()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn e2e_help_exits_zero() {
    empack_assert_cmd()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn e2e_test_project_creates_tempdir() {
    let project = TestProject::new();
    assert!(project.dir().exists());
}
