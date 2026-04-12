use empack_lib::EmpackExitCode;
use empack_tests::e2e::TestProject;
use std::path::{Path, PathBuf};
use std::process::Command;

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn configure_command_env(cmd: &mut Command, workdir: &Path) {
    cmd.env("NO_COLOR", "1");
    let cache_dir = workdir.join(".empack-cache");
    std::fs::create_dir_all(&cache_dir).expect("create EMPACK_CACHE_DIR fallback");
    cmd.env("EMPACK_CACHE_DIR", cache_dir);

    #[cfg(windows)]
    {
        let local_app_data = workdir.join(".windows-localappdata");
        let roaming_app_data = workdir.join(".windows-appdata");
        let user_profile = workdir.join(".windows-userprofile");
        let temp_dir = workdir.join(".windows-temp");

        std::fs::create_dir_all(&local_app_data).expect("create LOCALAPPDATA fallback");
        std::fs::create_dir_all(&roaming_app_data).expect("create APPDATA fallback");
        std::fs::create_dir_all(&user_profile).expect("create USERPROFILE fallback");
        std::fs::create_dir_all(&temp_dir).expect("create TEMP fallback");

        cmd.env("LOCALAPPDATA", local_app_data);
        cmd.env("LocalAppData", workdir.join(".windows-localappdata"));
        cmd.env("APPDATA", roaming_app_data);
        cmd.env("USERPROFILE", user_profile);
        cmd.env("TEMP", temp_dir.clone());
        cmd.env("TMP", temp_dir);
    }
}

fn cargo_empack_cmd(workdir: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(workspace_root());
    cmd.args(["run", "-q", "-p", "empack", "--", "--workdir"]);
    cmd.arg(workdir);
    configure_command_env(&mut cmd, workdir);
    cmd
}

fn cargo_empack_root_cmd() -> Command {
    let root = workspace_root();
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root);
    cmd.args(["run", "-q", "-p", "empack", "--"]);
    configure_command_env(&mut cmd, &root);
    cmd
}

fn write_executable(path: &Path, script: &str) {
    std::fs::write(path, script)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", path.display(), e));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("set executable bit");
    }
}

#[cfg(windows)]
fn write_failing_packwiz_binary(workdir: &Path) -> PathBuf {
    let path = workdir.join("fake-packwiz-fail.cmd");
    let script = "@echo off\r\n1>&2 echo packwiz remove failed\r\nexit /b 1\r\n";
    write_executable(&path, script);
    path
}

#[cfg(not(windows))]
fn write_failing_packwiz_binary(workdir: &Path) -> PathBuf {
    let path = workdir.join("fake-packwiz-fail");
    let script = "#!/bin/sh\nprintf 'packwiz remove failed\\n' >&2\nexit 1\n";
    write_executable(&path, script);
    path
}

#[test]
fn e2e_parse_error_exits_two() {
    let output = cargo_empack_root_cmd()
        .arg("--definitely-invalid-flag")
        .output()
        .expect("spawn parse-error command");

    assert_eq!(
        output.status.code(),
        Some(EmpackExitCode::Usage.as_i32()),
        "unexpected output:\n{}",
        combined_output(&output)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--definitely-invalid-flag"),
        "stderr should mention the invalid flag:\n{}",
        combined_output(&output)
    );
}

#[test]
fn e2e_uninitialized_build_exits_two() {
    let project = TestProject::new();
    let output = cargo_empack_cmd(project.dir())
        .args(["build", "mrpack"])
        .output()
        .expect("spawn build command");

    assert_eq!(
        output.status.code(),
        Some(EmpackExitCode::Usage.as_i32()),
        "unexpected output:\n{}",
        combined_output(&output)
    );

    let combined = combined_output(&output);
    assert!(
        combined.contains("Not in a modpack directory"),
        "expected uninitialized project error, got:\n{combined}"
    );
}

#[test]
fn e2e_packwiz_process_failure_exits_one() {
    let project = TestProject::workflow_fixture("exit-remove-fail", "fabric", "1.21.1");
    let fake_packwiz = write_failing_packwiz_binary(project.dir());

    let output = cargo_empack_cmd(project.dir())
        .env("EMPACK_PACKWIZ_BIN", fake_packwiz)
        .args(["remove", "sodium"])
        .output()
        .expect("spawn remove command");

    assert_eq!(
        output.status.code(),
        Some(EmpackExitCode::General.as_i32()),
        "unexpected output:\n{}",
        combined_output(&output)
    );

    let combined = combined_output(&output);
    assert!(
        combined.contains("packwiz remove failed"),
        "expected packwiz stderr to propagate, got:\n{combined}"
    );
}

#[test]
fn e2e_network_failure_exits_three() {
    let project = TestProject::workflow_fixture("exit-network-fail", "fabric", "1.21.1");

    let output = cargo_empack_cmd(project.dir())
        .env("EMPACK_NET_TIMEOUT", "1")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("https_proxy", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("all_proxy", "http://127.0.0.1:9")
        .args(["add", "sodium"])
        .output()
        .expect("spawn add command");

    assert_eq!(
        output.status.code(),
        Some(EmpackExitCode::Network.as_i32()),
        "unexpected output:\n{}",
        combined_output(&output)
    );
}
