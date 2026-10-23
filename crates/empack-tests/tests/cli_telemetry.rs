use empack_tests::harness::process::TestProject;

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

fn verify_chrome_trace(project: &TestProject) {
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
    assert!(
        trace.is_some(),
        "selected binary lacks telemetry: no trace produced"
    );
    let events: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(trace.unwrap().path()).unwrap())
            .expect("Chrome trace must be valid JSON after shutdown");
    assert!(
        events
            .iter()
            .any(|event| event["name"] == "empack.command" && event["args"]["command"] == "sync"),
        "trace must contain the executed command span"
    );
}

#[test]
#[ignore = "requires a product executable built with telemetry"]
fn cli_telemetry_chrome_trace() {
    let project = TestProject::workflow_fixture("chrome-trace", "fabric", "1.21.1");
    verify_chrome_trace(&project);
}

#[test]
#[ignore = "requires a product executable built with telemetry"]
fn cli_telemetry_otlp_exports_and_exits_when_collector_fails() {
    let project = TestProject::workflow_fixture("otlp-trace", "fabric", "1.21.1");
    verify_chrome_trace(&project);
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

#[test]
#[ignore = "requires a product executable built with telemetry"]
fn cli_telemetry_failed_command_omits_private_selectors() {
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
    assert!(
        trace.is_some(),
        "selected binary lacks telemetry: no trace produced"
    );
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
