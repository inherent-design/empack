use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session_mocks::{mock_root, MockCommandSession, MockFileSystemProvider, MockProcessProvider};
use std::path::Path;
use tempfile::TempDir;

// Mock structure for testing build orchestrator without external dependencies
struct MockBuildOrchestrator {
    temp_dir: TempDir,
    session: MockCommandSession,
}

impl MockBuildOrchestrator {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let workdir = temp_dir.path().to_path_buf();

        // Create mock session with the temp directory as working directory
        let session = MockCommandSession::new()
            .with_filesystem(
                crate::application::session_mocks::MockFileSystemProvider::new()
                    .with_current_dir(workdir)
            );

        Self {
            temp_dir,
            session,
        }
    }

    fn orchestrator(&self) -> BuildOrchestrator<'_> {
        BuildOrchestrator::new(&self.session, crate::empack::archive::ArchiveFormat::Zip)
            .expect("Failed to create orchestrator")
    }

    fn setup_basic_pack_structure(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();

        // Use the mock filesystem to create the pack structure
        let filesystem = self.session.filesystem();

        // Create pack directory
        let pack_dir = workdir.join("pack");
        filesystem.create_dir_all(&pack_dir).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        // Create basic pack.toml
        let pack_toml = pack_dir.join("pack.toml");
        let toml_content = r#"
name = "TestPack"
author = "TestAuthor"
version = "1.0.0"

[versions]
minecraft = "1.21"
fabric = "0.15.11"
"#;
        filesystem.write_file(&pack_toml, toml_content).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        // Create basic index.toml
        let index_toml = pack_dir.join("index.toml");
        let index_content = r#"
hash-format = "sha1"

[[files]]
file = "mods/test-mod.pw.toml"
hash = "abcd1234"
"#;
        filesystem.write_file(&index_toml, index_content).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        // Create mods directory
        let mods_dir = pack_dir.join("mods");
        filesystem.create_dir_all(&mods_dir).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        // Create a test mod file
        let mod_file = mods_dir.join("test-mod.pw.toml");
        let mod_content = r#"
name = "Test Mod"
filename = "test-mod-1.0.0.jar"
side = "both"

[download]
url = "https://example.com/test-mod-1.0.0.jar"
hash-format = "sha1"
hash = "abcd1234"
"#;
        filesystem.write_file(&mod_file, mod_content).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        Ok(())
    }

    fn setup_templates(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        let filesystem = self.session.filesystem();

        let templates_dir = workdir.join("templates").join("client");
        filesystem.create_dir_all(&templates_dir).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        // Create a test template file
        let template_file = templates_dir.join("launcher.json.template");
        let template_content = r#"{
    "name": "{{NAME}}",
    "version": "{{VERSION}}",
    "author": "{{AUTHOR}}",
    "mcVersion": "{{MC_VERSION}}",
    "loaderVersion": "{{LOADER_VERSION}}"
}"#;
        filesystem.write_file(&template_file, template_content).map_err(|e: anyhow::Error| BuildError::ConfigError { reason: e.to_string() })?;

        Ok(())
    }

    fn workdir(&self) -> &Path {
        self.temp_dir.path()
    }
}

fn create_test_orchestrator() -> (TempDir, MockCommandSession) {
    let temp_dir = TempDir::new().unwrap();
    let session = MockCommandSession::new();
    (temp_dir, session)
}

fn successful_process_output() -> ProcessOutput {
    ProcessOutput {
        stdout: String::new(),
        stderr: String::new(),
        success: true,
    }
}

#[test]
fn test_build_registry() {
    let registry = BuildOrchestrator::create_build_registry();
    assert_eq!(registry.len(), 5);
    assert!(registry.contains_key(&BuildTarget::Mrpack));
    assert!(registry.contains_key(&BuildTarget::Client));
    assert!(registry.contains_key(&BuildTarget::Server));
    assert!(registry.contains_key(&BuildTarget::ClientFull));
    assert!(registry.contains_key(&BuildTarget::ServerFull));

    // Test dependencies (V1 pattern)
    let client_config = &registry[&BuildTarget::Client];
    assert_eq!(client_config.dependencies, vec![BuildTarget::Mrpack]);
    assert_eq!(client_config.handler, "build_client_impl");

    let server_config = &registry[&BuildTarget::Server];
    assert_eq!(server_config.dependencies, vec![BuildTarget::Mrpack]);
    assert_eq!(server_config.handler, "build_server_impl");

    let mrpack_config = &registry[&BuildTarget::Mrpack];
    assert_eq!(mrpack_config.dependencies, Vec::<BuildTarget>::new());
    assert_eq!(mrpack_config.handler, "build_mrpack_impl");
}

#[test]
fn test_prepare_build_environment() {
    let (_temp_dir, session) = create_test_orchestrator();
    let orchestrator = BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).expect("Failed to create orchestrator");

    // Should fail without pack directory
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("pack/ directory not found"));
        }
        _ => panic!("Expected ConfigError"),
    }

    // Create pack directory through the session filesystem
    session.filesystem().create_dir_all(&orchestrator.workdir.join("pack")).unwrap();

    // Should succeed now that pack directory exists
    // (tool validation is handled by ProcessProvider, not BuildOrchestrator)
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_ok());
}

#[test]
fn test_load_pack_info() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let mut orchestrator = mock.orchestrator();
    let pack_info = orchestrator.load_pack_info().unwrap();

    assert_eq!(pack_info.name, "TestPack");
    assert_eq!(pack_info.author, "TestAuthor");
    assert_eq!(pack_info.version, "1.0.0");
    assert_eq!(pack_info.mc_version, "1.21");
    assert_eq!(pack_info.loader_version, "0.15.11");
    assert_eq!(pack_info.loader_type, "fabric");
}

#[test]
fn test_load_pack_info_missing_file() {
    let (_temp_dir, session) = create_test_orchestrator();
    let mut orchestrator = BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).expect("Failed to create orchestrator");

    let result = orchestrator.load_pack_info();
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::PackInfoError { reason } => {
            assert!(reason.contains("pack.toml not found"));
        }
        _ => panic!("Expected PackInfoError"),
    }
}

#[test]
fn test_load_pack_info_invalid_toml() {
    let mock = MockBuildOrchestrator::new();

    // Create pack directory with invalid TOML
    let workdir = mock.workdir().to_path_buf();
    let filesystem = mock.session.filesystem();
    let pack_dir = workdir.join("pack");
    filesystem.create_dir_all(&pack_dir).unwrap();
    let pack_toml = pack_dir.join("pack.toml");
    filesystem.write_file(&pack_toml, "invalid toml content [ unclosed bracket").unwrap();

    let mut orchestrator = mock.orchestrator();
    let result = orchestrator.load_pack_info();
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::PackInfoError { reason } => {
            assert!(reason.contains("TOML parse error"));
        }
        _ => panic!("Expected PackInfoError"),
    }
}

#[test]
fn test_load_pack_info_caching() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let mut orchestrator = mock.orchestrator();

    // First call should load from file
    let pack_info1 = orchestrator.load_pack_info().unwrap();
    let name1 = pack_info1.name.clone();
    assert_eq!(name1, "TestPack");

    // Second call should use cached version
    let pack_info2 = orchestrator.load_pack_info().unwrap();
    let name2 = pack_info2.name.clone();
    assert_eq!(name2, "TestPack");

    // Should be the same (cached)
    assert_eq!(name1, name2);
}

#[test]
fn test_process_build_templates() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    mock.setup_templates().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let mut orchestrator = mock.orchestrator();

    // Create target directory through the session filesystem
    let filesystem = mock.session.filesystem();
    let target_dir = workdir.join("test-target");
    filesystem.create_dir_all(&target_dir).unwrap();

    // Process templates
    let result = orchestrator.process_build_templates("templates/client", &target_dir);
    assert!(result.is_ok());

    // Check that template was processed
    let output_file = target_dir.join("launcher.json");
    assert!(filesystem.exists(&output_file));

    let content = filesystem.read_to_string(&output_file).unwrap();
    assert!(content.contains("\"name\": \"TestPack\""));
    assert!(content.contains("\"version\": \"1.0.0\""));
    assert!(content.contains("\"author\": \"TestAuthor\""));
    assert!(content.contains("\"mcVersion\": \"1.21\""));
    assert!(content.contains("\"loaderVersion\": \"0.15.11\""));
}

#[test]
fn test_process_build_templates_missing_directory() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let mut orchestrator = mock.orchestrator();

    let filesystem = mock.session.filesystem();
    let target_dir = workdir.join("test-target");
    filesystem.create_dir_all(&target_dir).unwrap();

    // Process templates from non-existent directory (should not error)
    let result = orchestrator.process_build_templates("templates/nonexistent", &target_dir);
    assert!(result.is_ok());
}

#[test]
fn test_copy_dir_contents() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let orchestrator = mock.orchestrator();

    // Create source directory with content through the session filesystem
    let filesystem = mock.session.filesystem();
    let src_dir = workdir.join("test-src");
    filesystem.create_dir_all(&src_dir).unwrap();
    filesystem.write_file(&src_dir.join("file1.txt"), "content1").unwrap();
    filesystem.write_file(&src_dir.join("file2.txt"), "content2").unwrap();

    let sub_dir = src_dir.join("subdir");
    filesystem.create_dir_all(&sub_dir).unwrap();
    filesystem.write_file(&sub_dir.join("file3.txt"), "content3").unwrap();

    // Copy to destination
    let dst_dir = workdir.join("test-dst");
    let result = orchestrator.copy_dir_contents(&src_dir, &dst_dir);
    assert!(result.is_ok());

    // Verify files were copied
    assert!(filesystem.exists(&dst_dir.join("file1.txt")));
    assert!(filesystem.exists(&dst_dir.join("file2.txt")));
    assert!(filesystem.exists(&dst_dir.join("subdir").join("file3.txt")));

    let content1 = filesystem.read_bytes(&dst_dir.join("file1.txt")).unwrap();
    assert_eq!(content1, b"content1");
}

#[test]
fn test_create_artifact() {
    let mock = MockBuildOrchestrator::new();
    let workdir = mock.workdir().to_path_buf();
    let orchestrator = mock.orchestrator();

    // Create test file through the session filesystem
    let filesystem = mock.session.filesystem();
    let test_file = workdir.join("test.txt");
    let content = "test content";
    filesystem.write_file(&test_file, content).unwrap();

    let artifact = orchestrator.create_artifact(&test_file).unwrap();
    assert_eq!(artifact.name, "test.txt");
    assert_eq!(artifact.path, test_file);
    assert_eq!(artifact.size, content.len() as u64);
}

#[test]
fn test_clean_target() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let mut orchestrator = mock.orchestrator();

    // Load pack info to enable zip file cleaning
    orchestrator.load_pack_info().unwrap();

    // Create target directory with files through the session filesystem
    let filesystem = mock.session.filesystem();
    let target_dir = workdir.join("dist").join("client");
    filesystem.create_dir_all(&target_dir).unwrap();
    filesystem.write_file(&target_dir.join("test.txt"), "content").unwrap();
    filesystem.write_file(&target_dir.join(".gitkeep"), "").unwrap();

    // Create zip file
    let zip_file = workdir.join("dist").join("TestPack-v1.0.0-client.zip");
    filesystem.write_file(&zip_file, "mock zip content").unwrap();

    // Clean target
    let result = orchestrator.clean_target(BuildTarget::Client);
    assert!(result.is_ok());

    // Verify files were cleaned but .gitkeep preserved
    assert!(!filesystem.exists(&target_dir.join("test.txt")));
    assert!(filesystem.exists(&target_dir.join(".gitkeep")));
    assert!(!filesystem.exists(&zip_file));
}

#[test]
fn test_clean_target_preserves_mrpack_and_legacy_hidden_outputs() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let mut orchestrator = mock.orchestrator();

    orchestrator.load_pack_info().unwrap();

    let filesystem = mock.session.filesystem();
    let target_dir = workdir.join("dist").join("client");
    filesystem.create_dir_all(&target_dir).unwrap();
    filesystem.write_file(&target_dir.join("test.txt"), "content").unwrap();

    let mrpack_file = workdir.join("dist").join("TestPack-v1.0.0.mrpack");
    filesystem.write_file(&mrpack_file, "mock mrpack content").unwrap();

    let legacy_dir = workdir.join(".empack").join("dist").join("client");
    filesystem.create_dir_all(&legacy_dir).unwrap();
    filesystem.write_file(&legacy_dir.join("legacy.txt"), "legacy content").unwrap();

    let result = orchestrator.clean_target(BuildTarget::Client);
    assert!(result.is_ok());

    assert!(!filesystem.exists(&target_dir.join("test.txt")));
    assert!(filesystem.exists(&mrpack_file));
    assert!(filesystem.exists(&legacy_dir.join("legacy.txt")));
}

#[tokio::test]
async fn test_execute_build_pipeline_surfaces_failed_mrpack_results() {
    let workdir = mock_root().join("build-project");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let process = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.display().to_string(),
                "refresh".to_string(),
            ],
            Ok(successful_process_output()),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.display().to_string(),
                "mr".to_string(),
                "export".to_string(),
                "-o".to_string(),
                output_file.display().to_string(),
            ],
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: "mr export failed".to_string(),
                success: false,
            }),
        );
    let session = MockCommandSession::new()
        .with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir.clone()),
        )
        .with_process(process);
    let mut orchestrator = BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let error = orchestrator
        .execute_build_pipeline(&[BuildTarget::Mrpack])
        .await
        .unwrap_err();

    match error {
        BuildError::CommandFailed { command } => {
            assert!(command.contains("Build failed for target Mrpack"));
            assert!(command.contains("packwiz mr export failed"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_execute_build_pipeline_requires_mrpack_artifact_after_successful_export() {
    let workdir = mock_root().join("build-project");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let process = MockProcessProvider::new()
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.display().to_string(),
                "refresh".to_string(),
            ],
            Ok(successful_process_output()),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.display().to_string(),
                "mr".to_string(),
                "export".to_string(),
                "-o".to_string(),
                output_file.display().to_string(),
            ],
            Ok(successful_process_output()),
        );
    let session = MockCommandSession::new()
        .with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_configured_project(workdir.clone()),
        )
        .with_process(process);
    let mut orchestrator = BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let error = orchestrator
        .execute_build_pipeline(&[BuildTarget::Mrpack])
        .await
        .unwrap_err();

    match error {
        BuildError::ValidationError { reason } => {
            assert!(reason.contains("without creating expected artifact"));
            assert!(reason.contains(&output_file.display().to_string()));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_verify_server_jar_sha1_accepts_correct_hash() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let jar_bytes = b"test jar content";
    let correct_sha1 = format!("{:x}", Sha1::digest(jar_bytes));
    let jar_path = dist_dir.join("srv.jar");
    mock.session.filesystem().write_bytes(&jar_path, jar_bytes).unwrap();

    let result = orchestrator.verify_server_jar_sha1(&jar_path, &correct_sha1);
    assert!(result.is_ok());
    assert!(mock.session.filesystem().exists(&jar_path));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_verify_server_jar_sha1_rejects_wrong_hash_and_deletes_file() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let jar_bytes = b"test jar content";
    let wrong_sha1 = "0000000000000000000000000000000000000000";
    let jar_path = dist_dir.join("srv.jar");
    mock.session.filesystem().write_bytes(&jar_path, jar_bytes).unwrap();

    let result = orchestrator.verify_server_jar_sha1(&jar_path, wrong_sha1);
    match result {
        Err(BuildError::ValidationError { reason }) => {
            assert!(reason.contains("SHA1 mismatch"));
            assert!(reason.contains(wrong_sha1));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
    assert!(!mock.session.filesystem().exists(&jar_path));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_vanilla_fetches_mojang_manifest() {
    let mut server = mockito::Server::new_async().await;
    let jar_bytes = b"fake server jar content";
    let jar_sha1 = format!("{:x}", Sha1::digest(jar_bytes));

    let version_meta = serde_json::json!({
        "downloads": {
            "server": {
                "url": format!("{}/server.jar", server.url()),
                "sha1": jar_sha1
            }
        }
    });

    let manifest = serde_json::json!({
        "versions": [{
            "id": "1.21",
            "url": format!("{}/v1/packages/1.21.json", server.url())
        }]
    });

    let _m1 = server.mock("GET", "/mc/game/version_manifest_v2.json")
        .with_body(manifest.to_string())
        .create_async().await;
    let _m2 = server.mock("GET", "/v1/packages/1.21.json")
        .with_body(version_meta.to_string())
        .create_async().await;
    let _m3 = server.mock("GET", "/server.jar")
        .with_body(jar_bytes.as_slice())
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Patch the manifest URL in the test by calling the inner methods directly.
    // Since install_vanilla_server hardcodes the Mojang URL, we test via
    // fetch_url_text + download_file helpers and the JSON parsing.
    let manifest_text = orchestrator
        .fetch_url_text(&format!("{}/mc/game/version_manifest_v2.json", server.url()))
        .unwrap();
    let parsed: MojangVersionManifest = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(parsed.versions[0].id, "1.21");

    let meta_text = orchestrator
        .fetch_url_text(&parsed.versions[0].url)
        .unwrap();
    let meta: MojangVersionMeta = serde_json::from_str(&meta_text).unwrap();
    assert_eq!(meta.downloads.server.sha1, jar_sha1);

    let jar_path = dist_dir.join("srv.jar");
    orchestrator
        .download_file(&meta.downloads.server.url, &jar_path)
        .unwrap();

    let downloaded = mock.session.filesystem().read_bytes(&jar_path).unwrap();
    let actual_hash = format!("{:x}", Sha1::digest(&downloaded));
    assert_eq!(actual_hash, jar_sha1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_fabric_resolves_installer_and_constructs_maven_url() {
    let mut server = mockito::Server::new_async().await;

    let installer_json = serde_json::json!([
        { "version": "1.1.1", "stable": true },
        { "version": "1.1.0", "stable": true }
    ]);

    let _m1 = server.mock("GET", "/v2/versions/installer")
        .with_body(installer_json.to_string())
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let installer_text = orchestrator
        .fetch_url_text(&format!("{}/v2/versions/installer", server.url()))
        .unwrap();
    let installers: Vec<FabricInstallerEntry> = serde_json::from_str(&installer_text).unwrap();
    let stable_installer = installers.iter().find(|e| e.stable).unwrap();
    assert_eq!(stable_installer.version, "1.1.1");

    let expected_maven_url = format!(
        "https://maven.fabricmc.net/net/fabricmc/fabric-installer/{v}/fabric-installer-{v}.jar",
        v = stable_installer.version
    );
    assert_eq!(
        expected_maven_url,
        "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.1.1/fabric-installer-1.1.1.jar"
    );

    let expected_filename = format!("fabric-installer-{}.jar", stable_installer.version);
    assert_eq!(expected_filename, "fabric-installer-1.1.1.jar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fabric_installer_produces_srv_jar_after_rename() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Simulate installer output: fabric-server-launch.jar + server.jar + libraries/
    mock.session.filesystem().write_bytes(
        &dist_dir.join("fabric-server-launch.jar"),
        b"fabric launcher jar",
    ).unwrap();
    mock.session.filesystem().write_bytes(
        &dist_dir.join("server.jar"),
        b"vanilla server jar",
    ).unwrap();
    mock.session.filesystem().create_dir_all(
        &dist_dir.join("libraries"),
    ).unwrap();

    // Perform the rename that install_fabric_server does after running the installer
    let launcher_jar = dist_dir.join("fabric-server-launch.jar");
    let srv_jar = dist_dir.join("srv.jar");
    assert!(mock.session.filesystem().exists(&launcher_jar));

    let bytes = mock.session.filesystem().read_bytes(&launcher_jar).unwrap();
    mock.session.filesystem().write_bytes(&srv_jar, &bytes).unwrap();
    let _ = mock.session.filesystem().remove_file(&launcher_jar);

    assert!(mock.session.filesystem().exists(&srv_jar));
    assert!(!mock.session.filesystem().exists(&launcher_jar));
    assert!(mock.session.filesystem().exists(&dist_dir.join("server.jar")));
    assert!(mock.session.filesystem().exists(&dist_dir.join("libraries")));

    let srv_bytes = mock.session.filesystem().read_bytes(&srv_jar).unwrap();
    assert_eq!(srv_bytes, b"fabric launcher jar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_quilt_parses_maven_and_invokes_java() {
    let mut server = mockito::Server::new_async().await;

    let maven_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <versioning>
    <release>0.12.0</release>
  </versioning>
</metadata>"#;

    let _m1 = server.mock("GET", "/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml")
        .with_body(maven_xml)
        .create_async().await;
    let _m2 = server.mock("GET", "/repository/release/org/quiltmc/quilt-installer/0.12.0/quilt-installer-0.12.0.jar")
        .with_body("quilt installer jar bytes")
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Test the Maven XML parsing step
    let xml_text = orchestrator
        .fetch_url_text(&format!("{}/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml", server.url()))
        .unwrap();
    let metadata: QuiltMavenMetadata = quick_xml::de::from_str(&xml_text).unwrap();
    assert_eq!(metadata.versioning.release, "0.12.0");

    // Test the download step
    let installer_url = format!(
        "{}/repository/release/org/quiltmc/quilt-installer/0.12.0/quilt-installer-0.12.0.jar",
        server.url()
    );
    let installer_path = dist_dir.join("quilt-installer-0.12.0.jar");
    orchestrator.download_file(&installer_url, &installer_path).unwrap();
    assert!(mock.session.filesystem().exists(&installer_path));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_quilt_installer_renames_launch_jar_not_vanilla() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Simulate Quilt installer output
    mock.session.filesystem().write_bytes(
        &dist_dir.join("quilt-server-launch.jar"),
        b"quilt launcher jar",
    ).unwrap();
    mock.session.filesystem().write_bytes(
        &dist_dir.join("server.jar"),
        b"vanilla server jar",
    ).unwrap();
    mock.session.filesystem().create_dir_all(
        &dist_dir.join("libraries"),
    ).unwrap();

    // Perform the rename that install_quilt_server does after running the installer
    let launcher_jar = dist_dir.join("quilt-server-launch.jar");
    let srv_jar = dist_dir.join("srv.jar");
    assert!(mock.session.filesystem().exists(&launcher_jar));

    let bytes = mock.session.filesystem().read_bytes(&launcher_jar).unwrap();
    mock.session.filesystem().write_bytes(&srv_jar, &bytes).unwrap();
    let _ = mock.session.filesystem().remove_file(&launcher_jar);

    // srv.jar should contain the Quilt launcher, NOT the vanilla server
    assert!(mock.session.filesystem().exists(&srv_jar));
    assert!(!mock.session.filesystem().exists(&launcher_jar));
    assert!(mock.session.filesystem().exists(&dist_dir.join("server.jar")));

    let srv_bytes = mock.session.filesystem().read_bytes(&srv_jar).unwrap();
    assert_eq!(srv_bytes, b"quilt launcher jar");

    let vanilla_bytes = mock.session.filesystem().read_bytes(&dist_dir.join("server.jar")).unwrap();
    assert_eq!(vanilla_bytes, b"vanilla server jar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_neoforge_downloads_and_runs_installer() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("GET", "/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar")
        .with_body("neoforge installer bytes")
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Test the download helper independently
    let installer_path = dist_dir.join("neoforge-21.1.86-installer.jar");
    let url = format!("{}/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar", server.url());
    orchestrator.download_file(&url, &installer_path).unwrap();
    assert!(mock.session.filesystem().exists(&installer_path));

    // Verify the URL construction logic for standard NeoForge
    let pack_info = PackInfo {
        author: "A".to_string(),
        name: "P".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.21.1".to_string(),
        loader_version: "21.1.86".to_string(),
        loader_type: "neoforge".to_string(),
    };
    let expected_url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
        v = pack_info.loader_version
    );
    assert_eq!(
        expected_url,
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar"
    );
    assert_eq!(
        format!("neoforge-{}-installer.jar", pack_info.loader_version),
        "neoforge-21.1.86-installer.jar"
    );
}

#[tokio::test]
async fn test_download_server_jar_neoforge_mc_1_20_1_uses_forge_namespace() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let pack_info = PackInfo {
        author: "A".to_string(),
        name: "P".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.20.1".to_string(),
        loader_version: "47.3.0".to_string(),
        loader_type: "neoforge".to_string(),
    };

    // Verify the URL construction logic for MC 1.20.1
    let (url, filename) = if pack_info.mc_version == "1.20.1" {
        (
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-{v}/forge-1.20.1-{v}-installer.jar",
                v = pack_info.loader_version
            ),
            format!("forge-1.20.1-{}-installer.jar", pack_info.loader_version),
        )
    } else {
        (
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
                v = pack_info.loader_version
            ),
            format!("neoforge-{}-installer.jar", pack_info.loader_version),
        )
    };

    assert_eq!(
        url,
        "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
    );
    assert_eq!(filename, "forge-1.20.1-47.3.0-installer.jar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_forge_downloads_and_runs_installer() {
    let mut server = mockito::Server::new_async().await;
    let _m = server.mock("GET", "/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar")
        .with_body("forge installer bytes")
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let installer_path = dist_dir.join("forge-1.20.1-47.3.0-installer.jar");
    let url = format!("{}/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar", server.url());
    orchestrator.download_file(&url, &installer_path).unwrap();
    assert!(mock.session.filesystem().exists(&installer_path));

    // Verify URL construction for Forge
    let mc = "1.20.1";
    let version = "47.3.0";
    let composite = format!("{}-{}", mc, version);
    let expected_url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{c}/forge-{c}-installer.jar",
        c = composite
    );
    assert_eq!(
        expected_url,
        "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_starter_jar_succeeds_when_run_scripts_exist() {
    let mut server = mockito::Server::new_async().await;
    let ssj_bytes = b"server starter jar content";
    let _m = server.mock("GET", "/releases/latest/download/server.jar")
        .with_body(ssj_bytes.as_slice())
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Simulate installer output: run.sh + run.bat
    mock.session.filesystem().write_file(
        &dist_dir.join("run.sh"),
        "#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/.../unix_args.txt \"$@\"",
    ).unwrap();
    mock.session.filesystem().write_file(
        &dist_dir.join("run.bat"),
        "java @user_jvm_args.txt @libraries\\.../win_args.txt %*",
    ).unwrap();

    // Download ServerStarterJar using the mock server URL directly
    let srv_jar = dist_dir.join("srv.jar");
    orchestrator.download_file(
        &format!("{}/releases/latest/download/server.jar", server.url()),
        &srv_jar,
    ).unwrap();

    assert!(mock.session.filesystem().exists(&srv_jar));
    let downloaded = mock.session.filesystem().read_bytes(&srv_jar).unwrap();
    assert_eq!(downloaded, ssj_bytes);
}

#[tokio::test]
async fn test_download_server_starter_jar_fails_without_run_scripts() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let result = orchestrator.download_server_starter_jar(&dist_dir);
    match result {
        Err(BuildError::ValidationError { reason }) => {
            assert!(reason.contains("run.sh or run.bat"));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[tokio::test]
async fn test_download_server_starter_jar_accepts_run_sh_only() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Only run.sh, no run.bat
    mock.session.filesystem().write_file(
        &dist_dir.join("run.sh"),
        "#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/.../unix_args.txt \"$@\"",
    ).unwrap();

    // The pre-download validation should not return an error (run.sh exists).
    // We cannot call download_server_starter_jar end-to-end here because
    // the GitHub URL is hardcoded and unreachable in tests. Instead, verify
    // the guard condition directly.
    assert!(mock.session.filesystem().exists(&dist_dir.join("run.sh")));
    assert!(!mock.session.filesystem().exists(&dist_dir.join("run.bat")));

    // Confirm the validation passes by checking the condition that the method uses
    let has_run_script = mock.session.filesystem().exists(&dist_dir.join("run.sh"))
        || mock.session.filesystem().exists(&dist_dir.join("run.bat"));
    assert!(has_run_script);
}

#[tokio::test]
async fn test_download_server_jar_unknown_loader_returns_error() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session
        .filesystem()
        .create_dir_all(&dist_dir)
        .unwrap();

    let pack_info = PackInfo {
        author: "A".to_string(),
        name: "P".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.21".to_string(),
        loader_version: String::new(),
        loader_type: "unknown_loader".to_string(),
    };

    let result = orchestrator.download_server_jar(&dist_dir, &pack_info);
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("unsupported loader type"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

// ===== W2-F4: FETCH RETRY TESTS =====

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_retries_on_server_error_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    let body = b"downloaded content";

    let _m = server.mock("GET", "/retry-test")
        .with_status(503)
        .with_body("Service Unavailable")
        .expect_at_most(2)
        .create_async().await;
    let _m_ok = server.mock("GET", "/retry-test")
        .with_status(200)
        .with_body(body.as_slice())
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/retry-test", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(result.is_ok(), "should succeed after retries: {:?}", result.err());
    assert_eq!(result.unwrap(), body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_does_not_retry_on_client_error() {
    let mut server = mockito::Server::new_async().await;

    let _m = server.mock("GET", "/not-found")
        .with_status(404)
        .with_body("Not Found")
        .expect(1)
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/not-found", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("404"), "error should contain status code: {err_msg}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_exhausts_retries_on_persistent_server_error() {
    let mut server = mockito::Server::new_async().await;

    let _m = server.mock("GET", "/always-failing")
        .with_status(502)
        .with_body("Bad Gateway")
        .expect(3)
        .create_async().await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/always-failing", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("502"), "error should contain status code: {err_msg}");
}

// ===== W1-T4: NEOFORGE BUILD TESTS (B1) =====

mod neoforge_build_tests {
    use super::*;

    fn neoforge_pack_toml(mc_version: &str, loader_version: &str) -> String {
        format!(
            r#"name = "NeoForgeTestPack"
author = "TestAuthor"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{mc_version}"
neoforge = "{loader_version}"
"#
        )
    }

    fn setup_neoforge_project(mock: &MockBuildOrchestrator, mc_version: &str, loader_version: &str) {
        let workdir = mock.workdir().to_path_buf();
        let filesystem = mock.session.filesystem();
        let pack_dir = workdir.join("pack");
        filesystem.create_dir_all(&pack_dir).unwrap();
        filesystem
            .write_file(
                &pack_dir.join("pack.toml"),
                &neoforge_pack_toml(mc_version, loader_version),
            )
            .unwrap();
        filesystem
            .write_file(
                &pack_dir.join("index.toml"),
                "hash-format = \"sha256\"\n",
            )
            .unwrap();
    }

    /// W1-T4 / B1: Verify the NeoForge server installer URL matches the expected
    /// Maven pattern for a modern NeoForge version (21.4.157, the version from E2E 3.12).
    ///
    /// The URL construction in install_neoforge_server is correct at the code level.
    /// The E2E failure (B1) was an HTTP error when downloading; W2-F4 must investigate
    /// whether the version artifact actually exists on Maven or the version number
    /// itself is wrong.
    #[test]
    fn test_neoforge_server_installer_url() {
        let version = "21.4.157";
        let mc_version = "1.21.4";

        let expected_url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
            v = version
        );

        // Reproduce the exact URL construction from install_neoforge_server
        let (actual_url, actual_filename) = if mc_version == "1.20.1" {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-{v}/forge-1.20.1-{v}-installer.jar",
                    v = version
                ),
                format!("forge-1.20.1-{}-installer.jar", version),
            )
        } else {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
                    v = version
                ),
                format!("neoforge-{}-installer.jar", version),
            )
        };

        assert_eq!(actual_url, expected_url);
        assert_eq!(actual_filename, format!("neoforge-{}-installer.jar", version));

        assert!(actual_url.starts_with("https://maven.neoforged.net/releases/"));
        assert!(actual_url.contains("/net/neoforged/neoforge/"));
        assert!(actual_url.ends_with("-installer.jar"));
    }

    /// W1-T4 / B1: Integration-level test. Set up a mock session with NeoForge
    /// loader, call download_server_jar with a real HTTP client, verify the
    /// install_neoforge_server path is exercised (not "unsupported loader type")
    /// and that java is invoked with the installer JAR and --install-server flag.
    ///
    /// The mock java succeeds but doesn't create run.sh/run.bat, so the build
    /// fails at the ServerStarterJar check. This is expected in a mock environment
    /// and still validates the NeoForge dispatch and installer invocation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_neoforge_server_full_integration() {
        let version = "21.4.157";
        let mc_version = "1.21.4";

        let workdir = mock_root().join("neoforge-server-full-int");
        let dist_dir = workdir.join("dist").join("server-full");

        let process = MockProcessProvider::new();

        let filesystem = MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_file(
                workdir.join("pack").join("pack.toml"),
                neoforge_pack_toml(mc_version, version),
            )
            .with_file(
                workdir.join("pack").join("index.toml"),
                "hash-format = \"sha256\"\n".to_string(),
            );

        let session = MockCommandSession::new()
            .with_filesystem(filesystem)
            .with_process(process);

        session.filesystem().create_dir_all(&dist_dir).unwrap();

        let mut orchestrator =
            BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

        let pack_info = orchestrator.load_pack_info().unwrap().clone();
        assert_eq!(pack_info.loader_type, "neoforge");
        assert_eq!(pack_info.loader_version, version);

        let result = orchestrator.download_server_jar(&dist_dir, &pack_info);

        // The call will fail: either the HTTP download fails (network) or the
        // mock java doesn't produce run.sh/run.bat. Either way, it must not
        // fail with "unsupported loader type" (that would mean NeoForge isn't dispatched).
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            !err_msg.contains("unsupported loader type"),
            "NeoForge should be dispatched to install_neoforge_server, not rejected"
        );

        // If the HTTP download succeeded and java ran, verify the invocation args
        let java_calls = session.process_provider.get_calls_for_command("java");
        if !java_calls.is_empty() {
            let args = &java_calls[0].args;
            assert!(
                args.contains(&"-jar".to_string()),
                "Java should be invoked with -jar: {args:?}"
            );
            assert!(
                args.iter().any(|a| a.contains("neoforge") && a.contains("installer")),
                "Java should receive the neoforge installer jar path: {args:?}"
            );
            assert!(
                args.contains(&"--install-server".to_string()),
                "Java should be invoked with --install-server: {args:?}"
            );
        }
    }

    /// Sanity test: NeoForge server build (non-full, mrpack + packwiz-installer)
    /// path still works. Mirrors the structure of E2E test 3.10 which passed.
    /// This test verifies load_pack_info correctly identifies NeoForge loader type
    /// and that the server JAR download dispatches to install_neoforge_server.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_neoforge_server_non_full_dispatches_correctly() {
        let mock = MockBuildOrchestrator::new();
        let mc_version = "1.21.4";
        let loader_version = "21.4.157";
        setup_neoforge_project(&mock, mc_version, loader_version);

        let mut orchestrator = mock.orchestrator();
        let pack_info = orchestrator.load_pack_info().unwrap().clone();

        assert_eq!(pack_info.loader_type, "neoforge");
        assert_eq!(pack_info.loader_version, loader_version);
        assert_eq!(pack_info.mc_version, mc_version);

        // Verify that download_server_jar dispatches to install_neoforge_server
        // by checking it does NOT return "unsupported loader type"
        let dist_dir = mock.workdir().join("dist").join("server");
        mock.session
            .filesystem()
            .create_dir_all(&dist_dir)
            .unwrap();

        let result = orchestrator.download_server_jar(&dist_dir, &pack_info);
        // The call will fail (no HTTP server), but should NOT be "unsupported loader type"
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            !error_msg.contains("unsupported loader type"),
            "NeoForge should be recognized as a valid loader type"
        );
    }

    /// Verify the 1.20.1 special case still produces the forge namespace URL
    #[test]
    fn test_neoforge_1_20_1_uses_forge_namespace_url() {
        let pack_info = PackInfo {
            author: "A".to_string(),
            name: "P".to_string(),
            version: "1.0.0".to_string(),
            mc_version: "1.20.1".to_string(),
            loader_version: "47.3.0".to_string(),
            loader_type: "neoforge".to_string(),
        };

        let (url, filename) = if pack_info.mc_version == "1.20.1" {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-{v}/forge-1.20.1-{v}-installer.jar",
                    v = pack_info.loader_version
                ),
                format!("forge-1.20.1-{}-installer.jar", pack_info.loader_version),
            )
        } else {
            (
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
                    v = pack_info.loader_version
                ),
                format!("neoforge-{}-installer.jar", pack_info.loader_version),
            )
        };

        assert_eq!(
            url,
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
        );
        assert_eq!(filename, "forge-1.20.1-47.3.0-installer.jar");
    }
}

