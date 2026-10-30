use empack_tests::harness::archive::{zip_member, zip_members};
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix, offline_command};

#[path = "support/files.rs"]
mod files;
#[path = "support/native_exports.rs"]
mod native_exports;
#[path = "support/build_profiles.rs"]
mod profiles;
#[path = "support/provider_exports.rs"]
mod provider_exports;

#[test]
fn cli_build_relative_workdir_writes_expected_mrpack_metadata() {
    let project = TestProject::workflow_fixture("relative-pack", "fabric", "1.21.1");
    let mut cmd = offline_command(project.dir());
    cmd.current_dir(project.dir().parent().unwrap())
        .arg("--workdir")
        .arg(project.dir().file_name().unwrap())
        .args(["--yes", "build", "modrinth"]);
    cmd.timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    let archive = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_member(&archive, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["name"], "relative-pack");
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert!(manifest["files"].as_array().unwrap().is_empty());
}

#[test]
fn cli_build_modrinth_creates_named_artifact() {
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    let output = offline_command(project.dir())
        .args(["--yes", "build", "modrinth"])
        .output()
        .expect("failed to spawn");
    assert!(output.status.success(), "empack build modrinth failed");

    let artifact = assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        artifact
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test-pack-")),
        "unexpected mrpack artifact path: {}",
        artifact.display()
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&zip_member(&artifact, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    assert_eq!(manifest["name"], "test-pack");
}

#[test]
fn cli_fabric_prism_tar_gz_preserves_exact_components_and_managed_commands() {
    use empack_tests::harness::archive::distribution_inventory;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
    project.configure_native_distribution("test.pack", 21);
    offline_command(project.dir())
        .args([
            "--yes",
            "build",
            "--ad-hoc",
            "--format",
            "tar.gz",
            "prism",
            "--delivery",
            "references",
        ])
        .assert()
        .success();
    let artifact = assert_dist_artifact_suffix(project.dir(), "-prism-references.tar.gz");
    let members = distribution_inventory(&artifact, "tar.gz");
    assert_eq!(
        members.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            ".minecraft/.empack-consumer/release.json",
            "instance.cfg",
            "mmc-pack.json",
        ]
    );
    let bytes = |name: &str| members[name].small.as_ref().unwrap().as_slice();
    assert_eq!(
        serde_json::from_slice::<Value>(bytes("mmc-pack.json")).unwrap(),
        json!({
            "formatVersion":1,"components":[
                {"uid":"net.minecraft","version":"1.21.1","important":true},
                {"uid":"net.fabricmc.fabric-loader","version":"0.15.0"}
            ]
        })
    );
    let release = bytes(".minecraft/.empack-consumer/release.json");
    let descriptor: Value = serde_json::from_slice(release).unwrap();
    assert_eq!(descriptor["pack"], "test.pack");
    assert_eq!(
        descriptor["runtime"],
        json!({
            "minecraft":"1.21.1", "loader":{"kind":"fabric","version":"0.15.0"}, "java_major":21,
        })
    );
    assert_eq!(descriptor["files"], json!([]));
    assert_eq!(descriptor["choices"], json!([]));
    assert_eq!(descriptor["require_subscription"], false);
    let hash: String = Sha256::digest(release)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let prepare = format!(
        "empack --workdir \"$INST_DIR\" --yes instance prepare \"$INST_DIR/.minecraft/.empack-consumer/release.json\" --sha256 {hash} --layout prism --side client"
    );
    let wrapper = "empack --workdir \"$INST_DIR\" --yes instance launch --";
    let configuration = std::str::from_utf8(bytes("instance.cfg")).unwrap();
    for setting in [
        "ConfigVersion=1.2".to_owned(),
        "OverrideCommands=true".to_owned(),
        format!("PreLaunchCommand={}", json!(prepare)),
        format!("WrapperCommand={}", json!(wrapper)),
    ] {
        assert!(
            configuration.lines().any(|line| line == setting),
            "{configuration}"
        );
    }
    assert!(members.values().all(|member| member.mode == 0o644));
}

#[test]
fn cli_clean_scopes_require_approval_and_preserve_authoring_and_recovery() {
    use empack_lib::engine::content::store::{ContentStoreLimits, FileContentStore};
    use std::{fs, path::Path};

    for target in [None, Some("builds"), Some("cache"), Some("all")] {
        let project = TestProject::workflow_fixture("test-pack", "fabric", "1.21.1");
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        let cache = host.path().join("cache");
        let command = || {
            let mut command = offline_command(project.dir());
            command
                .arg("--state-dir")
                .arg(&state)
                .arg("--cache-dir")
                .arg(&cache);
            command
        };
        for (name, bytes) in [
            (
                "pack/config/author.txt",
                b"authored configuration".as_slice(),
            ),
            (
                "templates/client/author.txt",
                b"authored template".as_slice(),
            ),
            ("world/level.dat", b"player data".as_slice()),
            ("author-notes.txt", b"author notes".as_slice()),
        ] {
            let path = project.dir().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        command()
            .args(["--yes", "build", "modrinth"])
            .assert()
            .success();
        let state_before_neighbor = files::snapshot(&state);
        let neighbor = TestProject::workflow_fixture("neighbor", "fabric", "1.21.1");
        offline_command(neighbor.dir())
            .arg("--state-dir")
            .arg(&state)
            .arg("--cache-dir")
            .arg(&cache)
            .args(["--yes", "build", "modrinth"])
            .assert()
            .success();
        let neighbor_files = files::snapshot(neighbor.dir());
        let neighbor_state: Vec<_> = files::snapshot(&state)
            .into_iter()
            .filter(|(path, bytes)| state_before_neighbor.get(path) != Some(bytes))
            .collect();
        assert!(!neighbor_state.is_empty());
        let artifact = assert_dist_artifact_suffix(project.dir(), ".mrpack");
        let artifact_bytes = fs::read(&artifact).unwrap();
        // Cleanup needs the selected native namespace, not valid authoring semantics.
        fs::write(
            project.dir().join("empack.yml"),
            b"unfinished: [author text",
        )
        .unwrap();
        fs::write(state.join("author-note"), b"keep host note").unwrap();
        let store = cache.join("content-v1");
        // Component setup initializes disposable native storage; this is not acquisition evidence.
        drop(FileContentStore::open(&store, ContentStoreLimits::default()).unwrap());
        let blob = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad.blob";
        fs::write(store.join(blob), b"abc").unwrap();
        fs::write(store.join("author-download.jar"), b"keep download").unwrap();
        fs::write(cache.join("neighbor.png"), b"keep neighboring cache file").unwrap();
        let before_project = files::snapshot(project.dir());
        let before_host = files::snapshot(host.path());
        let before_state = files::snapshot(&state);
        let mut expected_cache = files::snapshot(&cache);
        let selection = || {
            let mut args = vec!["clean"];
            args.extend(target);
            args
        };

        let preview = command()
            .arg("--dry-run")
            .args(selection())
            .assert()
            .success();
        let output = preview.get_output();
        let console = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            console.contains("Dry run complete - no changes applied"),
            "{console}"
        );
        assert_eq!(
            files::snapshot(project.dir()),
            before_project,
            "preview {target:?}"
        );
        assert_eq!(
            files::snapshot(host.path()),
            before_host,
            "preview {target:?}"
        );

        command()
            .args(selection())
            .assert()
            .code(2)
            .stderr(predicates::str::contains(
                "Noninteractive execution requires --yes",
            ));
        assert_eq!(
            files::snapshot(project.dir()),
            before_project,
            "unapproved {target:?}"
        );
        assert_eq!(
            files::snapshot(host.path()),
            before_host,
            "unapproved {target:?}"
        );
        command()
            .arg("--yes")
            .args(selection())
            .arg("unknown")
            .assert()
            .failure()
            .stderr(predicates::str::contains("Unknown cleanup target"));
        assert_eq!(
            files::snapshot(project.dir()),
            before_project,
            "invalid target {target:?}"
        );
        assert_eq!(
            files::snapshot(host.path()),
            before_host,
            "invalid target {target:?}"
        );

        command().arg("--yes").args(selection()).assert().success();
        let mut expected_project = before_project;
        if target != Some("cache") {
            expected_project.retain(|path, _| !path.starts_with("dist"));
        } else {
            assert_eq!(fs::read(&artifact).unwrap(), artifact_bytes);
        }
        if matches!(target, Some("cache" | "all")) {
            expected_cache
                .remove(&Path::new("content-v1").join(blob))
                .unwrap();
        }
        assert_eq!(
            files::snapshot(project.dir()),
            expected_project,
            "applied {target:?}"
        );
        assert_eq!(
            files::snapshot(&cache),
            expected_cache,
            "applied {target:?}"
        );
        // A new publication replaces this project's completed journal. Another
        // project's evidence and unrelated state must stay byte-for-byte intact.
        if target == Some("cache") {
            assert_eq!(files::snapshot(&state), before_state);
        }
        for (path, bytes) in neighbor_state {
            assert_eq!(
                fs::read(state.join(&path)).unwrap(),
                bytes,
                "neighbor recovery {path:?} / {target:?}"
            );
        }
        assert_eq!(
            fs::read(state.join("author-note")).unwrap(),
            b"keep host note"
        );
        assert_eq!(files::snapshot(neighbor.dir()), neighbor_files);
        let after_host = files::snapshot(host.path());
        command().arg("--yes").args(selection()).assert().success();
        assert_eq!(
            files::snapshot(project.dir()),
            expected_project,
            "repeated {target:?}"
        );
        assert_eq!(
            files::snapshot(host.path()),
            after_host,
            "repeated {target:?}"
        );
    }
}

#[test]
fn cli_clean_empty_cache_and_pending_scopes_create_no_storage() {
    use std::fs;
    let invocation = TestProject::new();
    let host = tempfile::tempdir().unwrap();
    for target in ["cache", "continuation", "import", "sync", "retained"] {
        for dry_run in [true, false] {
            let mut command = offline_command(invocation.dir());
            command
                .arg("--state-dir")
                .arg(host.path().join("state"))
                .arg("--cache-dir")
                .arg(host.path().join("cache"))
                .arg(if dry_run { "--dry-run" } else { "--yes" })
                .args(["clean", target])
                .assert()
                .success();
            assert_eq!(
                fs::read_dir(invocation.dir()).unwrap().count(),
                0,
                "{target} dry={dry_run}"
            );
            assert_eq!(
                fs::read_dir(host.path()).unwrap().count(),
                0,
                "{target} dry={dry_run}"
            );
        }
    }
}

#[test]
fn cli_build_prism_references_records_runtime() {
    let project = TestProject::new();
    offline_command(project.dir())
        .args([
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "--pack-name",
            "test-pack",
        ])
        .assert()
        .success();
    project.configure_native_distribution("matrix.pack", 21);
    offline_command(project.dir())
        .args(["--yes", "build", "prism", "--delivery", "references"])
        .assert()
        .success();
    let artifact = assert_dist_artifact_suffix(project.dir(), "-prism-references.zip");
    let members = zip_members(&artifact);
    let release: serde_json::Value =
        serde_json::from_slice(&members[".minecraft/.empack-consumer/release.json"]).unwrap();
    assert_eq!(release["pack"], "matrix.pack");
    assert_eq!(release["runtime"]["minecraft"], "1.21.1");
    assert!(!members.keys().any(|name| name.contains("packwiz")));
}
