//! Native snapshot exports and public installation, with literal content oracles.
use crate::files::snapshot;
use empack_tests::harness::{
    archive::{Member, distribution_inventory},
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
    workspace::snapshot as observe_workspace,
};
use predicates::str::contains;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const AUTHOR: &str = r#"# Keep the publisher's source and durable choice identity.
schema: 5
pack: {id: native.fixture, name: Native fixture, version: '1'}
runtime: {minecraft: '1.21.1', loader: vanilla, java: 21}
layout: {config: config}
dependencies:
  extra:
    path: vendor/extra.txt
    type: config
    side: both
    optional: {choice: extras, default: false}
build:
  default: [native]
  profiles:
    native: {target: empack, delivery: references, environment: both, updates: snapshot, format: zip}
"#;
const CONTENT: [(&str, &str, &[u8]); 5] = [
    ("managed.txt", "common", b"common bytes\n"),
    ("managed.txt", "client", b"client bytes\n"),
    ("managed.txt", "server", b"server bytes\n"),
    ("config/settings.txt", "common", b"default configuration\n"),
    ("config/extra.txt", "common", b"optional configuration\n"),
];
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn command(root: &Path, host: &Path) -> assert_cmd::Command {
    let mut command = offline_command(root);
    command
        .arg("--state-dir")
        .arg(host.join("state"))
        .arg("--cache-dir")
        .arg(host.join("cache"));
    command
}
fn fixture() -> (TestProject, tempfile::TempDir) {
    let project = TestProject::new();
    let host = tempfile::tempdir().unwrap();
    put(project.dir(), "empack.yml", AUTHOR.as_bytes());
    for (name, layer, bytes) in CONTENT {
        let path = if name == "config/extra.txt" {
            "vendor/extra.txt".to_owned()
        } else if layer == "common" {
            format!("pack/{name}")
        } else {
            format!("overrides/{layer}/{name}")
        };
        put(project.dir(), &path, bytes);
    }
    put(project.dir(), "dist/prior.zip", b"prior artifact");
    command(project.dir(), host.path())
        .args(["--yes", "sync"])
        .assert()
        .success();
    (project, host)
}

fn inventory(members: &BTreeMap<String, Member>) -> String {
    let payload = members["release.json"].small.as_ref().unwrap();
    let descriptor: Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(descriptor["schema"], 1);
    assert_eq!(descriptor["pack"], "native.fixture");
    assert_eq!(descriptor["version"], "1");
    assert_eq!(descriptor["minimum_engine"], ">=0.8.0-beta");
    assert_eq!(descriptor["require_subscription"], false);
    assert!(descriptor.get("server_launch").is_none());
    assert_eq!(
        descriptor["runtime"],
        json!({"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java_major":21})
    );
    assert_eq!(
        descriptor["choices"],
        json!([{"key":"extras","alternatives":["disabled","enabled"],"default":"disabled","description":null}])
    );
    let files = descriptor["files"].as_array().unwrap();
    assert_eq!(files.len(), CONTENT.len());
    assert_eq!(
        files
            .iter()
            .map(|f| f["key"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        CONTENT.len()
    );
    let mut names = BTreeSet::from(["release.json".to_owned()]);
    for (destination, layer, bytes) in CONTENT {
        let file = files
            .iter()
            .find(|f| f["destination"] == destination && f["layer"] == layer)
            .unwrap();
        let digest = hash(bytes);
        let path = format!("assets/{digest}");
        assert_eq!(file["sha256"], digest);
        assert_eq!(file["bytes"], bytes.len());
        assert_eq!(
            file["policy"],
            if destination.starts_with("config/") {
                "seed"
            } else {
                "managed"
            }
        );
        assert_eq!(file["readonly"], false);
        assert_eq!(file["executable"], false);
        assert_eq!(file["source"], json!({"kind":"asset","path":path}));
        assert!(file.get("asset").is_none());
        assert!(file.get("world_root").is_none());
        assert_eq!(file["assertions"], json!([]));
        for side in ["client", "server"] {
            assert_eq!(
                file[side],
                if destination == "config/extra.txt" {
                    json!({"kind":"choice","key":"extras","value":"enabled"})
                } else if layer != "common" && layer != side {
                    json!({"kind":"unsupported"})
                } else {
                    json!({"kind":"required"})
                }
            );
        }
        let member = &members[&path];
        assert_eq!(member.small.as_deref(), Some(bytes));
        assert_eq!(member.bytes, bytes.len() as u64);
        assert_eq!(member.sha256, digest);
        names.insert(path);
    }
    assert_eq!(members.keys().cloned().collect::<BTreeSet<_>>(), names);
    assert!(members.values().all(|member| member.mode == 0o644));
    hash(payload)
}

#[test]
fn cli_native_snapshots_preserve_inventory_choices_and_install_verified_exports() {
    let (project, host) = fixture();
    let mut author = snapshot(project.dir());
    let mut previous = None;
    for delivery in ["references", "bundled"] {
        for format in ["zip", "tar.gz", "7z"] {
            let built = command(project.dir(), host.path())
                .args(["--yes", "build", "--delivery", delivery, "--format", format])
                .assert()
                .success();
            let archive =
                assert_dist_artifact_suffix(project.dir(), &format!("-empack-{delivery}.empack"));
            let members = distribution_inventory(&archive, format);
            let identity = inventory(&members);
            built.stdout(contains(&identity));
            // These are local author sources: both delivery policies retain bytes.
            // URL reference-versus-bundled behavior has separate provider fixtures.
            if let Some(previous) = previous.replace(members.clone()) {
                assert_eq!(
                    members, previous,
                    "format/delivery changed local release meaning"
                );
            }
            author.insert(
                archive.strip_prefix(project.dir()).unwrap().into(),
                fs::read(&archive).unwrap(),
            );
            assert_eq!(snapshot(project.dir()), author);
            let export = tempfile::tempdir().unwrap();
            for (name, member) in &members {
                put(export.path(), name, member.small.as_ref().unwrap());
            }
            let exported = snapshot(export.path());
            let descriptor = export.path().join("release.json");
            let mut installations = vec![("game", "client"), ("game", "server")];
            // Archive format and side inventory are already covered above. One
            // format proves the independent Prism placement/profile boundary.
            if delivery == "references" && format == "zip" {
                installations.push(("prism", "client"));
            }
            for (layout, side) in installations {
                for extra in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let instance = root.path().join("instance Ω with spaces");
                    let game = if layout == "prism" {
                        ".minecraft"
                    } else {
                        "game"
                    };
                    put(
                        &instance,
                        &format!("{game}/unowned.txt"),
                        b"player sentinel",
                    );
                    // Instance installation does not reinterpret an author draft.
                    put(&instance, "empack.yml", b"invalid author draft: [\n");
                    let profile = br#"{ "formatVersion":1,"components":[{"uid":"net.minecraft","version":"1.21.1","important":true},{"uid":"fixture.extra","version":"1","note":"keep"}],"note":"retain formatting" }"#;
                    if layout == "prism" {
                        put(&instance, "mmc-pack.json", profile);
                        put(&instance, "instance.cfg", b"user launcher settings\n");
                    }
                    let install = |digest: &str| {
                        let mut cmd = command(&instance, host.path());
                        cmd.args(["--yes", "instance", "install"])
                            .arg(&descriptor)
                            .args(["--sha256", digest, "--side", side, "--layout", layout]);
                        if extra {
                            cmd.args(["--choice", "extras=enabled"]);
                        }
                        cmd
                    };
                    let before = snapshot(&instance);
                    install(&"0".repeat(64))
                        .assert()
                        .failure()
                        .stderr(contains("Selected release payload identity mismatch"));
                    assert_eq!(snapshot(&instance), before, "wrong digest changed instance");
                    // Included selected bytes must be checked even if an earlier
                    // installation populated the shared disposable cache.
                    let asset = export.path().join(format!("assets/{}", hash(CONTENT[3].2)));
                    fs::write(&asset, vec![b'!'; CONTENT[3].2.len()]).unwrap();
                    install(&identity)
                        .assert()
                        .failure()
                        .stderr(contains("Content differs from the accepted observation"));
                    assert_eq!(
                        snapshot(&instance),
                        before,
                        "corrupt asset changed instance"
                    );
                    fs::write(&asset, CONTENT[3].2).unwrap();
                    install(&identity).assert().success();
                    let mut expected = BTreeMap::from([
                        (
                            ".empack-layout".into(),
                            format!("empack-{layout}-layout-v1\n").into_bytes(),
                        ),
                        ("unowned.txt".into(), b"player sentinel".to_vec()),
                        (
                            "managed.txt".into(),
                            if side == "client" {
                                CONTENT[1].2
                            } else {
                                CONTENT[2].2
                            }
                            .to_vec(),
                        ),
                        ("config/settings.txt".into(), CONTENT[3].2.to_vec()),
                    ]);
                    if extra {
                        expected.insert("config/extra.txt".into(), CONTENT[4].2.to_vec());
                    }
                    assert_eq!(snapshot(&instance.join(game)), expected);
                    assert_eq!(
                        fs::read(instance.join("empack.yml")).unwrap(),
                        b"invalid author draft: [\n"
                    );
                    if layout == "prism" {
                        assert_eq!(fs::read(instance.join("mmc-pack.json")).unwrap(), profile);
                        assert_eq!(
                            fs::read(instance.join("instance.cfg")).unwrap(),
                            b"user launcher settings\n"
                        );
                        assert!(!instance.join("game").exists());
                        assert!(!instance.join("minecraft").exists());
                    }
                    command(&instance, host.path())
                        .args(["instance", "inspect"])
                        .assert()
                        .success()
                        .stdout(contains(&identity));
                    put(
                        &instance,
                        &format!("{game}/config/settings.txt"),
                        b"player configuration",
                    );
                    let installed = snapshot(&instance);
                    for _ in 0..2 {
                        command(&instance, host.path())
                            .args(["--yes", "instance", "prepare"])
                            .arg(&descriptor)
                            .args(["--sha256", &identity])
                            .assert()
                            .success();
                        assert_eq!(
                            snapshot(&instance),
                            installed,
                            "repeat preparation changed saved choices or seed bytes"
                        );
                    }
                    assert_eq!(snapshot(export.path()), exported);
                }
            }
        }
    }
    assert_eq!(snapshot(project.dir()), author);
}

#[test]
fn cli_snapshot_install_requires_explicit_ownership_in_both_layouts() {
    let (project, host) = fixture();
    command(project.dir(), host.path())
        .args(["--yes", "build"])
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), "-empack-references.empack");
    let members = distribution_inventory(&archive, "zip");
    let identity = inventory(&members);
    let export = tempfile::tempdir().unwrap();
    for (name, member) in &members {
        put(export.path(), name, member.small.as_ref().unwrap());
    }
    let authored = snapshot(project.dir());
    let exported = observe_workspace(export.path());
    for layout in ["game", "prism"] {
        let root = tempfile::tempdir().unwrap();
        let instance = root.path().join("unowned instance Ω");
        let game = if layout == "prism" {
            ".minecraft"
        } else {
            "game"
        };
        put(&instance, "notes.txt", b"unrelated root content");
        put(&instance, "empack.yml", b"invalid author draft: [\n");
        put(
            &instance,
            &format!("{game}/unowned.txt"),
            b"player sentinel",
        );
        let managed = instance.join(game).join("managed.txt");
        let install = |side: &str| {
            let mut cmd = command(&instance, host.path());
            cmd.args(["--yes", "instance", "install"])
                .arg(export.path().join("release.json"))
                .args(["--sha256", &identity, "--side", side, "--layout", layout]);
            cmd
        };
        // Identical bytes do not establish ownership, nor does blanket --yes.
        for bytes in [CONTENT[1].2, b"different local bytes".as_slice()] {
            fs::write(&managed, bytes).unwrap();
            let before = observe_workspace(&instance);
            install("client")
                .assert()
                .failure()
                .stderr(contains("UnownedDestination"));
            assert_eq!(observe_workspace(&instance), before);
        }
        fs::remove_file(&managed).unwrap();
        let marker = instance.join(game).join(".empack-layout");
        fs::write(&marker, format!("empack-{layout}-layout-v1\n")).unwrap();
        let before = observe_workspace(&instance);
        install("client").assert().failure().stderr(contains(
            "Layout marker is not owned by this completed instance",
        ));
        assert_eq!(observe_workspace(&instance), before);
        fs::remove_file(marker).unwrap();
        if layout == "prism" {
            let before = observe_workspace(&instance);
            install("server")
                .assert()
                .failure()
                .stderr(contains("Prism layout requires the client environment"));
            assert_eq!(observe_workspace(&instance), before);
            put(
                &instance,
                "minecraft/notes.txt",
                b"other launcher game root",
            );
            let before = observe_workspace(&instance);
            install("client")
                .assert()
                .failure()
                .stderr(contains("Prism layout requires minecraft/ to be absent"));
            assert_eq!(observe_workspace(&instance), before);
            fs::remove_file(instance.join("minecraft/notes.txt")).unwrap();
            fs::remove_dir(instance.join("minecraft")).unwrap();
        }
        fs::write(&managed, b"explicitly replaced local bytes").unwrap();
        install("client")
            .args(["--replace", "managed.txt"])
            .assert()
            .success();
        assert_eq!(fs::read(&managed).unwrap(), CONTENT[1].2);
        assert_eq!(
            fs::read(instance.join("notes.txt")).unwrap(),
            b"unrelated root content"
        );
        assert_eq!(
            fs::read(instance.join("empack.yml")).unwrap(),
            b"invalid author draft: [\n"
        );
        assert_eq!(
            fs::read(instance.join(game).join("unowned.txt")).unwrap(),
            b"player sentinel"
        );
        let installed = observe_workspace(&instance);
        install("client").assert().success();
        assert_eq!(observe_workspace(&instance), installed);
        assert_eq!(observe_workspace(export.path()), exported);
        assert_eq!(snapshot(project.dir()), authored);
    }
}

#[test]
fn cli_instance_inspection_retains_choices_history_and_refuses_invalid_completed_state() {
    let (project, author_host) = fixture();
    let host = tempfile::tempdir().unwrap();
    let player = tempfile::tempdir().unwrap();
    let instance = player.path().join("instance Ω");
    put(&instance, "player-notes.txt", b"retain player work");
    let mut previous = None;
    for version in ["1", "2"] {
        put(
            project.dir(),
            "empack.yml",
            AUTHOR
                .replace("version: '1'", &format!("version: '{version}'"))
                .as_bytes(),
        );
        command(project.dir(), author_host.path())
            .args(["--yes", "sync"])
            .assert()
            .success();
        command(project.dir(), author_host.path())
            .args(["--yes", "build", "--delivery", "bundled"])
            .assert()
            .success();
        let archive =
            assert_dist_artifact_suffix(project.dir(), &format!("{version}-empack-bundled.empack"));
        let members = distribution_inventory(&archive, "zip");
        let payload = members["release.json"].small.as_ref().unwrap();
        let identity = hash(payload);
        let export = tempfile::tempdir().unwrap();
        for (name, member) in &members {
            put(export.path(), name, member.small.as_ref().unwrap());
        }
        let mut install = command(&instance, host.path());
        install
            .args(["--yes", "instance", "install"])
            .arg(export.path().join("release.json"))
            .args([
                "--sha256", &identity, "--layout", "game", "--side", "client",
            ]);
        if previous.is_none() {
            install.args(["--choice", "extras=enabled"]);
        }
        install.assert().success();
        assert_eq!(
            fs::read(instance.join("game/config/extra.txt")).unwrap(),
            CONTENT[4].2
        );
        let before = observe_workspace(&instance);
        let stored = observe_workspace(host.path());
        let exported = observe_workspace(export.path());
        let inspected = command(&instance, host.path())
            .args(["instance", "inspect"])
            .output()
            .unwrap();
        assert!(inspected.status.success(), "{inspected:?}");
        assert!(inspected.stderr.is_empty(), "{inspected:?}");
        let text = String::from_utf8(inspected.stdout).unwrap();
        assert!(
            text.contains(&format!(
                "Pack native.fixture {version} (Client), release {identity}; game directory: game/"
            )),
            "{text}"
        );
        assert!(
            text.lines().any(|line| line.ends_with(" extras=enabled")),
            "{text}"
        );
        let history: Vec<_> = text
            .lines()
            .filter_map(|line| line.split_once("Retained release: ").map(|(_, id)| id))
            .collect();
        assert_eq!(
            history,
            previous.iter().map(String::as_str).collect::<Vec<_>>()
        );
        let status = command(&instance, host.path())
            .args(["status", "--context", "instance", "--json"])
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        assert!(status.stderr.is_empty(), "{status:?}");
        let report: Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(report["content_verified"], false);
        assert_eq!(report["selected"], "instance");
        let selected = report["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["context"] == "instance")
            .unwrap();
        assert_eq!(selected["state"], "valid");
        assert_eq!(selected["version"], version);
        assert_eq!(selected["exact"], identity);
        assert_eq!(observe_workspace(&instance), before);
        assert_eq!(observe_workspace(host.path()), stored);
        assert_eq!(observe_workspace(export.path()), exported);

        let record = instance.join(".empack/instance.json");
        let original = fs::read(&record).unwrap();
        for problem in ["unknown-schema", "wrong-root", "unknown-field", "malformed"] {
            let mut invalid: Value = serde_json::from_slice(&original).unwrap();
            match problem {
                "unknown-schema" => invalid["schema"] = json!(999),
                "wrong-root" => invalid["root"] = json!("0".repeat(64)),
                "unknown-field" => invalid["unrecognized"] = json!(true),
                "malformed" => {}
                _ => unreachable!(),
            }
            let bytes = if problem == "malformed" {
                b"{invalid completed instance\n".to_vec()
            } else {
                serde_json::to_vec(&invalid).unwrap()
            };
            fs::write(&record, &bytes).unwrap();
            let invalid_before = observe_workspace(&instance);
            let refused = command(&instance, host.path())
                .args(["instance", "inspect"])
                .output()
                .unwrap();
            assert!(!refused.status.success(), "{problem}: {refused:?}");
            assert!(!refused.stderr.is_empty(), "{problem}: {refused:?}");
            assert!(!String::from_utf8_lossy(&refused.stdout).contains("Pack native.fixture"));
            let status = command(&instance, host.path())
                .args(["status", "--context", "instance", "--json"])
                .assert()
                .success();
            let report: Value = serde_json::from_slice(&status.get_output().stdout).unwrap();
            assert_eq!(report["content_verified"], false);
            assert_eq!(report["contexts"][0]["state"], "invalid", "{problem}");
            assert_eq!(observe_workspace(&instance), invalid_before, "{problem}");
            assert_eq!(observe_workspace(host.path()), stored, "{problem}");
            fs::write(&record, &original).unwrap();
        }
        assert_eq!(observe_workspace(&instance), before);
        let retained = instance.join(format!(".empack/releases/{identity}.json"));
        let descriptor = fs::read(&retained).unwrap();
        let mut changed: Value = serde_json::from_slice(&descriptor).unwrap();
        changed["version"] = json!("tampered");
        fs::write(&retained, serde_json::to_vec(&changed).unwrap()).unwrap();
        let corrupt = observe_workspace(&instance);
        command(&instance, host.path())
            .args(["instance", "inspect"])
            .assert()
            .failure()
            .stderr(contains("Retained release descriptor changed"));
        assert_eq!(observe_workspace(&instance), corrupt);
        assert_eq!(observe_workspace(host.path()), stored);
        fs::write(&retained, descriptor).unwrap();
        assert_eq!(observe_workspace(&instance), before);
        assert_eq!(
            fs::read(instance.join("player-notes.txt")).unwrap(),
            b"retain player work"
        );
        previous = Some(identity);
    }
}

#[test]
fn cli_instance_choices_retain_decisions_through_missing_content_and_conflicts() {
    let (project, author_host) = fixture();
    put(
        project.dir(),
        "empack.bindings.yml",
        b"schema: 1\nbindings: {}\npolicies:\n  config/extra.txt: managed\n",
    );
    command(project.dir(), author_host.path())
        .args(["--yes", "sync"])
        .assert()
        .success();
    command(project.dir(), author_host.path())
        .args(["--yes", "build"])
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), "-empack-references.empack");
    let members = distribution_inventory(&archive, "zip");
    let payload = members["release.json"].small.as_ref().unwrap();
    let descriptor: Value = serde_json::from_slice(payload).unwrap();
    let optional = descriptor["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["destination"] == "config/extra.txt")
        .unwrap();
    assert_eq!(optional["policy"], "managed");
    let identity = hash(payload);
    let export = tempfile::tempdir().unwrap();
    for (name, member) in &members {
        put(export.path(), name, member.small.as_ref().unwrap());
    }
    let authored = snapshot(project.dir());
    let exported = observe_workspace(export.path());
    let player = tempfile::tempdir().unwrap();
    let instance = player.path().join("instance Ω");
    // A separate host cache cannot supply the optional bytes from the author's build.
    let host = tempfile::tempdir().unwrap();
    put(&instance, "game/config/settings.txt", b"player settings");
    put(
        &instance,
        "game/saves/player/level.dat",
        b"player world sentinel",
    );
    put(&instance, "game/unowned.txt", b"player note");
    command(&instance, host.path())
        .args(["--yes", "instance", "install"])
        .arg(export.path().join("release.json"))
        .args(["--sha256", &identity])
        .assert()
        .success();
    let extra = instance.join("game/config/extra.txt");
    assert!(!extra.exists());
    let inspect = |selected: &str| {
        let before = observe_workspace(&instance);
        let stored = observe_workspace(host.path());
        command(&instance, host.path())
            .args(["instance", "options"])
            .assert()
            .success()
            .stdout(contains(format!("extras={selected}")));
        assert_eq!(observe_workspace(&instance), before);
        assert_eq!(observe_workspace(host.path()), stored);
    };
    let choose = |selected: &str| {
        let mut cmd = command(&instance, host.path());
        cmd.args([
            "--yes",
            "instance",
            "options",
            "--choice",
            &format!("extras={selected}"),
        ]);
        cmd
    };
    inspect("disabled");
    let before = observe_workspace(&instance);
    choose("not-an-alternative").assert().failure();
    assert_eq!(observe_workspace(&instance), before);
    choose("enabled").arg("--dry-run").assert().success();
    assert_eq!(observe_workspace(&instance), before);
    inspect("disabled");
    choose("enabled")
        .assert()
        .failure()
        .stderr(contains("exact manual inputs remain pending"));
    assert_eq!(observe_workspace(&instance), before);
    inspect("disabled");
    let listed = command(&instance, host.path())
        .args(["pending", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed: Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(listed["schema"], 1);
    let operations = listed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    let operation = &operations[0];
    assert_eq!(operation["kind"], "instance");
    let inputs = operation["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["needed"], true);
    assert_eq!(inputs[0]["bytes"], CONTENT[4].2.len());
    assert_eq!(
        inputs[0]["digests"],
        json!([["selected-sha256", hash(CONTENT[4].2)]])
    );
    let supplied = player.path().join("renamed optional content.txt");
    let association = format!(
        "{}={}",
        inputs[0]["key"].as_str().unwrap(),
        supplied.display()
    );
    fs::write(&supplied, vec![b'!'; CONTENT[4].2.len()]).unwrap();
    let resume = || {
        let mut cmd = command(&instance, host.path());
        cmd.args([
            "--yes",
            "pending",
            "resume",
            operation["id"].as_str().unwrap(),
            "--file",
            &association,
        ]);
        cmd
    };
    resume()
        .assert()
        .failure()
        .stderr(contains("Content differs from the accepted observation"));
    assert_eq!(observe_workspace(&instance), before);
    fs::write(&supplied, CONTENT[4].2).unwrap();
    resume().assert().success();
    assert_eq!(fs::read(&extra).unwrap(), CONTENT[4].2);
    inspect("enabled");
    choose("disabled").assert().success();
    assert!(
        !extra.exists(),
        "disabling must retire unchanged managed content"
    );
    inspect("disabled");
    choose("enabled")
        .arg("--assets")
        .arg(export.path())
        .assert()
        .success();
    fs::write(&extra, b"player modified optional content").unwrap();
    let before = observe_workspace(&instance);
    choose("disabled")
        .assert()
        .failure()
        .stderr(contains("ModifiedManagedFile"));
    assert_eq!(observe_workspace(&instance), before);
    inspect("enabled");
    choose("disabled")
        .args(["--preserve", "config/extra.txt"])
        .assert()
        .success();
    inspect("disabled");
    assert_eq!(
        fs::read(&extra).unwrap(),
        b"player modified optional content"
    );
    assert_eq!(
        fs::read(instance.join("game/config/settings.txt")).unwrap(),
        b"player settings"
    );
    assert_eq!(
        fs::read(instance.join("game/saves/player/level.dat")).unwrap(),
        b"player world sentinel"
    );
    assert_eq!(
        fs::read(instance.join("game/unowned.txt")).unwrap(),
        b"player note"
    );
    assert_eq!(observe_workspace(export.path()), exported);
    assert_eq!(snapshot(project.dir()), authored);
}

#[test]
fn cli_native_export_requires_declared_local_bytes_even_for_reference_delivery() {
    let (project, _) = fixture();
    for delivery in ["references", "bundled"] {
        let host = tempfile::tempdir().unwrap();
        command(project.dir(), host.path())
            .args(["--yes", "build", "--delivery", delivery])
            .assert()
            .success();
        let archive =
            assert_dist_artifact_suffix(project.dir(), &format!("-empack-{delivery}.empack"));
        inventory(&distribution_inventory(&archive, "zip"));
        let local = project.dir().join("vendor/extra.txt");
        fs::remove_file(&local).unwrap();
        let before = snapshot(project.dir());
        // A cold host cannot recover missing local input from an earlier cache.
        let cold = tempfile::tempdir().unwrap();
        command(project.dir(), cold.path())
            .args(["--yes", "build", "--delivery", delivery])
            .assert()
            .failure()
            .stderr(contains("vendor/extra.txt"));
        assert_eq!(snapshot(project.dir()), before);
        fs::write(local, CONTENT[4].2).unwrap();
        let mut expected = snapshot(project.dir());
        command(project.dir(), cold.path())
            .args(["--yes", "build", "--delivery", delivery])
            .assert()
            .success();
        inventory(&distribution_inventory(&archive, "zip"));
        expected.insert(
            archive.strip_prefix(project.dir()).unwrap().into(),
            fs::read(&archive).unwrap(),
        );
        assert_eq!(snapshot(project.dir()), expected);
    }
}

#[test]
fn cli_native_export_requires_pack_identity_and_java_without_replacing_prior_output() {
    let (project, host) = fixture();
    command(project.dir(), host.path())
        .args(["--yes", "build"])
        .assert()
        .success();
    for (from, to, diagnostic) in [
        ("id: native.fixture, ", "", "Missing field 'id'"),
        (", java: 21", "", "Missing field 'java'"),
    ] {
        put(
            project.dir(),
            "empack.yml",
            AUTHOR.replace(from, to).as_bytes(),
        );
        let before = snapshot(project.dir());
        let host_before = snapshot(host.path());
        command(project.dir(), host.path())
            .args(["--yes", "build"])
            .assert()
            .failure()
            .stderr(contains(diagnostic));
        assert_eq!(snapshot(project.dir()), before);
        assert_eq!(snapshot(host.path()), host_before);
    }
}
