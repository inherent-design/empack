//! Public pending commands with separate invocation, destination, and host storage.
use empack_tests::harness::process::offline_command;
use empack_tests::harness::workspace::snapshot;
use serde_json::{Value, json};
use std::path::Path;

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
        .map(|path| path.exists().then(|| snapshot(path)))
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
            path.exists().then(|| snapshot(path)),
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
    let (directory, schema) = match kind {
        "build" => ("pending-builds", 5_u32),
        "import" => ("pending-imports", 1),
        "sync" => ("pending-sync", 2),
        "instance" => ("pending-instances", 3),
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
    let wire: Value = serde_json::from_slice(&original_bytes).unwrap();
    assert_eq!(wire["schema"], schema);
    let mut faults = vec![
        (b"{malformed".to_vec(), "Invalid pending record"),
        (
            br#"{"schema":4294967295}"#.to_vec(),
            "Unsupported saved record schema",
        ),
    ];
    for schema in (0..schema).chain(std::iter::once(u32::MAX)) {
        let mut rejected = wire.clone();
        rejected["schema"] = schema.into();
        faults.push((
            serde_json::to_vec(&rejected).unwrap(),
            "Unsupported saved record schema",
        ));
    }
    for (bytes, diagnostic) in faults {
        fs::write(record, bytes).unwrap();
        let preserved = snapshot(state);
        let author = snapshot(invocation);
        let cache = state.with_extension("cache");
        let cached = cache.exists().then(|| snapshot(&cache));
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
                text.contains("cannot read saved work") && text.contains(diagnostic),
                "{text}"
            );
            assert!(!text.contains("Next: empack pending resume"), "{text}");
            assert_eq!(snapshot(state), preserved);
            assert_eq!(snapshot(invocation), author);
            assert_eq!(cache.exists().then(|| snapshot(&cache)), cached);
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
        assert_eq!(snapshot(state), preserved);
        assert_eq!(snapshot(invocation), author);
        assert_eq!(cache.exists().then(|| snapshot(&cache)), cached);
        command(invocation, root, state)
            .args(["--dry-run", "pending", "discard", id])
            .assert()
            .success();
        assert_eq!(snapshot(state), preserved);
        assert_eq!(snapshot(invocation), author);
        assert_eq!(cache.exists().then(|| snapshot(&cache)), cached);
        // Restore only the test-owned fault, then confirm the intact record's
        // public identity and obligations without resuming or authorizing it.
        fs::write(record, &original_bytes).unwrap();
        let restored = snapshot(state);
        assert_eq!(single(invocation, root, state, kind), original);
        assert_eq!(snapshot(state), restored);
        assert_eq!(snapshot(invocation), author);
        assert_eq!(cache.exists().then(|| snapshot(&cache)), cached);
    }
    fs::write(record, b"{malformed").unwrap();
    let broken = single(invocation, root, state, kind);
    let id = broken["id"].as_str().unwrap();
    let mut expected = snapshot(state).files;
    expected
        .remove(record.strip_prefix(state).unwrap())
        .unwrap();
    let author = snapshot(invocation).files;
    command(invocation, root, state)
        .args(["pending", "discard", id])
        .assert()
        .success();
    assert_eq!(snapshot(state).files, expected);
    assert_eq!(snapshot(invocation).files, author);
    assert_eq!(
        report(invocation, root, state, &["pending", "list", "--json"])["operations"],
        json!([])
    );
}
