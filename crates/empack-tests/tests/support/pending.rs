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
