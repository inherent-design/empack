//! Restricted content exercises native saved recipes, not simulated packwiz output.
use empack_tests::{
    e2e::{TestProject, assert_dist_artifact_suffix},
    fixtures::restricted::fixture,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[path = "support/files.rs"]
mod files;
use files::snapshot;

fn command(project: &TestProject, state: &Path) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::from_std(project.cmd());
    command.arg("--state-dir").arg(state).arg("--yes");
    // No external backend can manufacture the missing-content decision.
    command.env(
        "EMPACK_PACKWIZ_BIN",
        project.dir().join("unavailable-packwiz"),
    );
    command.timeout(std::time::Duration::from_secs(90));
    command
}
fn saved(state: &Path) -> PathBuf {
    let records: Vec<_> = fs::read_dir(state.join("pending-builds"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(records.len(), 1);
    records[0].clone()
}

#[test]
fn cli_restricted_all_targets_keep_prior_artifacts_and_require_explicit_recipe_cleanup() {
    let project = fixture("restricted-all");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    fs::create_dir_all(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
    let before = snapshot(project.dir());
    command(&project, &state)
        .args([
            "build", "--ad-hoc", "modrinth", "prism", "server", "--clean",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), before);
    let record = saved(&state);
    let recorded = fs::read(&record).unwrap();
    command(&project, &state)
        .args(["--dry-run", "clean", "continuation"])
        .assert()
        .success();
    assert_eq!(fs::read(&record).unwrap(), recorded);
    assert_eq!(snapshot(project.dir()), before);
    command(&project, &state)
        .args(["clean", "continuation"])
        .assert()
        .success();
    assert!(!record.exists());
    assert_eq!(snapshot(project.dir()), before);
}

#[test]
fn cli_restricted_wait_preserves_preview_and_resumes_verified_downloads() {
    let project = fixture("restricted-wait");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let before = snapshot(project.dir());
    for args in [
        vec!["build", "modrinth", "--wait-downloads", "0"],
        vec!["build", "modrinth", "--wait-downloads", "3601"],
        vec!["build", "modrinth", "--wait-downloads", "5"],
    ] {
        command(&project, &state).args(args).assert().failure();
        assert_eq!(snapshot(project.dir()), before);
        assert!(!state.exists());
    }
    command(&project, &state)
        .args([
            "--dry-run",
            "build",
            "modrinth",
            "--wait-downloads",
            "60",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success();
    assert_eq!(snapshot(project.dir()), before);
    assert!(!state.exists());
    // Resolve and qualify the product before starting the observer deadline.
    let mut waiting = command(&project, &state);
    let watched_state = state.clone();
    let destination = downloads.join("renamed-download.bin");
    let producer = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let recorded = fs::read_dir(watched_state.join("pending-builds"))
                .ok()
                .is_some_and(|entries| {
                    entries.flatten().any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|extension| extension == "json")
                    })
                });
            if recorded {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "waiting must save a recipe before polling"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        fs::write(destination, b"payload").unwrap();
    });
    waiting
        .args([
            "build",
            "modrinth",
            "--wait-downloads",
            "10",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .assert()
        .success();
    producer.join().unwrap();
    assert!(
        !fs::read_dir(state.join("pending-builds"))
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"))
    );
    empack_tests::fixtures::restricted::assert_mrpack(
        &assert_dist_artifact_suffix(project.dir(), ".mrpack"),
        "restricted-wait",
    );
    assert_eq!(
        fs::read(downloads.join("renamed-download.bin")).unwrap(),
        b"payload"
    );
    for (path, bytes) in before {
        assert_eq!(fs::read(project.dir().join(path)).unwrap(), bytes);
    }
}

#[test]
fn cli_restricted_wait_timeout_retains_recipe_and_previous_artifacts() {
    let project = fixture("restricted-wait-timeout");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    fs::write(downloads.join("manual.zip"), b"INVALID").unwrap();
    let original_downloads = snapshot(&downloads);
    fs::create_dir(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
    let before = snapshot(project.dir());
    command(&project, &state)
        .args([
            "build",
            "modrinth",
            "--wait-downloads",
            "1",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Download wait expired"));
    assert_eq!(snapshot(project.dir()), before);
    assert!(saved(&state).is_file());
    assert_eq!(snapshot(&downloads), original_downloads);
    fs::write(downloads.join("renamed.bin"), b"payload").unwrap();
    command(&project, &state)
        .args([
            "build",
            "--continue",
            "--wait-downloads",
            "5",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .assert()
        .success();
    empack_tests::fixtures::restricted::assert_mrpack(
        &assert_dist_artifact_suffix(project.dir(), ".mrpack"),
        "restricted-wait-timeout",
    );
    let mut expected_downloads = original_downloads;
    expected_downloads.insert("renamed.bin".into(), b"payload".to_vec());
    assert_eq!(snapshot(&downloads), expected_downloads);
    for (path, bytes) in before {
        assert_eq!(fs::read(project.dir().join(path)).unwrap(), bytes);
    }
}

#[test]
fn cli_restricted_wait_refuses_a_replaced_recipe() {
    let project = fixture("restricted-wait-replacement");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let before = snapshot(project.dir());
    // Resolve and qualify the product before starting the observer deadline.
    let mut waiting = command(&project, &state);
    let watched_state = state.clone();
    let destination = downloads.join("payload.bin");
    let replace = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let record = loop {
            let record = fs::read_dir(watched_state.join("pending-builds"))
                .ok()
                .and_then(|entries| {
                    entries.flatten().map(|entry| entry.path()).find(|path| {
                        path.extension()
                            .is_some_and(|extension| extension == "json")
                    })
                });
            if let Some(record) = record {
                break record;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
        value["recipe"]["allow_optional_metadata_loss"] = true.into();
        let replacement = serde_json::to_vec(&value).unwrap();
        fs::write(&record, &replacement).unwrap();
        fs::write(destination, b"payload").unwrap();
        (record, replacement)
    });
    waiting
        .args([
            "build",
            "modrinth",
            "--wait-downloads",
            "10",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .assert()
        .failure()
        .stderr(predicates::str::contains("replaced while waiting"));
    let (record, replacement) = replace.join().unwrap();
    assert_eq!(fs::read(record).unwrap(), replacement);
    assert_eq!(snapshot(project.dir()), before);
}

#[cfg(unix)]
#[test]
fn cli_restricted_wait_interrupt_retains_recovery_state() {
    let project = fixture("restricted-wait-interrupt");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    fs::create_dir(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
    let before = snapshot(project.dir());
    let mut cmd = project.cmd();
    cmd.arg("--state-dir")
        .arg(&state)
        .args([
            "--yes",
            "build",
            "modrinth",
            "--wait-downloads",
            "60",
            "--downloads-dir",
        ])
        .arg(&downloads)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if fs::read_dir(state.join("pending-builds"))
            .ok()
            .is_some_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "json")
                })
            })
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("waiting did not save its recipe");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        std::process::Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("interrupted waiting did not stop promptly");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    assert!(saved(&state).is_file());
    assert_eq!(snapshot(project.dir()), before);
}
