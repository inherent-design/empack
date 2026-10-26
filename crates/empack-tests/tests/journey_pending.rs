//! Component setup supplies an exact restricted lock; every recovery action uses
//! a fresh executable and only public pending output, never a private record key.
use empack_tests::{
    fixtures::restricted::fixture,
    harness::process::{TestProject, assert_dist_artifact_suffix},
};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[path = "support/approval.rs"]
mod approval;
#[path = "support/files.rs"]
mod files;
use files::snapshot;

#[path = "support/pending.rs"]
mod public_pending;
#[path = "support/retained.rs"]
mod retained;

fn command(project: &TestProject, state: &Path) -> assert_cmd::Command {
    public_pending::command(project.dir(), project.dir(), state)
}

fn report(project: &TestProject, state: &Path, args: &[&str]) -> Value {
    public_pending::report(project.dir(), project.dir(), state, args)
}

fn pending(project: &TestProject, state: &Path, kind: &str) -> Value {
    public_pending::single(project.dir(), project.dir(), state, kind)
}

#[test]
fn journey_pending_build_uses_public_keys_and_resumes_verified_renamed_bytes() {
    let project = fixture("pending-public");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let before = snapshot(project.dir());
    command(&project, &state)
        .args(["--dry-run", "build", "modrinth"])
        .assert()
        .success();
    assert_eq!(snapshot(project.dir()), before);
    assert!(!state.exists());
    command(&project, &state)
        .args(["build", "modrinth"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), before);

    let operation = pending(&project, &state, "build");
    assert_ne!(operation["state"], "stale");
    assert_eq!(operation["inputs"].as_array().unwrap().len(), 1);
    let input = &operation["inputs"][0];
    assert_eq!(input["bytes"], 7);
    assert_eq!(
        input["digests"],
        json!([[
            "sha256",
            "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5"
        ]])
    );
    assert_eq!(input["verification"], "not supplied; verification required");
    assert_eq!(
        input["provider_selection"],
        json!({
            "provider":"curseforge", "project":"123", "selection":"456"
        })
    );
    let key = input["key"].as_str().unwrap();
    let id = operation["id"].as_str().unwrap();
    let supplied = host.path().join("renamed download.bin");
    // Same length as the selected payload: digest verification, not length or name.
    fs::write(&supplied, b"INVALID").unwrap();
    let association = format!("{key}={}", supplied.display());
    let retained = snapshot(&state);
    for args in [
        vec!["pending", "resume", id, "--file", &association],
        vec!["pending", "resume", id, "--file", "unknown=missing"],
        vec![
            "pending",
            "resume",
            id,
            "--file",
            &association,
            "--file",
            &association,
        ],
    ] {
        command(&project, &state).args(args).assert().failure();
        assert_eq!(snapshot(project.dir()), before);
        assert_eq!(snapshot(&state), retained);
        assert_eq!(pending(&project, &state, "build"), operation);
    }

    fs::write(&supplied, b"payload").unwrap();
    // Execute the next command exposed by pending, adding only the displayed key.
    let next: Vec<_> = operation["next"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap())
        .collect();
    command(&project, &state)
        .args(next)
        .args(["--file", &association])
        .assert()
        .success();
    let completed = report(&project, &state, &["pending", "list", "--json"]);
    assert_eq!(completed["operations"], json!([]));
    assert_eq!(fs::read(&supplied).unwrap(), b"payload");
    empack_tests::fixtures::restricted::assert_mrpack(
        &assert_dist_artifact_suffix(project.dir(), ".mrpack"),
        "pending-public",
    );
    for (name, bytes) in before {
        assert_eq!(fs::read(project.dir().join(name)).unwrap(), bytes);
    }
}

#[test]
fn journey_pending_stale_build_requires_explicit_discard_and_preserves_user_data() {
    let project = fixture("pending-stale");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    fs::create_dir(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.mrpack"), b"prior archive").unwrap();
    command(&project, &state)
        .args(["build", "modrinth"])
        .assert()
        .failure();
    let operation = pending(&project, &state, "build");
    let id = operation["id"].as_str().unwrap();
    let supplied = host.path().join("renamed.bin");
    fs::write(&supplied, b"payload").unwrap();
    let association = format!(
        "{}={}",
        operation["inputs"][0]["key"].as_str().unwrap(),
        supplied.display()
    );
    let author_path = project.dir().join("empack.yml");
    let author = fs::read_to_string(&author_path).unwrap();
    fs::write(&author_path, format!("# Edited after suspension\n{author}")).unwrap();
    let before = snapshot(project.dir());
    let retained = snapshot(&state);
    let stale = pending(&project, &state, "build");
    assert_eq!(stale["id"], id);
    assert_eq!(stale["state"], "stale");
    command(&project, &state)
        .args(["pending", "resume", id, "--file", &association])
        .assert()
        .failure()
        .stderr(predicates::str::contains("stale"));
    command(&project, &state)
        .args(["--dry-run", "pending", "discard", id])
        .assert()
        .success();
    assert_eq!(snapshot(&state), retained);
    assert_eq!(snapshot(project.dir()), before);
    command(&project, &state)
        .args(["pending", "discard", id])
        .assert()
        .success();
    assert_eq!(
        report(&project, &state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
    assert_eq!(snapshot(project.dir()), before);
    assert_eq!(fs::read(&supplied).unwrap(), b"payload");
    command(&project, &state)
        .args(["pending", "resume", id])
        .assert()
        .failure()
        .stderr(predicates::str::contains("No pending operation"));
    assert_eq!(snapshot(project.dir()), before);
}

/// Select by the public exact provider identity; the association key is opaque.
fn sync_input(operation: &Value, project: &str) -> Value {
    operation["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["provider_selection"]["project"] == project)
        .unwrap()
        .clone()
}

#[test]
fn journey_pending_sync_retains_partial_inputs_across_restart_and_cleanup() {
    sync_restart_and_cleanup(false);
}

#[test]
fn journey_pending_sync_legacy_continue_retains_partial_inputs_across_restart_and_cleanup() {
    sync_restart_and_cleanup(true);
}

fn sync_restart_and_cleanup(legacy: bool) {
    let project = empack_tests::fixtures::restricted::pair("pending-sync");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let original = snapshot(project.dir());
    command(&project, &state)
        .args(["sync", "--materialize"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), original);
    let operation = pending(&project, &state, "sync");
    assert_eq!(operation["requirements_complete"], true);
    assert_eq!(operation["inputs"].as_array().unwrap().len(), 2);
    let first = sync_input(&operation, "123");
    let second = sync_input(&operation, "124");
    assert_eq!(first["bytes"], 5);
    assert_eq!(second["bytes"], 6);
    assert_eq!(first["provider_selection"]["selection"], "456");
    assert_eq!(second["provider_selection"]["selection"], "457");
    assert_eq!(first["verification"], "not supplied; verification required");
    assert_eq!(
        second["verification"],
        "not supplied; verification required"
    );
    assert_eq!(
        first["digests"],
        json!([[
            "sha512",
            "7fdd80dbdded156323d36c459e5fd133a4d888c227320cfb7042be9feb35d7f07201e535697af914e69d6f46b2a88655c86c2371288052ccd4fa92058b01d3fd"
        ]])
    );
    assert_eq!(
        second["digests"],
        json!([[
            "sha512",
            "9381e9a67aa361751cea90178c094ad6133742163cbd14f146be5c3ee6606d4e8ab4bdd839e7c672baa6eb87e06f59b2d3a68ad0533f2a13ef6c0c5d8769216a"
        ]])
    );
    let id = operation["id"].as_str().unwrap();
    let first_path = host.path().join("renamed first download.bin");
    let association = format!(
        "{}={}",
        first["key"].as_str().unwrap(),
        first_path.display()
    );
    fs::write(&first_path, b"wrong").unwrap();
    let retained = snapshot(&state);
    command(&project, &state)
        .args(["--dry-run", "pending", "resume", id])
        .assert()
        .success();
    command(&project, &state)
        .args(["pending", "resume", id, "--file", &association])
        .assert()
        .failure();
    assert_eq!(snapshot(&state), retained);
    assert_eq!(snapshot(project.dir()), original);

    fs::write(&first_path, b"first").unwrap();
    command(&project, &state)
        .args(["pending", "resume", id, "--file", &association])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), original);
    let partial = pending(&project, &state, "sync");
    assert_eq!(
        sync_input(&partial, "123")["verification"],
        "retained; verification required"
    );
    assert_eq!(
        sync_input(&partial, "124")["verification"],
        "not supplied; verification required"
    );
    fs::remove_file(&first_path).unwrap();
    // Evict the ordinary cache too: later success must use the retained input,
    // not the download path or an incidental acquisition-cache hit.
    command(&project, &state)
        .args(["clean", "cache"])
        .assert()
        .success();
    assert!(
        snapshot(&state.with_extension("cache"))
            .values()
            .all(|bytes| bytes != b"first")
    );
    let protected = snapshot(&state);
    command(&project, &state)
        .args(["clean", "retained"])
        .assert()
        .success();
    assert_eq!(
        snapshot(&state),
        protected,
        "saved inputs remain protected from cleanup"
    );
    assert_eq!(pending(&project, &state, "sync"), partial);
    assert_eq!(snapshot(project.dir()), original);

    let second_path = host.path().join("second renamed.bin");
    fs::write(&second_path, b"second").unwrap();
    let second = sync_input(&partial, "124");
    let association = format!(
        "{}={}",
        second["key"].as_str().unwrap(),
        second_path.display()
    );
    let next: Vec<_> = partial["next"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap())
        .collect();
    let mut resume = command(&project, &state);
    if legacy {
        resume.args(["sync", "--continue"]);
    } else {
        resume.args(next);
    }
    resume.args(["--file", &association]).assert().success();
    assert_eq!(
        report(&project, &state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
    // Exact independent materialized inventory, not a planner-produced expectation.
    assert_eq!(
        snapshot(&project.dir().join("pack")),
        std::collections::BTreeMap::from([
            ("mods/one.jar".into(), b"first".to_vec()),
            ("mods/two.jar".into(), b"second".to_vec()),
        ])
    );
    let completed = snapshot(project.dir());
    for _ in 0..2 {
        command(&project, &state).arg("sync").assert().success();
        assert_eq!(snapshot(project.dir()), completed);
    }
    let retained = snapshot(&state);
    command(&project, &state)
        .args(["--dry-run", "clean", "retained"])
        .assert()
        .success();
    assert_eq!(snapshot(&state), retained);
    command(&project, &state)
        .args(["clean", "retained"])
        .assert()
        .success();
    let cleaned = snapshot(&state);
    assert_ne!(
        cleaned, retained,
        "completed continuation bytes can now be reclaimed"
    );
    command(&project, &state)
        .args(["clean", "retained"])
        .assert()
        .success();
    assert_eq!(snapshot(&state), cleaned, "cleanup converges");
    assert_eq!(snapshot(project.dir()), completed);
    assert_eq!(fs::read(&second_path).unwrap(), b"second");
    assert!(!first_path.exists());
    assert_eq!(
        fs::read(project.dir().join("empack.yml")).unwrap(),
        original[std::path::Path::new("empack.yml")]
    );
}

#[test]
fn journey_pending_sync_stale_capture_is_discarded_without_replaying_private_state() {
    for public_discard in [true, false] {
        let project = empack_tests::fixtures::restricted::pair("pending-sync-stale");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        command(&project, &state)
            .args(["sync", "--materialize"])
            .assert()
            .failure();
        let operation = pending(&project, &state, "sync");
        let id = operation["id"].as_str().unwrap();
        let author_path = project.dir().join("empack.yml");
        let author = fs::read_to_string(&author_path).unwrap();
        fs::write(author_path, format!("# Changed since suspension\n{author}")).unwrap();
        fs::create_dir_all(project.dir().join("pack/config")).unwrap();
        fs::write(
            project.dir().join("pack/config/user.txt"),
            b"retained author content",
        )
        .unwrap();
        let original = snapshot(project.dir());
        let retained = snapshot(&state);
        let stale = pending(&project, &state, "sync");
        assert_eq!(stale["id"], id);
        assert_eq!(stale["state"], "stale");
        command(&project, &state)
            .args(["pending", "resume", id])
            .assert()
            .failure()
            .stderr(predicates::str::contains("stale"));
        // Legacy inspection/cleanup remains a supported alias; no private record is injected.
        command(&project, &state)
            .args(["sync", "--continue", "--dry-run"])
            .assert()
            .failure();
        command(&project, &state)
            .args(["clean", "sync", "--dry-run"])
            .assert()
            .success();
        command(&project, &state)
            .args(["pending", "discard", id, "--dry-run"])
            .assert()
            .success();
        assert_eq!(snapshot(&state), retained);
        assert_eq!(snapshot(project.dir()), original);
        if public_discard {
            command(&project, &state)
                .args(["pending", "discard", id])
                .assert()
                .success();
        } else {
            command(&project, &state)
                .args(["clean", "sync"])
                .assert()
                .success();
        }
        assert_eq!(
            report(&project, &state, &["pending", "list", "--json"])["operations"],
            json!([])
        );
        command(&project, &state)
            .args(["clean", "sync"])
            .assert()
            .success();
        assert_eq!(snapshot(project.dir()), original);
    }
}

#[test]
fn journey_pending_build_corruption_is_visible_and_explicitly_discardable() {
    let project = fixture("corrupt-build");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    command(&project, &state)
        .args(["build", "modrinth"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    public_pending::corrupt_record(project.dir(), project.dir(), &state, "build");
}

#[test]
fn journey_pending_sync_corruption_is_visible_and_explicitly_discardable() {
    let project = empack_tests::fixtures::restricted::pair("corrupt-sync");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    command(&project, &state)
        .args(["sync", "--materialize"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    public_pending::corrupt_record(project.dir(), project.dir(), &state, "sync");
}

#[test]
fn journey_pending_missing_records_and_invalid_ids_do_not_create_storage() {
    let project = TestProject::new();
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    let before = snapshot(project.dir());
    assert_eq!(
        report(&project, &state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
    command(&project, &state)
        .args(["pending", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No saved pending operations"));
    for action in ["show", "resume", "discard"] {
        for (id, diagnostic) in [
            ("1234", "No pending operation matches"),
            ("", "hexadecimal identity"),
            ("not-hex", "hexadecimal identity"),
        ] {
            command(&project, &state)
                .args(["pending", action, id])
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(snapshot(project.dir()), before);
            assert!(!state.exists());
            assert!(!state.with_extension("cache").exists());
        }
    }
}

#[test]
fn journey_pending_sync_resume_requires_fresh_approval() {
    let project = empack_tests::fixtures::restricted::pair("sync-approval");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    command(&project, &state)
        .args(["sync", "--materialize"])
        .assert()
        .failure();
    let operation = pending(&project, &state, "sync");
    fs::create_dir_all(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"keep prior artifact").unwrap();
    let mut associations = Vec::new();
    for (id, bytes) in [("123", b"first".as_slice()), ("124", b"second".as_slice())] {
        let input = project.dir().join(format!("renamed-{id}.bin"));
        fs::write(&input, bytes).unwrap();
        associations.push(format!(
            "{}={}",
            sync_input(&operation, id)["key"].as_str().unwrap(),
            input.display()
        ));
    }
    approval::decline_resume(
        project.dir(),
        project.dir(),
        &state,
        &[
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &associations[0],
            "--file",
            &associations[1],
        ],
        "(?i)apply this remote content acquisition plan",
    );
    assert_eq!(pending(&project, &state, "sync"), operation);
}

#[test]
fn journey_pending_sync_reverifies_corrupt_and_missing_retained_bytes() {
    retained_payload_recovery("sync");
}

#[test]
fn journey_pending_build_reverifies_corrupt_and_missing_retained_bytes() {
    retained_payload_recovery("build");
}

fn retained_payload_recovery(kind: &str) {
    let project = empack_tests::fixtures::restricted::pair("pending-retained-fault");
    let host = tempfile::tempdir().unwrap();
    let state = host.path().join("state");
    // A different project shares this host store; its saved work must survive
    // corruption, re-suspension and successful completion of the selected project.
    let neighbor = fixture("pending-neighbor");
    command(&neighbor, &state)
        .args(["build", "modrinth"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    let neighbor_operation = pending(&neighbor, &state, "build");
    let neighbor_files = snapshot(neighbor.dir());
    let neighbor_state = snapshot(&state);
    fs::create_dir(project.dir().join("dist")).unwrap();
    fs::write(project.dir().join("dist/prior.zip"), b"prior artifact").unwrap();
    let original = snapshot(project.dir());
    let args = match kind {
        "sync" => ["sync", "--materialize"],
        "build" => ["build", "modrinth"],
        _ => unreachable!(),
    };
    command(&project, &state)
        .args(args)
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    let first_path = host.path().join("first renamed.bin");
    let second_path = host.path().join("second renamed.bin");
    fs::write(&first_path, b"first").unwrap();
    fs::write(&second_path, b"second").unwrap();
    let operation = pending(&project, &state, kind);
    command(&project, &state)
        .args([
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &format!(
                "{}={}",
                sync_input(&operation, "123")["key"].as_str().unwrap(),
                first_path.display()
            ),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    fs::remove_file(&first_path).unwrap();
    command(&project, &state)
        .args(["clean", "cache"])
        .assert()
        .success();
    let cache = state.with_extension("cache");
    assert!(!cache.exists() || snapshot(&cache).values().all(|b| b != b"first"));
    let partial = pending(&project, &state, kind);
    let backing = retained::backing(&state, b"first");
    let association = format!(
        "{}={}",
        sync_input(&partial, "124")["key"].as_str().unwrap(),
        second_path.display()
    );
    fs::write(&backing, b"WRONG").unwrap();
    let corrupted = snapshot(&state);
    command(&project, &state)
        .args([
            "pending",
            "resume",
            partial["id"].as_str().unwrap(),
            "--file",
            &association,
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("digest mismatch"));
    assert_eq!(snapshot(&state), corrupted);
    assert_eq!(snapshot(project.dir()), original);
    assert_eq!(pending(&project, &state, kind), partial);
    assert_eq!(pending(&neighbor, &state, "build"), neighbor_operation);
    assert_eq!(snapshot(neighbor.dir()), neighbor_files);
    for (path, bytes) in &neighbor_state {
        assert_eq!(&fs::read(state.join(path)).unwrap(), bytes);
    }

    // Missing backing is a fresh obligation, never evidence that it was verified.
    // The continuation may legitimately be replaced while retaining the second input.
    fs::remove_file(&backing).unwrap();
    command(&project, &state)
        .args([
            "pending",
            "resume",
            partial["id"].as_str().unwrap(),
            "--file",
            &association,
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("continuation was saved"));
    assert_eq!(snapshot(project.dir()), original);
    let missing = pending(&project, &state, kind);
    assert_eq!(
        sync_input(&missing, "123")["verification"],
        "not supplied; verification required"
    );
    assert_eq!(
        sync_input(&missing, "124")["verification"],
        "retained; verification required"
    );
    fs::remove_file(&second_path).unwrap();
    command(&project, &state)
        .args(["clean", "cache"])
        .assert()
        .success();
    fs::write(&first_path, b"first").unwrap();
    command(&project, &state)
        .args([
            "pending",
            "resume",
            missing["id"].as_str().unwrap(),
            "--file",
            &format!(
                "{}={}",
                sync_input(&missing, "123")["key"].as_str().unwrap(),
                first_path.display()
            ),
        ])
        .assert()
        .success();
    assert_eq!(
        report(&project, &state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
    if kind == "sync" {
        assert_eq!(
            snapshot(&project.dir().join("pack")),
            std::collections::BTreeMap::from([
                ("mods/one.jar".into(), b"first".to_vec()),
                ("mods/two.jar".into(), b"second".to_vec()),
            ])
        );
    } else {
        let members = empack_tests::harness::archive::zip_members(&assert_dist_artifact_suffix(
            project.dir(),
            ".mrpack",
        ));
        assert_eq!(
            members.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "modrinth.index.json",
                "overrides/mods/one.jar",
                "overrides/mods/two.jar"
            ]
        );
        assert_eq!(members["overrides/mods/one.jar"], b"first");
        assert_eq!(members["overrides/mods/two.jar"], b"second");
        assert_eq!(
            serde_json::from_slice::<Value>(&members["modrinth.index.json"]).unwrap(),
            json!({
                "formatVersion":1,"game":"minecraft","versionId":"1.0.0","name":"pending-retained-fault",
                "files":[],"dependencies":{"minecraft":"1.21.1","fabric-loader":"0.15.0"}
            })
        );
    }
    for (path, bytes) in original {
        assert_eq!(fs::read(project.dir().join(path)).unwrap(), bytes);
    }
    assert_eq!(fs::read(&first_path).unwrap(), b"first");
    assert!(!second_path.exists());
    assert_eq!(pending(&neighbor, &state, "build"), neighbor_operation);
    assert_eq!(snapshot(neighbor.dir()), neighbor_files);
    for (path, bytes) in neighbor_state {
        assert_eq!(fs::read(state.join(path)).unwrap(), bytes);
    }
}
