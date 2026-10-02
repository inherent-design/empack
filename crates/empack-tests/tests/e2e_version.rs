use empack_tests::e2e::{TestProject, configure_fake_packwiz, empack_assert_cmd, empack_cmd};
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
    empack_assert_cmd().arg("--help").assert().success();
}

#[test]
fn e2e_test_project_creates_tempdir() {
    let project = TestProject::new();
    assert!(project.dir().exists());
}

fn telemetry_command(project: &TestProject, profile: &str) -> assert_cmd::Command {
    let mut cmd = project.cmd();
    configure_fake_packwiz(&mut cmd, project.dir());
    cmd.env("EMPACK_PROFILE", profile)
        .env("OTEL_SDK_DISABLED", "false")
        .env("OTEL_TRACES_SAMPLER", "always_on")
        .arg("sync");
    let mut cmd = assert_cmd::Command::from_std(cmd);
    cmd.timeout(std::time::Duration::from_secs(15));
    cmd
}

fn verify_chrome_trace(project: &TestProject) -> bool {
    telemetry_command(project, "chrome").assert().success();
    let trace = std::fs::read_dir(project.dir())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("trace-") && name.ends_with(".json"))
        });
    if !empack_tests::e2e::prerequisite_available(
        trace.is_some(),
        "binary lacks telemetry feature; trace file not produced",
    ) {
        return false;
    }
    let events: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(trace.unwrap().path()).unwrap())
            .expect("Chrome trace must be valid JSON after shutdown");
    assert!(
        events.iter().any(|event| event["name"] == "handle_sync"),
        "trace must contain the executed command span"
    );
    true
}

#[test]
fn e2e_telemetry_chrome_trace() {
    let project = TestProject::workflow_fixture("chrome-trace", "fabric", "1.21.1");
    verify_chrome_trace(&project);
}

#[test]
fn e2e_telemetry_otlp_exports_and_exits_when_collector_fails() {
    let project = TestProject::workflow_fixture("otlp-trace", "fabric", "1.21.1");
    if !verify_chrome_trace(&project) {
        return;
    }
    for status in [200, 503] {
        let mut server = mockito::Server::new();
        let request = server
            .mock("POST", "/v1/traces")
            .match_header("content-type", "application/x-protobuf")
            .match_body(mockito::Matcher::Regex("handle_sync".to_string()))
            .with_status(status)
            .expect_at_least(1)
            .create();
        telemetry_command(&project, "otlp")
            .env(
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
                format!("{}/v1/traces", server.url()),
            )
            .env("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL", "http/protobuf")
            .env("OTEL_EXPORTER_OTLP_TRACES_COMPRESSION", "none")
            .env_remove("OTEL_EXPORTER_OTLP_HEADERS")
            .env_remove("OTEL_EXPORTER_OTLP_TRACES_HEADERS")
            .assert()
            .success();
        request.assert();
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
