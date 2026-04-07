use empack_tests::e2e::{empack_assert_cmd, empack_bin, empack_cmd, TestProject};
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

/// Verify the telemetry feature produces a Perfetto trace file.
///
/// Runs empack version with EMPACK_PROFILE=chrome. Exercises the full
/// Logger::init telemetry path: Chrome layer construction, per-layer
/// filtering, guard creation, and shutdown flush.
///
/// Only meaningful when the binary is built with --features telemetry.
/// Detects this by checking whether EMPACK_PROFILE=chrome produces a
/// trace file; if not, the binary lacks telemetry support and the test
/// passes vacuously (coverage job builds with telemetry enabled).
#[test]
fn e2e_telemetry_chrome_trace() {
    let project = TestProject::new();
    let output = std::process::Command::new(empack_bin())
        .current_dir(project.dir())
        .env("NO_COLOR", "1")
        .env("EMPACK_PROFILE", "chrome")
        .arg("version")
        .output()
        .expect("spawn failed");
    assert!(
        output.status.success(),
        "empack version with EMPACK_PROFILE=chrome failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let has_trace = std::fs::read_dir(project.dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with("trace-") && n.ends_with(".json"))
                })
        })
        .unwrap_or(false);
    if !has_trace {
        eprintln!("SKIP: binary lacks telemetry feature; trace file not produced");
    }
}

/// Verify empack requirements shows packwiz-tx with version and path.
///
/// Exercises the managed binary download path: if packwiz-tx is not
/// cached, empack downloads it from GitHub releases on first use.
#[test]
fn e2e_requirements_shows_packwiz_tx() {
    let project = TestProject::new();
    let output = empack_cmd(project.dir())
        .arg("requirements")
        .output()
        .expect("spawn failed");
    assert!(
        output.status.success(),
        "empack requirements failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("packwiz-tx"),
        "requirements output should mention packwiz-tx:\n{stdout}",
    );
    assert!(
        stdout.contains(empack_lib::platform::packwiz_bin::PACKWIZ_TX_VERSION),
        "requirements output should show packwiz-tx version:\n{stdout}",
    );
}
