use empack_tests::harness::process::{TestProject, empack_assert_cmd, empack_cmd};
use predicates::prelude::*;

#[test]
fn cli_version_output() {
    let project = TestProject::new();
    empack_assert_cmd(project.dir())
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            empack_tests::harness::process::expected_cli_version(),
        ));
}

#[test]
fn cli_help_exits_zero() {
    let project = TestProject::new();
    empack_assert_cmd(project.dir())
        .arg("--help")
        .assert()
        .success();
}

/// Runtime inspection describes native capabilities without installing tooling.
#[test]
fn cli_requirements_describes_native_capabilities_without_bootstrap() {
    let project = TestProject::new();
    let output = empack_cmd(project.dir())
        .env("EMPACK_PACKWIZ_BIN", project.dir().join("missing-backend"))
        .arg("requirements")
        .output()
        .expect("spawn failed");
    assert!(
        output.status.success(),
        "requirements failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("native engine; no external dependency manager required"),
        "{stdout}"
    );
    assert!(stdout.contains("ZIP, TAR.GZ and 7z"), "{stdout}");
    assert!(
        stdout.contains("CurseForge also requires an API key"),
        "{stdout}"
    );
    assert!(!project.dir().join("pack").exists());
    assert!(!project.dir().join("empack.yml").exists());
}
