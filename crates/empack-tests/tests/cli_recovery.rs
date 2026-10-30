//! Public recovery instructions, selected contexts and actual shell interpretation.
//! Crash setup is a private protocol fixture, not a runnable modpack.
use empack_tests::harness::{
    process::{empack_bin, file_sha256, offline_command, offline_empack_cmd},
    publication::Driver,
    workspace::{Snapshot, snapshot},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[path = "support/approval.rs"]
mod approval;

struct Fixture {
    _root: tempfile::TempDir,
    invocation: PathBuf,
    parent: PathBuf,
    project: PathBuf,
    state: PathBuf,
    bin: PathBuf,
}

fn installed_program() -> tempfile::TempDir {
    let bin = tempfile::tempdir().unwrap();
    let selected = empack_bin();
    let executable = bin.path().join(if cfg!(windows) {
        "empack.exe"
    } else {
        "empack"
    });
    if fs::hard_link(&selected, &executable).is_err() {
        fs::copy(&selected, &executable).unwrap();
    }
    assert_eq!(
        file_sha256(&selected).unwrap(),
        file_sha256(&executable).unwrap()
    );
    bin
}

impl Fixture {
    fn new(kind: &str, driver: &Driver, bin: &Path) -> Self {
        let root = tempfile::tempdir().unwrap();
        let invocation = root.path().join("invocation");
        let parent = root.path().join("author parent");
        fs::create_dir(&invocation).unwrap();
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("neighbor.txt"), b"keep neighbor").unwrap();
        let project = parent.join("selected ' $HOME; %TEMP% Ω");
        let state = root.path().join("state ' $HOME; %TEMP% Ω");
        let fixture = Self {
            _root: root,
            bin: bin.into(),
            invocation,
            parent,
            project,
            state,
        };
        if kind == "creation" {
            driver
                .command(&fixture.invocation)
                .args([
                    "--exact",
                    "engine::publication::creation::tests::new_root_crash_child",
                    "--nocapture",
                ])
                .env("EMPACK_TEST_CREATE_SELECTED", &fixture.project)
                .env("EMPACK_TEST_CREATE_STATE", &fixture.state)
                .env("EMPACK_TEST_CREATE_POINT", "IntentDurable")
                .assert()
                .code(93);
            driver.verify();
        } else {
            fs::create_dir(&fixture.project).unwrap();
            fs::write(fixture.project.join("empack.yml"), b"old intent").unwrap();
            fs::write(fixture.project.join("user-note.txt"), b"keep user bytes").unwrap();
            if kind == "cleanup" {
                for (name, bytes) in [
                    ("dist/a.zip", b"first artifact".as_slice()),
                    ("dist/nested/b.mrpack", b"second artifact".as_slice()),
                    ("pack/config/value", b"keep configuration".as_slice()),
                    ("world/level.dat", b"keep player bytes".as_slice()),
                ] {
                    let path = fixture.project.join(name);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, bytes).unwrap();
                }
                driver
                    .command(&fixture.invocation)
                    .args([
                        "--exact",
                        "engine::api::cleanup::tests::cleanup_crash_child",
                        "--nocapture",
                    ])
                    .env("EMPACK_TEST_CLEANUP_PROJECT", &fixture.project)
                    .env("EMPACK_TEST_CLEANUP_STATE", &fixture.state)
                    .assert()
                    .code(86);
                driver.verify();
            } else {
                fixture.crash(driver, false);
                if kind == "restoring" {
                    fixture.crash(driver, true);
                }
            }
        }
        fixture
    }

    fn crash(&self, driver: &Driver, restore: bool) {
        let mut command = driver.command(&self.invocation);
        command
            .args([
                "--exact",
                "engine::publication::tests::crash_worker",
                "--nocapture",
            ])
            .env("EMPACK_PUBLICATION_CRASH_PROJECT", &self.project)
            .env("EMPACK_PUBLICATION_CRASH_STATE", &self.state)
            .env("EMPACK_PUBLICATION_CRASH_POINT", "TargetChanged")
            .env("EMPACK_PUBLICATION_CRASH_OCCURRENCE", "1");
        if restore {
            command.env("EMPACK_PUBLICATION_RESTORE", "1");
        }
        command.assert().code(86);
        driver.verify();
    }

    fn command(&self) -> assert_cmd::Command {
        let mut command = offline_command(&self.invocation);
        command
            .arg("--workdir")
            .arg(&self.project)
            .arg("--state-dir")
            .arg(&self.state)
            .arg("--cache-dir")
            .arg(self.state.with_extension("cache"));
        command
    }

    fn observe(&self) -> Snapshot {
        snapshot(self._root.path())
    }

    fn shell(&self, text: &str) -> assert_cmd::Command {
        #[cfg(unix)]
        let script = text.to_owned();
        #[cfg(windows)]
        // Use the built-in .NET constructor; the isolated shell does not need
        // cmdlet/module discovery merely to configure its output encoding.
        let script = format!(
            "try {{ [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ [Console]::Error.WriteLine($_.Exception.ToString()); exit 1 }}; {text}; exit $LASTEXITCODE"
        );
        self.shell_script(&script)
    }

    fn shell_script(&self, text: &str) -> assert_cmd::Command {
        let environment = offline_empack_cmd(&self.invocation);
        #[cfg(unix)]
        let mut command = Command::new("/bin/sh");
        #[cfg(windows)]
        let mut command = Command::new(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        );
        command.env_clear().current_dir(&self.invocation);
        for (key, value) in environment.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command.env("PATH", &self.bin).env("PATHEXT", ".EXE");
        #[cfg(unix)]
        command.args(["-c", text]);
        #[cfg(windows)]
        command.args(["-NoProfile", "-NonInteractive", "-Command", text]);
        let mut command = assert_cmd::Command::from_std(command);
        command.timeout(Duration::from_secs(30));
        command
    }

    fn inspect(&self) -> String {
        let before = self.observe();
        let output = self
            .command()
            .args(["recover", "inspect"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(self.observe(), before);
        String::from_utf8(output).unwrap()
    }

    #[cfg(windows)]
    fn initialize_shell(&self, check_lookup: bool) {
        let before = self.observe();
        if check_lookup {
            self.shell_script("[Console]::WriteLine('Bare shell ready'); exit 0")
                .assert()
                .success()
                .stdout(predicates::str::contains("Bare shell ready"));
            self.shell("[Console]::WriteLine('Recovery shell ready')")
                .assert()
                .success()
                .stdout(predicates::str::contains("Recovery shell ready"));
        }
        // Each fixture has a fresh USERPROFILE. Initialize PowerShell's native
        // output pipeline with an OS command before measuring empack's effects.
        // /d prevents cmd.exe from running user AutoRun commands.
        self.shell(
            r#"& "$env:SystemRoot\System32\cmd.exe" /d /c echo Native recovery shell ready"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains("Native recovery shell ready"));
        let mut after = self.observe();
        let mut initialized = Vec::new();
        for directory in [
            "invocation/.harness-home",
            "invocation/.harness-home/AppData",
            "invocation/.harness-home/AppData/Roaming",
        ] {
            let path = Path::new(directory);
            if !before.entries.contains_key(path) && after.entries.contains_key(path) {
                assert_eq!(after.entries.remove(path), Some((true, 0)));
                initialized.push(directory);
            }
        }
        assert_eq!(
            after, before,
            "shell setup changed more than empty profile directories"
        );
        eprintln!("Windows native shell initialized empty profile directories: {initialized:?}");
        if check_lookup {
            let initialized = self.observe();
            self.shell("empack --version")
                .assert()
                .success()
                .stdout(predicates::str::contains(format!(
                    "empack {}",
                    empack_tests::harness::process::expected_cli_version()
                )));
            assert_eq!(
                self.observe(),
                initialized,
                "version changed the initialized shell's state"
            );
        }
    }

    fn completed(&self, kind: &str, action: &str) {
        let expected = if kind == "creation" {
            BTreeMap::from([
                (PathBuf::from("empack.yml"), b"intent".to_vec()),
                (PathBuf::from("empack.lock"), b"lock".to_vec()),
                (PathBuf::from("pack/config/value"), b"payload".to_vec()),
            ])
        } else {
            let mut files = BTreeMap::from([
                (
                    PathBuf::from("empack.yml"),
                    if action == "restore" || kind == "restoring" {
                        b"old intent".to_vec()
                    } else {
                        b"new intent".to_vec()
                    },
                ),
                (PathBuf::from("user-note.txt"), b"keep user bytes".to_vec()),
            ]);
            if action == "finish" && kind != "restoring" {
                files.insert(
                    PathBuf::from("pack/config/new.txt"),
                    b"new content".to_vec(),
                );
            }
            files
        };
        assert_eq!(snapshot(&self.project).files, expected);
        assert_eq!(
            fs::read(self.parent.join("neighbor.txt")).unwrap(),
            b"keep neighbor"
        );
        assert!(
            self.inspect()
                .contains("No interrupted engine operation to recover")
        );
    }
}

fn printed_command(text: &str, label: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    let index = lines
        .iter()
        .position(|line| line.contains(label))
        .unwrap_or_else(|| panic!("missing public command {label:?}: {text}"));
    if let Some((_, command)) = lines[index].split_once("): ") {
        command.to_owned()
    } else {
        lines[index + 1].trim().to_owned()
    }
}

#[test]
fn cli_recovery_commands_bind_the_reported_context_and_available_actions() {
    let driver = Driver::from_inventory();
    let program = installed_program();
    let bin = program.path();
    for (kind, action) in [
        ("publication", "finish"),
        ("publication", "restore"),
        ("restoring", "finish"),
        ("creation", "finish"),
    ] {
        let fixture = Fixture::new(kind, &driver, bin);
        #[cfg(windows)]
        fixture.initialize_shell(kind == "publication" && action == "finish");
        let before = fixture.observe();
        let mut blocked = if kind == "creation" {
            let mut command = offline_command(&fixture.invocation);
            command
                .arg("--workdir")
                .arg(&fixture.parent)
                .arg("--state-dir")
                .arg(&fixture.state)
                .args([
                    "--yes",
                    "init",
                    "--modloader",
                    "none",
                    "--mc-version",
                    "1.21.1",
                ])
                .arg(fixture.project.file_name().unwrap());
            command
        } else {
            let mut command = fixture.command();
            command.args(["--yes", "sync"]);
            command
        };
        let output = blocked.assert().failure().get_output().stderr.clone();
        assert_eq!(fixture.observe(), before);
        let inspection =
            printed_command(&String::from_utf8(output).unwrap(), "Recovery inspection (");
        let report = fixture
            .shell(&inspection)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert_eq!(
            fixture.observe(),
            before,
            "printed inspection changed state"
        );
        let report = String::from_utf8(report).unwrap();
        assert!(report.contains("Interrupted operation:"), "{report}");
        let operation = report
            .split_once("Interrupted operation: ")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        fixture
            .command()
            .args([
                "--yes",
                "recover",
                "finish",
                "--operation",
                "wrong-operation",
            ])
            .assert()
            .failure()
            .stderr(predicates::str::contains("differs from --operation"));
        assert_eq!(fixture.observe(), before);
        if kind == "creation" {
            fixture
                .command()
                .args(["--yes", "recover", "restore", "--operation", operation])
                .assert()
                .failure()
                .stderr(predicates::str::contains("finishing only"));
            assert_eq!(fixture.observe(), before);
        }
        if kind != "publication" {
            assert!(!report.contains("Alternative ("), "{report}");
        }
        let selected = printed_command(
            &report,
            if action == "finish" {
                "Next ("
            } else {
                "Alternative ("
            },
        );
        fixture
            .shell(&selected)
            .assert()
            .code(2)
            .stderr(predicates::str::contains("requires --yes"));
        assert_eq!(fixture.observe(), before);
        fixture
            .shell(&format!("{selected} --yes --dry-run"))
            .assert()
            .success();
        assert_eq!(fixture.observe(), before);
        fixture
            .shell(&format!("{selected} --yes"))
            .assert()
            .success();
        fixture.completed(kind, action);
        let completed = fixture.observe();
        fixture
            .shell(&format!("{selected} --yes"))
            .assert()
            .failure()
            .stderr(predicates::str::contains("no longer pending"));
        assert_eq!(
            fixture.observe(),
            completed,
            "stale printed command changed the completed project"
        );
    }
    driver.verify();
}

#[test]
fn cli_cleanup_crash_recovers_exact_artifacts_and_preserves_other_scopes() {
    let driver = Driver::native_fixture("engine::api::cleanup::tests::cleanup_crash_child");
    let program = installed_program();
    for action in ["finish", "restore"] {
        let fixture = Fixture::new("cleanup", &driver, program.path());
        assert!(!fixture.project.join("dist/a.zip").exists());
        assert_eq!(
            fs::read(fixture.project.join("dist/nested/b.mrpack")).unwrap(),
            b"second artifact"
        );
        let report = fixture.inspect();
        let operation = report
            .split_once("Interrupted operation: ")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        let pending = fixture.observe();
        for target in ["builds", "all"] {
            fixture
                .command()
                .args(["--yes", "clean", target])
                .assert()
                .failure()
                .stderr(predicates::str::contains("requires publication recovery"));
            assert_eq!(fixture.observe(), pending);
        }
        let args = ["recover", action, "--operation", operation];
        approval::preserve_without_approval(
            &fixture.invocation,
            &fixture.project,
            &fixture.state,
            &args,
            "(?i)apply this recovery plan",
        );
        fixture.command().arg("--yes").args(args).assert().success();
        let mut expected = BTreeMap::from([
            (PathBuf::from("empack.yml"), b"old intent".to_vec()),
            (PathBuf::from("user-note.txt"), b"keep user bytes".to_vec()),
            (
                PathBuf::from("pack/config/value"),
                b"keep configuration".to_vec(),
            ),
            (
                PathBuf::from("world/level.dat"),
                b"keep player bytes".to_vec(),
            ),
        ]);
        if action == "restore" {
            expected.insert(PathBuf::from("dist/a.zip"), b"first artifact".to_vec());
            expected.insert(
                PathBuf::from("dist/nested/b.mrpack"),
                b"second artifact".to_vec(),
            );
        }
        assert_eq!(snapshot(&fixture.project).files, expected);
        assert_eq!(
            fs::read(fixture.parent.join("neighbor.txt")).unwrap(),
            b"keep neighbor"
        );
        for _ in 0..2 {
            assert!(
                fixture
                    .inspect()
                    .contains("No interrupted engine operation to recover")
            );
        }
        let unrelated = fixture.parent.join("private-driver-control");
        fs::create_dir_all(unrelated.join("dist")).unwrap();
        fs::write(unrelated.join("dist/keep.zip"), b"not selected for cleanup").unwrap();
        let unrelated_before = snapshot(&unrelated);
        fixture
            .command()
            .env("EMPACK_TEST_CLEANUP_PROJECT", &unrelated)
            .env("EMPACK_TEST_CLEANUP_STATE", &fixture.state)
            .args(["--yes", "clean", "builds"])
            .assert()
            .success();
        assert_eq!(snapshot(&unrelated), unrelated_before);
        expected.retain(|path, _| !path.starts_with("dist"));
        assert_eq!(snapshot(&fixture.project).files, expected);
        driver.verify();
    }
}

#[test]
fn cli_recovery_decline_and_prompt_interrupt_preserve_pending_work() {
    let driver = Driver::from_inventory();
    let program = installed_program();
    let fixture = Fixture::new("publication", &driver, program.path());
    let report = fixture.inspect();
    let operation = report
        .split_once("Interrupted operation: ")
        .unwrap()
        .1
        .split_whitespace()
        .next()
        .unwrap();
    let args = ["recover", "finish", "--operation", operation];
    approval::preserve_without_approval(
        &fixture.invocation,
        &fixture.project,
        &fixture.state,
        &args,
        "(?i)apply this recovery plan",
    );
    let before = fixture.observe();
    let mut command = offline_empack_cmd(&fixture.invocation);
    command
        .arg("--workdir")
        .arg(&fixture.project)
        .arg("--state-dir")
        .arg(&fixture.state)
        .arg("--cache-dir")
        .arg(fixture.state.with_extension("cache"))
        .args(args);
    empack_tests::harness::terminal::interrupt(command, "(?i)apply this recovery plan");
    assert_eq!(fixture.observe(), before);
    assert!(fixture.inspect().contains(operation));
    fixture.command().arg("--yes").args(args).assert().success();
    fixture.completed("publication", "finish");
}
