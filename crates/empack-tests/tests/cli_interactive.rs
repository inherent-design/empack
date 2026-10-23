//! Native prompt and desktop assistance contracts. No fake packwiz or installer output.
use empack_tests::harness::process::{TestProject, assert_project_initialized, empack_cmd};
use expectrl::{Expect, Regex};
#[cfg(unix)]
#[path = "support/desktop_assistance.rs"]
mod desktop_assistance;
#[path = "support/files.rs"]
mod files;
#[path = "support/terminal.rs"]
mod terminal;
#[cfg(unix)]
use desktop_assistance::{browser_command, browser_fixture};
use std::{fs, time::Duration};

#[test]
fn cli_pty_init_accepts_author_decisions() {
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
    let mut terminal = terminal::spawn(cmd);
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
fn cli_pty_init_decline_preserves_absent_destination() {
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
    let mut terminal = terminal::spawn(cmd);
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal
        .expect(Regex("(?i)apply this initialization plan"))
        .unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    assert!(!project.dir().join("declined").exists());
}

#[test]
fn cli_pty_pending_resume_requires_fresh_approval() {
    use empack_tests::{fixtures::restricted::fixture, harness::process::offline_command};
    let project = fixture("pending-approval");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    offline_command(project.dir())
        .arg("--state-dir")
        .arg(&state)
        .args(["--yes", "build", "modrinth"])
        .assert()
        .failure();
    let output = offline_command(project.dir())
        .arg("--state-dir")
        .arg(&state)
        .args(["pending", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["operations"].as_array().unwrap().len(), 1);
    let operation = &report["operations"][0];
    let input = host.path().join("renamed.bin");
    fs::write(&input, b"payload").unwrap();
    let association = format!(
        "{}={}",
        operation["inputs"][0]["key"].as_str().unwrap(),
        input.display()
    );
    let before = files::snapshot(project.dir());
    let retained = files::snapshot(&state);
    let mut cmd = empack_cmd(project.dir());
    cmd.arg("--state-dir").arg(&state).args([
        "pending",
        "resume",
        operation["id"].as_str().unwrap(),
        "--file",
        &association,
    ]);
    let mut terminal = terminal::spawn(cmd);
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal.expect(Regex("(?i)apply this build plan")).unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    terminal.expect(expectrl::Eof).unwrap();
    assert_eq!(files::snapshot(project.dir()), before);
    assert_eq!(files::snapshot(&state), retained);
    assert_eq!(fs::read(&input).unwrap(), b"payload");
}

#[cfg(unix)]
#[test]
fn cli_pty_download_assistance_preview_and_decline_are_read_only() {
    let (project, host) = browser_fixture();
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["--dry-run", "build", "modrinth", "--open-downloads"]);
    assert_cmd::Command::from_std(cmd).assert().success();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["build", "modrinth", "--open-downloads"]);
    let mut terminal = terminal::spawn(cmd);
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal
        .expect(Regex("(?i)apply this save pending build plan"))
        .unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
}
