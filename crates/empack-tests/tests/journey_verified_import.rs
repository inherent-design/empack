//! Executable native import contracts; explicit files preserve the archive's source evidence.
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::harness::{
    archive::zip_members,
    process::{TestProject, assert_dist_artifact_suffix, empack_cmd},
};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

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
        &[
            (
                "modrinth.index.json",
                &serde_json::to_vec(&manifest).unwrap(),
            ),
            ("overrides/config/common.properties", b"shared=true\n"),
            ("client-overrides/options.txt", b"renderDistance:8\n"),
            ("server-overrides/server.properties", b"motd=Round trip\n"),
        ],
    )
    .unwrap();
    archive
}
fn read(root: &Path) -> empack_core::model::ResolvedProject {
    let intent = empack_lib::testing::read_authoring(&root.join("empack.yml")).unwrap();
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
fn journey_verified_url_import_sync_export_and_remove_preserve_contract() {
    verified_url_lifecycle(
        "sha256",
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
    );
}

#[test]
fn journey_md5_import_preserves_weak_source_evidence_through_native_export() {
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
    assert_eq!(resolved.intent().roots.len(), 4);
    let urls: Vec<_> = resolved
        .intent()
        .roots
        .iter()
        .filter(|(_, root)| matches!(root.source, empack_core::model::SourceIntent::Url(_)))
        .collect();
    assert_eq!(urls.len(), 1);
    let (key, _) = urls[0];
    let file = &resolved.lock().dependencies[key].files.as_slice()[0];
    assert_eq!(
        file.placements.as_slice()[0]
            .destination
            .relative()
            .as_str(),
        "resourcepacks/declared.zip"
    );
    let empack_core::requirements::Requirement::Optional(choice) =
        &file.placements.as_slice()[0].requirements.client
    else {
        panic!("import must retain optional participation");
    };
    assert!(choice.default_enabled);
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
    let exported_path = assert_dist_artifact_suffix(&imported, ".mrpack");
    let exported_bytes = fs::read(&exported_path).unwrap();
    let inventory = assert_mrpack(&exported_path);

    // Mrpack retains optional participation, but not the author's default. A fresh
    // import must ask for that decision again instead of inventing an enabled value.
    let second = project.dir().join("reopened");
    let selected_bytes = project.dir().join("renamed.bin");
    let association = format!("resourcepacks/declared.zip={}", selected_bytes.display());
    assert_cmd::Command::from_std(project.cmd())
        .args([
            "--yes",
            "init",
            "--from",
            exported_path.to_str().unwrap(),
            "--import-file",
            &association,
            "reopened",
        ])
        .assert()
        .failure();
    assert!(!second.exists());
    run(
        project.dir(),
        &[
            "init",
            "--from",
            exported_path.to_str().unwrap(),
            "--import-file",
            &association,
            "--import-optional-default",
            "false",
            "reopened",
        ],
    );
    let reopened = read(&second);
    assert_eq!(reopened.intent().runtime.minecraft.as_str(), "1.21.1");
    assert_eq!(
        reopened.intent().runtime.loader,
        empack_core::model::LoaderKind::Vanilla
    );
    let dependencies = &reopened.lock().dependencies;
    assert_eq!(dependencies.len(), 4);
    let placements: Vec<_> = dependencies
        .values()
        .flat_map(|dependency| dependency.files.as_slice())
        .flat_map(|file| file.placements.as_slice())
        .filter(|placement| {
            placement.destination.relative().as_str() == "resourcepacks/declared.zip"
        })
        .collect();
    assert_eq!(placements.len(), 1);
    let placement = placements[0];
    let empack_core::requirements::Requirement::Optional(choice) = &placement.requirements.client
    else {
        panic!("import must retain optional participation");
    };
    assert!(
        !choice.default_enabled,
        "the explicit new decision must survive"
    );
    assert_eq!(
        placement.requirements.server,
        empack_core::requirements::Requirement::Unsupported
    );
    let reopened_author = fs::read(second.join("empack.yml")).unwrap();
    let reopened_lock = fs::read(second.join("empack.lock")).unwrap();
    run(&second, &["sync"]);
    run(
        &second,
        &["build", "modrinth", "--allow-optional-metadata-loss"],
    );
    assert_eq!(
        assert_mrpack(&assert_dist_artifact_suffix(&second, ".mrpack")),
        inventory
    );
    assert_eq!(
        fs::read(second.join("empack.yml")).unwrap(),
        reopened_author
    );
    assert_eq!(fs::read(second.join("empack.lock")).unwrap(), reopened_lock);
    assert_eq!(fs::read(&exported_path).unwrap(), exported_bytes);
    run(&imported, &["build", "prism", "--optional-defaults"]);
    let mut referenced = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(
            &imported,
            "prism-references.zip",
        ))
        .unwrap(),
    )
    .unwrap();
    let release = empack_lib::engine::release::DecodedRelease::decode(&member(
        &mut referenced,
        ".minecraft/.empack-consumer/release.json",
    ))
    .unwrap();
    let released = release
        .document()
        .files
        .iter()
        .find(|file| file.destination == "resourcepacks/declared.zip")
        .unwrap();
    assert!(released.asset.is_none());
    assert!(matches!(
        released.source,
        empack_lib::engine::release::ReleaseSource::Url { .. }
    ));
    assert!(
        released
            .assertions
            .iter()
            .any(|digest| digest.algorithm == algorithm && digest.value == hash)
    );
    assert!(
        referenced
            .by_name(".minecraft/resourcepacks/declared.zip")
            .is_err()
    );
    // Bundled bytes remain an explicit override of the imported reference profile.
    run(
        &imported,
        &[
            "build",
            "prism",
            "--delivery",
            "bundled",
            "--optional-defaults",
        ],
    );
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
    // Imported files do not establish a complete dependency graph. Keeping the
    // three override roots must not silently authorize deletion of another root.
    assert_cmd::Command::from_std(empack_cmd(&imported))
        .args(["--yes", "remove", key.as_str()])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Retained dependency evidence is incomplete",
        ));
    assert_eq!(fs::read(imported.join("empack.yml")).unwrap(), original);
    assert_eq!(fs::read(imported.join("empack.lock")).unwrap(), locked);
    assert_eq!(fs::read(&exported_path).unwrap(), exported_bytes);
    run(
        &imported,
        &["remove", key.as_str(), "--acknowledge-unknown"],
    );
    run(&imported, &["sync"]);
    let remaining = read(&imported);
    assert_eq!(remaining.intent().roots.len(), 3);
    assert!(!remaining.intent().roots.contains_key(key));
    assert!(
        remaining
            .intent()
            .roots
            .values()
            .all(|root| !matches!(root.source, empack_core::model::SourceIntent::Url(_)))
    );
}

/// The oracle is literal format data, not an inventory regenerated by empack.
fn assert_mrpack(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut members = zip_members(path);
    let index: serde_json::Value =
        serde_json::from_slice(&members.remove("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(
        index,
        serde_json::json!({
            "formatVersion":1, "game":"minecraft", "name":"verified-url", "versionId":"1.0.0",
            "dependencies":{"minecraft":"1.21.1"},
            "files":[{"path":"resourcepacks/declared.zip", "fileSize":7,
                "downloads":["https://example.invalid/different-name.bin","https://example.invalid/fallback"],
                "hashes":{"sha1":"f07e5a815613c5abeddc4b682247a4c42d8a95df",
                    "sha512":"70b33ce9c9047e30f917e7ea13e42f7767008c3f4f9c9baf49e4390fc625549e9625eee39b94545074e8a1824cf3f238463b11bc03d97348e0fc2999ca1fff7f"},
                "env":{"client":"optional","server":"unsupported"}}]
        })
    );
    assert_eq!(
        members,
        BTreeMap::from([
            (
                "overrides/config/common.properties".into(),
                b"shared=true\n".to_vec()
            ),
            (
                "client-overrides/options.txt".into(),
                b"renderDistance:8\n".to_vec()
            ),
            (
                "server-overrides/server.properties".into(),
                b"motd=Round trip\n".to_vec()
            ),
        ])
    );
    members
}

#[test]
fn journey_import_associations_reject_wrong_bytes_unknown_slots_and_duplicates() {
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
        4
    );
}
