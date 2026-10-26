//! Executable exit contracts exercise native documents, publication and cancellation.
use empack_lib::EmpackExitCode;
use empack_tests::harness::process::TestProject;
use std::{
    fs,
    io::Write,
    process::{Command, Output},
};

#[cfg(unix)]
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

fn command(project: &TestProject) -> Command {
    let mut command = project.cmd();
    command
        .env_remove("EMPACK_DRY_RUN")
        .env_remove("EMPACK_YES")
        .env("EMPACK_STATE_DIR", project.dir().join(".host-state"))
        .env("EMPACK_PACKWIZ_BIN", project.dir().join("must-not-run"));
    command
}
fn output(project: &TestProject, arguments: &[&str]) -> Output {
    command(project).args(arguments).output().unwrap()
}
fn check(output: &Output, code: EmpackExitCode, message: &str) {
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(code.as_i32()), "{diagnostic}");
    assert!(diagnostic.contains(message), "{diagnostic}");
}
fn fixture() -> TestProject {
    TestProject::workflow_fixture("exit-contract", "fabric", "1.21.1")
}
fn add_local(project: &TestProject) {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    fs::write(
        project.dir().join("fixture.jar"),
        zip.finish().unwrap().into_inner(),
    )
    .unwrap();
    let result = output(project, &["add", "--yes", "fixture.jar"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
/// Report stdout remains a single JSON value even when diagnostic logging is enabled.
#[test]
fn cli_json_reports_separate_logging_and_preserve_author_inputs() {
    let project = TestProject::new();
    fs::write(
        project.dir().join("empack.yml"),
        include_str!("../../../tests/fixtures/authoring/empack.yml"),
    )
    .unwrap();
    assert!(output(&project, &["--yes", "sync"]).status.success());
    let author = fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = fs::read(project.dir().join("empack.lock")).unwrap();
    let routes: &[&[&str]] = &[
        &["status", "--json"],
        &["manifest", "check", "--json"],
        &["manifest", "diff", "--json"],
        &["pending", "list", "--json"],
        &["outdated", "--json"],
        &["build", "--explain", "--json"],
        &[
            "migrate",
            "--plan",
            "--json",
            "--minecraft",
            "1.21.1",
            "--loader",
            "vanilla",
        ],
    ];
    for color in ["never", "always", "auto"] {
        for format in ["text", "json", "yaml"] {
            for stream in ["stderr", "stdout"] {
                for route in routes {
                    let result = command(&project)
                        .args([
                            "--color",
                            color,
                            "--log-level",
                            "2",
                            "--log-format",
                            format,
                            "--log-output",
                            stream,
                        ])
                        .args(*route)
                        .output()
                        .unwrap();
                    let context = format!(
                        "{route:?} color={color} format={format} stream={stream}: {result:?}"
                    );
                    assert!(result.status.success(), "{context}");
                    let value: serde_json::Value = serde_json::from_slice(&result.stdout)
                        .unwrap_or_else(|error| panic!("{error}: {context}"));
                    assert!(value.is_object(), "{context}");
                    assert!(!result.stdout.contains(&0x1b), "{context}");
                    assert!(
                        String::from_utf8_lossy(&result.stderr)
                            .contains("Logger initialized successfully"),
                        "logging must remain observable: {context}"
                    );
                    assert_eq!(fs::read(project.dir().join("empack.yml")).unwrap(), author);
                    assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
                    assert!(!project.dir().join("dist").exists());
                }
            }
        }
    }
    // Human output still honors an explicit log destination.
    for stream in ["stderr", "stdout"] {
        let result = command(&project)
            .args([
                "--color",
                "never",
                "--log-level",
                "2",
                "--log-output",
                stream,
                "status",
            ])
            .output()
            .unwrap();
        assert!(result.status.success());
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert_eq!(
            stdout.contains("Logger initialized successfully"),
            stream == "stdout"
        );
        assert_eq!(
            stderr.contains("Logger initialized successfully"),
            stream == "stderr"
        );
        assert!(stdout.contains("Metadata inspection only"));
    }
    // A refused JSON request has diagnostics, not a spurious successful payload.
    for route in [
        vec!["manifest", "explain", "unknown-key", "--json"],
        vec!["pending", "show", "unknown-key", "--json"],
    ] {
        let result = command(&project)
            .args([
                "--color",
                "never",
                "--log-level",
                "2",
                "--log-output",
                "stdout",
            ])
            .args(&route)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty(), "{result:?}");
        let diagnostic = String::from_utf8(result.stderr).unwrap();
        assert!(diagnostic.contains("Error:"), "{diagnostic}");
        assert!(
            diagnostic.contains("Logger initialized successfully"),
            "{diagnostic}"
        );
        assert_eq!(fs::read(project.dir().join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
    }
}

#[test]
fn cli_parse_error_exits_two() {
    check(
        &output(&TestProject::new(), &["--definitely-invalid-flag"]),
        EmpackExitCode::Usage,
        "--definitely-invalid-flag",
    );
}
#[test]
fn cli_uninitialized_build_exits_two() {
    check(
        &output(&TestProject::new(), &["build", "modrinth"]),
        EmpackExitCode::Usage,
        "empack.yml",
    );
}
#[test]
fn cli_direct_zip_without_type_exits_two() {
    check(
        &output(&fixture(), &["add", "https://example.invalid/pack.zip"]),
        EmpackExitCode::Usage,
        "--type",
    );
}
#[test]
fn cli_unattended_build_requires_explicit_approval() {
    let project = fixture();
    let before = fs::read(project.dir().join("empack.lock")).unwrap();
    check(
        &output(&project, &["build", "modrinth"]),
        EmpackExitCode::Usage,
        "--yes",
    );
    assert!(!project.dir().join("dist").exists());
    assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), before);
}
#[test]
fn cli_malformed_intent_and_lock_exit_two() {
    for name in ["empack.yml", "empack.lock"] {
        let project = fixture();
        let path = project.dir().join(name);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"unexpected-field: true\n")
            .unwrap();
        let before = fs::read(&path).unwrap();
        check(
            &output(&project, &["build", "--yes", "modrinth"]),
            EmpackExitCode::Usage,
            "Invalid",
        );
        assert_eq!(fs::read(path).unwrap(), before);
        assert!(!project.dir().join("dist").exists());
    }
}
#[test]
fn cli_missing_authored_source_cannot_build_from_an_installed_copy() {
    let project = fixture();
    add_local(&project);
    assert!(project.dir().join("pack/mods/fixture.jar").is_file());
    fs::remove_file(project.dir().join("fixture.jar")).unwrap();
    let result = output(&project, &["build", "--yes", "modrinth"]);
    assert!(
        !result.status.success(),
        "Missing authored content must not build from its installed copy"
    );
    assert!(
        !project
            .dir()
            .join("dist/exit-contract-1.0.0.mrpack")
            .exists()
    );
}
#[test]
fn cli_tracked_local_parent_dir_validation_exits_two() {
    let project = fixture();
    add_local(&project);
    let sentinel = tempfile::NamedTempFile::new_in(project.dir().parent().unwrap()).unwrap();
    fs::write(sentinel.path(), b"outside bytes").unwrap();
    let path = project.dir().join("empack.yml");
    let mut intent: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    intent["dependencies"]["fixture"]["path"] = serde_json::json!(format!(
        "../{}",
        sentinel.path().file_name().unwrap().to_str().unwrap()
    ));
    fs::write(&path, serde_saphyr::to_string(&intent).unwrap()).unwrap();
    check(
        &output(&project, &["build", "--yes", "modrinth"]),
        EmpackExitCode::Usage,
        "component",
    );
    assert_eq!(fs::read(sentinel.path()).unwrap(), b"outside bytes");
}
#[test]
fn cli_occupied_artifact_directory_fails_without_deletion() {
    let project = fixture();
    let artifact = project.dir().join("dist/exit-contract-1.0.0.mrpack");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("sentinel"), b"retain").unwrap();
    let result = output(&project, &["build", "--yes", "modrinth"]);
    assert!(!result.status.success());
    assert_eq!(fs::read(artifact.join("sentinel")).unwrap(), b"retain");
}
fn proxy(command: &mut Command, address: &str) {
    for name in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        command.env(name, address);
    }
    command.env("NO_PROXY", "").env("no_proxy", "");
}
#[test]
fn cli_network_failure_exits_three() {
    let project = fixture();
    let mut cmd = command(&project);
    proxy(&mut cmd, "http://127.0.0.1:9");
    let result = cmd
        .env("EMPACK_NET_TIMEOUT", "1")
        .args(["add", "--yes", "--platform", "modrinth", "sodium"])
        .output()
        .unwrap();
    check(&result, EmpackExitCode::Network, "Error:");
}
#[cfg(unix)]
#[test]
fn cli_interrupt_exits_130() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let project = fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut cmd = command(&project);
    proxy(
        &mut cmd,
        &format!("http://{}", listener.local_addr().unwrap()),
    );
    let stderr = tempfile::NamedTempFile::new().unwrap();
    let mut child = Child(
        cmd.args(["add", "--yes", "--platform", "modrinth", "sodium"])
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr.reopen().unwrap()))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let connection = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "Native HTTP request did not start"
                );
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "CLI exited before acquisition"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
    };
    assert!(
        Command::new("kill")
            .args(["-INT", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Cancellation did not retire the native request"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let diagnostic = fs::read_to_string(stderr.path()).unwrap();
    assert_eq!(
        status.code(),
        Some(EmpackExitCode::Interrupted.as_i32()),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("Error: Operation interrupted"),
        "Cancellation must reach executable error rendering: {diagnostic}"
    );
    drop(connection);
    assert!(!project.dir().join("pack/mods").exists());
}

#[cfg(unix)]
#[test]
fn cli_runtime_cancellation_retains_cleanup_diagnostic_and_explicit_recovery() {
    use empack_tests::harness::process::{
        assert_dist_artifact_suffix, empack_cmd, offline_command,
    };
    use predicates::str::contains;
    use sha2::{Digest, Sha256};

    struct Runtime {
        host: std::process::Child,
        pid: Option<String>,
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            if let Some(pid) = &self.pid {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", pid])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = self.host.kill();
            let _ = self.host.wait();
        }
    }

    let root = tempfile::tempdir().unwrap();
    let author = root.path().join("author");
    fs::create_dir_all(author.join("pack")).unwrap();
    fs::write(author.join("pack/managed.txt"), b"publisher content").unwrap();
    fs::write(
        author.join("empack.yml"),
        r#"schema: 5
pack:
  id: verification.runtime
  name: Runtime Fixture
  version: '1.0.0'
runtime:
  minecraft: '1.21.1'
  loader: vanilla
  java: 21
dependencies: {}
build:
  default: [native]
  profiles:
    native:
      target: empack
      delivery: bundled
      environment: client
      updates: snapshot
      format: zip
"#,
    )
    .unwrap();
    offline_command(&author)
        .args(["--yes", "sync"])
        .assert()
        .success();
    offline_command(&author)
        .args(["--yes", "build"])
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(&author, "-empack-bundled-client.empack");
    let export = root.path().join("release");
    fs::create_dir(&export).unwrap();
    zip::ZipArchive::new(fs::File::open(archive).unwrap())
        .unwrap()
        .extract(&export)
        .unwrap();
    let payload = fs::read(export.join("release.json")).unwrap();
    let id: String = Sha256::digest(&payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    for interfere_with_cleanup in [false, true] {
        let instance = root
            .path()
            .join(format!("instance-{interfere_with_cleanup}"));
        fs::create_dir(&instance).unwrap();
        offline_command(&instance)
            .args(["--yes", "instance", "install"])
            .arg(export.join("release.json"))
            .args(["--sha256", &id])
            .assert()
            .success();
        let stderr = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        let mut runtime = Runtime {
            host: empack_cmd(&instance)
                .args([
                    "--yes",
                    "instance",
                    "launch",
                    "--",
                    "/bin/sh",
                    "-c",
                    // One child, blocked on inherited input; no Java/network/fault switch.
                    "printf '%s\\n' \"$$\" > runtime-pid; read -r line",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::from(stderr.reopen().unwrap()))
                .spawn()
                .unwrap(),
            pid: None,
        };
        let pid_file = instance.join("game/runtime-pid");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(pid) = fs::read_to_string(&pid_file)
                && pid.trim().parse::<u32>().is_ok_and(|pid| pid > 1)
            {
                runtime.pid = Some(pid.trim().to_owned());
                break;
            }
            assert!(Instant::now() < deadline, "Runtime did not start");
            assert!(
                runtime.host.try_wait().unwrap().is_none(),
                "{}",
                fs::read_to_string(stderr.path()).unwrap()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let markers = fs::read_dir(instance.join(".host-state"))
            .unwrap()
            .map(|entry| entry.unwrap().path().join("runtime.json"))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        assert_eq!(markers.len(), 1, "One owned runtime must have one marker");
        let marker = &markers[0];
        if interfere_with_cleanup {
            // Controlled external edit to real ownership evidence, not a product hook.
            fs::write(marker, b"externally changed runtime evidence").unwrap();
        }
        assert!(
            Command::new("/bin/kill")
                .args(["-INT", &runtime.host.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = runtime.host.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "Host cancellation did not retire"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let pid = runtime.pid.as_ref().unwrap();
        let remaining = Command::new("/bin/kill")
            .args(["-0", "--", &format!("-{pid}")])
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(!remaining.status.success(), "Owned runtime group survived");
        assert!(
            String::from_utf8_lossy(&remaining.stderr).contains("No such process"),
            "{remaining:?}"
        );
        runtime.pid = None;
        assert_eq!(status.code(), Some(130));
        let diagnostic = fs::read_to_string(stderr.path()).unwrap();
        assert!(diagnostic.contains("Operation interrupted"), "{diagnostic}");
        if interfere_with_cleanup {
            assert!(
                diagnostic.contains("Runtime evidence changed before retirement"),
                "{diagnostic}"
            );
            assert!(
                diagnostic.contains("recovery evidence could not be cleared"),
                "{diagnostic}"
            );
            assert!(
                diagnostic.contains("instance recover-runtime --acknowledge-stopped"),
                "{diagnostic}"
            );
            assert_eq!(
                fs::read(marker).unwrap(),
                b"externally changed runtime evidence"
            );
            offline_command(&instance)
                .args([
                    "--yes", "instance", "launch", "--", "/bin/sh", "-c", "exit 0",
                ])
                .assert()
                .failure()
                .stderr(contains("retirement is unconfirmed"));
            offline_command(&instance)
                .args(["--yes", "instance", "recover-runtime"])
                .assert()
                .failure()
                .stderr(contains("--acknowledge-stopped"));
            assert!(marker.is_file());
            offline_command(&instance)
                .args([
                    "--yes",
                    "instance",
                    "recover-runtime",
                    "--acknowledge-stopped",
                ])
                .assert()
                .success();
        } else {
            assert!(!diagnostic.contains("recovery is required"), "{diagnostic}");
        }
        assert!(!marker.exists());
        offline_command(&instance)
            .args([
                "--yes", "instance", "launch", "--", "/bin/sh", "-c", "exit 0",
            ])
            .assert()
            .success();
        offline_command(&instance)
            .args([
                "--yes", "instance", "launch", "--", "/bin/sh", "-c", "exit 7",
            ])
            .assert()
            .code(1)
            .stderr(contains("Instance runtime exited with exit status: 7"));
        assert!(!marker.exists());
        assert_eq!(
            fs::read(instance.join("game/managed.txt")).unwrap(),
            b"publisher content"
        );
        assert_eq!(fs::read(export.join("release.json")).unwrap(), payload);
    }
}

#[test]
fn cli_unknown_native_build_target_exits_two() {
    check(
        &output(&fixture(), &["build", "not-a-target", "--yes"]),
        EmpackExitCode::Usage,
        "invalid value 'not-a-target'",
    );
}

#[test]
fn cli_native_download_transport_failure_exits_three() {
    let project = fixture();
    let before = fs::read(project.dir().join("empack.lock")).unwrap();
    let result = command(&project)
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "")
        .env("EMPACK_NET_TIMEOUT", "1")
        .args([
            "add",
            "--yes",
            "--type",
            "resourcepack",
            "https://example.invalid/assets.zip",
        ])
        .output()
        .unwrap();
    check(&result, EmpackExitCode::Network, "Download");
    assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), before);
    assert!(
        empack_lib::testing::read_authoring(&project.dir().join("empack.yml"))
            .unwrap()
            .intent()
            .roots
            .is_empty()
    );
}

#[test]
fn cli_invalid_consumer_delivery_exits_two_without_publication() {
    let project = fixture();
    let manifest = fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = fs::read(project.dir().join("empack.lock")).unwrap();
    check(
        &output(
            &project,
            &["build", "modrinth", "--delivery", "bundled", "--yes"],
        ),
        EmpackExitCode::Usage,
        "dependency delivery",
    );
    assert_eq!(
        fs::read(project.dir().join("empack.yml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read(project.dir().join("empack.lock")).unwrap(), lock);
    assert!(!project.dir().join("dist").exists());
}
