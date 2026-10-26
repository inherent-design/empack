//! Native snapshot exports and public installation, with literal content oracles.
use crate::files::snapshot;
use empack_tests::harness::{
    archive::{Member, distribution_inventory},
    process::{TestProject, assert_dist_artifact_suffix, offline_command},
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
            for side in ["client", "server"] {
                for extra in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let instance = root.path().join("instance Ω with spaces");
                    put(&instance, "game/unowned.txt", b"player sentinel");
                    let install = |digest: &str| {
                        let mut cmd = command(&instance, host.path());
                        cmd.args(["--yes", "instance", "install"])
                            .arg(&descriptor)
                            .args(["--sha256", digest, "--side", side, "--layout", "game"]);
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
                        (".empack-layout".into(), b"empack-game-layout-v1\n".to_vec()),
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
                    assert_eq!(snapshot(&instance.join("game")), expected);
                    command(&instance, host.path())
                        .args(["instance", "inspect"])
                        .assert()
                        .success()
                        .stdout(contains(&identity));
                    put(
                        &instance,
                        "game/config/settings.txt",
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
