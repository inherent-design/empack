//! Existing-source journeys over public commands; no live provider or consumer claim.
use super::*;

#[test]
fn journey_local_authoring_sync_export_remove_preserves_bytes() {
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
fn journey_local_adoption_restores_resolution_without_reinstall() {
    let project = initialized();
    jar(&project, "fixture.jar");
    fs::write(project.dir().join("prior-build.zip"), b"Prior artifact").unwrap();
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
    // An occupied destination with different bytes is not an adoption candidate.
    fs::write(
        project.dir().join("pack/mods/fixture.jar"),
        b"wrong installed bytes",
    )
    .unwrap();
    let wrong = snapshot(&project);
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), wrong);
    fs::copy(
        project.dir().join("fixture.jar"),
        project.dir().join("pack/mods/fixture.jar"),
    )
    .unwrap();
    let before = project_snapshot(&project);
    let installed = project.dir().join("pack/mods/fixture.jar");
    let source_time = fs::metadata(project.dir().join("fixture.jar"))
        .unwrap()
        .modified()
        .unwrap();
    let installed_time = fs::metadata(&installed).unwrap().modified().unwrap();
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--dry-run", "--yes"])
        .assert()
        .success();
    assert_eq!(project_snapshot(&project), before);
    command(&project)
        .args(["adopt", "--from", "fixture.jar", "--yes"])
        .assert()
        .success();
    assert_eq!(read_project(&project).intent().roots.len(), 1);
    let mut expected = before;
    let mut actual = project_snapshot(&project);
    for name in ["empack.yml", "empack.lock", "empack.bindings.yml"] {
        actual.remove(Path::new(name));
        expected.remove(Path::new(name));
    }
    assert_eq!(actual, expected);
    assert_eq!(
        fs::metadata(project.dir().join("fixture.jar"))
            .unwrap()
            .modified()
            .unwrap(),
        source_time
    );
    assert_eq!(
        fs::metadata(&installed).unwrap().modified().unwrap(),
        installed_time
    );
    let resolved = empack_tests::harness::process::read_project(project.dir());
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
    let restored = empack_tests::harness::process::read_project(project.dir());
    assert_eq!(restored.intent(), resolved.intent());
    // Without the old lock, the inferred display title is reconstructed from
    // the logical key. Identity, exact byte evidence and placements must survive.
    let mut expected_dependencies = resolved.lock().dependencies.clone();
    let key = empack_core::model::DependencyKey::parse("fixture").unwrap();
    assert_eq!(expected_dependencies[&key].title, "fixture.jar");
    expected_dependencies.get_mut(&key).unwrap().title = "fixture".into();
    assert_eq!(restored.lock().dependencies, expected_dependencies);
    assert_eq!(restored.lock().runtime, resolved.lock().runtime);
    let mut actual = project_snapshot(&project);
    actual.remove(Path::new("empack.lock"));
    let mut expected = before;
    expected.retain(|path, _| !path.starts_with(".host-state"));
    assert_eq!(
        actual, expected,
        "restoring the lock must not rewrite payloads or authoring"
    );
    assert_eq!(
        fs::metadata(&installed).unwrap().modified().unwrap(),
        installed_time
    );
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
fn journey_independent_add_reports_partial_effects_and_converges() {
    let project = initialized();
    let cache = tempfile::tempdir().unwrap();
    let run = || {
        let mut cmd = command(&project);
        cmd.env("EMPACK_CACHE_DIR", cache.path());
        cmd
    };
    jar(&project, "existing.jar");
    jar(&project, "fresh.jar");
    fs::write(
        project.dir().join("author-note.md"),
        b"Keep the review notes",
    )
    .unwrap();
    fs::write(project.dir().join("prior-build.zip"), b"Previous artifact").unwrap();
    run()
        .args(["add", "existing.jar", "--yes"])
        .assert()
        .success();
    let before = project_snapshot(&project);
    let prior = empack_tests::harness::process::read_project(project.dir());
    run()
        .args(["add", "existing.jar", "fresh.jar", "--yes"])
        .assert()
        .failure();
    assert_eq!(project_snapshot(&project), before);
    for dry in [true, false] {
        let mut cmd = run();
        cmd.args([
            "add",
            "existing.jar",
            "fresh.jar",
            "--continue-independent",
            "--yes",
        ]);
        if dry {
            cmd.arg("--dry-run");
        }
        let result = cmd.assert();
        let output = result.get_output();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for expected in [
            "Ready independent group: fresh",
            "Preserve blocked group existing:",
        ] {
            assert!(text.contains(expected), "{text}");
        }
        if dry {
            result.success();
            assert_eq!(project_snapshot(&project), before);
            assert!(!text.contains("Recorded 1 dependency bindings"), "{text}");
        } else {
            result.failure();
            for expected in [
                "Recorded 1 dependency bindings and 0 exact references",
                "Addition completed only part of its displayed effects",
                "Dependency batch incomplete: 1 successful groups, 1 blocked groups",
            ] {
                assert!(text.contains(expected), "{text}");
            }
        }
    }
    assert_eq!(read_project(&project).intent().roots.len(), 2);
    assert_eq!(
        fs::read(project.dir().join("pack/mods/existing.jar")).unwrap(),
        before[Path::new("pack/mods/existing.jar")]
    );
    assert_eq!(
        fs::read(project.dir().join("pack/mods/fresh.jar")).unwrap(),
        fs::read(project.dir().join("fresh.jar")).unwrap()
    );
    let resolved = empack_tests::harness::process::read_project(project.dir());
    let existing = empack_core::model::DependencyKey::parse("existing").unwrap();
    assert_eq!(
        resolved.lock().dependencies[&existing],
        prior.lock().dependencies[&existing]
    );
    assert_eq!(resolved.lock().runtime, prior.lock().runtime);
    let mut expected = before.clone();
    expected.insert(
        PathBuf::from("pack/mods/fresh.jar"),
        fs::read(project.dir().join("fresh.jar")).unwrap(),
    );
    let mut actual = project_snapshot(&project);
    for name in ["empack.yml", "empack.lock", "empack.bindings.yml"] {
        actual.remove(Path::new(name));
        expected.remove(Path::new(name));
    }
    assert_eq!(
        actual, expected,
        "partial success must preserve all other files"
    );
    let after = project_snapshot(&project);
    for _ in 0..2 {
        run().args(["sync", "--yes"]).assert().success();
    }
    assert_eq!(project_snapshot(&project), after);
    let retry = run()
        .args([
            "add",
            "existing.jar",
            "fresh.jar",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .failure();
    let output = retry.get_output();
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("Dependency batch incomplete: 0 successful groups, 2 blocked groups"),
        "{text}"
    );
    assert!(
        !text.contains("Recorded "),
        "retry must not publish another receipt: {text}"
    );
    assert_eq!(project_snapshot(&project), after);
    // Input interpretation must finish before an independent footprint exists.
    // A malformed source is therefore an all-requested preflight failure.
    fs::write(
        project.dir().join("existing.jar"),
        b"invalid replacement archive",
    )
    .unwrap();
    jar_version(&project, "fresh.jar", "2");
    let malformed = project_snapshot(&project);
    run()
        .args([
            "update",
            "existing",
            "fresh",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Inspect dependency existing"));
    assert_eq!(project_snapshot(&project), malformed);
    fs::write(
        project.dir().join("existing.jar"),
        &before[Path::new("existing.jar")],
    )
    .unwrap();
    // With resolved inputs, a modified installed file blocks only its group.
    // The other root must perform an actual update, not merely a successful no-op.
    fs::write(
        project.dir().join("pack/mods/existing.jar"),
        b"player local customization",
    )
    .unwrap();
    let changed_sources = project_snapshot(&project);
    run()
        .args(["update", "existing", "fresh", "--yes"])
        .assert()
        .failure();
    assert_eq!(project_snapshot(&project), changed_sources);
    for dry in [true, false] {
        let mut cmd = run();
        cmd.args([
            "update",
            "existing",
            "fresh",
            "--continue-independent",
            "--yes",
        ]);
        if dry {
            cmd.arg("--dry-run");
        }
        let result = cmd.assert();
        let output = result.get_output();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for expected in [
            "Ready independent group: fresh",
            "Preserve blocked group existing:",
        ] {
            assert!(text.contains(expected), "{text}");
        }
        if dry {
            result.success();
            assert_eq!(project_snapshot(&project), changed_sources);
        } else {
            result.failure();
            for expected in [
                "Checked 1 selected dependencies: 1 selections changed, 0 retained; preserved authored intent",
                "Update completed only part of its displayed effects",
                "Dependency batch incomplete: 1 successful groups, 1 blocked groups",
            ] {
                assert!(text.contains(expected), "{text}");
            }
        }
    }
    let next = empack_tests::harness::process::read_project(project.dir());
    assert_eq!(next.intent(), resolved.intent());
    assert_eq!(
        next.lock().dependencies[&existing],
        resolved.lock().dependencies[&existing]
    );
    let mut expected = changed_sources;
    expected.insert(
        PathBuf::from("pack/mods/fresh.jar"),
        fs::read(project.dir().join("fresh.jar")).unwrap(),
    );
    let mut actual = project_snapshot(&project);
    assert_ne!(
        actual.remove(Path::new("empack.lock")),
        expected.remove(Path::new("empack.lock"))
    );
    assert_eq!(actual, expected);
    // Repair the rejected installed file, then retry: the successful root must not
    // publish its update again or manufacture a changed-selection count.
    fs::write(
        project.dir().join("pack/mods/existing.jar"),
        &before[Path::new("existing.jar")],
    )
    .unwrap();
    let after = project_snapshot(&project);
    run()
        .args([
            "update",
            "existing",
            "fresh",
            "--continue-independent",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "0 selections changed, 2 retained",
        ));
    assert_eq!(project_snapshot(&project), after);
    for _ in 0..2 {
        run().args(["sync", "--yes"]).assert().success();
        assert_eq!(project_snapshot(&project), after);
    }
}

#[test]
fn journey_generated_automation_selects_profiles_and_rejects_wrong_tag() {
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
