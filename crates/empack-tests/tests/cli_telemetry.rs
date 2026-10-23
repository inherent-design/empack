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

fn chrome_trace(project: &TestProject) -> String {
    let traces: Vec<_> = std::fs::read_dir(project.dir())
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("trace-") && name.ends_with(".json"))
        })
        .collect();
    assert_eq!(traces.len(), 1, "telemetry must flush one trace");
    std::fs::read_to_string(traces[0].path()).unwrap()
}

fn verify_chrome_trace(project: &TestProject) {
    telemetry_command(project, "chrome").assert().success();
    let events: Vec<serde_json::Value> = serde_json::from_str(&chrome_trace(project))
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
    let raw = chrome_trace(&project);
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

#[test]
#[ignore = "requires a product executable built with telemetry"]
fn cli_telemetry_json_report_keeps_logs_and_private_configuration_out_of_payload() {
    let project = TestProject::workflow_fixture("json-trace", "fabric", "1.21.1");
    let secret = "PRIVATE_API_KEY_MUST_NOT_ENTER_OUTPUT";
    let output = project
        .cmd()
        .env("EMPACK_PROFILE", "chrome")
        .env("EMPACK_KEY_MODRINTH", secret)
        .env("EMPACK_KEY_CURSEFORGE", secret)
        .args([
            "--color",
            "always",
            "--log-level",
            "4",
            "--log-format",
            "json",
            "--log-output",
            "stdout",
            "status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["selected"], "author");
    assert_eq!(report["content_verified"], false);
    assert!(!output.stdout.contains(&0x1b));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Logger initialized successfully"));
    for line in stderr.lines() {
        serde_json::from_str::<serde_json::Value>(line).expect("diagnostic log must be JSON");
    }
    assert!(!String::from_utf8(output.stdout).unwrap().contains(secret));
    assert!(!stderr.contains(secret));
    let raw = chrome_trace(&project);
    assert!(!raw.contains(secret));
    let events: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event["name"] == "empack.command" && event["args"]["command"] == "status")
    );
    assert!(
        events
            .iter()
            .any(|event| event["args"]["outcome"] == "success")
    );
}
