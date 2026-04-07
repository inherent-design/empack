use empack_tests::e2e::{empack_assert_cmd, empack_bin, TestProject};
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
    assert!(has_trace, "EMPACK_PROFILE=chrome should produce a trace-*.json file");
}
