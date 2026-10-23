//! Component setup supplies an exact restricted lock; every recovery action uses
//! a fresh executable and only public pending output, never a private record key.
use empack_tests::{
    fixtures::restricted::fixture,
    harness::{
        archive::zip_members,
        process::{TestProject, assert_dist_artifact_suffix, offline_command},
    },
};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[path = "support/files.rs"]
mod files;
use files::snapshot;

fn command(project: &TestProject, state: &Path) -> assert_cmd::Command {
    let mut command = offline_command(project.dir());
    command.arg("--state-dir").arg(state).arg("--yes");
    command
}

fn report(project: &TestProject, state: &Path, args: &[&str]) -> Value {
    let before_project = snapshot(project.dir());
    let before_state = state.exists().then(|| snapshot(state));
    let output = command(project, state)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(snapshot(project.dir()), before_project);
    assert_eq!(state.exists().then(|| snapshot(state)), before_state);
    serde_json::from_slice(&output).unwrap()
}

fn pending(project: &TestProject, state: &Path) -> Value {
    let listed = report(project, state, &["pending", "list", "--json"]);
    assert_eq!(listed["schema"], 1);
    assert_eq!(listed["operations"].as_array().unwrap().len(), 1);
    let operation = listed["operations"][0].clone();
    let id = operation["id"].as_str().unwrap();
    let shown = report(project, state, &["pending", "show", id, "--json"]);
    assert_eq!(shown["operations"], json!([operation]));
    assert_eq!(operation["kind"], "build");
    operation
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

    let operation = pending(&project, &state);
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
        assert_eq!(pending(&project, &state), operation);
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
    let members = zip_members(&assert_dist_artifact_suffix(project.dir(), ".mrpack"));
    assert_eq!(
        members.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["modrinth.index.json", "overrides/resourcepacks/manual.zip"]
    );
    assert_eq!(members["overrides/resourcepacks/manual.zip"], b"payload");
    let index: Value = serde_json::from_slice(&members["modrinth.index.json"]).unwrap();
    assert_eq!(index["formatVersion"], 1);
    assert_eq!(index["game"], "minecraft");
    assert_eq!(index["files"], json!([]));
    assert_eq!(index["dependencies"]["minecraft"], "1.21.1");
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
    let operation = pending(&project, &state);
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
    let stale = pending(&project, &state);
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
