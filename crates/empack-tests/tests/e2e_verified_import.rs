use empack_lib::application::session::{ArchiveProvider, LiveArchiveProvider};
use empack_lib::empack::archive::ArchiveFormat;
use empack_tests::e2e::{TestProject, assert_dist_artifact_suffix, empack_cmd};
use std::path::Path;

fn run(project: &Path, args: &[&str]) {
    assert_cmd::Command::from_std(empack_cmd(project))
        .args(args)
        .timeout(std::time::Duration::from_secs(90))
        .assert()
        .success();
}

fn fixture(project: &TestProject, url: &str, hash: &str, optional: bool) -> std::path::PathBuf {
    let source = project.dir().join("source");
    std::fs::create_dir(&source).unwrap();
    let manifest = serde_json::json!({
        "formatVersion":1,"game":"minecraft","name":"verified-url","versionId":"1.0.0",
        "dependencies":{"minecraft":"1.21.1","fabric-loader":"0.15.11"},
        "files":[{"path":"resourcepacks/declared.zip","downloads":[url,format!("{url}/fallback")],"fileSize":7,
            "hashes":{"sha256":hash},"env":{"client":if optional {"optional"} else {"required"},"server":"unsupported"}}]
    });
    std::fs::write(
        source.join("modrinth.index.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let archive = project.dir().join("input.mrpack");
    LiveArchiveProvider
        .create_archive(&source, &archive, ArchiveFormat::Zip)
        .unwrap();
    archive
}

#[test]
fn e2e_verified_url_import_sync_export_and_remove_preserve_contract() {
    empack_tests::skip_if_no_packwiz!();
    let mut server = mockito::Server::new();
    let _download = server
        .mock("GET", "/different-name.bin")
        .with_body("payload")
        .create();
    let project = TestProject::new();
    let archive = fixture(
        &project,
        &format!("{}/different-name.bin", server.url()),
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        true,
    );
    run(
        project.dir(),
        &[
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--yes",
            "imported",
        ],
    );
    let imported = project.dir().join("imported");
    let metadata = imported.join("pack/resourcepacks/declared.zip.pw.toml");
    let original = std::fs::read_to_string(&metadata).unwrap();
    let parsed: toml::Value = toml::from_str(&original).unwrap();
    assert_eq!(parsed["filename"].as_str(), Some("declared.zip"));
    assert_eq!(parsed["side"].as_str(), Some("client"));
    assert_eq!(parsed["option"]["optional"].as_bool(), Some(true));
    let intent = std::fs::read_to_string(imported.join("empack.yml")).unwrap();
    assert!(intent.contains("status: url"));
    assert!(!intent.contains("project_id:"));
    for _ in 0..2 {
        run(&imported, &["sync"]);
    }
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), original);
    std::fs::remove_file(&metadata).unwrap();
    run(&imported, &["sync", "--dry-run"]);
    assert!(!metadata.exists());
    run(&imported, &["sync"]);
    assert_eq!(std::fs::read_to_string(&metadata).unwrap(), original);
    let _unavailable = server
        .mock("GET", "/different-name.bin")
        .with_status(503)
        .create();
    let _fallback = server
        .mock("GET", "/different-name.bin/fallback")
        .with_body("payload")
        .create();
    run(&imported, &["build", "mrpack"]);
    let artifact = assert_dist_artifact_suffix(&imported, ".mrpack");
    let inspect = imported.join("inspect");
    LiveArchiveProvider
        .extract_zip(&artifact, &inspect)
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(inspect.join("modrinth.index.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["files"][0]["path"], "resourcepacks/declared.zip");
    assert_eq!(manifest["files"][0]["env"]["client"], "optional");
    assert_eq!(manifest["files"][0]["env"]["server"], "unsupported");
    assert_eq!(manifest["files"][0]["fileSize"], 7);
    assert_eq!(
        manifest["files"][0]["downloads"].as_array().unwrap().len(),
        2
    );
    assert!(
        std::fs::read_to_string(&metadata)
            .unwrap()
            .contains("/different-name.bin/fallback")
    );
    let offline_primary = server
        .mock("GET", "/different-name.bin")
        .with_status(503)
        .expect(0)
        .create();
    let offline_fallback = server
        .mock("GET", "/different-name.bin/fallback")
        .with_status(503)
        .expect(0)
        .create();
    run(&imported, &["build", "mrpack"]);
    offline_primary.assert();
    offline_fallback.assert();
    run(&imported, &["sync"]);
    run(&imported, &["sync"]);
    run(&imported, &["remove", "url:resourcepacks/declared.zip"]);
    run(&imported, &["sync"]);
    assert!(!metadata.exists());
}

#[test]
fn e2e_url_import_rejects_changed_remote_bytes_before_initialization() {
    let mut server = mockito::Server::new();
    let _download = server
        .mock("GET", "/changed.bin")
        .with_body("changed")
        .create();
    let project = TestProject::new();
    let archive = fixture(
        &project,
        &format!("{}/changed.bin", server.url()),
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        false,
    );
    assert_cmd::Command::from_std(empack_cmd(project.dir()))
        .args([
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--yes",
            "imported",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
    assert!(!project.dir().join("imported/empack.yml").exists());
    assert!(!project.dir().join("imported/pack").exists());
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
    let content = ["first", "second"].into_iter().map(|name| ContentEntry::PlatformReferenced(PlatformRef {
        destination_path: format!("mods/{name}.jar"), platform: ProjectPlatform::Modrinth,
        project_id: name.into(), file_id: Some("v1".into()),
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
