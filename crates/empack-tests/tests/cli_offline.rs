//! Offline executable contracts; connected sequences live in the journeys module.
//! Component codecs are fixture setup, not handwritten-authoring acceptance.
use empack_tests::harness::process::TestProject;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[path = "cli_offline/journeys.rs"]
mod journeys;

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
fn cli_bootstrap_probe_ignores_configuration_and_cannot_execute_work() {
    let project = TestProject::new();
    let dotenv = b"EMPACK_CPU_JOBS=invalid\n";
    fs::write(project.dir().join(".env"), dotenv).unwrap();
    command(&project)
        .env("EMPACK_CPU_JOBS", "invalid")
        .env("EMPACK_COLOR", "invalid")
        .args(["--bootstrap-protocol"])
        .assert()
        .success()
        .stdout("empack-bootstrap/1\n")
        .stderr("");
    for arguments in [
        vec!["--bootstrap-protocol", "status"],
        vec!["--bootstrap-protocol", "--yes"],
        vec!["--bootstrap-protocol", "--bootstrap-protocol"],
    ] {
        command(&project).args(arguments).assert().failure();
    }
    assert_eq!(fs::read(project.dir().join(".env")).unwrap(), dotenv);
    assert!(!project.dir().join("empack.yml").exists());
}

#[test]
fn cli_sync_missing_lock_and_failure_preserve_sources() {
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
    jar_version(project, name, "1");
}
fn jar_version(project: &TestProject, name: &str, version: &str) {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(
        zip,
        r#"{{"schemaVersion":1,"id":"fixture","version":"{version}"}}"#
    )
    .unwrap();
    fs::write(project.dir().join(name), zip.finish().unwrap().into_inner()).unwrap();
}
fn read_project(project: &TestProject) -> empack_lib::engine::documents::DecodedIntent {
    empack_lib::testing::read_authoring(&project.dir().join("empack.yml")).unwrap()
}
#[test]
fn cli_preview_and_failed_add_batch_preserve_snapshot() {
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
fn cli_update_requires_apply_even_with_yes() {
    let project = initialized();
    jar_version(&project, "fixture.jar", "1");
    command(&project)
        .args(["add", "fixture.jar", "--yes"])
        .assert()
        .success();
    let installed = fs::read(project.dir().join("pack/mods/fixture.jar")).unwrap();
    jar_version(&project, "fixture.jar", "2");
    let before = snapshot(&project);
    for args in [
        vec!["update", "fixture", "--select"],
        vec!["--yes", "update", "fixture", "--select"],
    ] {
        command(&project)
            .args(args)
            .assert()
            .code(2)
            .stderr(predicates::str::contains(
                "Version selection requires an interactive terminal",
            ));
        assert_eq!(snapshot(&project), before);
    }

    for selector in ["fixture", "--all"] {
        command(&project)
            .args(["update", selector, "--yes"])
            .assert()
            .success()
            .stdout(predicates::str::contains("Preview only; use --apply"));
        assert_eq!(snapshot(&project), before, "preview for {selector}");
        assert_eq!(
            fs::read(project.dir().join("pack/mods/fixture.jar")).unwrap(),
            installed
        );
    }
    for dry in ["false", "true"] {
        command(&project)
            .env("EMPACK_DRY_RUN", dry)
            .args(["update", "fixture", "--yes"])
            .assert()
            .success();
        assert_eq!(
            snapshot(&project),
            before,
            "An update without --apply must remain a preview: {dry}"
        );
    }
    for (dry, explicit) in [("true", false), ("false", true)] {
        let mut cmd = command(&project);
        cmd.env("EMPACK_DRY_RUN", dry);
        if explicit {
            cmd.arg("--dry-run");
        }
        cmd.args(["update", "--all", "--apply", "--yes"])
            .assert()
            .code(2)
            .stderr(predicates::str::contains(
                "--apply cannot be combined with --dry-run",
            ));
        assert_eq!(snapshot(&project), before);
    }
    let output = command(&project)
        .args(["update", "--all", "--json", "--quiet", "--yes"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["requested"], serde_json::json!(["fixture"]));
    assert_eq!(report["proposal"], "ready");
    assert_eq!(report["changed_selections"], serde_json::json!(["fixture"]));
    assert!(
        report["discovery"].is_null(),
        "Local content must not acquire fake provider evidence"
    );
    let installed_change = report["file_changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "pack/mods/fixture.jar")
        .unwrap();
    use sha2::Digest;
    assert_eq!(
        installed_change["after"]["sha256"],
        sha2::Sha256::digest(fs::read(project.dir().join("fixture.jar")).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    assert_eq!(snapshot(&project), before);
    fs::write(project.dir().join("pack/mods/fixture.jar"), b"local edit").unwrap();
    let blocked_before = snapshot(&project);
    let failed = command(&project)
        .args(["update", "fixture", "--json", "--quiet"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let blocked: serde_json::Value = serde_json::from_slice(&failed).unwrap();
    assert_eq!(blocked["requested"], serde_json::json!(["fixture"]));
    assert_eq!(blocked["proposal"], "blocked");
    assert!(blocked["diagnostic"].is_object());
    assert_eq!(snapshot(&project), blocked_before);
    fs::write(project.dir().join("pack/mods/fixture.jar"), &installed).unwrap();
    assert_eq!(snapshot(&project), before);
    for args in [
        vec!["update"],
        vec!["update", "fixture", "--all"],
        vec!["update", "--all", "--apply", "--dry-run"],
        vec!["update", "--all", "--apply", "--json"],
        vec!["update", "--all", "--details", "--json"],
    ] {
        command(&project).args(args).assert().code(2);
        assert_eq!(snapshot(&project), before);
    }
    for dry in [None, Some("false")] {
        if dry.is_some() {
            jar_version(&project, "fixture.jar", "3");
        }
        let mut cmd = command(&project);
        if let Some(dry) = dry {
            cmd.env("EMPACK_DRY_RUN", dry);
        }
        cmd.args(["update", "--all", "--apply", "--yes"])
            .assert()
            .success();
        assert_eq!(
            fs::read(project.dir().join("pack/mods/fixture.jar")).unwrap(),
            fs::read(project.dir().join("fixture.jar")).unwrap()
        );
        assert_ne!(snapshot(&project), before);
    }
}

#[test]
fn cli_headless_search_refuses_implicit_choice() {
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
fn cli_empty_sync_is_noop() {
    let project = initialized();
    let before = project_snapshot(&project);
    command(&project).args(["sync", "--yes"]).assert().success();
    assert_eq!(project_snapshot(&project), before);
}
#[test]
fn cli_remove_rejects_directory_target() {
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
fn cli_remove_rejects_symlinked_ancestor() {
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
fn cli_invalid_choice_arguments_preserve_project() {
    let project = initialized();
    let before = snapshot(&project);
    for args in [
        vec!["build", "modrinth", "--optional", "choice=maybe"],
        vec!["pending", "resume", "abcdef", "--optional-defaults"],
        vec!["remove", "fixture", "--forget", "--acknowledge-unknown"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn cli_forced_import_invalid_destination_preserves_existing_project() {
    assert_forced_import_rejects_invalid_input(false);
}

#[test]
fn cli_forced_import_bad_crc_preserves_existing_project() {
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
fn cli_recovery_inspection_is_read_only_without_valid_project() {
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
fn cli_independent_add_preserves_conflicting_unowned_directory() {
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
        .failure()
        .stderr(predicates::str::contains(
            "dependency information is incomplete",
        ));
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
    assert_eq!(project_snapshot(&project), before);
    assert!(!project.dir().join("pack/mods/ready.jar").exists());
    // The author can explicitly select the other file as one complete operation.
    command(&project)
        .args(["add", "ready.jar", "--yes"])
        .assert()
        .success();
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
fn cli_build_explanation_is_json_and_preserves_missing_lock_workspace() {
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
        vec!["pending", "resume", "abcdef", "--profile", "prism"],
        vec!["build", "--json"],
        vec!["build", "--ad-hoc"],
        vec!["build", "curseforge", "--explain"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn cli_empty_assessment_and_noop_migration_are_read_only() {
    let project = initialized();
    let before = snapshot(&project);
    let output = command(&project)
        .args([
            "update",
            "--all",
            "--json",
            "--release-policy",
            "stable-only",
        ])
        .assert()
        .success()
        .stderr(predicates::str::is_empty())
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["requested"], serde_json::json!([]));
    assert_eq!(report["proposal"], "unchanged");
    assert!(report["discovery"].is_null());
    assert_eq!(report["source_kinds"], serde_json::json!({}));
    let text = command(&project)
        .args(["update", "--all", "--quiet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(text).unwrap().trim_end(),
        "No dependencies to update."
    );
    let output = command(&project)
        .args([
            "migrate",
            "--minecraft",
            "1.21.1",
            "--loader",
            "vanilla",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["schema"], 3);
    assert_eq!(report["proposal"], "unchanged");
    assert_eq!(report["native_publication_performed"], false);
    assert_eq!(report["file_changes"], serde_json::json!([]));
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
        vec!["update", "--all", "--release-policy", "latest"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn cli_update_terminal_clears_progress_before_one_final_report() {
    use empack_tests::harness::{process, terminal};
    use expectrl::Expect;
    let project = initialized();
    let path = project.dir().join("manual-pack.zip");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"pack":{"pack_format":34,"description":"Offline progress fixture"}}"#)
        .unwrap();
    fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();
    command(&project)
        .args(["--yes", "add", "--type", "resourcepack"])
        .arg(&path)
        .assert()
        .success();
    let before = project_snapshot(&project);
    for columns in [80, 40] {
        let mut command = process::offline_empack_cmd(project.dir());
        command
            .env("TERM", "xterm-256color")
            .args(["update", "--all"]);
        let mut session = terminal::spawn(command);
        session.set_expect_timeout(Some(Duration::from_secs(15)));
        #[cfg(unix)]
        session
            .get_process_mut()
            .set_window_size(columns, 40)
            .unwrap();
        #[cfg(windows)]
        session
            .get_process_mut()
            .resize(columns as i16, 40)
            .unwrap();
        #[cfg(not(windows))]
        let output = {
            let output = session.expect(expectrl::Eof).unwrap().as_bytes().to_vec();
            terminal::wait_for_exit(session.get_process(), 0);
            output
        };
        #[cfg(windows)]
        let output = {
            assert_eq!(session.get_process().wait(Some(15_000)).unwrap(), 0);
            let mut output = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                match session.try_read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => output.extend_from_slice(&buffer[..size]),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("read terminal: {error}"),
                }
                assert!(output.len() < 1 << 20, "unbounded terminal output");
            }
            output
        };
        let mut parser = vt100::Parser::new(40, columns, 0);
        parser.process(&output);
        let screen = parser.screen().contents();
        assert!(
            !screen.contains("Checking"),
            "stale progress at {columns} columns: {screen:?}"
        );
        assert_eq!(screen.matches("Proposal:").count(), 1, "{screen:?}");
        assert_eq!(
            screen.matches("Update plan: 1 dependency").count(),
            1,
            "{screen:?}"
        );
        assert!(screen.contains("manual-pack: no change"), "{screen:?}");
        assert!(screen.contains("Preview only; use --apply"), "{screen:?}");
    }
    assert_eq!(project_snapshot(&project), before);
}

#[test]
fn cli_empty_update_is_unchanged_and_obsolete_queries_are_rejected() {
    let project = initialized();
    let output = command(&project)
        .args(["update", "--all", "--json"])
        .assert()
        .success()
        .stderr(predicates::str::is_empty())
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["proposal"], "unchanged");
    assert_eq!(report["requested"], serde_json::json!([]));
    for args in [vec!["outdated"], vec!["update", "--all", "--strict"]] {
        command(&project).args(args).assert().code(2);
    }
}

#[test]
fn cli_unassociated_saved_operations_identify_records_without_changing_them() {
    let project = initialized();
    let state = project.dir().join(".host-state");
    let mut records = Vec::new();
    for entry in fs::read_dir(&state).unwrap() {
        let directory = entry.unwrap().path();
        let identity = directory.join("root-identity.json");
        if identity.is_file() {
            fs::remove_file(identity).unwrap();
            fs::write(
                directory.join("retained-evidence"),
                b"unfinished work; keep this",
            )
            .unwrap();
            records.push(directory);
        }
    }
    assert!(
        !records.is_empty(),
        "initialization must retain its directory association"
    );
    let before = empack_tests::harness::workspace::snapshot(project.dir());
    for args in [
        vec!["status"],
        vec!["update", "--all"],
        vec!["--yes", "sync"],
    ] {
        let output = command(&project)
            .args(args)
            .assert()
            .code(1)
            .get_output()
            .clone();
        assert!(
            output.stdout.is_empty(),
            "an unsafe read must not produce a success report"
        );
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(
            diagnostic.contains("Saved operations cannot be matched safely"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("Keep these records"), "{diagnostic}");
        let reported = diagnostic
            .lines()
            .filter_map(|line| line.strip_prefix("Missing directory identity: "))
            .map(|quoted| {
                let path: String = serde_json::from_str(quoted).unwrap();
                fs::canonicalize(path).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(reported.len(), records.len(), "{diagnostic}");
        for path in &records {
            // Windows may report the same directory through a short-name alias.
            // Compare the actual preserved location, not its escaped spelling.
            assert!(
                reported.contains(&fs::canonicalize(path).unwrap()),
                "{diagnostic}"
            );
        }
        assert!(!diagnostic.contains("--state-dir"), "{diagnostic}");
        assert_eq!(
            empack_tests::harness::workspace::snapshot(project.dir()),
            before
        );
    }
}

#[test]
fn cli_check_reports_document_failures_as_json_without_changing_inputs() {
    let project = initialized();
    let before = snapshot(&project);
    for args in [
        vec!["check", "--json"],
        vec!["check", "--documents", "--json"],
    ] {
        let output = command(&project).args(args).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["complete"], true);
        assert_eq!(report["lock"], "current");
        assert_eq!(snapshot(&project), before);
    }
    fs::write(project.dir().join("empack.lock"), b"damaged lock").unwrap();
    let before = snapshot(&project);
    let output = command(&project)
        .args(["check", "--documents", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(report["lock"], "invalid");
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["state"] == "failed")
    );
    assert!(!output.stderr.is_empty());
    assert_eq!(snapshot(&project), before);
    for args in [
        vec!["manifest", "check"],
        vec!["check", "--documents", "--online"],
        vec!["check", "--json", "--details"],
    ] {
        command(&project).args(args).assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}
