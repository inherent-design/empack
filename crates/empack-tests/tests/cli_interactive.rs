//! Native prompt and desktop assistance contracts. No fake packwiz or installer output.
use empack_tests::harness::process::{TestProject, empack_cmd};
use expectrl::{Expect, Regex};
#[path = "support/approval.rs"]
mod approval;
#[cfg(unix)]
#[path = "support/desktop_assistance.rs"]
mod desktop_assistance;
#[path = "support/files.rs"]
mod files;
#[path = "support/initialization.rs"]
mod initialization;
#[cfg(unix)]
use desktop_assistance::{browser_command, browser_fixture};
use empack_tests::harness::terminal;
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
    #[cfg(not(windows))]
    terminal.expect(expectrl::Eof).unwrap();
    #[cfg(windows)]
    assert_eq!(terminal.get_process().wait(Some(20_000)).unwrap(), 0);
    let pack = project.dir().join("interactive-test");
    initialization::assert_new_project(&pack, "my-test-pack", "1.0.0", &["Test Author"]);
}

#[test]
fn cli_pty_init_decline_preserves_absent_destination() {
    let project = TestProject::new();
    let before = files::snapshot(project.dir());
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
    #[cfg(not(windows))]
    terminal.expect(expectrl::Eof).unwrap();
    #[cfg(windows)]
    assert_eq!(terminal.get_process().wait(Some(20_000)).unwrap(), 0);
    assert!(!project.dir().join("declined").exists());
    assert_eq!(files::snapshot(project.dir()), before);
}

#[test]
fn cli_local_plans_require_explicit_approval_and_preserve_refused_work() {
    use empack_tests::harness::{archive::zip_members, process::offline_command};
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, io::Write};

    for operation in ["initialization", "addition", "build"] {
        let project = TestProject::workflow_fixture("decisions", "vanilla", "1.21.1");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        fs::create_dir_all(project.dir().join("pack/config")).unwrap();
        fs::write(
            project.dir().join("pack/config/options.txt"),
            b"author configuration",
        )
        .unwrap();
        fs::create_dir_all(project.dir().join("dist")).unwrap();
        fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
        let mut jar = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        jar.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        jar.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
            .unwrap();
        let jar = jar.finish().unwrap().into_inner();
        fs::write(project.dir().join("fixture.jar"), &jar).unwrap();
        let args = match operation {
            "initialization" => vec![
                "init",
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
                "--pack-name",
                "New Pack",
                "--pack-version",
                "1.0.0",
                "--author",
                "Tester",
                "new",
            ],
            "addition" => vec!["add", "fixture.jar"],
            "build" => vec!["build", "modrinth"],
            _ => unreachable!(),
        };
        approval::preserve_without_approval(
            project.dir(),
            project.dir(),
            &state,
            &args,
            &format!("(?i)apply this {operation} plan"),
        );
        // An actual affirmative request must reach the intended publication,
        // rather than making every refusal pass through an unrelated failure.
        offline_command(project.dir())
            .arg("--state-dir")
            .arg(&state)
            .arg("--cache-dir")
            .arg(state.with_extension("cache"))
            .arg("--yes")
            .args(&args)
            .assert()
            .success();
        match operation {
            "initialization" => {
                initialization::assert_new_project(
                    &project.dir().join("new"),
                    "New Pack",
                    "1.0.0",
                    &["Tester"],
                );
            }
            "addition" => {
                assert_eq!(
                    fs::read(project.dir().join("pack/mods/fixture.jar")).unwrap(),
                    jar
                );
                let author: Value = serde_saphyr::from_str(
                    &fs::read_to_string(project.dir().join("empack.yml")).unwrap(),
                )
                .unwrap();
                assert_eq!(author["dependencies"].as_object().unwrap().len(), 1);
            }
            "build" => {
                let mut members = zip_members(&project.dir().join("dist/decisions-1.0.0.mrpack"));
                let index: Value =
                    serde_json::from_slice(&members.remove("modrinth.index.json").unwrap())
                        .unwrap();
                assert_eq!(
                    index,
                    json!({"formatVersion":1,"game":"minecraft","name":"decisions","versionId":"1.0.0","files":[],"dependencies":{"minecraft":"1.21.1"}})
                );
                assert_eq!(
                    members,
                    BTreeMap::from([(
                        "overrides/config/options.txt".into(),
                        b"author configuration".to_vec()
                    )])
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(
            fs::read(project.dir().join("dist/prior.zip")).unwrap(),
            b"prior artifact"
        );
        assert_eq!(fs::read(project.dir().join("fixture.jar")).unwrap(), jar);
    }
}

#[test]
fn cli_pty_pending_resume_requires_fresh_approval() {
    use empack_tests::{fixtures::restricted::fixture, harness::process::offline_command};
    let project = fixture("pending-approval");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    fs::create_dir_all(project.dir().join("dist")).unwrap();
    fs::write(
        project.dir().join("dist/prior.mrpack"),
        b"keep prior artifact",
    )
    .unwrap();
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
    approval::preserve_without_approval(
        project.dir(),
        project.dir(),
        &state,
        &[
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &association,
        ],
        "(?i)apply this build plan",
    );
    assert_eq!(fs::read(&input).unwrap(), b"payload");
}

#[cfg(unix)]
#[test]
fn cli_pty_download_assistance_preview_and_decline_are_read_only() {
    let (project, host) = browser_fixture();
    let before = files::snapshot(project.dir());
    let downloads = files::snapshot(&host.path().join("downloads"));
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["--dry-run", "build", "modrinth", "--open-downloads"]);
    assert_cmd::Command::from_std(cmd).assert().success();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
    assert_eq!(files::snapshot(project.dir()), before);
    assert_eq!(files::snapshot(&host.path().join("downloads")), downloads);
    let mut cmd = browser_command(&project, host.path());
    cmd.args(["build", "modrinth", "--open-downloads"]);
    let mut terminal = terminal::spawn(cmd);
    terminal.set_expect_timeout(Some(Duration::from_secs(20)));
    terminal
        .expect(Regex("(?i)apply this save pending build plan"))
        .unwrap();
    terminal.send_line("n").unwrap();
    terminal.expect(Regex("(?i)not applied")).unwrap();
    terminal.expect(expectrl::Eof).unwrap();
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("opened").exists());
    assert_eq!(files::snapshot(project.dir()), before);
    assert_eq!(files::snapshot(&host.path().join("downloads")), downloads);
}
