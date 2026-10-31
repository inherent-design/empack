//! Public pending commands with separate invocation, destination, and host storage.
use empack_tests::harness::process::offline_command;
use serde_json::{Value, json};
use std::path::Path;

use crate::files;

pub fn command(invocation: &Path, root: &Path, state: &Path) -> assert_cmd::Command {
    let mut command = offline_command(invocation);
    command
        .arg("--workdir")
        .arg(root)
        .arg("--state-dir")
        .arg(state)
        .arg("--cache-dir")
        .arg(state.with_extension("cache"))
        .arg("--yes");
    command
}

pub fn report(invocation: &Path, root: &Path, state: &Path, args: &[&str]) -> Value {
    let cache = state.with_extension("cache");
    let paths = [invocation, root, state, &cache];
    let before: Vec<_> = paths
        .iter()
        .map(|path| path.exists().then(|| files::snapshot(path)))
        .collect();
    let output = command(invocation, root, state)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    for (path, expected) in paths.into_iter().zip(before) {
        assert_eq!(
            path.exists().then(|| files::snapshot(path)),
            expected,
            "inspection changed {}",
            path.display()
        );
    }
    serde_json::from_slice(&output).unwrap()
}

pub fn single(invocation: &Path, root: &Path, state: &Path, kind: &str) -> Value {
    let listed = report(invocation, root, state, &["pending", "list", "--json"]);
    assert_eq!(listed["schema"], 1);
    assert_eq!(listed["operations"].as_array().unwrap().len(), 1);
    let operation = listed["operations"][0].clone();
    let id = operation["id"].as_str().unwrap();
    let shown = report(invocation, root, state, &["pending", "show", id, "--json"]);
    assert_eq!(shown["operations"], json!([operation]));
    assert_eq!(operation["kind"], kind);
    operation
}

/// Inject corruption into one native record, without interpreting its private payload.
/// Every subsequent inspection, refusal and cleanup uses the public identity.
pub fn corrupt_record(invocation: &Path, root: &Path, state: &Path, kind: &str) {
    use std::fs;
    let original = single(invocation, root, state, kind);
    let directory = match kind {
        "build" => "pending-builds",
        "import" => "pending-imports",
        "sync" => "pending-sync",
        "instance" => "pending-instances",
        _ => panic!("unsupported fixture family"),
    };
    let records: Vec<_> = fs::read_dir(state.join(directory))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    assert_eq!(records.len(), 1, "fault must target one existing record");
    let record = &records[0];
    let original_bytes = fs::read(record).unwrap();
    fs::write(state.join("unrelated-download.bin"), b"keep user bytes").unwrap();
    for (bytes, diagnostic) in [
        (b"{malformed".as_slice(), "Invalid pending record"),
        (
            br#"{"schema":4294967295}"#.as_slice(),
            "Unsupported saved record schema",
        ),
    ] {
        fs::write(record, bytes).unwrap();
        let preserved = files::snapshot(state);
        let author = files::snapshot(invocation);
        let cache = state.with_extension("cache");
        let cached = cache.exists().then(|| files::snapshot(&cache));
        let broken = single(invocation, root, state, kind);
        assert_ne!(broken["id"], original["id"]);
        assert_eq!(broken["state"], "unavailable");
        assert_eq!(broken["requirements_complete"], false);
        assert_eq!(broken["inputs"], json!([]));
        assert!(broken["diagnostic"].as_str().unwrap().contains(diagnostic));
        let id = broken["id"].as_str().unwrap();
        for args in [vec!["pending", "list"], vec!["pending", "show", id]] {
            let output = command(invocation, root, state)
                .args(args)
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let text = String::from_utf8(output).unwrap();
            assert!(
                text.contains("unavailable") && text.contains(diagnostic),
                "{text}"
            );
            assert!(text.contains("Metadata only"), "{text}");
            assert_eq!(files::snapshot(state), preserved);
            assert_eq!(files::snapshot(invocation), author);
            assert_eq!(cache.exists().then(|| files::snapshot(&cache)), cached);
        }
        command(invocation, root, state)
            .args(["pending", "show", original["id"].as_str().unwrap()])
            .assert()
            .failure()
            .stderr(predicates::str::contains("No pending operation matches"));
        command(invocation, root, state)
            .args(["pending", "resume", id])
            .assert()
            .failure();
        assert_eq!(files::snapshot(state), preserved);
        assert_eq!(files::snapshot(invocation), author);
        command(invocation, root, state)
            .args(["--dry-run", "pending", "discard", id])
            .assert()
            .success();
        assert_eq!(files::snapshot(state), preserved);
        assert_eq!(files::snapshot(invocation), author);
        // Recover fixture bytes only to exercise the next fault independently.
        fs::write(record, &original_bytes).unwrap();
    }
    fs::write(record, b"{malformed").unwrap();
    let broken = single(invocation, root, state, kind);
    let id = broken["id"].as_str().unwrap();
    let mut expected = files::snapshot(state);
    expected
        .remove(record.strip_prefix(state).unwrap())
        .unwrap();
    let author = files::snapshot(invocation);
    command(invocation, root, state)
        .args(["pending", "discard", id])
        .assert()
        .success();
    assert_eq!(files::snapshot(state), expected);
    assert_eq!(files::snapshot(invocation), author);
    assert_eq!(
        report(invocation, root, state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
}
