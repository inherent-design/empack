//! Executable native import contracts; explicit files preserve the archive's source evidence.
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix, empack_cmd};
use std::{fs, io::Read, path::Path};

fn run(project: &Path, args: &[&str]) {
    assert_cmd::Command::from_std(empack_cmd(project))
        .arg("--yes")
        .args(args)
        .timeout(std::time::Duration::from_secs(90))
        .assert()
        .success();
}

fn fixture(project: &TestProject, algorithm: &str, hash: &str) -> std::path::PathBuf {
    let manifest = serde_json::json!({
        "formatVersion":1,"game":"minecraft","name":"verified-url","versionId":"1.0.0",
        "dependencies":{"minecraft":"1.21.1"},
        "files":[{"path":"resourcepacks/declared.zip","downloads":["https://example.invalid/different-name.bin","https://example.invalid/fallback"],"fileSize":7,
            "hashes":{(algorithm):hash},"env":{"client":"optional","server":"unsupported"}}]
    });
    let archive = project.dir().join("input.mrpack");
    empack_tests::fixtures::write_zip(
        &archive,
        &[(
            "modrinth.index.json",
            &serde_json::to_vec(&manifest).unwrap(),
        )],
    )
    .unwrap();
    archive
}
fn read(root: &Path) -> empack_core::model::ResolvedProject {
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.join("empack.yml")).unwrap(), "fixture")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("empack.lock")).unwrap(),
            &intent,
            "fixture",
        )
        .unwrap()
}
fn member(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn e2e_verified_url_import_sync_export_and_remove_preserve_contract() {
    verified_url_lifecycle(
        "sha256",
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
    );
}

#[test]
fn e2e_md5_import_preserves_weak_source_evidence_through_native_export() {
    verified_url_lifecycle("md5", "321c3cf486ed509164edec1e1981fec8");
}

fn verified_url_lifecycle(algorithm: &str, hash: &str) {
    let project = TestProject::new();
    let archive = fixture(&project, algorithm, hash);
    fs::write(project.dir().join("renamed.bin"), b"payload").unwrap();
    let args = [
        "init",
        "--from",
        archive.to_str().unwrap(),
        "--import-file",
        "resourcepacks/declared.zip=renamed.bin",
        "--import-optional-default",
        "true",
        "imported",
    ];
    assert_cmd::Command::from_std(project.cmd())
        .arg("--yes")
        .arg("--dry-run")
        .args(args)
        .assert()
        .success();
    assert!(!project.dir().join("imported").exists());
    run(project.dir(), &args);
    let imported = project.dir().join("imported");
    let resolved = read(&imported);
    assert_eq!(resolved.intent().roots.len(), 1);
    let (key, root) = resolved.intent().roots.iter().next().unwrap();
    assert!(matches!(
        root.source,
        empack_core::model::SourceIntent::Url(_)
    ));
    let file = &resolved.lock().dependencies[key].files.as_slice()[0];
    assert_eq!(
        file.placements.as_slice()[0]
            .destination
            .relative()
            .as_str(),
        "resourcepacks/declared.zip"
    );
    assert!(matches!(
        file.placements.as_slice()[0].requirements.client,
        empack_core::requirements::Requirement::Optional(_)
    ));
    assert_eq!(
        file.placements.as_slice()[0].requirements.server,
        empack_core::requirements::Requirement::Unsupported
    );
    let original = fs::read(imported.join("empack.yml")).unwrap();
    let locked = fs::read(imported.join("empack.lock")).unwrap();
    assert!(String::from_utf8(original.clone()).unwrap().contains(hash));
    for _ in 0..2 {
        run(&imported, &["sync"]);
    }
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), original);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), locked);
    // The authored optional decision must be acknowledged for this lossy export format.
    assert_cmd::Command::from_std(empack_cmd(&imported))
        .args(["--yes", "build", "modrinth"])
        .assert()
        .failure();
    run(
        &imported,
        &["build", "modrinth", "--allow-optional-metadata-loss"],
    );
    let mut archive = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(&imported, ".mrpack")).unwrap(),
    )
    .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&member(&mut archive, "modrinth.index.json")).unwrap();
    assert_eq!(manifest["files"][0]["path"], "resourcepacks/declared.zip");
    assert_eq!(manifest["files"][0]["env"]["client"], "optional");
    assert_eq!(manifest["files"][0]["env"]["server"], "unsupported");
    assert_eq!(manifest["files"][0]["fileSize"], 7);
    assert_eq!(
        manifest["files"][0]["hashes"]["sha512"],
        "70b33ce9c9047e30f917e7ea13e42f7767008c3f4f9c9baf49e4390fc625549e9625eee39b94545074e8a1824cf3f238463b11bc03d97348e0fc2999ca1fff7f"
    );
    assert_eq!(
        manifest["files"][0]["downloads"].as_array().unwrap().len(),
        2
    );
    run(&imported, &["build", "prism", "--optional-defaults"]);
    let mut client = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(&imported, "prism-bundled.zip")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        member(&mut client, ".minecraft/resourcepacks/declared.zip"),
        b"payload"
    );
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), original);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), locked);
    run(&imported, &["remove", key.as_str()]);
    run(&imported, &["sync"]);
    assert!(read(&imported).intent().roots.is_empty());
}

#[test]
fn e2e_import_associations_reject_wrong_bytes_unknown_slots_and_duplicates() {
    let project = TestProject::new();
    let archive = fixture(
        &project,
        "sha256",
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
    );
    fs::write(project.dir().join("selected.bin"), b"changed").unwrap();
    for associations in [
        vec!["declared:0=selected.bin"],
        vec!["missing=selected.bin"],
        vec![
            "declared:0=selected.bin",
            "resourcepacks/declared.zip=selected.bin",
        ],
    ] {
        let mut command = assert_cmd::Command::from_std(project.cmd());
        command.args([
            "--yes",
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--import-optional-default",
            "true",
            "imported",
        ]);
        for association in associations {
            command.args(["--import-file", association]);
        }
        command.assert().failure();
        assert!(!project.dir().join("imported").exists());
    }
    fs::write(project.dir().join("selected.bin"), b"payload").unwrap();
    run(
        project.dir(),
        &[
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--import-file",
            "declared:0=selected.bin",
            "--import-optional-default",
            "false",
            "imported",
        ],
    );
    assert_eq!(
        read(&project.dir().join("imported")).intent().roots.len(),
        1
    );
}
