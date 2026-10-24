//! Public commands retain explicit content kinds, side choices and world grouping.
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[path = "support/content_kinds.rs"]
mod content_kinds;
#[path = "support/files.rs"]
mod files;

const AUTHOR: &str = r#"# Literal routing and consumer profiles.
schema: 5
pack: {id: content.kinds, name: Content Kinds, version: '1', authors: [Fixture]}
runtime: {minecraft: '1.21.1', loader: vanilla, java: 21}
layout: {data-pack: config/paxi/datapacks, world: saves}
dependencies: {}
build:
  default: [client, native-client, native-server]
  profiles:
    client: {target: prism, delivery: bundled, environment: client}
    native-client: {target: empack, delivery: bundled, environment: client}
    native-server: {target: empack, delivery: bundled, environment: server}
"#;

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn yaml(path: &Path) -> Value {
    serde_saphyr::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
fn save_yaml(path: &Path, value: &Value) {
    fs::write(path, serde_saphyr::to_string(value).unwrap()).unwrap();
}

fn native_inventory(root: &Path, side: &str, expected: &BTreeMap<String, Vec<u8>>) {
    let archive = zip_members(&assert_dist_artifact_suffix(
        root,
        &format!("empack-bundled-{side}.empack"),
    ));
    let descriptor: Value = serde_json::from_slice(&archive["release.json"]).unwrap();
    let entries = descriptor["files"].as_array().unwrap();
    assert_eq!(entries.len(), expected.len());
    assert_eq!(
        entries
            .iter()
            .map(|f| f["destination"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        expected.keys().map(String::as_str).collect()
    );
    let mut members = BTreeSet::from(["release.json".to_owned()]);
    for entry in entries {
        let path = entry["destination"].as_str().unwrap();
        let bytes = &expected[path];
        let hash = empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(bytes).into()).hex();
        assert_eq!(entry["sha256"], hash);
        assert_eq!(entry["bytes"], bytes.len());
        let member = format!("assets/{hash}");
        assert_eq!(&archive[&member], bytes);
        members.insert(member);
        if path.starts_with("saves/adventure/") {
            assert_eq!(entry["world_root"], "saves/adventure");
            assert_eq!(entry["policy"], "seed");
        } else {
            assert!(entry.get("world_root").is_none());
        }
        assert_eq!(
            entry[if side == "client" { "server" } else { "client" }],
            json!({"kind":"unsupported"})
        );
    }
    assert_eq!(archive.keys().cloned().collect::<BTreeSet<_>>(), members);
    let choices = if side == "client" {
        json!([{"key":"art","alternatives":["disabled","enabled"],"default":"disabled","description":null}])
    } else {
        json!([])
    };
    assert_eq!(descriptor["choices"], choices);
}
fn prism_inventory(root: &Path, expected: &BTreeMap<String, Vec<u8>>) {
    let archive = zip_members(&assert_dist_artifact_suffix(root, "prism-bundled.zip"));
    let mut names = BTreeSet::from(["instance.cfg".to_owned(), "mmc-pack.json".to_owned()]);
    for (path, bytes) in expected {
        let member = format!(".minecraft/{path}");
        assert_eq!(&archive[&member], bytes);
        names.insert(member);
    }
    assert_eq!(archive.keys().cloned().collect::<BTreeSet<_>>(), names);
}

#[test]
fn journey_cli_content_kinds_preserve_sides_choices_and_world_groups() {
    let project = TestProject::new();
    let root = project.dir();
    let host = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = offline_command(root);
        command
            .env("EMPACK_STATE_DIR", host.path().join("state"))
            .env("EMPACK_CACHE_DIR", host.path().join("cache"))
            .env("TMPDIR", host.path())
            .env("TMP", host.path())
            .env("TEMP", host.path());
        command
    };
    write(root, "empack.yml", AUTHOR.as_bytes());
    write(root, "dist/prior.zip", b"prior artifact");
    command().args(["--yes", "sync"]).assert().success();
    let mut payloads = BTreeMap::new();
    for case in content_kinds::CASES {
        let source = format!("vendor sources/{}", case.filename);
        let bytes = content_kinds::archive(case.members);
        write(root, &source, &bytes);
        if case.kind != "mod" {
            let before = files::snapshot(root);
            command()
                .args(["--yes", "add", &source])
                .assert()
                .failure()
                .stderr(predicates::str::contains("ambiguous; supply --type"));
            assert_eq!(files::snapshot(root), before);
        }
        let before = files::snapshot(root);
        command()
            .args(["--dry-run", "add", "--type", case.kind, &source])
            .assert()
            .success();
        assert_eq!(files::snapshot(root), before);
        command()
            .args(["--yes", "add", "--type", case.kind, &source])
            .assert()
            .success();
        assert_eq!(
            fs::read(root.join("pack").join(case.destination)).unwrap(),
            bytes
        );
        assert_eq!(fs::read(root.join(source)).unwrap(), bytes);
        payloads.insert(case.destination.to_owned(), bytes);
    }
    let world_source = "vendor sources/adventure.zip";
    for bad in [
        vec![
            ("First/level.dat", b"first".as_slice()),
            ("Second/level.dat", b"second".as_slice()),
        ],
        vec![
            ("Wrapped/level.dat", b"world".as_slice()),
            ("outside.txt", b"outside".as_slice()),
        ],
    ] {
        write(root, world_source, &content_kinds::archive(&bad));
        let before = files::snapshot(root);
        command()
            .args(["--yes", "add", "--type", "world", world_source])
            .assert()
            .failure();
        assert_eq!(files::snapshot(root), before);
    }
    let world = [
        ("Wrapped/level.dat", b"world".as_slice()),
        ("Wrapped/region/r.0.0.mca", b"region".as_slice()),
    ];
    write(root, world_source, &content_kinds::archive(&world));
    let before = files::snapshot(root);
    command()
        .args(["--dry-run", "add", "--type", "world", world_source])
        .assert()
        .success();
    assert_eq!(files::snapshot(root), before);
    command()
        .args(["--yes", "add", "--type", "world", world_source])
        .assert()
        .success();
    payloads.insert("saves/adventure/level.dat".into(), b"world".to_vec());
    payloads.insert(
        "saves/adventure/region/r.0.0.mca".into(),
        b"region".to_vec(),
    );
    for (path, bytes) in &payloads {
        assert_eq!(&fs::read(root.join("pack").join(path)).unwrap(), bytes);
    }
    let lock = yaml(&root.join("empack.lock"));
    assert_eq!(lock["dependencies"].as_object().unwrap().len(), 5);
    assert_eq!(
        lock["dependencies"]["adventure"]["files"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Author choices are ordinary document edits; do not use production encoders as the oracle.
    let mut author = yaml(&root.join("empack.yml"));
    let mut bindings = yaml(&root.join("empack.bindings.yml"));
    for (key, environment) in [
        (
            "resources",
            json!({"client":{"optional":"art","default-enabled":false},"server":"unsupported"}),
        ),
        ("data", json!({"client":"unsupported","server":"required"})),
    ] {
        let dependency = author["dependencies"][key].as_object_mut().unwrap();
        for field in ["side", "optional"] {
            dependency.remove(field);
        }
        dependency.insert("environment".into(), environment.clone());
        for placement in bindings["bindings"][key]["placement"]
            .as_array_mut()
            .unwrap()
        {
            placement["environment"] = environment.clone();
        }
    }
    save_yaml(&root.join("empack.yml"), &author);
    save_yaml(&root.join("empack.bindings.yml"), &bindings);
    command().args(["--yes", "sync"]).assert().success();
    let stable = files::snapshot(root);
    for _ in 0..2 {
        command().args(["--yes", "sync"]).assert().success();
        assert_eq!(files::snapshot(root), stable);
    }
    let client: BTreeMap<_, _> = payloads
        .iter()
        .filter(|(path, _)| !path.starts_with("config/"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let server: BTreeMap<_, _> = payloads
        .iter()
        .filter(|(path, _)| {
            !path.starts_with("resourcepacks/") && !path.starts_with("shaderpacks/")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for enabled in [false, true] {
        command()
            .args([
                "--yes",
                "build",
                "--optional",
                if enabled { "art=true" } else { "art=false" },
            ])
            .assert()
            .success();
        native_inventory(root, "client", &client);
        native_inventory(root, "server", &server);
        let mut selected = client.clone();
        if !enabled {
            selected.remove("resourcepacks/resources.zip");
        }
        prism_inventory(root, &selected);
    }
    for args in [
        vec!["--yes", "build"],
        vec!["--yes", "build", "--optional", "typo=true"],
    ] {
        let before = files::snapshot(root);
        command()
            .args(&args)
            .assert()
            .failure()
            .stderr(predicates::str::contains("choice"));
        assert_eq!(files::snapshot(root), before);
    }
    // Unselected optional input is not needed; selected input must supply its exact bytes.
    fs::remove_file(root.join("vendor sources/resources.zip")).unwrap();
    fs::remove_file(root.join("pack/resourcepacks/resources.zip")).unwrap();
    fs::remove_dir_all(host.path().join("cache")).unwrap();
    command()
        .args([
            "--yes",
            "build",
            "--profile",
            "client",
            "--optional-defaults",
        ])
        .assert()
        .success();
    let mut selected = client;
    selected.remove("resourcepacks/resources.zip");
    prism_inventory(root, &selected);
    let before = files::snapshot(root);
    command()
        .args([
            "--yes",
            "build",
            "--profile",
            "client",
            "--optional",
            "art=true",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("resources.zip"));
    assert_eq!(files::snapshot(root), before);
}
