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
    let mut cmd = empack_tests::harness::process::offline_command(project.dir());
    cmd.env(
        "EMPACK_PACKWIZ_BIN",
        project.dir().join("must-not-be-executed"),
    )
    .timeout(Duration::from_secs(15));
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
    for missing_lock in [false, true] {
        let project = initialized();
        let intent_path = project.dir().join("empack.yml");
        let mut intent = empack_lib::testing::read_authoring(&intent_path)
            .unwrap()
            .intent()
            .clone();
        let dependency = DependencyIntent {
            releases: None,
            source: SourceIntent::Local(
                PortableRelPath::parse("settings.toml", PathSyntax::ProjectContent).unwrap(),
            ),
            kind: ContentKind::Config,
            selection: DependencySelection::Automatic,
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
        empack_lib::testing::write_authoring(&intent_path, &intent).unwrap();
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
        empack_lib::testing::write_authoring(&intent_path, &intent).unwrap();
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
    empack_lib::testing::read_authoring(&project.dir().join("empack.yml")).unwrap()
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
        .args(["build", "modrinth", "--yes"])
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
        vec!["build", "modrinth", "--optional", "choice=maybe"],
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

#[test]
fn smoke_independent_add_publishes_successes_once_and_reports_preserved_failures() {
    let project = initialized();
    jar(&project, "existing.jar");
    jar(&project, "fresh.jar");
    command(&project)
        .args(["add", "existing.jar", "--yes"])
        .assert()
        .success();
    let before = project_snapshot(&project);
    command(&project)
        .args(["add", "existing.jar", "fresh.jar", "--yes"])
        .assert()
        .failure();
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args([
            "add",
            "existing.jar",
            "fresh.jar",
            "--continue-independent",
            "--dry-run",
            "--yes",
        ])
        .assert()
        .success();
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args([
            "add",
            "existing.jar",
            "fresh.jar",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Dependency batch incomplete"));
    assert_eq!(read_project(&project).intent().roots.len(), 2);
    assert_eq!(
        fs::read(project.dir().join("pack/mods/existing.jar")).unwrap(),
        before[Path::new("pack/mods/existing.jar")]
    );
    assert_eq!(
        fs::read(project.dir().join("pack/mods/fresh.jar")).unwrap(),
        fs::read(project.dir().join("fresh.jar")).unwrap()
    );
    let after = project_snapshot(&project);
    for _ in 0..2 {
        command(&project).args(["sync", "--yes"]).assert().success();
    }
    assert_eq!(project_snapshot(&project), after);
    command(&project)
        .args([
            "add",
            "existing.jar",
            "fresh.jar",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .failure();
    assert_eq!(project_snapshot(&project), after);
    command(&project)
        .args([
            "update",
            "existing",
            "fresh",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .success();
    assert_eq!(project_snapshot(&project), after);
}

#[test]
fn smoke_independent_add_preserves_unowned_directory_and_installs_unrelated_file() {
    let project = initialized();
    jar(&project, "blocked.jar");
    jar(&project, "ready.jar");
    let destination = project.dir().join("pack/mods/blocked.jar");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("sentinel"), b"unowned").unwrap();
    let before = project_snapshot(&project);
    command(&project)
        .args([
            "add",
            "blocked.jar",
            "ready.jar",
            "--continue-independent",
            "--dry-run",
            "--yes",
        ])
        .assert()
        .success();
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args([
            "add",
            "blocked.jar",
            "ready.jar",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .failure();
    assert_eq!(fs::read(destination.join("sentinel")).unwrap(), b"unowned");
    assert_eq!(
        fs::read(project.dir().join("pack/mods/ready.jar")).unwrap(),
        fs::read(project.dir().join("ready.jar")).unwrap()
    );
    let intent = read_project(&project);
    assert_eq!(intent.intent().roots.len(), 1);
    assert!(
        intent
            .intent()
            .roots
            .contains_key(&empack_core::model::DependencyKey::parse("ready").unwrap())
    );
}

#[test]
fn smoke_build_explanation_is_json_and_preserves_missing_lock_workspace() {
    let project = initialized();
    fs::remove_file(project.dir().join("empack.lock")).unwrap();
    let before = snapshot(&project);
    let output = command(&project)
        .args([
            "build",
            "--profile",
            "prism",
            "--explain",
            "--json",
            "--format",
            "tar.gz",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 2);
    assert_eq!(report["lock"], "missing");
    assert_eq!(report["optional_policy"]["mode"], "preserve");
    assert_eq!(report["profiles"][0]["recipe"]["format"], "tar.gz");
    assert_eq!(report["profiles"][0]["origins"]["format"], "cli-override");
    assert!(report["declared_manual_inputs"].is_null());
    assert_eq!(snapshot(&project), before);
    for args in [
        vec!["build", "--profile", "prism", "server"],
        vec!["build", "--continue", "--profile", "prism"],
        vec!["build", "--json"],
        vec!["build", "--ad-hoc"],
        vec!["build", "curseforge", "--explain"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_pending_list_is_read_only_json_without_an_author_project() {
    let project = TestProject::new();
    let before = snapshot(&project);
    let output = command(&project)
        .args(["pending", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["operations"], serde_json::json!([]));
    assert_eq!(snapshot(&project), before);
    for action in ["show", "resume", "discard"] {
        command(&project)
            .args(["pending", action, "1234"])
            .assert()
            .failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_outdated_json_and_runtime_plan_preserve_author_workspace() {
    let project = initialized();
    let before = snapshot(&project);
    let output = command(&project)
        .args(["outdated", "--json", "--release-policy", "stable-only"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["release_policy"], "stable-only");
    assert_eq!(report["joint"]["state"], "not-applicable");
    assert_eq!(report["dependencies"], serde_json::json!([]));
    let output = command(&project)
        .args([
            "migrate",
            "--minecraft",
            "1.21.1",
            "--loader",
            "vanilla",
            "--plan",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 2);
    assert_eq!(report["proposed_runtime"]["minecraft"], "1.21.1");
    assert_eq!(report["proposed_runtime"]["loader"], "vanilla");
    assert_eq!(report["current_runtime"], report["proposed_runtime"]);
    assert_eq!(report["dependencies"], serde_json::json!([]));
    assert_eq!(snapshot(&project), before);
    for args in [
        vec![
            "migrate",
            "--minecraft",
            "1.21.1",
            "--loader",
            "vanilla",
            "--json",
        ],
        vec![
            "migrate",
            "--minecraft",
            "1.21.1",
            "--loader",
            "vanilla",
            "--replace",
            "bad",
        ],
        vec!["outdated", "--release-policy", "latest"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_generated_workflow_builds_named_profiles_and_rejects_wrong_release_tag() {
    let project = initialized();
    let intent_path = project.dir().join("empack.yml");
    let mut intent = empack_lib::testing::read_authoring(&intent_path)
        .unwrap()
        .intent()
        .clone();
    assert!(intent.distribution.profiles.values().all(
        |profile| profile.recipe.delivery() == empack_core::distribution::Delivery::References
    ));
    intent.metadata.name = "Pack With Spaces".into();
    intent.metadata.version = "1.2.3-beta".into();
    intent.distribution.defaults = vec!["modrinth".into(), "prism".into()];
    empack_lib::testing::write_authoring(&intent_path, &intent).unwrap();
    command(&project).args(["sync", "--yes"]).assert().success();
    fs::create_dir_all(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/stale.zip"), b"unselected").unwrap();
    let runner = tempfile::tempdir().unwrap();
    let binary = project.cmd();
    let path = std::env::join_paths(
        std::iter::once(Path::new(binary.get_program()).parent().unwrap().to_owned())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let helper = |action: &str| {
        let mut child =
            std::process::Command::new(if cfg!(windows) { "python" } else { "python3" });
        for (key, value) in binary.get_envs() {
            match value {
                Some(value) => {
                    child.env(key, value);
                }
                None => {
                    child.env_remove(key);
                }
            }
        }
        child
            .current_dir(project.dir())
            .args([".github/scripts/empack-ci.py", action])
            .env("PATH", &path)
            .env("RUNNER_TEMP", runner.path())
            .env("EMPACK_PROFILES", "")
            .env("GITHUB_REF_NAME", "v9.9.9")
            .env("EMPACK_STATE_DIR", project.dir().join(".host-state"))
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("HTTP_PROXY", "http://127.0.0.1:9");
        let mut child = assert_cmd::Command::from_std(child);
        child.timeout(Duration::from_secs(60));
        child
    };
    helper("release")
        .assert()
        .failure()
        .stderr(predicates::str::contains("must match authored version"));
    assert!(!runner.path().join("empack-artifacts").exists());
    helper("validate").assert().success();
    let output = runner.path().join("empack-artifacts");
    let names: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 3);
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("prism-references.zip"))
    );
    assert!(
        !names
            .iter()
            .any(|name| name.contains(' ') || name == "stale.zip")
    );
    let sums = fs::read_to_string(output.join("SHA256SUMS")).unwrap();
    assert_eq!(sums.lines().count(), 2);
    for line in sums.lines() {
        let (_, name) = line.split_once("  ").unwrap();
        assert!(output.join(name).is_file());
    }
}

#[test]
fn smoke_strict_outdated_accepts_complete_empty_assessment() {
    let project = initialized();
    let output = command(&project)
        .args(["outdated", "--strict", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["joint"]["state"], "not-applicable");
    assert_eq!(report["dependencies"].as_array().unwrap().len(), 0);
}
