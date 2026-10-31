//! Actual provider/runtime routes; frozen host cases separately prove controlled changes.
use super::{author_snapshot, run, snapshot};
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, read_project},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const AUTHOR: &str = r#"# Keep the exact pin and local policy across runtime changes.
schema: 5
pack: {id: live.migration, name: Migration Fixture, version: '1', authors: [Fixture]}
runtime:
  minecraft: '1.21.1'
  loader:
    kind: fabric
    pin: '0.16.14'
  java: 21
dependencies:
  renderer: {source: 'modrinth:AANobbMI', side: client}
  history:
    source: modrinth:8qkXwOnk
    pin: 3siYJiWG
    side: client
  settings: {path: vendor/settings.toml, type: config}
build:
  default: [native-client, native-server]
  profiles:
    native-client: {target: empack, delivery: bundled, environment: client}
    native-server: {target: empack, delivery: bundled, environment: server}
    prism: {target: prism, delivery: bundled, environment: client}
"#;
const BINDINGS: &str = r#"# Stable roles, independent of catalog filenames.
schema: 1
bindings:
  settings:
    placement:
      - destination: config/settings.toml
        layer: common
        environment: {client: required, server: required}
  renderer:
    placement:
      files:
        primary:
          - destination: mods/renderer.jar
            layer: common
            environment: {client: required, server: unsupported}
  history:
    placement:
      files:
        primary:
          - destination: mods/history.jar
            layer: common
            environment: {client: required, server: unsupported}
"#;
const SETTINGS: &[u8] = b"# Local author policy.\nenabled = true\n";

fn native(root: &Path, side: &str, expected: &BTreeMap<String, Vec<u8>>) {
    let members = zip_members(&assert_dist_artifact_suffix(
        root,
        &format!("empack-bundled-{side}.empack"),
    ));
    let release: Value = serde_json::from_slice(&members["release.json"]).unwrap();
    assert_eq!(release["runtime"]["minecraft"], "1.21.2");
    assert_eq!(release["runtime"]["java_major"], 21);
    let files = release["files"].as_array().unwrap();
    assert_eq!(files.len(), expected.len());
    assert_eq!(
        files
            .iter()
            .map(|file| file["destination"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        expected.keys().map(String::as_str).collect()
    );
    let mut names = BTreeSet::from(["release.json".to_owned()]);
    for file in files {
        let path = file["destination"].as_str().unwrap();
        let bytes = &expected[path];
        let hash = empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(bytes).into()).hex();
        assert_eq!(file["sha256"], hash);
        assert_eq!(file["bytes"], bytes.len());
        let name = format!("assets/{hash}");
        assert_eq!(&members[&name], bytes);
        names.insert(name);
    }
    assert_eq!(members.keys().cloned().collect::<BTreeSet<_>>(), names);
}

pub(super) fn journey() {
    let project = TestProject::new();
    let root = project.dir();
    fs::create_dir_all(root.join("vendor")).unwrap();
    fs::create_dir_all(root.join("templates/client")).unwrap();
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("empack.yml"), AUTHOR).unwrap();
    fs::write(root.join("empack.bindings.yml"), BINDINGS).unwrap();
    fs::write(root.join("vendor/settings.toml"), SETTINGS).unwrap();
    fs::write(root.join("dist/prior.zip"), b"previous published artifact").unwrap();
    let profile = json!({"formatVersion":1,"components":[
        {"uid":"net.minecraft","version":"1.21.1","important":true},
        {"uid":"net.fabricmc.fabric-loader","version":"0.16.14","important":true}
    ]});
    fs::write(
        root.join("templates/client/mmc-pack.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    run(&project, &["--yes", "sync", "--materialize"]);
    let old = read_project(root);
    assert_eq!(old.intent().roots.len(), 3);
    let before = author_snapshot(root);
    let args = [
        "migrate",
        "--minecraft",
        "1.21.2",
        "--loader",
        "fabric",
        "--loader-version",
        "0.16.14",
        "--materialize",
    ];
    let mut plan = args.to_vec();
    plan.extend(["--plan", "--json"]);
    let output = run(&project, &plan);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], 2);
    assert_eq!(report["current_runtime"]["minecraft"], "1.21.1");
    assert_eq!(report["proposed_runtime"]["minecraft"], "1.21.2");
    assert_eq!(author_snapshot(root), before);
    for mode in ["--dry-run", "--yes"] {
        let mut command = args.to_vec();
        command.push(mode);
        let output = run(&project, &command);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("Migration candidate: 1.21.1 / fabric 0.16.14"),
            "{text}"
        );
        assert!(text.contains("-> 1.21.2 / fabric 0.16.14"), "{text}");
        for row in report["dependencies"].as_array().unwrap() {
            assert!(
                text.contains(&format!(
                    "{}: {}, {}",
                    row["key"].as_str().unwrap(),
                    row["change"].as_str().unwrap(),
                    row["requirement"].as_str().unwrap()
                )),
                "{text}"
            );
            for stage in ["before", "after"] {
                let pin = &row[stage]["selection"];
                if pin.is_object() {
                    assert!(
                        text.contains(&format!(
                            "{}:{}@{}",
                            pin["provider"].as_str().unwrap(),
                            pin["project"].as_str().unwrap(),
                            pin["selection"].as_str().unwrap()
                        )),
                        "{text}"
                    );
                }
            }
        }
        if mode == "--dry-run" {
            assert_eq!(author_snapshot(root), before);
        }
    }
    let next = read_project(root);
    assert_eq!(next.intent().roots, old.intent().roots);
    assert_eq!(next.intent().distribution, old.intent().distribution);
    assert_eq!(next.lock().runtime.minecraft.as_str(), "1.21.2");
    let rows = report["dependencies"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let row = |key: &str| rows.iter().find(|row| row["key"] == key).unwrap();
    assert_eq!(
        row("history")["before"]["selection"]["selection"],
        "3siYJiWG"
    );
    assert_eq!(
        row("history")["after"]["selection"]["selection"],
        "3siYJiWG"
    );
    assert_eq!(row("settings")["manual_compatibility"], true);
    assert_eq!(row("settings")["change"], "retained");
    assert_ne!(
        row("renderer")["before"]["selection"],
        row("renderer")["after"]["selection"]
    );
    let mut client = BTreeMap::from([("config/settings.toml".to_owned(), SETTINGS.to_vec())]);
    for key in ["renderer", "history"] {
        let destination = format!("mods/{key}.jar");
        assert_eq!(
            row(key)["after"]["placements"][0]["destination"],
            destination
        );
        let bytes = fs::read(root.join("pack").join(&destination)).unwrap();
        assert_eq!(
            row(key)["after"]["files"][0]["expected"]["digests"]["sha512"],
            empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&bytes).into()).hex()
        );
        client.insert(destination, bytes);
    }
    assert_eq!(
        fs::read(root.join("vendor/settings.toml")).unwrap(),
        SETTINGS
    );
    assert_eq!(
        fs::read_to_string(root.join("empack.bindings.yml")).unwrap(),
        BINDINGS
    );
    assert!(
        fs::read_to_string(root.join("empack.yml"))
            .unwrap()
            .starts_with("# Keep the exact pin")
    );
    for (path, bytes) in &before {
        if path == Path::new("empack.yml")
            || path == Path::new("empack.lock")
            || path.starts_with("pack")
        {
            continue;
        }
        assert_eq!(
            &fs::read(root.join(path)).unwrap(),
            bytes,
            "unrelated {path:?}"
        );
    }
    assert_eq!(
        snapshot(&root.join("pack"), &[]),
        client
            .iter()
            .map(|(path, bytes)| (std::path::PathBuf::from(path), bytes.clone()))
            .collect()
    );
    run(&project, &["--yes", "build"]);
    native(root, "client", &client);
    native(
        root,
        "server",
        &BTreeMap::from([("config/settings.toml".to_owned(), SETTINGS.to_vec())]),
    );
    let before_refusal = author_snapshot(root);
    let output = super::execute(&project, &["--yes", "build", "--profile", "prism"], None);
    assert!(!output.status.success(), "stale Prism profile accepted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("runtime"),
        "{output:?}"
    );
    assert_eq!(author_snapshot(root), before_refusal);
    fs::remove_file(root.join("templates/client/mmc-pack.json")).unwrap();
    run(&project, &["--yes", "build", "--profile", "prism"]);
    let members = zip_members(&assert_dist_artifact_suffix(root, "prism-bundled.zip"));
    let profile: Value = serde_json::from_slice(&members["mmc-pack.json"]).unwrap();
    assert!(
        profile["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["uid"] == "net.minecraft" && c["version"] == "1.21.2")
    );
    for (path, bytes) in &client {
        assert_eq!(&members[&format!(".minecraft/{path}")], bytes);
    }
    // A deliberate different-project replacement is allowed; unlike a redundant
    // canonical-ID request it intentionally resets this root's exact-pin policy.
    let before_replace = author_snapshot(root);
    let mut replace = args.to_vec();
    replace.extend(["--replace", "history=modrinth:gvQqBUqZ"]);
    let mut preview = replace.clone();
    preview.extend(["--plan", "--json"]);
    let output = run(&project, &preview);
    let replacement: Value = serde_json::from_slice(&output.stdout).unwrap();
    let changed = replacement["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "history")
        .unwrap();
    assert_eq!(changed["source_replaced"], true);
    assert_eq!(changed["before"]["selection"]["project"], "8qkXwOnk");
    assert_eq!(changed["after"]["selection"]["project"], "gvQqBUqZ");
    assert_eq!(author_snapshot(root), before_replace);
    replace.push("--yes");
    run(&project, &replace);
    let replaced = read_project(root);
    let history = empack_core::model::DependencyKey::parse("history").unwrap();
    assert_eq!(
        replaced.intent().roots[&history].selection,
        empack_core::model::DependencySelection::Automatic
    );
    assert_eq!(
        replaced.lock().dependencies[&history]
            .selected
            .as_ref()
            .unwrap()
            .project
            .to_string(),
        "gvQqBUqZ"
    );
    for key in ["renderer", "settings"] {
        let key = empack_core::model::DependencyKey::parse(key).unwrap();
        assert_eq!(replaced.intent().roots[&key], next.intent().roots[&key]);
        assert_eq!(
            replaced.lock().dependencies[&key],
            next.lock().dependencies[&key]
        );
    }
    let replacement_bytes = fs::read(root.join("pack/mods/history.jar")).unwrap();
    assert_eq!(
        changed["after"]["files"][0]["expected"]["digests"]["sha512"],
        empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&replacement_bytes).into())
            .hex()
    );
    client.insert("mods/history.jar".into(), replacement_bytes);
    let mut expected = before_replace;
    for path in ["empack.yml", "empack.lock", "pack/mods/history.jar"] {
        expected.insert(path.into(), fs::read(root.join(path)).unwrap());
    }
    assert_eq!(
        author_snapshot(root),
        expected,
        "replacement changed unselected files"
    );
    run(&project, &["--yes", "build"]);
    native(root, "client", &client);
    native(
        root,
        "server",
        &BTreeMap::from([("config/settings.toml".to_owned(), SETTINGS.to_vec())]),
    );
    let stable = author_snapshot(root);
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(author_snapshot(root), stable);
    }
}
