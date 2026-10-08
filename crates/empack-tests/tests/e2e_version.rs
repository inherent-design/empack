use empack_tests::e2e::{TestProject, empack_assert_cmd, empack_cmd};
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
    cmd.env("EMPACK_PACKWIZ_BIN", project.dir().join("missing-backend"));
    cmd.env("EMPACK_PROFILE", profile)
        .env("OTEL_SDK_DISABLED", "false")
        .env("OTEL_TRACES_SAMPLER", "always_on")
        .args(["--yes", "sync"]);
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
        events
            .iter()
            .any(|event| event["name"] == "empack.command" && event["args"]["command"] == "sync"),
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
            .match_body(mockito::Matcher::Regex("empack.command".to_string()))
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

/// Runtime inspection describes native capabilities without installing tooling.
#[test]
fn e2e_requirements_describes_native_capabilities_without_bootstrap() {
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
        stdout.contains("native engine; no packwiz executable required"),
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

#[test]
fn e2e_telemetry_failed_command_omits_private_selectors() {
    let project = TestProject::workflow_fixture("failed-trace", "fabric", "1.21.1");
    let private = "PRIVATE_SELECTOR_MUST_NOT_ENTER_TRACE";
    let output = project
        .cmd()
        .env("EMPACK_PROFILE", "chrome")
        .args(["--yes", "add", private])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let trace = std::fs::read_dir(project.dir())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("trace-") && name.ends_with(".json"))
        });
    if !empack_tests::e2e::prerequisite_available(trace.is_some(), "binary lacks telemetry feature")
    {
        return;
    }
    let raw = std::fs::read_to_string(trace.unwrap().path()).unwrap();
    assert!(
        !raw.contains(private),
        "command tracing leaked the input selector"
    );
    let events: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event["name"] == "empack.command" && event["args"]["command"] == "add")
    );
    assert!(
        events
            .iter()
            .any(|event| event["args"]["outcome"] == "failure")
    );
}
