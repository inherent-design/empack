//! Native prompt and desktop assistance contracts. No fake packwiz or installer output.
use empack_tests::e2e::{TestProject, assert_project_initialized, empack_cmd};
use expectrl::{Expect, Regex, Session};
#[cfg(unix)]
use std::path::Path;
use std::{fs, time::Duration};

#[test]
fn e2e_init_interactive_responds_to_prompts() {
    let project = TestProject::new();
    let mut cmd = empack_cmd(project.dir());
    cmd.args([
        "init",
        "--modloader",
        "none",
        "--mc-version",
        "1.21.1",
        "interactive-test",
    ]);
    let mut terminal = Session::spawn(cmd).unwrap();
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal.expect(Regex("(?i)modpack name")).unwrap();
    terminal.send_line("my-test-pack").unwrap();
    terminal.expect(Regex("(?i)author")).unwrap();
    terminal.send_line("Test Author").unwrap();
    terminal.expect(Regex("(?i)version")).unwrap();
    terminal.send_line("1.0.0").unwrap();
    terminal
        .expect(Regex("(?i)apply this initialization plan"))
        .unwrap();
    terminal.send_line("y").unwrap();
    terminal.expect(Regex("(?i)initialized")).unwrap();
    let pack = project.dir().join("interactive-test");
    assert_project_initialized(&pack);
    let intent = fs::read_to_string(pack.join("empack.yml")).unwrap();
    assert!(intent.contains("my-test-pack"));
    assert!(intent.contains("Test Author"));
}

#[test]
fn e2e_init_interactive_decline_keeps_destination_absent() {
    let project = TestProject::new();
    let mut cmd = empack_cmd(project.dir());
    cmd.args([
        "init",
        "--modloader",
        "none",
        "--mc-version",
        "1.21.1",
        "--pack-name",
        "Declined",
        "--author",
        "Tester",
        "--pack-version",
        "1.0",
        "declined",
    ]);
    let mut terminal = Session::spawn(cmd).unwrap();
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal
        .expect(Regex("(?i)apply this initialization plan"))
        .unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    assert!(!project.dir().join("declined").exists());
}

#[cfg(unix)]
fn browser_fixture() -> (TestProject, tempfile::TempDir) {
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::ResolvedPin,
    };
    let identity = ProviderProjectId::Modrinth(ModrinthProjectId::parse("w0TnApzs").unwrap());
    let project = empack_tests::fixtures::restricted::with_provider(
        "browser-native",
        ResolvedPin {
            selection: identity.parse_pin("lDYpMiqk").unwrap(),
            project: identity,
        },
    );
    let host = tempfile::tempdir().unwrap();
    let (opener, _) = empack_lib::platform::browser_open_command();
    fs::create_dir(host.path().join("bin")).unwrap();
    fs::create_dir(host.path().join("downloads")).unwrap();
    let file = host.path().join("bin").join(opener);
    fs::write(&file, "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >> \"$EMPACK_TEST_BROWSER_LOG\"\nprintf payload > \"$EMPACK_TEST_BROWSER_FILE\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    (project, host)
}
#[cfg(unix)]
fn browser_command(project: &TestProject, host: &Path) -> std::process::Command {
    let mut cmd = project.cmd();
    let paths = std::env::var_os("PATH").unwrap_or_default();
    cmd.env(
        "PATH",
        std::env::join_paths(
            std::iter::once(host.join("bin")).chain(std::env::split_paths(&paths)),
        )
        .unwrap(),
    );
    cmd.env("EMPACK_TEST_BROWSER_LOG", host.join("opened"));
    cmd.env(
        "EMPACK_TEST_BROWSER_FILE",
        host.join("downloads/renamed.bin"),
    );
    cmd.arg("--state-dir").arg(host.join("state"));
    cmd
}

#[cfg(unix)]
#[test]
fn e2e_build_browser_preview_and_decline_have_no_desktop_effects() {
    let (project, host) = browser_fixture();
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["--dry-run", "build", "mrpack", "--open-downloads"]);
    assert_cmd::Command::from_std(cmd).assert().success();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["build", "mrpack", "--open-downloads"]);
    let mut terminal = Session::spawn(cmd).unwrap();
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal
        .expect(Regex("(?i)apply this save pending build plan"))
        .unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
}

/// Requires the official Modrinth metadata API; the desktop opener and downloaded bytes are fixtures.
#[cfg(unix)]
#[test]
fn e2e_live_browser_opens_exact_provider_page_then_resumes_verified_content() {
    let (project, host) = browser_fixture();
    let mut cmd = browser_command(&project, host.path());
    cmd.args([
        "--yes",
        "build",
        "mrpack",
        "--open-downloads",
        "--wait-downloads",
        "10",
        "--downloads-dir",
    ])
    .arg(host.path().join("downloads"));
    assert_cmd::Command::from_std(cmd)
        .timeout(Duration::from_secs(90))
        .assert()
        .success();
    let opened = fs::read_to_string(host.path().join("opened")).unwrap();
    assert_eq!(
        opened.trim(),
        "https://modrinth.com/resourcepack/faithful-32x/version/lDYpMiqk"
    );
    empack_tests::e2e::assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        !fs::read_dir(host.path().join("state/pending-builds"))
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"))
    );
}
