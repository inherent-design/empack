//! Executable native import contracts; explicit files preserve the archive's source evidence.
use empack_lib::application::session::{ArchiveProvider, LiveArchiveProvider};
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
        .args(["--yes", "build", "mrpack"])
        .assert()
        .failure();
    run(
        &imported,
        &["build", "mrpack", "--allow-optional-metadata-loss"],
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
    run(&imported, &["build", "client-full", "--optional-defaults"]);
    let mut client = zip::ZipArchive::new(
        fs::File::open(assert_dist_artifact_suffix(&imported, "client-full.zip")).unwrap(),
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

#[tokio::test(flavor = "multi_thread")]
async fn e2e_batched_provider_import_preserves_optional_requirements_through_export() {
    use empack_lib::application::AppConfig;
    use empack_lib::application::session::CommandSession;
    use empack_lib::empack::parsing::ModLoader;
    use empack_lib::empack::{
        ContentEntry, ImportConfig, ModpackManifest, PackIdentity, PlatformRef, ResolvedManifest,
        RuntimeTarget, SideEnv, SideRequirement, execute_import,
    };
    use empack_lib::primitives::{ProjectPlatform, ProjectType};
    use std::collections::HashMap;
    empack_tests::skip_if_no_packwiz!();
    let mut server = mockito::Server::new_async().await;
    let _download = server
        .mock("GET", "/content.jar")
        .with_body("payload")
        .create_async()
        .await;
    let project = TestProject::new();
    let target = project.dir().join("imported");
    let session = CommandSession::new(AppConfig {
        workdir: Some(target.clone()),
        ..Default::default()
    });
    let content = [("Project1", "first"), ("Project2", "second")].into_iter().map(|(id, name)| ContentEntry::PlatformReferenced(PlatformRef {
        destination_path: format!("mods/{name}.jar"), platform: ProjectPlatform::Modrinth,
        project_id: id.into(), file_id: Some("Version1".into()),
        hashes: HashMap::from([("sha512".into(), "70b33ce9c9047e30f917e7ea13e42f7767008c3f4f9c9baf49e4390fc625549e9625eee39b94545074e8a1824cf3f238463b11bc03d97348e0fc2999ca1fff7f".into())]),
        download_urls: vec![format!("{}/content.jar", server.url())],
        env: SideEnv { client: SideRequirement::Optional, server: SideRequirement::Unsupported }, required: true,
        resolved_name: Some(name.into()), resolved_slug: Some(name.into()), resolved_type: Some(ProjectType::Mod), cf_class_id: None,
    })).collect();
    execute_import(
        ResolvedManifest {
            manifest: ModpackManifest {
                identity: PackIdentity {
                    name: "optional".into(),
                    version: "1.0.0".into(),
                    author: None,
                    summary: None,
                },
                target: RuntimeTarget {
                    minecraft_version: "1.21.1".into(),
                    loader: ModLoader::Fabric,
                    loader_version: "0.15.11".into(),
                },
                content,
                overrides: vec![],
                source_platform: ProjectPlatform::Modrinth,
                archive_path: project.dir().join("unused.mrpack"),
            },
            warnings: vec![],
        },
        ImportConfig {
            target_dir: target.clone(),
            pack_name: "optional".into(),
            author: "Test".into(),
            version: "1.0.0".into(),
            datapack_folder: None,
            acceptable_game_versions: None,
        },
        &session,
    )
    .await
    .unwrap();
    for name in ["first", "second"] {
        let path = target.join(format!("pack/mods/{name}.pw.toml"));
        let mut metadata: toml::Value =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(metadata["option"]["optional"].as_bool(), Some(true));
        assert_eq!(metadata["side"].as_str(), Some("client"));
        // Simulate a backend reinstall that drops optional metadata.
        metadata.as_table_mut().unwrap().remove("option");
        std::fs::write(path, toml::to_string(&metadata).unwrap()).unwrap();
    }
    assert_cmd::Command::from_std(empack_cmd(&target))
        .args(["build", "mrpack", "--dry-run"])
        .assert()
        .failure();
    run(&target, &["sync"]);
    run(&target, &["sync"]);
    run(&target, &["build", "mrpack"]);
    let artifact = assert_dist_artifact_suffix(&target, ".mrpack");
    let inspect = target.join("inspect");
    LiveArchiveProvider
        .extract_zip(&artifact, &inspect)
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(inspect.join("modrinth.index.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["files"].as_array().unwrap().len(), 2);
    for file in manifest["files"].as_array().unwrap() {
        assert_eq!(file["env"]["client"], "optional");
        assert_eq!(file["env"]["server"], "unsupported");
    }
    let path = target.join("pack/mods/first.pw.toml");
    let mut metadata: toml::Value =
        toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    metadata["download"]
        .as_table_mut()
        .unwrap()
        .insert("mode".into(), "metadata:curseforge".into());
    std::fs::write(path, toml::to_string(&metadata).unwrap()).unwrap();
    assert_cmd::Command::from_std(empack_cmd(&target))
        .args(["build", "mrpack"])
        .assert()
        .failure();
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_curseforge_import_places_each_content_type_in_its_own_directory() {
    use empack_lib::application::{AppConfig, session::CommandSession};
    use empack_lib::empack::parsing::ModLoader;
    use empack_lib::empack::{
        ContentEntry, ImportConfig, ModpackManifest, PackIdentity, PlatformRef, ResolvedManifest,
        RuntimeTarget, SideEnv, SideRequirement, execute_import,
    };
    use empack_lib::primitives::{ProjectPlatform, ProjectType};
    empack_tests::skip_if_no_packwiz!();
    let project = TestProject::new();
    let target = project.dir().join("imported");
    let session = CommandSession::new(AppConfig {
        workdir: Some(target.clone()),
        ..Default::default()
    });
    let cases = [
        ("shaderpacks", ProjectType::Shader, 6552),
        ("resourcepacks", ProjectType::ResourcePack, 12),
        ("mods", ProjectType::Mod, 6),
        ("saves", ProjectType::World, 17),
    ];
    let content = cases
        .iter()
        .enumerate()
        .map(|(i, (folder, kind, class))| {
            ContentEntry::PlatformReferenced(PlatformRef {
                destination_path: format!("{folder}/fixture.zip"),
                platform: ProjectPlatform::CurseForge,
                project_id: (100 + i).to_string(),
                file_id: Some((200 + i).to_string()),
                hashes: [("sha1".into(), "a".repeat(40))].into(),
                download_urls: vec![],
                env: SideEnv {
                    client: SideRequirement::Required,
                    server: SideRequirement::Required,
                },
                required: true,
                resolved_name: Some(folder.to_string()),
                resolved_slug: Some(folder.to_string()),
                resolved_type: Some(*kind),
                cf_class_id: Some(*class),
            })
        })
        .collect();
    execute_import(
        ResolvedManifest {
            manifest: ModpackManifest {
                identity: PackIdentity {
                    name: "types".into(),
                    version: "1.0.0".into(),
                    author: None,
                    summary: None,
                },
                target: RuntimeTarget {
                    minecraft_version: "1.21.1".into(),
                    loader: ModLoader::Fabric,
                    loader_version: "0.15.11".into(),
                },
                content,
                overrides: vec![],
                source_platform: ProjectPlatform::CurseForge,
                archive_path: project.dir().join("unused.zip"),
            },
            warnings: vec![],
        },
        ImportConfig {
            target_dir: target.clone(),
            pack_name: "types".into(),
            author: "Test".into(),
            version: "1.0.0".into(),
            datapack_folder: None,
            acceptable_game_versions: None,
        },
        &session,
    )
    .await
    .unwrap();
    for (folder, _, _) in cases {
        assert!(
            target
                .join(format!("pack/{folder}/{folder}.pw.toml"))
                .is_file()
        );
        assert!(!target.join(format!("pack/{folder}.pw.toml")).exists());
    }
    run(&target, &["sync"]);
    run(&target, &["sync"]);
}
