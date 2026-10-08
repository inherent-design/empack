//! Offline executable contracts for the native engine. No fake backend or cache-seeded API.
use empack_tests::e2e::TestProject;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

fn command(project: &TestProject) -> assert_cmd::Command {
    let mut cmd = project.cmd();
    cmd.env_remove("EMPACK_WORKDIR")
        .env_remove("EMPACK_DRY_RUN")
        .env("EMPACK_STATE_DIR", project.dir().join(".host-state"))
        .env(
            "EMPACK_PACKWIZ_BIN",
            project.dir().join("must-not-be-executed"),
        )
        .env("EMPACK_COLOR", "never")
        .env("EMPACK_NET_TIMEOUT", "1")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "");
    let mut cmd = assert_cmd::Command::from_std(cmd);
    cmd.timeout(Duration::from_secs(15));
    cmd
}
fn initialized() -> TestProject {
    let project = TestProject::new();
    command(&project)
        .args([
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "--pack-name",
            "smoke",
            "--pack-version",
            "1",
        ])
        .assert()
        .success();
    project
}

#[test]
fn smoke_authored_sync_creates_missing_lock_and_preserves_failed_batches() {
    use empack_core::{model::*, path::*, requirements::*};
    use empack_lib::engine::documents::DocumentCodec;
    for missing_lock in [false, true] {
        let project = initialized();
        let intent_path = project.dir().join("empack.yml");
        let mut intent = DocumentCodec
            .decode_intent(&fs::read(&intent_path).unwrap(), "fixture")
            .unwrap()
            .intent()
            .clone();
        let dependency = DependencyIntent {
            source: SourceIntent::Local(
                PortableRelPath::parse("settings.toml", PathSyntax::ProjectContent).unwrap(),
            ),
            kind: ContentKind::Config,
            version: VersionIntent::FollowCompatible,
            placement: PlacementIntent::Explicit(
                NonEmpty::new(vec![Placement {
                    destination: InstallDestination::parse("config/settings.toml").unwrap(),
                    layer: ContentLayer::Common,
                    requirements: Requirements {
                        client: Requirement::Required,
                        server: Requirement::Required,
                    },
                }])
                .unwrap(),
            ),
            requirements: Requirements {
                client: Requirement::Required,
                server: Requirement::Required,
            },
        };
        intent.roots.insert(
            DependencyKey::parse("settings").unwrap(),
            dependency.clone(),
        );
        fs::write(project.dir().join("settings.toml"), b"enabled = true").unwrap();
        fs::write(&intent_path, DocumentCodec.encode_intent(&intent).unwrap()).unwrap();
        if missing_lock {
            fs::remove_file(project.dir().join("empack.lock")).unwrap();
        }
        let before = snapshot(&project);
        command(&project)
            .args(["sync", "--yes", "--dry-run"])
            .assert()
            .success();
        assert_eq!(snapshot(&project), before);
        command(&project).args(["sync", "--yes"]).assert().success();
        assert_eq!(
            fs::read(project.dir().join("pack/config/settings.toml")).unwrap(),
            b"enabled = true"
        );
        let before = project_snapshot(&project);
        for _ in 0..2 {
            command(&project).args(["sync", "--yes"]).assert().success();
        }
        assert_eq!(project_snapshot(&project), before);
        let mut missing = dependency;
        missing.source = SourceIntent::Local(
            PortableRelPath::parse("missing.toml", PathSyntax::ProjectContent).unwrap(),
        );
        missing.placement = PlacementIntent::Explicit(
            NonEmpty::new(vec![Placement {
                destination: InstallDestination::parse("config/missing.toml").unwrap(),
                layer: ContentLayer::Common,
                requirements: missing.requirements.clone(),
            }])
            .unwrap(),
        );
        intent
            .roots
            .insert(DependencyKey::parse("missing").unwrap(), missing);
        fs::write(intent_path, DocumentCodec.encode_intent(&intent).unwrap()).unwrap();
        let before = snapshot(&project);
        command(&project).args(["sync", "--yes"]).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}
fn snapshot(project: &TestProject) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                visit(root, &entry.path(), files);
            } else if kind.is_symlink() {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read_link(entry.path())
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                );
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(project.dir(), project.dir(), &mut files);
    files
}
fn project_snapshot(project: &TestProject) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot(project)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(".host-state"))
        .collect()
}
fn jar(project: &TestProject, name: &str) {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    fs::write(project.dir().join(name), zip.finish().unwrap().into_inner()).unwrap();
}
fn read_project(project: &TestProject) -> empack_lib::engine::documents::DecodedIntent {
    empack_lib::engine::documents::DocumentCodec
        .decode_intent(
            &fs::read(project.dir().join("empack.yml")).unwrap(),
            "smoke",
        )
        .unwrap()
}
#[test]
fn smoke_native_lifecycle_preserves_exact_files_across_sync_build_remove() {
    let project = initialized();
    jar(&project, "fixture.jar");
    command(&project)
        .args(["add", "fixture.jar", "--yes"])
        .assert()
        .success();
    assert_eq!(read_project(&project).intent().roots.len(), 1);
    let before = project_snapshot(&project);
    command(&project)
        .args(["sync", "--materialize", "--dry-run"])
        .assert()
        .success();
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args(["sync", "--materialize", "--yes"])
        .assert()
        .success();
    for _ in 0..2 {
        command(&project).args(["sync", "--yes"]).assert().success();
    }
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args(["build", "mrpack", "--yes"])
        .assert()
        .success();
    let mut archive =
        zip::ZipArchive::new(fs::File::open(project.dir().join("dist/smoke-1.mrpack")).unwrap())
            .unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name("overrides/mods/fixture.jar")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, fs::read(project.dir().join("fixture.jar")).unwrap());
    drop(archive);
    command(&project)
        .args(["remove", "fixture", "--yes"])
        .assert()
        .success();
    command(&project).args(["sync", "--yes"]).assert().success();
    assert!(read_project(&project).intent().roots.is_empty());
    assert!(!project.dir().join("pack/mods/fixture.jar").exists());
}
#[test]
fn smoke_native_preview_and_failed_batch_preserve_every_file() {
    let project = initialized();
    jar(&project, "fixture.jar");
    let before = snapshot(&project);
    command(&project)
        .args(["add", "fixture.jar", "--dry-run", "--yes"])
        .assert()
        .success();
    assert_eq!(snapshot(&project), before);
    command(&project)
        .args(["add", "fixture.jar", "missing.jar", "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
    command(&project)
        .args([
            "init",
            "--force",
            "--dry-run",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
        ])
        .assert()
        .success();
    assert_eq!(snapshot(&project), before);
}
#[test]
fn smoke_headless_search_is_not_first_result_authorization() {
    let project = initialized();
    let before = snapshot(&project);
    command(&project)
        .args(["add", "Sodium", "--yes"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("deliberate choice"));
    assert_eq!(snapshot(&project), before);
}
#[test]
fn smoke_sync_empty_project_is_successful_and_preserves_project() {
    let project = initialized();
    let before = project_snapshot(&project);
    command(&project).args(["sync", "--yes"]).assert().success();
    assert_eq!(project_snapshot(&project), before);
}
#[test]
fn smoke_local_removal_cannot_delete_a_directory() {
    let project = initialized();
    jar(&project, "fixture.jar");
    command(&project)
        .args(["add", "fixture.jar", "--yes"])
        .assert()
        .success();
    let selected = project.dir().join("pack/mods/fixture.jar");
    fs::remove_file(&selected).unwrap();
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("sentinel"), b"keep").unwrap();
    let before = snapshot(&project);
    command(&project)
        .args(["remove", "fixture", "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
}
#[cfg(unix)]
#[test]
fn smoke_local_removal_cannot_follow_a_symlinked_ancestor() {
    let project = initialized();
    jar(&project, "fixture.jar");
    command(&project)
        .args(["add", "fixture.jar", "--yes"])
        .assert()
        .success();
    let outside = tempfile::tempdir().unwrap();
    fs::rename(
        project.dir().join("pack/mods/fixture.jar"),
        outside.path().join("fixture.jar"),
    )
    .unwrap();
    fs::remove_dir(project.dir().join("pack/mods")).unwrap();
    std::os::unix::fs::symlink(outside.path(), project.dir().join("pack/mods")).unwrap();
    let before = snapshot(&project);
    let outside_before = fs::read(outside.path().join("fixture.jar")).unwrap();
    command(&project)
        .args(["remove", "fixture", "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
    assert_eq!(
        fs::read(outside.path().join("fixture.jar")).unwrap(),
        outside_before
    );
}
#[test]
fn smoke_native_requirements_do_not_install_a_backend() {
    let project = TestProject::new();
    command(&project)
        .env("PATH", "")
        .arg("requirements")
        .assert()
        .success();
    assert!(!project.dir().join(".empack-cache/bin").exists());
}
#[test]
fn smoke_invalid_optional_flags_do_not_mutate_project() {
    let project = initialized();
    let before = snapshot(&project);
    for args in [
        vec!["build", "mrpack", "--optional", "choice=maybe"],
        vec!["build", "--continue", "--optional-defaults"],
        vec!["remove", "fixture", "--forget", "--acknowledge-unknown"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}
#[test]
fn smoke_dotenv_precedence_reaches_command_execution() {
    for (local, environment, cli, preserved) in [
        (None, None, false, true),
        (Some(false), None, false, false),
        (Some(true), Some(false), false, false),
        (Some(true), Some(false), true, true),
    ] {
        let project = initialized();
        std::fs::write(project.dir().join(".env"), "EMPACK_DRY_RUN=true\n").unwrap();
        if let Some(value) = local {
            std::fs::write(
                project.dir().join(".env.local"),
                format!("EMPACK_DRY_RUN={value}\n"),
            )
            .unwrap();
        }
        std::fs::create_dir(project.dir().join("dist")).unwrap();
        let artifact = project.dir().join("dist/keep.zip");
        std::fs::write(&artifact, "artifact").unwrap();
        let mut cmd = command(&project);
        if let Some(value) = environment {
            cmd.env("EMPACK_DRY_RUN", value.to_string());
        }
        cmd.args(["clean", "builds", "--yes"]);
        if cli {
            cmd.arg("--dry-run");
        }
        cmd.assert().success();
        assert_eq!(
            artifact.exists(),
            preserved,
            "local={local:?}, env={environment:?}, cli={cli}"
        );
    }
}

#[test]
fn smoke_malformed_dotenv_is_a_configuration_error() {
    let project = TestProject::new();
    std::fs::write(project.dir().join(".env"), "EMPACK_DRY_RUN='unterminated\n").unwrap();
    command(&project).arg("version").assert().code(2);
}

#[test]
fn smoke_malformed_dotenv_does_not_block_clap_help_or_version() {
    for filename in [".env", ".env.local"] {
        let project = TestProject::new();
        std::fs::write(
            project.dir().join(filename),
            "EMPACK_DRY_RUN='unterminated\n",
        )
        .unwrap();
        for args in [
            vec!["--help"],
            vec!["--version"],
            vec!["init", "--help"],
            vec!["build", "--help"],
        ] {
            command(&project)
                .args(args)
                .assert()
                .success()
                .stdout(predicates::str::contains("empack"));
        }
    }
}

#[test]
fn smoke_version_does_not_resolve_managed_tooling() {
    let project = TestProject::new();
    command(&project)
        .env_remove("EMPACK_PACKWIZ_BIN")
        .env("PATH", "")
        .arg("version")
        .assert()
        .success();
    assert!(!project.dir().join(".empack-cache/bin").exists());
}

#[test]
fn smoke_forced_import_invalid_destination_preserves_existing_project() {
    assert_forced_import_rejects_invalid_input(false);
}

#[test]
fn smoke_forced_import_bad_crc_preserves_existing_project() {
    assert_forced_import_rejects_invalid_input(true);
}

fn assert_forced_import_rejects_invalid_input(corrupt_crc: bool) {
    let project = initialized();
    for (path, bytes) in [
        ("pack/config/existing.toml", "original config"),
        ("overrides/client/options.txt", "original options"),
        ("dist/prior.zip", "prior artifact"),
        ("templates/user.template", "user template"),
    ] {
        let path = project.dir().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let manifest = serde_json::json!({
        "formatVersion":1,"game":"minecraft","name":"replacement","versionId":"1",
        "dependencies":{"minecraft":"1.21.1","fabric-loader":"0.15.11"},"files":[]
    })
    .to_string();
    let output = tempfile::tempdir().unwrap();
    let archive = output.path().join("invalid-replacement.mrpack");
    empack_tests::fixtures::write_zip(
        &archive,
        &[
            ("modrinth.index.json", manifest.as_bytes()),
            ("overrides/config/MOD.txt", b"replacement"),
        ],
    )
    .unwrap();
    let mut bytes = std::fs::read(&archive).unwrap();
    if corrupt_crc {
        let central = bytes
            .windows(4)
            .enumerate()
            .find_map(|(offset, signature)| {
                (signature == b"PK\x01\x02"
                    && bytes.get(offset + 46..offset + 46 + b"overrides/config/MOD.txt".len())
                        == Some(b"overrides/config/MOD.txt".as_slice()))
                .then_some(offset)
            })
            .unwrap();
        bytes[central + 16] ^= 1;
    } else {
        let positions: Vec<_> = bytes
            .windows(7)
            .enumerate()
            .filter_map(|(i, value)| (value == b"MOD.txt").then_some(i))
            .collect();
        assert_eq!(positions.len(), 2);
        for position in positions {
            bytes[position..position + 7].copy_from_slice(b"CON.txt");
        }
    }
    std::fs::write(&archive, bytes).unwrap();
    let before = snapshot(&project);
    command(&project)
        .args([
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--force",
            "--yes",
            ".",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(if corrupt_crc {
            "Invalid checksum"
        } else {
            "reserved device name"
        }));
    assert_eq!(snapshot(&project), before);
    for (path, bytes) in [
        ("pack/config/existing.toml", "original config"),
        ("overrides/client/options.txt", "original options"),
        ("dist/prior.zip", "prior artifact"),
        ("templates/user.template", "user template"),
    ] {
        assert_eq!(
            std::fs::read_to_string(project.dir().join(path)).unwrap(),
            bytes
        );
    }
}

#[test]
fn smoke_recovery_inspection_needs_no_pack_and_creates_no_state() {
    let project = TestProject::new();
    // Engine recovery owns journal coordination and must not acquire the legacy mutation lock.
    let _legacy_lock =
        empack_lib::application::persistence::ProjectLock::acquire(project.dir()).unwrap();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    std::fs::write(
        project.dir().join("empack.yml"),
        b"invalid authoring is unrelated",
    )
    .unwrap();
    command(&project)
        .arg("--state-dir")
        .arg(&state)
        .args(["recover", "inspect"])
        .assert()
        .success();
    assert!(!state.exists());
    assert!(!project.dir().join(".empack-state").exists());
    assert_eq!(
        std::fs::read(project.dir().join("empack.yml")).unwrap(),
        b"invalid authoring is unrelated"
    );
    command(&project)
        .arg("--state-dir")
        .arg(&state)
        .args(["recover", "finish", "--operation", "not-pending"])
        .assert()
        .failure();
    assert!(!state.exists());
    command(&project)
        .arg("--state-dir")
        .arg(&state)
        .arg("--workdir")
        .arg(project.dir().join("absent-project"))
        .args(["recover", "inspect"])
        .assert()
        .success();
    assert!(!project.dir().join("absent-project").exists());
    assert!(!state.exists());
}

#[test]
fn smoke_adopt_sources_and_restore_missing_lock_without_installing_files() {
    let project = initialized();
    jar(&project, "fixture.jar");
    let before = snapshot(&project);
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
    fs::create_dir_all(project.dir().join("pack/mods")).unwrap();
    fs::copy(
        project.dir().join("fixture.jar"),
        project.dir().join("pack/mods/fixture.jar"),
    )
    .unwrap();
    let before = snapshot(&project);
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--dry-run", "--yes"])
        .assert()
        .success();
    assert_eq!(snapshot(&project), before);
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--yes"])
        .assert()
        .success();
    assert_eq!(read_project(&project).intent().roots.len(), 1);
    fs::remove_file(project.dir().join("empack.lock")).unwrap();
    let before = snapshot(&project);
    command(&project)
        .args(["adopt", "fixture", "--dry-run", "--yes"])
        .assert()
        .success();
    assert_eq!(snapshot(&project), before);
    command(&project)
        .args(["adopt", "fixture", "--yes"])
        .assert()
        .success();
    let adopted = project_snapshot(&project);
    for _ in 0..2 {
        command(&project).args(["sync", "--yes"]).assert().success();
    }
    assert_eq!(project_snapshot(&project), adopted);
    assert_eq!(
        fs::read(project.dir().join("fixture.jar")).unwrap(),
        fs::read(project.dir().join("pack/mods/fixture.jar")).unwrap()
    );
}
