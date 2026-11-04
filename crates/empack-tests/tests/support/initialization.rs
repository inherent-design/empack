//! Independent author/scaffold expectations shared by terminal and unattended init.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

pub fn assert_new_project(root: &Path, name: &str, version: &str, authors: &[&str]) -> String {
    let author: Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join("empack.yml")).unwrap()).unwrap();
    let id = author["pack"]["id"].as_str().unwrap();
    let suffix = id.strip_prefix("pack-").expect("portable pack identity");
    assert_eq!(suffix.len(), 32);
    assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let mut expected: Value = serde_saphyr::from_str(include_str!(
        "../../../../tests/fixtures/initialization/empack.yml"
    ))
    .unwrap();
    expected["pack"]["id"] = json!(id);
    expected["pack"]["name"] = json!(name);
    expected["pack"]["version"] = json!(version);
    if !authors.is_empty() {
        expected["pack"]["authors"] = json!(authors);
    }
    assert_eq!(
        author, expected,
        "all authored semantics, including reference defaults"
    );

    let mut lock: Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join("empack.lock")).unwrap()).unwrap();
    let revision = lock
        .as_object_mut()
        .unwrap()
        .remove("intent-revision")
        .unwrap();
    let revision = revision.as_str().unwrap();
    assert_eq!(revision.len(), 64);
    assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        lock,
        json!({
            "schema": 2,
            "acceptable-versions": [],
            "coverage": {},
            "dependencies": {},
            "required-edges": {},
            "resolver": "empack-initialize-v0.5",
            "runtime": {"loader": "vanilla", "loader-version": null, "minecraft": "1.21.1"}
        })
    );

    fn inventory(root: &Path, at: &Path, files: &mut BTreeSet<String>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                inventory(root, &entry.path(), files);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut files = BTreeSet::new();
    inventory(root, root, &mut files);
    assert_eq!(
        files,
        [
            ".github/scripts/empack-ci.py",
            ".github/workflows/release.yml",
            ".github/workflows/validate.yml",
            ".gitignore",
            "empack.yml",
            "empack.lock",
            "templates/client/instance.cfg.template",
            "templates/server/game/server.properties.template",
        ]
        .map(str::to_owned)
        .into_iter()
        .collect()
    );
    // No bindings document is needed for an empty project. Consumer scripts are
    // recipe-owned at build time, not stale copies in the author template tree.
    let client = fs::read_to_string(root.join("templates/client/instance.cfg.template")).unwrap();
    for line in [
        "ConfigVersion=1.2",
        "InstanceType=OneSix",
        "name={{ini_quote NAME}}",
        "ExportAuthor={{ini_quote AUTHOR}}",
        "OverrideCommands=true",
        "PreLaunchCommand={{#if INSTANCE_PREPARE_COMMAND}}{{ini_quote INSTANCE_PREPARE_COMMAND}}{{/if}}",
        "WrapperCommand={{#if INSTANCE_WRAPPER_COMMAND}}{{ini_quote INSTANCE_WRAPPER_COMMAND}}{{/if}}",
    ] {
        assert!(
            client.lines().any(|actual| actual == line),
            "missing template setting {line}"
        );
    }
    let server =
        fs::read_to_string(root.join("templates/server/game/server.properties.template")).unwrap();
    for line in [
        "server-name={{properties_value NAME}}",
        "online-mode=true",
        "level-name=world",
    ] {
        assert!(
            server.lines().any(|actual| actual == line),
            "missing server setting {line}"
        );
    }
    id.to_owned()
}
