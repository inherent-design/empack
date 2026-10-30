//! Real process exit and APFS remount, followed by ordinary CLI recovery.
//! The private libtest driver prepares protocol fixtures, not runnable packs.
#![cfg(target_os = "macos")]

use empack_tests::harness::{
    native_volume::ApfsFixture,
    process::{offline_command, offline_empack_cmd},
};
use predicates::str::contains;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "support/files.rs"]
mod files;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Resolve a test executable from this attempt's compiler output, never PATH or
/// a target-directory glob that could select an older feature graph.
fn publication_driver() -> (PathBuf, String) {
    let inventory = PathBuf::from(
        std::env::var_os("EMPACK_TEST_INVENTORY")
            .expect("run this case through scripts/verify.py native-remount"),
    );
    assert!(inventory.is_absolute());
    let bytes = fs::read(&inventory).unwrap();
    assert_eq!(
        digest(&bytes),
        std::env::var("EMPACK_TEST_INVENTORY_SHA256").unwrap()
    );
    let inventory: Value = serde_json::from_slice(&bytes).unwrap();
    let drivers: Vec<_> = inventory["rust-suites"]
        .as_object()
        .unwrap()
        .values()
        .filter(|suite| suite["package-name"] == "empack-lib" && suite["kind"] == "lib")
        .collect();
    assert_eq!(
        drivers.len(),
        1,
        "one compiler-discovered library test binary required"
    );
    let driver = PathBuf::from(drivers[0]["binary-path"].as_str().unwrap());
    assert!(driver.is_absolute() && driver.is_file());
    let hash = digest(&fs::read(&driver).unwrap());
    eprintln!(
        "Publication crash driver: {} SHA256={hash}",
        driver.display()
    );
    (driver, hash)
}

fn driver_command(driver: &Path, invocation: &Path) -> assert_cmd::Command {
    let environment = offline_empack_cmd(invocation);
    let mut command = assert_cmd::Command::new(driver);
    command
        .env_clear()
        .current_dir(invocation)
        .timeout(Duration::from_secs(45));
    for (key, value) in environment.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    command
}

fn operation(output: &str) -> String {
    let operation = output
        .split_once("Interrupted operation: ")
        .and_then(|(_, value)| value.split_whitespace().next())
        .unwrap_or_else(|| panic!("missing public recovery selector in {output:?}"))
        .to_owned();
    assert!(operation.starts_with("op-"));
    operation
}

fn command(invocation: &Path, selected: &Path, state: &Path) -> assert_cmd::Command {
    let mut command = offline_command(invocation);
    command
        .arg("--workdir")
        .arg(selected)
        .arg("--state-dir")
        .arg(state)
        .arg("--cache-dir")
        .arg(state.with_extension("cache"));
    command
}

fn inspect(invocation: &Path, selected: &Path, state: &Path) -> String {
    let output = command(invocation, selected, state)
        .args(["recover", "inspect"])
        .assert()
        .success()
        .get_output()
        .clone();
    format!(
        "{}{}",
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap()
    )
}

#[test]
#[ignore = "creates disposable APFS images; selected by native-remount"]
fn journey_creation_recovery_survives_crash_and_native_remount() {
    let (driver, driver_hash) = publication_driver();
    let volume = ApfsFixture::new().unwrap();
    let host = tempfile::tempdir().unwrap();
    let invocation = host.path();
    let state = invocation.join("state");
    let mut operations = Vec::new();
    for point in [
        "CreationIndexDurable",
        "IntentDurable",
        "TargetChanged",
        "DirectorySynced",
    ] {
        let parent = volume.root().join(point);
        fs::create_dir(&parent).unwrap();
        let selected = parent.join("new");
        // offline_command supplies the isolated environment, then only this
        // test executable receives the private cfg(test) fault controls.
        let mut crash = driver_command(&driver, invocation);
        crash
            .args([
                "--exact",
                "engine::publication::creation::tests::new_root_crash_child",
                "--nocapture",
            ])
            .env("EMPACK_TEST_CREATE_SELECTED", &selected)
            .env("EMPACK_TEST_CREATE_STATE", &state)
            .env("EMPACK_TEST_CREATE_POINT", point)
            .timeout(Duration::from_secs(45))
            .assert()
            .code(93);
        assert_eq!(digest(&fs::read(&driver).unwrap()), driver_hash);
        fs::write(parent.join("user-note.txt"), b"keep neighbor").unwrap();
        let output = inspect(invocation, &selected, &state);
        let operation = operation(&output);
        operations.push((parent, selected, operation));
    }
    let before = files::snapshot(volume.root());
    let host_before = files::snapshot(invocation);
    volume.remount().unwrap();
    assert_eq!(files::snapshot(volume.root()), before);
    assert_eq!(files::snapshot(invocation), host_before);
    for (_, selected, operation) in &operations {
        assert!(inspect(invocation, selected, &state).contains(operation));
    }
    assert_eq!(
        files::snapshot(volume.root()),
        before,
        "read-only inspection changed project bytes"
    );
    assert_eq!(
        files::snapshot(invocation),
        host_before,
        "read-only inspection changed host bytes"
    );
    for (parent, selected, operation) in operations {
        command(invocation, &selected, &state)
            .args(["--yes", "recover", "finish", "--operation", &operation])
            .assert()
            .success();
        assert_eq!(
            files::snapshot(&selected),
            std::collections::BTreeMap::from([
                (PathBuf::from("empack.yml"), b"intent".to_vec()),
                (PathBuf::from("empack.lock"), b"lock".to_vec()),
                (PathBuf::from("pack/config/value"), b"payload".to_vec()),
            ])
        );
        assert_eq!(
            fs::read(parent.join("user-note.txt")).unwrap(),
            b"keep neighbor"
        );
        let mut children: Vec<_> = fs::read_dir(&parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        children.sort();
        assert_eq!(children, ["new", "user-note.txt"]);
        for _ in 0..2 {
            command(invocation, &selected, &state)
                .args(["recover", "inspect"])
                .assert()
                .success()
                .stdout(contains("No interrupted engine operation to recover"));
        }
    }
    volume.finish().unwrap();
}

#[test]
#[ignore = "creates disposable APFS images; selected by native-remount"]
fn journey_publication_finish_and_restore_survive_crash_and_native_remount() {
    let (driver, driver_hash) = publication_driver();
    let volume = ApfsFixture::new().unwrap();
    let host = tempfile::tempdir().unwrap();
    let invocation = host.path();
    let state = invocation.join("state");
    let mut operations = Vec::new();
    for action in ["finish", "restore"] {
        for point in [
            "IntentDurable",
            "TargetChanged",
            "DirectorySynced",
            "Verified",
        ] {
            let selected = volume.root().join(format!("{action}-{point}"));
            fs::create_dir(&selected).unwrap();
            fs::write(selected.join("empack.yml"), b"old intent").unwrap();
            fs::write(selected.join("user-note.txt"), b"keep user bytes").unwrap();
            driver_command(&driver, invocation)
                .args([
                    "--exact",
                    "engine::publication::tests::crash_worker",
                    "--nocapture",
                ])
                .env("EMPACK_PUBLICATION_CRASH_PROJECT", &selected)
                .env("EMPACK_PUBLICATION_CRASH_STATE", &state)
                .env("EMPACK_PUBLICATION_CRASH_POINT", point)
                .env("EMPACK_PUBLICATION_CRASH_OCCURRENCE", "1")
                .assert()
                .code(86);
            assert_eq!(digest(&fs::read(&driver).unwrap()), driver_hash);
            let operation = operation(&inspect(invocation, &selected, &state));
            operations.push((selected, operation, action));
        }
    }
    let before = files::snapshot(volume.root());
    let host_before = files::snapshot(invocation);
    volume.remount().unwrap();
    for (selected, operation, _) in &operations {
        assert!(inspect(invocation, selected, &state).contains(operation));
    }
    assert_eq!(files::snapshot(volume.root()), before);
    assert_eq!(files::snapshot(invocation), host_before);
    for (selected, operation, action) in operations {
        command(invocation, &selected, &state)
            .args(["--yes", "recover", action, "--operation", &operation])
            .assert()
            .success();
        let mut expected = std::collections::BTreeMap::from([
            (PathBuf::from("empack.yml"), b"old intent".to_vec()),
            (PathBuf::from("user-note.txt"), b"keep user bytes".to_vec()),
        ]);
        if action == "finish" {
            expected.insert(PathBuf::from("empack.yml"), b"new intent".to_vec());
            expected.insert(
                PathBuf::from("pack/config/new.txt"),
                b"new content".to_vec(),
            );
        }
        assert_eq!(files::snapshot(&selected), expected);
        for _ in 0..2 {
            assert!(
                inspect(invocation, &selected, &state)
                    .contains("No interrupted engine operation to recover")
            );
            assert_eq!(files::snapshot(&selected), expected);
        }
    }
    volume.finish().unwrap();
}
