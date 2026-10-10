//! Executable exit contracts exercise native documents, publication and cancellation.
use empack_lib::EmpackExitCode;
use empack_tests::e2e::TestProject;
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
#[test]
fn e2e_parse_error_exits_two() {
    check(
        &output(&TestProject::new(), &["--definitely-invalid-flag"]),
        EmpackExitCode::Usage,
        "--definitely-invalid-flag",
    );
}
#[test]
fn e2e_uninitialized_build_exits_two() {
    check(
        &output(&TestProject::new(), &["build", "modrinth"]),
        EmpackExitCode::Usage,
        "empack.yml",
    );
}
#[test]
fn e2e_direct_zip_without_type_exits_two() {
    check(
        &output(&fixture(), &["add", "https://example.invalid/pack.zip"]),
        EmpackExitCode::Usage,
        "--type",
    );
}
#[test]
fn e2e_unattended_build_requires_explicit_approval() {
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
fn e2e_malformed_intent_and_lock_exit_two() {
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
fn e2e_missing_tracked_file_cannot_report_success() {
    let project = fixture();
    add_local(&project);
    fs::remove_file(project.dir().join("pack/mods/fixture.jar")).unwrap();
    let result = output(&project, &["build", "--yes", "modrinth"]);
    assert!(
        !result.status.success(),
        "Missing locked content must not build successfully"
    );
    assert!(
        !project
            .dir()
            .join("dist/exit-contract-1.0.0.mrpack")
            .exists()
    );
}
#[test]
fn e2e_tracked_local_parent_dir_validation_exits_two() {
    let project = fixture();
    add_local(&project);
    let sentinel = tempfile::NamedTempFile::new_in(project.dir().parent().unwrap()).unwrap();
    fs::write(sentinel.path(), b"outside bytes").unwrap();
    let path = project.dir().join("empack.yml");
    let mut intent: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    intent["dependencies"]["fixture"]["source"]["path"] = serde_json::json!(format!(
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
fn e2e_occupied_artifact_directory_fails_without_deletion() {
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
fn e2e_network_failure_exits_three() {
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
fn e2e_interrupt_exits_130() {
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
    let mut child = Child(
        cmd.args(["add", "--yes", "--platform", "modrinth", "sodium"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
    assert_eq!(status.code(), Some(EmpackExitCode::Interrupted.as_i32()));
    drop(connection);
    assert!(!project.dir().join("pack/mods").exists());
}

#[test]
fn e2e_unknown_native_build_target_exits_two() {
    check(
        &output(&fixture(), &["build", "not-a-target", "--yes"]),
        EmpackExitCode::Usage,
        "invalid value 'not-a-target'",
    );
}

#[test]
fn e2e_native_download_transport_failure_exits_three() {
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
        empack_lib::engine::documents::DocumentCodec
            .decode_intent(&fs::read(project.dir().join("empack.yml")).unwrap(), "test")
            .unwrap()
            .intent()
            .roots
            .is_empty()
    );
}

#[test]
fn e2e_invalid_consumer_delivery_exits_two_without_publication() {
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
