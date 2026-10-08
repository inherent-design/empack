//! Restricted content exercises native saved recipes, not simulated packwiz output.
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    identity::{CurseForgeProjectId, ProviderProjectId},
    model::*,
    path::InstallDestination,
    requirements::{Requirement, Requirements},
};
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn fixture(name: &str) -> TestProject {
    let project = TestProject::workflow_fixture(name, "fabric", "1.21.1");
    let source = DocumentCodec
        .decode_intent(&fs::read(project.dir().join("empack.yml")).unwrap(), "test")
        .unwrap();
    let prior = DocumentCodec
        .decode_lock(
            &fs::read(project.dir().join("empack.lock")).unwrap(),
            &source,
            "test",
        )
        .unwrap();
    let mut intent = prior.intent().clone();
    let mut lock = prior.lock().clone();
    let key = DependencyKey::parse("manual-assets").unwrap();
    let identity = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
    let pin = ResolvedPin {
        project: identity.clone(),
        selection: identity.parse_pin("456").unwrap(),
    };
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Required,
    };
    let placements = NonEmpty::new(vec![Placement {
        destination: InstallDestination::parse("resourcepacks/manual.zip").unwrap(),
        layer: ContentLayer::Common,
        requirements: requirements.clone(),
    }])
    .unwrap();
    let digests = DigestSet::new(vec![
        ExpectedDigest::parse(
            "sha256",
            "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        )
        .unwrap(),
    ])
    .unwrap();
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            source: SourceIntent::Provider(identity.clone()),
            kind: ContentKind::ResourcePack,
            version: VersionIntent::Exact(pin.selection.clone()),
            placement: PlacementIntent::Explicit(placements.clone()),
            requirements,
        },
    );
    lock.dependencies.insert(
        key.clone(),
        LockedDependency {
            title: "Manual assets".into(),
            kind: ContentKind::ResourcePack,
            identity: ResolvedIdentity::Provider(identity),
            selected: Some(pin.clone()),
            files: NonEmpty::new(vec![ResolvedFile {
                slot: FileSlot::parse("primary").unwrap(),
                acquisition: AcquisitionSpec::Manual {
                    pin: Some(pin),
                    instructions: "Supply the verified provider file".into(),
                },
                expected: ExpectedContent {
                    digests: Some(digests.clone()),
                    size: Some(7),
                    accepted_observation: None,
                },
                provenance: Provenance {
                    source: "test fixture".into(),
                    location: None,
                    declared_digests: Some(digests),
                    conversions: vec![],
                },
                placements,
            }])
            .unwrap(),
        },
    );
    lock.coverage.insert(key, Coverage::CompleteForSelection);
    let bytes = DocumentCodec.encode_intent(&intent).unwrap();
    let revision = DocumentCodec
        .decode_intent(&bytes, "test")
        .unwrap()
        .semantic_revision();
    lock.intent_revision = revision;
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    fs::write(project.dir().join("empack.yml"), bytes).unwrap();
    fs::write(
        project.dir().join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    project
}
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
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn e2e_restricted_mrpack_preserves_preview_and_resumes_only_verified_content() {
    let project = fixture("restricted-mrpack");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let before = snapshot(project.dir());
    command(&project, &state)
        .args(["--dry-run", "build", "mrpack"])
        .assert()
        .success();
    assert_eq!(snapshot(project.dir()), before);
    assert!(!state.exists());
    command(&project, &state)
        .args(["build", "mrpack"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), before);
    let record = saved(&state);
    let recorded = fs::read(&record).unwrap();
    let supplied = host.path().join("download.zip");
    fs::write(&supplied, b"invalid").unwrap();
    let association = format!("locked:manual%2Dassets:primary={}", supplied.display());
    command(&project, &state)
        .args(["build", "--continue", "--associate-download", &association])
        .assert()
        .failure();
    assert_eq!(snapshot(project.dir()), before);
    assert_eq!(fs::read(&record).unwrap(), recorded);
    fs::write(&supplied, b"payload").unwrap();
    command(&project, &state)
        .args(["build", "--continue", "--associate-download", &association])
        .assert()
        .success();
    assert!(!record.exists());
    let mut archive = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(project.dir(), ".mrpack")).unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut archive
            .by_name("overrides/resourcepacks/manual.zip")
            .unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"payload");
    for name in ["empack.yml", "empack.lock"] {
        assert_eq!(
            fs::read(project.dir().join(name)).unwrap(),
            before[Path::new(name)]
        );
    }
}

#[test]
fn e2e_restricted_all_targets_keep_prior_artifacts_and_require_explicit_recipe_cleanup() {
    let project = fixture("restricted-all");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    fs::create_dir_all(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
    let before = snapshot(project.dir());
    command(&project, &state)
        .args(["build", "all", "--clean"])
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
fn e2e_restricted_wait_preserves_preview_and_resumes_verified_downloads() {
    let project = fixture("restricted-wait");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let before = snapshot(project.dir());
    for args in [
        vec!["build", "mrpack", "--wait-downloads", "0"],
        vec!["build", "mrpack", "--wait-downloads", "3601"],
        vec!["build", "mrpack", "--wait-downloads", "5"],
    ] {
        command(&project, &state).args(args).assert().failure();
        assert_eq!(snapshot(project.dir()), before);
        assert!(!state.exists());
    }
    command(&project, &state)
        .args([
            "--dry-run",
            "build",
            "mrpack",
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
    command(&project, &state)
        .args([
            "build",
            "mrpack",
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
    let mut archive = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(project.dir(), ".mrpack")).unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut archive
            .by_name("overrides/resourcepacks/manual.zip")
            .unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"payload");
}

#[test]
fn e2e_restricted_wait_timeout_retains_recipe_and_previous_artifacts() {
    let project = fixture("restricted-wait-timeout");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    fs::write(downloads.join("manual.zip"), b"impostor").unwrap();
    let before = snapshot(project.dir());
    command(&project, &state)
        .args([
            "build",
            "mrpack",
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
    assert_dist_artifact_suffix(project.dir(), ".mrpack");
}

#[test]
fn e2e_restricted_wait_refuses_a_replaced_recipe() {
    let project = fixture("restricted-wait-replacement");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let before = snapshot(project.dir());
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
        value["recipe"]["archive"] = "tar-gz".into();
        let replacement = serde_json::to_vec(&value).unwrap();
        fs::write(&record, &replacement).unwrap();
        fs::write(destination, b"payload").unwrap();
        (record, replacement)
    });
    command(&project, &state)
        .args([
            "build",
            "mrpack",
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
fn e2e_restricted_wait_interrupt_retains_recovery_state() {
    let project = fixture("restricted-wait-interrupt");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let downloads = host.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let before = snapshot(project.dir());
    let mut cmd = project.cmd();
    cmd.arg("--state-dir")
        .arg(&state)
        .args([
            "--yes",
            "build",
            "mrpack",
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
