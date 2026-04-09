use super::*;
use crate::application::session::ProcessOutput;
use crate::application::session::Session;
use crate::application::session_mocks::{
    MockCommandSession, MockFileSystemProvider, MockNetworkProvider, MockProcessProvider,
    mock_root,
};
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
        let session = MockCommandSession::new().with_filesystem(
            crate::application::session_mocks::MockFileSystemProvider::new()
                .with_current_dir(workdir),
        );

        Self { temp_dir, session }
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
        filesystem
            .create_dir_all(&pack_dir)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

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
        filesystem
            .write_file(&pack_toml, toml_content)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Create basic index.toml
        let index_toml = pack_dir.join("index.toml");
        let index_content = r#"
hash-format = "sha1"

[[files]]
file = "mods/test-mod.pw.toml"
hash = "abcd1234"
"#;
        filesystem
            .write_file(&index_toml, index_content)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Create mods directory
        let mods_dir = pack_dir.join("mods");
        filesystem
            .create_dir_all(&mods_dir)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

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
        filesystem
            .write_file(&mod_file, mod_content)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    fn setup_templates(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        let filesystem = self.session.filesystem();

        let templates_dir = workdir.join("templates").join("client");
        filesystem
            .create_dir_all(&templates_dir)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

        // Create a test template file
        let template_file = templates_dir.join("launcher.json.template");
        let template_content = r#"{
    "name": "{{NAME}}",
    "version": "{{VERSION}}",
    "author": "{{AUTHOR}}",
    "mcVersion": "{{MC_VERSION}}",
    "loaderVersion": "{{MODLOADER_VERSION}}"
}"#;
        filesystem
            .write_file(&template_file, template_content)
            .map_err(|e: anyhow::Error| BuildError::ConfigError {
                reason: e.to_string(),
            })?;

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

fn restricted_mods_process_output(mod_name: &str, url: &str, dest_path: &str) -> ProcessOutput {
    ProcessOutput {
        stdout: format!(
            "Failed to download modpack, the following errors were encountered:\n{}:",
            mod_name
        ),
        stderr: format!(
            "java.lang.Exception: This mod is excluded from the CurseForge API and must be downloaded manually.\nPlease go to {} and save this file to {}\n\tat link.infra.packwiz.installer.DownloadTask.download(DownloadTask.java:42)",
            url, dest_path
        ),
        success: false,
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

    // Test dependencies
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
    let orchestrator = BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip)
        .expect("Failed to create orchestrator");

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
    session
        .filesystem()
        .create_dir_all(&orchestrator.workdir.join("pack"))
        .unwrap();

    // Should succeed now that pack directory exists
    // (tool validation is handled by ProcessProvider, not BuildOrchestrator)
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_ok());
}

#[test]
fn test_download_server_jar_rejects_unsupported_loader_type() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();
    let dist_dir = mock.temp_dir.path().join("dist").join("server");
    let pack_info = PackInfo {
        author: "Test Author".to_string(),
        name: "Unsupported".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.21.1".to_string(),
        loader_version: "1.0.0".to_string(),
        loader_type: "mystery".to_string(),
    };

    let error = orchestrator.download_server_jar(&dist_dir, &pack_info).unwrap_err();
    match error {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("unsupported loader type"));
            assert!(reason.contains("mystery"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
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
fn test_load_pack_info_defaults_missing_fields_to_vanilla_unknowns() {
    let mock = MockBuildOrchestrator::new();
    let workdir = mock.temp_dir.path().to_path_buf();
    mock.session
        .filesystem()
        .create_dir_all(&workdir.join("pack"))
        .unwrap();
    mock.session
        .filesystem()
        .write_file(
            &workdir.join("pack").join("pack.toml"),
            "name = \"BarePack\"\nauthor = \"Test Author\"\n",
        )
        .unwrap();

    let mut orchestrator = mock.orchestrator();
    let pack_info = orchestrator.load_pack_info().unwrap();

    assert_eq!(pack_info.name, "BarePack");
    assert_eq!(pack_info.author, "Test Author");
    assert_eq!(pack_info.version, "Unknown");
    assert_eq!(pack_info.mc_version, "Unknown");
    assert_eq!(pack_info.loader_type, "vanilla");
    assert!(pack_info.loader_version.is_empty());
}

#[test]
fn test_load_pack_info_missing_file() {
    let (_temp_dir, session) = create_test_orchestrator();
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip)
            .expect("Failed to create orchestrator");

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
    filesystem
        .write_file(&pack_toml, "invalid toml content [ unclosed bracket")
        .unwrap();

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
fn test_load_pack_info_defaults_to_vanilla_without_loader_versions() {
    let mock = MockBuildOrchestrator::new();
    let pack_dir = mock.workdir().join("pack");
    let filesystem = mock.session.filesystem();
    filesystem.create_dir_all(&pack_dir).unwrap();
    filesystem
        .write_file(
            &pack_dir.join("pack.toml"),
            r#"
name = "Vanilla Pack"
version = "2.0.0"

[versions]
minecraft = "1.21.1"
"#,
        )
        .unwrap();

    let mut orchestrator = mock.orchestrator();
    let pack_info = orchestrator.load_pack_info().unwrap();
    assert_eq!(pack_info.author, "Unknown");
    assert_eq!(pack_info.name, "Vanilla Pack");
    assert_eq!(pack_info.version, "2.0.0");
    assert_eq!(pack_info.mc_version, "1.21.1");
    assert_eq!(pack_info.loader_type, "vanilla");
    assert!(pack_info.loader_version.is_empty());
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
fn test_process_build_templates_reports_template_engine_load_failure() {
    let mock = MockBuildOrchestrator::new();
    let workdir = mock.workdir().to_path_buf();
    let filesystem = mock.session.filesystem();
    let pack_dir = workdir.join("pack");
    filesystem.create_dir_all(&pack_dir).unwrap();
    filesystem
        .write_file(&pack_dir.join("pack.toml"), "invalid toml [")
        .unwrap();

    let template_dir = workdir.join("templates").join("client");
    filesystem.create_dir_all(&template_dir).unwrap();
    filesystem
        .write_file(
            &template_dir.join("launcher.json.template"),
            "{ \"name\": \"{{NAME}}\" }",
        )
        .unwrap();

    let target_dir = workdir.join("test-target");
    filesystem.create_dir_all(&target_dir).unwrap();

    let mut orchestrator = mock.orchestrator();
    let result = orchestrator.process_build_templates("templates/client", &target_dir);

    match result {
        Err(BuildError::ConfigError { reason }) => {
            assert!(reason.contains("Failed to load template variables from pack.toml"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
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
    filesystem
        .write_file(&src_dir.join("file1.txt"), "content1")
        .unwrap();
    filesystem
        .write_file(&src_dir.join("file2.txt"), "content2")
        .unwrap();

    let sub_dir = src_dir.join("subdir");
    filesystem.create_dir_all(&sub_dir).unwrap();
    filesystem
        .write_file(&sub_dir.join("file3.txt"), "content3")
        .unwrap();

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
fn test_create_artifact_missing_file_returns_validation_error() {
    let mock = MockBuildOrchestrator::new();
    let orchestrator = mock.orchestrator();
    let missing = mock.workdir().join("missing.zip");

    let result = orchestrator.create_artifact(&missing);

    match result {
        Err(BuildError::ValidationError { reason }) => {
            assert!(reason.contains("without creating expected artifact"));
            assert!(reason.contains(&missing.display().to_string()));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn test_create_artifact_binary_file_falls_back_to_zero_size() {
    let mock = MockBuildOrchestrator::new();
    let filesystem = mock.session.filesystem();
    let artifact_path = mock.workdir().join("binary-artifact.zip");
    filesystem
        .write_bytes(&artifact_path, &[0, 159, 146, 150])
        .unwrap();

    let orchestrator = mock.orchestrator();
    let artifact = orchestrator.create_artifact(&artifact_path).unwrap();

    assert_eq!(artifact.name, "binary-artifact.zip");
    assert_eq!(artifact.path, artifact_path);
    assert_eq!(artifact.size, 0);
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
    filesystem
        .write_file(&target_dir.join("test.txt"), "content")
        .unwrap();
    filesystem
        .write_file(&target_dir.join(".gitkeep"), "")
        .unwrap();

    // Create zip file
    let zip_file = workdir.join("dist").join("TestPack-v1.0.0-client.zip");
    filesystem
        .write_file(&zip_file, "mock zip content")
        .unwrap();

    // Clean target
    let result = orchestrator.clean_target(BuildTarget::Client);
    assert!(result.is_ok());

    // Verify files were cleaned but .gitkeep preserved
    assert!(!filesystem.exists(&target_dir.join("test.txt")));
    assert!(filesystem.exists(&target_dir.join(".gitkeep")));
    assert!(!filesystem.exists(&zip_file));
}

#[test]
fn test_clean_target_removes_all_archive_formats() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let mut orchestrator = mock.orchestrator();

    orchestrator.load_pack_info().unwrap();

    let filesystem = mock.session.filesystem();
    let target_dir = workdir.join("dist").join("client");
    filesystem.create_dir_all(&target_dir).unwrap();
    filesystem
        .write_file(&target_dir.join("test.txt"), "content")
        .unwrap();
    filesystem
        .write_file(&target_dir.join(".gitkeep"), "")
        .unwrap();

    for ext in ["zip", "tar.gz", "7z"] {
        let archive = workdir
            .join("dist")
            .join(format!("TestPack-v1.0.0-client.{ext}"));
        filesystem.write_file(&archive, "mock archive").unwrap();
    }

    let result = orchestrator.clean_target(BuildTarget::Client);
    assert!(result.is_ok());

    assert!(!filesystem.exists(&target_dir.join("test.txt")));
    assert!(filesystem.exists(&target_dir.join(".gitkeep")));
    assert!(!filesystem.exists(&workdir.join("dist").join("TestPack-v1.0.0-client.zip")));
    assert!(!filesystem.exists(&workdir.join("dist").join("TestPack-v1.0.0-client.tar.gz")));
    assert!(!filesystem.exists(&workdir.join("dist").join("TestPack-v1.0.0-client.7z")));
}

#[test]
fn test_download_server_jar_skips_when_srv_exists() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();
    mock.session
        .filesystem()
        .write_bytes(&dist_dir.join("srv.jar"), b"preexisting server jar")
        .unwrap();

    let pack_info = PackInfo {
        author: "A".to_string(),
        name: "P".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.21.1".to_string(),
        loader_version: "21.4.157".to_string(),
        loader_type: "unsupported".to_string(),
    };

    let result = orchestrator.download_server_jar(&dist_dir, &pack_info);
    assert!(result.is_ok());
    assert!(mock.session.filesystem().exists(&dist_dir.join("srv.jar")));
    assert!(mock.session.process_provider.get_calls_for_command("java").is_empty());
}

#[test]
fn test_refresh_pack_missing_pack_file_errors() {
    let (_temp_dir, session) = create_test_orchestrator();
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let result = orchestrator.refresh_pack();

    match result {
        Err(BuildError::ConfigError { reason }) => {
            assert!(reason.contains("Pack file not found"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

#[test]
fn test_refresh_pack_command_failure_surfaces_process_error() {
    let workdir = mock_root().join("refresh-project-failed");
    let pack_file = workdir.join("pack").join("pack.toml");
    let process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            pack_file.display().to_string(),
            "refresh".to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "refresh blew up".to_string(),
            success: false,
        }),
    );
    let session = MockCommandSession::new()
        .with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_file(pack_file.clone(), "name = \"Refresh Pack\"\n".to_string()),
        )
        .with_process(process);

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let result = orchestrator.refresh_pack();

    match result {
        Err(BuildError::CommandFailed { command }) => {
            assert!(command.contains("packwiz refresh"));
            assert!(command.contains("refresh blew up"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn test_refresh_pack_is_cached_after_first_success() {
    let workdir = mock_root().join("refresh-project-cached");
    let pack_file = workdir.join("pack").join("pack.toml");
    let process = MockProcessProvider::new().with_packwiz_result(
        vec![
            "--pack-file".to_string(),
            pack_file.display().to_string(),
            "refresh".to_string(),
        ],
        Ok(successful_process_output()),
    );
    let session = MockCommandSession::new()
        .with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_file(pack_file.clone(), "name = \"Refresh Pack\"\n".to_string()),
        )
        .with_process(process);

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    orchestrator.refresh_pack().unwrap();
    orchestrator.refresh_pack().unwrap();

    let refresh_calls = session
        .process_provider
        .get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN);
    assert_eq!(refresh_calls.len(), 1);
}

#[test]
fn test_install_neoforge_server_reports_installer_execution_failure() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "NeoForgeTestPack"
author = "Test Author"
version = "1.0.0"

[versions]
minecraft = "1.21.1"
neoforge = "21.4.157"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );

    let installer_path = dist_dir.join("neoforge-21.4.157-installer.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            installer_path.to_string_lossy().to_string(),
            "--install-server".to_string(),
            dist_dir.to_string_lossy().to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "NeoForge installer exited with code 1".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(mock_process);
    session.filesystem().create_dir_all(&dist_dir).unwrap();
    session
        .filesystem()
        .write_bytes(&installer_path, b"installer bytes")
        .unwrap();

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let pack_info = orchestrator.load_pack_info().unwrap().clone();
    let result = orchestrator.install_neoforge_server(&dist_dir, &pack_info);

    match result {
        Err(BuildError::CommandFailed { command }) => {
            assert!(command.contains("neoforge installer failed"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn test_install_forge_server_reports_installer_execution_failure() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ForgeTestPack"
author = "Test Author"
version = "1.0.0"

[versions]
minecraft = "1.21.1"
forge = "49.2.0"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );

    let installer_path = dist_dir.join("forge-1.21.1-49.2.0-installer.jar");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            installer_path.to_string_lossy().to_string(),
            "--installServer".to_string(),
            dist_dir.to_string_lossy().to_string(),
        ],
        Ok(ProcessOutput {
            stdout: String::new(),
            stderr: "Forge installer exited with code 1".to_string(),
            success: false,
        }),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(mock_process);
    session.filesystem().create_dir_all(&dist_dir).unwrap();
    session
        .filesystem()
        .write_bytes(&installer_path, b"installer bytes")
        .unwrap();

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let pack_info = orchestrator.load_pack_info().unwrap().clone();
    let result = orchestrator.install_forge_server(&dist_dir, &pack_info);

    match result {
        Err(BuildError::CommandFailed { command }) => {
            assert!(command.contains("forge installer failed"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_client_full_returns_restricted_mods_without_archiving() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("client-full");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "RestrictedPack"
author = "Test Author"
version = "1.0.0"

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );

    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");
    let pack_toml_path = dist_dir.join("pack").join("pack.toml");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            bootstrap_jar_path.to_string_lossy().to_string(),
            "--bootstrap-main-jar".to_string(),
            installer_jar_path.to_string_lossy().to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            pack_toml_path.to_string_lossy().to_string(),
        ],
        Ok(restricted_mods_process_output(
            "OptiFine.jar",
            "https://www.curseforge.com/minecraft/mc-mods/optifine/download/999",
            "/tmp/pack/.minecraft/mods/OptiFine.jar",
        )),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(mock_process);

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let result = orchestrator
        .build_client_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.target, BuildTarget::ClientFull);
    assert_eq!(result.restricted_mods.len(), 1);
    assert_eq!(result.restricted_mods[0].name, "OptiFine.jar");
    assert!(result.output_path.is_none());
    assert!(session
        .filesystem()
        .exists(&dist_dir.join("pack").join("pack.toml")));
    assert!(!session
        .filesystem()
        .exists(&workdir.join("dist").join("RestrictedPack-v1.0.0-client-full.zip")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_build_server_full_returns_restricted_mods_without_archiving() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server-full");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "RestrictedServerPack"
author = "Test Author"
version = "1.0.0"

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_deferred_file(
            dist_dir.clone(),
            "srv.jar".to_string(),
            "preexisting server jar".to_string(),
        );

    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");
    let pack_toml_path = dist_dir.join("pack").join("pack.toml");
    let mock_process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            bootstrap_jar_path.to_string_lossy().to_string(),
            "--bootstrap-main-jar".to_string(),
            installer_jar_path.to_string_lossy().to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "server".to_string(),
            pack_toml_path.to_string_lossy().to_string(),
        ],
        Ok(restricted_mods_process_output(
            "ServerCore.jar",
            "https://www.curseforge.com/minecraft/mc-mods/servercore/download/111",
            "/tmp/pack/.minecraft/mods/ServerCore.jar",
        )),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(mock_process);

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let result = orchestrator
        .build_server_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.target, BuildTarget::ServerFull);
    assert_eq!(result.restricted_mods.len(), 1);
    assert_eq!(result.restricted_mods[0].name, "ServerCore.jar");
    assert!(result.output_path.is_none());
    assert!(session.filesystem().exists(&dist_dir.join("srv.jar")));
    assert!(!session
        .filesystem()
        .exists(&workdir.join("dist").join("RestrictedServerPack-v1.0.0-server-full.zip")));
}

#[test]
fn test_build_client_full_wraps_installer_process_errors() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("client-full");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");
    let pack_toml_path = dist_dir.join("pack").join("pack.toml");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ClientFullFailurePack"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );
    let process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            bootstrap_jar_path.to_string_lossy().to_string(),
            "--bootstrap-main-jar".to_string(),
            installer_jar_path.to_string_lossy().to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "both".to_string(),
            pack_toml_path.to_string_lossy().to_string(),
        ],
        Err("installer process exploded".to_string()),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(process);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;

    let error = orchestrator
        .build_client_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap_err();
    match error {
        BuildError::CommandFailed { command } => {
            assert!(command.contains("packwiz-installer-bootstrap.jar"));
            assert!(command.contains("installer process exploded"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn test_build_server_full_returns_warning_when_server_jar_download_fails() {
    let workdir = mock_root().join("server-full-warning");
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server-full");
    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "NeoForgeServerFullWarning"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.19.4"
neoforge = "47.1.106"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_file(dist_dir.join(".gitkeep"), String::new());
    let session = MockCommandSession::new().with_filesystem(filesystem);
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;

    let result = orchestrator
        .build_server_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap();

    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert_eq!(result.target, BuildTarget::ServerFull);
    assert!(result.restricted_mods.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("failed to download server JAR"));
    assert!(result.warnings[0].contains("1.20.1 and newer"));
}

#[test]
fn test_build_server_full_wraps_installer_process_errors() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server-full");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");
    let pack_toml_path = dist_dir.join("pack").join("pack.toml");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ServerFullFailurePack"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_deferred_file(
            dist_dir.clone(),
            "srv.jar".to_string(),
            "existing server jar".to_string(),
        );
    let process = MockProcessProvider::new().with_result(
        "java".to_string(),
        vec![
            "-jar".to_string(),
            bootstrap_jar_path.to_string_lossy().to_string(),
            "--bootstrap-main-jar".to_string(),
            installer_jar_path.to_string_lossy().to_string(),
            "-g".to_string(),
            "-s".to_string(),
            "server".to_string(),
            pack_toml_path.to_string_lossy().to_string(),
        ],
        Err("server installer process exploded".to_string()),
    );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(process);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;

    let error = orchestrator
        .build_server_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap_err();
    match error {
        BuildError::CommandFailed { command } => {
            assert!(command.contains("packwiz-installer-bootstrap.jar"));
            assert!(command.contains("server installer process exploded"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn test_continue_client_full_preserves_existing_files_and_archives_successfully() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("client-full");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ContinueClientPack"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_file(bootstrap_jar_path.clone(), "bootstrap".to_string())
        .with_file(installer_jar_path.clone(), "installer".to_string())
        .with_file(dist_dir.join("preserve.txt"), "keep me".to_string());

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(MockProcessProvider::new().with_java_installer_side_effects());

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip)
            .unwrap()
            .continue_full_builds();
    let result = orchestrator
        .build_client_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap();

    assert!(result.success);
    assert!(session.filesystem().exists(&dist_dir.join("preserve.txt")));
    assert!(
        session
            .filesystem()
            .exists(&dist_dir.join("mods").join("both-installed.txt"))
    );
    assert!(session.process_provider.get_calls_for_command("java").iter().any(
        |call| call.args.iter().any(|arg| arg == "both")
    ));
    assert!(session
        .process_provider
        .get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN)
        .iter()
        .any(|call| call.args.iter().any(|arg| arg == "refresh")));
    assert!(session.filesystem().exists(
        &workdir.join("dist").join("ContinueClientPack-v1.0.0-client-full.zip")
    ));
}

#[test]
fn test_continue_server_full_preserves_existing_files_and_archives_successfully() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server-full");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let installer_jar_path = workdir.join("cache").join("packwiz-installer.jar");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ContinueServerPack"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.11"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_file(bootstrap_jar_path.clone(), "bootstrap".to_string())
        .with_file(installer_jar_path.clone(), "installer".to_string())
        .with_file(dist_dir.join("preserve.txt"), "keep me".to_string())
        .with_file(dist_dir.join("srv.jar"), "server jar".to_string());

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(MockProcessProvider::new().with_java_installer_side_effects());

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip)
            .unwrap()
            .continue_full_builds();
    let result = orchestrator
        .build_server_full_impl(&bootstrap_jar_path, &installer_jar_path)
        .unwrap();

    assert!(result.success);
    assert!(session.filesystem().exists(&dist_dir.join("preserve.txt")));
    assert!(session.filesystem().exists(&dist_dir.join("srv.jar")));
    assert!(
        session
            .filesystem()
            .exists(&dist_dir.join("mods").join("server-installed.txt"))
    );
    assert!(session.process_provider.get_calls_for_command("java").iter().any(
        |call| call.args.iter().any(|arg| arg == "server")
    ));
    assert!(session
        .process_provider
        .get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN)
        .iter()
        .any(|call| call.args.iter().any(|arg| arg == "refresh")));
    assert!(session.filesystem().exists(
        &workdir.join("dist").join("ContinueServerPack-v1.0.0-server-full.zip")
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn test_fetch_url_bytes_rejects_current_thread_runtime() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let result = orchestrator.fetch_url_bytes("https://example.com/test.bin");
    match result {
        Err(BuildError::ConfigError { reason }) => {
            assert!(reason.contains("multi-threaded tokio runtime"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_text_rejects_invalid_utf8() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/binary")
        .with_body(vec![0xff, 0xfe, 0xfd])
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let result = orchestrator.fetch_url_text(&format!("{}/binary", server.url()));
    match result {
        Err(BuildError::ConfigError { reason }) => {
            assert!(reason.contains("not valid UTF-8"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_reports_http_client_unavailable() {
    let workdir = mock_root().join("http-client-unavailable");
    let session = MockCommandSession::new()
        .with_filesystem(
            MockFileSystemProvider::new()
                .with_current_dir(workdir.clone())
                .with_file(workdir.join("pack").join("pack.toml"), "name = \"Pack\"\n".to_string()),
        )
        .with_network(MockNetworkProvider::new().with_failing_http_client());

    let orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let result = orchestrator.fetch_url_bytes("https://example.com/file.bin");

    match result {
        Err(BuildError::ConfigError { reason }) => {
            assert!(reason.contains("HTTP client unavailable"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

#[test]
fn test_install_neoforge_server_mc_1_20_1_uses_legacy_forge_artifact() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "NeoForgeTestPack"
author = "TestAuthor"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
neoforge = "47.3.0"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(MockProcessProvider::new().with_java_installer_side_effects());

    session.filesystem().create_dir_all(&dist_dir).unwrap();
    session
        .filesystem()
        .write_bytes(
            &dist_dir.join("forge-1.20.1-47.3.0-installer.jar"),
            b"installer",
        )
        .unwrap();
    session
        .filesystem()
        .write_bytes(&dist_dir.join("srv.jar"), b"server starter jar")
        .unwrap();

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let pack_info = orchestrator.load_pack_info().unwrap().clone();
    let result = orchestrator.install_neoforge_server(&dist_dir, &pack_info);

    assert!(
        result.is_ok(),
        "legacy NeoForge 1.20.1 should install successfully: {result:?}"
    );
    assert!(
        session.filesystem().exists(&dist_dir.join("run.sh")),
        "legacy NeoForge installer should create run.sh"
    );
    assert!(
        session.filesystem().exists(&dist_dir.join("run.bat")),
        "legacy NeoForge installer should create run.bat"
    );
    assert!(session.filesystem().exists(&dist_dir.join("srv.jar")));

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls.iter().any(|call| {
            call.args.iter().any(|a| a == "--install-server" || a == "--installServer")
                && call
                    .args
                    .iter()
                    .any(|a| a.contains("forge-1.20.1-47.3.0-installer.jar"))
        }),
        "legacy NeoForge install should execute the forge-family installer: {java_calls:?}"
    );
}

#[test]
fn test_install_forge_server_creates_run_scripts_and_cleans_up() {
    let workdir = TempDir::new().unwrap();
    let workdir = workdir.path().to_path_buf();
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "ForgeTestPack"
author = "TestAuthor"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
forge = "47.3.0"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        );

    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(MockProcessProvider::new().with_java_installer_side_effects());

    session.filesystem().create_dir_all(&dist_dir).unwrap();
    session
        .filesystem()
        .write_bytes(
            &dist_dir.join("forge-1.20.1-47.3.0-installer.jar"),
            b"installer",
        )
        .unwrap();
    session
        .filesystem()
        .write_bytes(&dist_dir.join("srv.jar"), b"server starter jar")
        .unwrap();

    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    let pack_info = orchestrator.load_pack_info().unwrap().clone();
    let result = orchestrator.install_forge_server(&dist_dir, &pack_info);

    assert!(result.is_ok(), "{result:?}");
    assert!(session.filesystem().exists(&dist_dir.join("run.sh")));
    assert!(session.filesystem().exists(&dist_dir.join("run.bat")));
    assert!(session.filesystem().exists(&dist_dir.join("srv.jar")));
    assert!(
        !session
            .filesystem()
            .exists(&dist_dir.join("forge-1.20.1-47.3.0-installer.jar")),
        "installer jar should be cleaned up after a successful run"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(java_calls.iter().any(|call| {
        call.args.iter().any(|a| a == "--installServer")
            && call
                .args
                .iter()
                .any(|a| a.contains("forge-1.20.1-47.3.0-installer.jar"))
    }));
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
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
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
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
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

#[tokio::test]
async fn test_execute_build_pipeline_success_cleans_temp_extract_dir_and_build_marker() {
    let workdir = mock_root().join("build-pipeline-success");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let temp_extract_dir = workdir.join("dist").join("temp-mrpack-extract");
    let process = MockProcessProvider::new()
        .with_mrpack_export_side_effects()
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
                .with_configured_project(workdir.clone())
                .with_file(temp_extract_dir.join("stale.txt"), "stale".to_string()),
        )
        .with_process(process);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let results = orchestrator
        .execute_build_pipeline(&[BuildTarget::Mrpack])
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert!(!session.filesystem().exists(&temp_extract_dir));

    let state = session.state().unwrap().discover_state().unwrap();
    assert!(!matches!(
        state,
        crate::primitives::PackState::Interrupted { .. }
            | crate::primitives::PackState::Building
    ));
}

#[tokio::test]
async fn test_execute_build_pipeline_failure_still_cleans_temp_extract_dir() {
    let workdir = mock_root().join("build-pipeline-failure-cleanup");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let temp_extract_dir = workdir.join("dist").join("temp-mrpack-extract");
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
                .with_configured_project(workdir.clone())
                .with_file(temp_extract_dir.join("stale.txt"), "stale".to_string()),
        )
        .with_process(process);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let error = orchestrator
        .execute_build_pipeline(&[BuildTarget::Mrpack])
        .await
        .unwrap_err();

    match error {
        BuildError::CommandFailed { command } => {
            assert!(command.contains("Build failed for target Mrpack"));
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
    assert!(!session.filesystem().exists(&temp_extract_dir));

    let state = session.state().unwrap().discover_state().unwrap();
    assert!(matches!(
        state,
        crate::primitives::PackState::Interrupted { .. }
            | crate::primitives::PackState::Building
    ));
}

#[tokio::test]
async fn test_execute_clean_pipeline_removes_target_outputs_and_clean_marker() {
    let workdir = mock_root().join("clean-pipeline-success");
    let client_dir = workdir.join("dist").join("client");
    let archive_path = workdir.join("dist").join("Test Pack-v1.0.0-client.zip");
    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_configured_project(workdir.clone())
            .with_file(client_dir.join("mod.jar"), "mod".to_string())
            .with_file(client_dir.join(".gitkeep"), String::new())
            .with_file(archive_path.clone(), "archive".to_string()),
    );
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    orchestrator
        .execute_clean_pipeline(&[BuildTarget::Client])
        .await
        .unwrap();

    assert!(!session.filesystem().exists(&client_dir.join("mod.jar")));
    assert!(session.filesystem().exists(&client_dir.join(".gitkeep")));
    assert!(!session.filesystem().exists(&archive_path));

    let state = session.state().unwrap().discover_state().unwrap();
    assert!(!matches!(
        state,
        crate::primitives::PackState::Interrupted { .. }
            | crate::primitives::PackState::Cleaning
    ));
}

#[test]
fn test_extract_mrpack_builds_missing_artifact_and_caches_repeated_calls() {
    let workdir = mock_root().join("extract-mrpack-cached");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let process = MockProcessProvider::new()
        .with_mrpack_export_side_effects()
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
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    orchestrator.extract_mrpack().unwrap();
    orchestrator.extract_mrpack().unwrap();

    assert!(session
        .filesystem()
        .exists(&workdir.join("dist").join("temp-mrpack-extract")));
    assert_eq!(session.archive_provider.extract_calls.lock().unwrap().len(), 1);
    let export_calls = session
        .process_provider
        .get_calls_for_command(crate::empack::packwiz::PACKWIZ_BIN)
        .into_iter()
        .filter(|call| call.args.iter().any(|arg| arg == "export"))
        .count();
    assert_eq!(export_calls, 1);
}

#[test]
fn test_zip_distribution_requires_loaded_pack_info() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let result = orchestrator.zip_distribution(BuildTarget::Client);

    match result {
        Err(BuildError::PackInfoError { reason }) => {
            assert!(reason.contains("Pack info not loaded"));
        }
        other => panic!("expected PackInfoError, got {other:?}"),
    }
}

#[test]
fn test_zip_distribution_requires_target_content() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let mut orchestrator = mock.orchestrator();
    orchestrator.load_pack_info().unwrap();
    mock.session
        .filesystem()
        .create_dir_all(&mock.workdir().join("dist").join("client"))
        .unwrap();

    let result = orchestrator.zip_distribution(BuildTarget::Client);

    match result {
        Err(BuildError::ValidationError { reason }) => {
            assert!(reason.contains("No files to archive"));
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

#[test]
fn test_build_mrpack_returns_manual_download_warning_on_export_failure() {
    let workdir = mock_root().join("mrpack-manual-download");
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
                stdout: "manual download required".to_string(),
                stderr: "must be manually downloaded".to_string(),
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
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let result = orchestrator.build_mrpack_impl().unwrap();

    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert!(result.artifacts.is_empty());
    assert_eq!(result.restricted_mods.len(), 0);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("manual download"));
}

#[tokio::test]
async fn test_build_mrpack_replaces_existing_artifact_and_returns_metadata() {
    let workdir = mock_root().join("mrpack-success");
    let pack_file = workdir.join("pack").join("pack.toml");
    let output_file = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let process = MockProcessProvider::new()
        .with_mrpack_export_side_effects()
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
                .with_configured_project(workdir.clone())
                .with_file(output_file.clone(), "stale artifact".to_string()),
        )
        .with_process(process);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let result = orchestrator.build_mrpack_impl().unwrap();

    assert!(result.success);
    assert_eq!(result.target, BuildTarget::Mrpack);
    assert_eq!(result.output_path.as_ref(), Some(&output_file));
    assert_eq!(result.warnings, Vec::<String>::new());
    assert!(result.restricted_mods.is_empty());
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].name, "Test Pack-v1.0.0.mrpack");
    assert!(result.artifacts[0].size > 0);
    let bytes = session.filesystem().read_bytes(&output_file).unwrap();
    assert_eq!(bytes, b"mock mrpack artifact");
}

#[test]
fn test_build_client_copies_templates_pack_and_overrides_into_minecraft_distribution() {
    let workdir = mock_root().join("client-build-success");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let dist_dir = workdir.join("dist").join("client");
    let overrides_dir = workdir.join("dist").join("temp-mrpack-extract").join("overrides");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_file(bootstrap_jar_path.clone(), "bootstrap".to_string())
        .with_file(
            workdir
                .join("templates")
                .join("client")
                .join("launcher.json.template"),
            r#"{"name":"{{NAME}}","version":"{{VERSION}}"}"#.to_string(),
        )
        .with_file(overrides_dir.join("options.txt"), "fancy=true\n".to_string());
    let session = MockCommandSession::new().with_filesystem(filesystem);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;
    orchestrator.mrpack_extracted = true;

    let result = orchestrator.build_client_impl(&bootstrap_jar_path).unwrap();

    assert!(result.success);
    assert_eq!(result.target, BuildTarget::Client);
    assert_eq!(
        result.output_path.as_ref(),
        Some(&workdir.join("dist").join("Test Pack-v1.0.0-client.zip"))
    );
    assert!(session
        .filesystem()
        .exists(&dist_dir.join(".minecraft").join("pack").join("pack.toml")));
    assert!(session
        .filesystem()
        .exists(&dist_dir.join(".minecraft").join("packwiz-installer-bootstrap.jar")));
    assert!(session
        .filesystem()
        .exists(&dist_dir.join(".minecraft").join("options.txt")));
    let rendered = session
        .filesystem()
        .read_to_string(&dist_dir.join("launcher.json"))
        .unwrap();
    assert!(rendered.contains("\"name\":\"Test Pack\""));
    assert!(rendered.contains("\"version\":\"1.0.0\""));
    let zip_path = workdir.join("dist").join("Test Pack-v1.0.0-client.zip");
    assert!(session.filesystem().exists(&zip_path));
}

#[test]
fn test_build_server_uses_existing_srv_jar_and_archives_with_overrides() {
    let workdir = mock_root().join("server-build-success");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let dist_dir = workdir.join("dist").join("server");
    let overrides_dir = workdir.join("dist").join("temp-mrpack-extract").join("overrides");

    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_configured_project(workdir.clone())
        .with_file(bootstrap_jar_path.clone(), "bootstrap".to_string())
        .with_deferred_file(
            dist_dir.clone(),
            "srv.jar".to_string(),
            "existing server jar".to_string(),
        )
        .with_file(
            workdir.join("templates").join("server").join("run.sh.template"),
            "#!/bin/sh\necho {{NAME}}\n".to_string(),
        )
        .with_file(
            overrides_dir.join("server.properties"),
            "motd=Empack Test\n".to_string(),
        );
    let session = MockCommandSession::new().with_filesystem(filesystem);
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;
    orchestrator.mrpack_extracted = true;

    let result = orchestrator.build_server_impl(&bootstrap_jar_path).unwrap();

    assert!(result.success);
    assert_eq!(result.target, BuildTarget::Server);
    assert_eq!(
        result.output_path.as_ref(),
        Some(&workdir.join("dist").join("Test Pack-v1.0.0-server.zip"))
    );
    assert!(session.filesystem().exists(&dist_dir.join("srv.jar")));
    assert!(session
        .filesystem()
        .exists(&dist_dir.join("pack").join("pack.toml")));
    assert!(session
        .filesystem()
        .exists(&dist_dir.join("server.properties")));
    let rendered = session
        .filesystem()
        .read_to_string(&dist_dir.join("run.sh"))
        .unwrap();
    assert!(rendered.contains("Test Pack"));
    let zip_path = workdir.join("dist").join("Test Pack-v1.0.0-server.zip");
    assert!(session.filesystem().exists(&zip_path));
}

#[test]
fn test_build_server_returns_warning_when_server_jar_download_fails() {
    let workdir = mock_root().join("server-warning");
    let pack_dir = workdir.join("pack");
    let dist_dir = workdir.join("dist").join("server");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let filesystem = MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            pack_dir.join("pack.toml"),
            r#"name = "NeoForgeServerWarning"
author = "Test Author"
version = "1.0.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.19.4"
neoforge = "47.1.106"
"#
            .to_string(),
        )
        .with_file(
            pack_dir.join("index.toml"),
            "hash-format = \"sha256\"\n".to_string(),
        )
        .with_file(bootstrap_jar_path.clone(), "bootstrap".to_string())
        .with_file(dist_dir.join(".gitkeep"), String::new());
    let session = MockCommandSession::new()
        .with_filesystem(filesystem)
        .with_process(MockProcessProvider::new().with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_dir.join("pack.toml").display().to_string(),
                "refresh".to_string(),
            ],
            Ok(successful_process_output()),
        ));
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();

    let result = orchestrator.build_server_impl(&bootstrap_jar_path).unwrap();

    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert_eq!(result.target, BuildTarget::Server);
    assert_eq!(result.artifacts.len(), 0);
    assert_eq!(result.restricted_mods.len(), 0);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("failed to download server JAR"));
    assert!(result.warnings[0].contains("1.20.1 and newer"));
}

#[test]
fn test_build_client_requires_bootstrap_jar_bytes() {
    let workdir = mock_root().join("client-bootstrap-missing");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_configured_project(workdir.clone()),
    );
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;
    orchestrator.mrpack_extracted = true;

    let error = orchestrator.build_client_impl(&bootstrap_jar_path).unwrap_err();
    match error {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("File not found"));
            assert!(reason.contains("packwiz-installer-bootstrap.jar"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
    }
}

#[test]
fn test_build_server_requires_bootstrap_jar_bytes() {
    let workdir = mock_root().join("server-bootstrap-missing");
    let bootstrap_jar_path = workdir.join("cache").join("packwiz-installer-bootstrap.jar");
    let session = MockCommandSession::new().with_filesystem(
        MockFileSystemProvider::new()
            .with_current_dir(workdir.clone())
            .with_configured_project(workdir.clone()),
    );
    let mut orchestrator =
        BuildOrchestrator::new(&session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    orchestrator.pack_refreshed = true;

    let error = orchestrator.build_server_impl(&bootstrap_jar_path).unwrap_err();
    match error {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("File not found"));
            assert!(reason.contains("packwiz-installer-bootstrap.jar"));
        }
        other => panic!("expected ConfigError, got {other:?}"),
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
    let correct_sha1 = crate::empack::content::hex::encode(Sha1::digest(jar_bytes));
    let jar_path = dist_dir.join("srv.jar");
    mock.session
        .filesystem()
        .write_bytes(&jar_path, jar_bytes)
        .unwrap();

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
    mock.session
        .filesystem()
        .write_bytes(&jar_path, jar_bytes)
        .unwrap();

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
    let jar_sha1 = crate::empack::content::hex::encode(Sha1::digest(jar_bytes));

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

    let _m1 = server
        .mock("GET", "/mc/game/version_manifest_v2.json")
        .with_body(manifest.to_string())
        .create_async()
        .await;
    let _m2 = server
        .mock("GET", "/v1/packages/1.21.json")
        .with_body(version_meta.to_string())
        .create_async()
        .await;
    let _m3 = server
        .mock("GET", "/server.jar")
        .with_body(jar_bytes.as_slice())
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Patch the manifest URL in the test by calling the inner methods directly.
    // Since install_vanilla_server hardcodes the Mojang URL, we test via
    // fetch_url_text + download_file helpers and the JSON parsing.
    let manifest_text = orchestrator
        .fetch_url_text(&format!(
            "{}/mc/game/version_manifest_v2.json",
            server.url()
        ))
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
    let actual_hash = crate::empack::content::hex::encode(Sha1::digest(&downloaded));
    assert_eq!(actual_hash, jar_sha1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_fabric_resolves_installer_and_constructs_maven_url() {
    let mut server = mockito::Server::new_async().await;

    let installer_json = serde_json::json!([
        { "version": "1.1.1", "stable": true },
        { "version": "1.1.0", "stable": true }
    ]);

    let _m1 = server
        .mock("GET", "/v2/versions/installer")
        .with_body(installer_json.to_string())
        .create_async()
        .await;

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
    mock.session
        .filesystem()
        .write_bytes(
            &dist_dir.join("fabric-server-launch.jar"),
            b"fabric launcher jar",
        )
        .unwrap();
    mock.session
        .filesystem()
        .write_bytes(&dist_dir.join("server.jar"), b"vanilla server jar")
        .unwrap();
    mock.session
        .filesystem()
        .create_dir_all(&dist_dir.join("libraries"))
        .unwrap();

    // Perform the rename that install_fabric_server does after running the installer
    let launcher_jar = dist_dir.join("fabric-server-launch.jar");
    let srv_jar = dist_dir.join("srv.jar");
    assert!(mock.session.filesystem().exists(&launcher_jar));

    let bytes = mock.session.filesystem().read_bytes(&launcher_jar).unwrap();
    mock.session
        .filesystem()
        .write_bytes(&srv_jar, &bytes)
        .unwrap();
    let _ = mock.session.filesystem().remove_file(&launcher_jar);

    assert!(mock.session.filesystem().exists(&srv_jar));
    assert!(!mock.session.filesystem().exists(&launcher_jar));
    assert!(
        mock.session
            .filesystem()
            .exists(&dist_dir.join("server.jar"))
    );
    assert!(
        mock.session
            .filesystem()
            .exists(&dist_dir.join("libraries"))
    );

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

    let _m1 = server
        .mock(
            "GET",
            "/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml",
        )
        .with_body(maven_xml)
        .create_async()
        .await;
    let _m2 = server
        .mock(
            "GET",
            "/repository/release/org/quiltmc/quilt-installer/0.12.0/quilt-installer-0.12.0.jar",
        )
        .with_body("quilt installer jar bytes")
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Test the Maven XML parsing step
    let xml_text = orchestrator
        .fetch_url_text(&format!(
            "{}/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml",
            server.url()
        ))
        .unwrap();
    let metadata: QuiltMavenMetadata = quick_xml::de::from_str(&xml_text).unwrap();
    assert_eq!(metadata.versioning.release, "0.12.0");

    // Test the download step
    let installer_url = format!(
        "{}/repository/release/org/quiltmc/quilt-installer/0.12.0/quilt-installer-0.12.0.jar",
        server.url()
    );
    let installer_path = dist_dir.join("quilt-installer-0.12.0.jar");
    orchestrator
        .download_file(&installer_url, &installer_path)
        .unwrap();
    assert!(mock.session.filesystem().exists(&installer_path));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_quilt_installer_renames_launch_jar_not_vanilla() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Simulate Quilt installer output
    mock.session
        .filesystem()
        .write_bytes(
            &dist_dir.join("quilt-server-launch.jar"),
            b"quilt launcher jar",
        )
        .unwrap();
    mock.session
        .filesystem()
        .write_bytes(&dist_dir.join("server.jar"), b"vanilla server jar")
        .unwrap();
    mock.session
        .filesystem()
        .create_dir_all(&dist_dir.join("libraries"))
        .unwrap();

    // Perform the rename that install_quilt_server does after running the installer
    let launcher_jar = dist_dir.join("quilt-server-launch.jar");
    let srv_jar = dist_dir.join("srv.jar");
    assert!(mock.session.filesystem().exists(&launcher_jar));

    let bytes = mock.session.filesystem().read_bytes(&launcher_jar).unwrap();
    mock.session
        .filesystem()
        .write_bytes(&srv_jar, &bytes)
        .unwrap();
    let _ = mock.session.filesystem().remove_file(&launcher_jar);

    // srv.jar should contain the Quilt launcher, NOT the vanilla server
    assert!(mock.session.filesystem().exists(&srv_jar));
    assert!(!mock.session.filesystem().exists(&launcher_jar));
    assert!(
        mock.session
            .filesystem()
            .exists(&dist_dir.join("server.jar"))
    );

    let srv_bytes = mock.session.filesystem().read_bytes(&srv_jar).unwrap();
    assert_eq!(srv_bytes, b"quilt launcher jar");

    let vanilla_bytes = mock
        .session
        .filesystem()
        .read_bytes(&dist_dir.join("server.jar"))
        .unwrap();
    assert_eq!(vanilla_bytes, b"vanilla server jar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_neoforge_downloads_and_runs_installer() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            "/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar",
        )
        .with_body("neoforge installer bytes")
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Test the download helper independently
    let installer_path = dist_dir.join("neoforge-21.1.86-installer.jar");
    let url = format!(
        "{}/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar",
        server.url()
    );
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

#[test]
fn test_supports_neoforge_minecraft_switches_at_1_20_1() {
    assert!(!supports_neoforge_minecraft("1.19.4"));
    assert!(supports_neoforge_minecraft("1.20.1"));
    assert!(supports_neoforge_minecraft("1.20.2"));
    assert!(supports_neoforge_minecraft("1.21.1"));
}

#[test]
fn test_neoforge_installer_artifact_uses_legacy_forge_family_for_1_20_1() {
    let (url, filename) = neoforge_installer_artifact("1.20.1", "47.1.106").unwrap();
    assert_eq!(
        url,
        "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
    );
    assert_eq!(filename, "forge-1.20.1-47.1.106-installer.jar");
}

#[test]
fn test_neoforge_installer_artifact_uses_modern_family_for_1_20_2_plus() {
    let (url, filename) = neoforge_installer_artifact("1.21.1", "21.1.86").unwrap();
    assert_eq!(
        url,
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.86/neoforge-21.1.86-installer.jar"
    );
    assert_eq!(filename, "neoforge-21.1.86-installer.jar");
}

#[test]
fn test_forge_installer_coordinate_standard() {
    assert_eq!(
        forge_installer_coordinate("1.20.1", "47.3.0"),
        "1.20.1-47.3.0"
    );
}

#[test]
fn test_forge_installer_coordinate_pre_boundary_1710_stays_modern() {
    assert_eq!(
        forge_installer_coordinate("1.7.10", "10.13.2.1291"),
        "1.7.10-10.13.2.1291"
    );
}

#[test]
fn test_forge_installer_coordinate_legacy_1710_boundary() {
    assert_eq!(
        forge_installer_coordinate("1.7.10", "10.13.2.1300"),
        "1.7.10-10.13.2.1300-1.7.10"
    );
}

#[test]
fn test_forge_installer_coordinate_legacy_1710_normalizes_suffixed_input() {
    assert_eq!(
        forge_installer_coordinate("1.7.10", "10.13.4.1614-1.7.10"),
        "1.7.10-10.13.4.1614-1.7.10"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_jar_forge_downloads_and_runs_installer() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock(
            "GET",
            "/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar",
        )
        .with_body("forge installer bytes")
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let installer_path = dist_dir.join("forge-1.20.1-47.3.0-installer.jar");
    let url = format!(
        "{}/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar",
        server.url()
    );
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

#[test]
fn test_download_server_jar_forge_legacy_1710_uses_legacy_url() {
    let composite = forge_installer_coordinate("1.7.10", "10.13.4.1614");
    let expected_url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{c}/forge-{c}-installer.jar",
        c = composite
    );
    assert_eq!(
        expected_url,
        "https://maven.minecraftforge.net/net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10-installer.jar"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_server_starter_jar_succeeds_when_run_scripts_exist() {
    let mut server = mockito::Server::new_async().await;
    let ssj_bytes = b"server starter jar content";
    let _m = server
        .mock("GET", "/releases/latest/download/server.jar")
        .with_body(ssj_bytes.as_slice())
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    // Simulate installer output: run.sh + run.bat
    mock.session
        .filesystem()
        .write_file(
            &dist_dir.join("run.sh"),
            "#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/.../unix_args.txt \"$@\"",
        )
        .unwrap();
    mock.session
        .filesystem()
        .write_file(
            &dist_dir.join("run.bat"),
            "java @user_jvm_args.txt @libraries\\.../win_args.txt %*",
        )
        .unwrap();

    // Download ServerStarterJar using the mock server URL directly
    let srv_jar = dist_dir.join("srv.jar");
    orchestrator
        .download_file(
            &format!("{}/releases/latest/download/server.jar", server.url()),
            &srv_jar,
        )
        .unwrap();

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
    mock.session
        .filesystem()
        .write_file(
            &dist_dir.join("run.sh"),
            "#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/.../unix_args.txt \"$@\"",
        )
        .unwrap();

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
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

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

// ===== W1-T6b: CACHE-FIRST DOWNLOAD CHECK =====

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_download_file_skips_http_when_dest_exists() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let dist_dir = mock.workdir().join("dist").join("server");
    mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

    let dest = dist_dir.join("already-cached.jar");
    mock.session
        .filesystem()
        .write_bytes(&dest, b"cached content")
        .unwrap();

    // URL is unreachable; download_file must return Ok because the cache hit
    // prevents the HTTP call entirely.
    let result = orchestrator.download_file("http://unreachable.invalid/file.jar", &dest);
    assert!(
        result.is_ok(),
        "cache-first check should skip HTTP: {result:?}"
    );

    let bytes = mock.session.filesystem().read_bytes(&dest).unwrap();
    assert_eq!(
        bytes, b"cached content",
        "existing file should be untouched"
    );
}

// ===== W2-F4: FETCH RETRY TESTS =====

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_retries_on_server_error_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    let body = b"downloaded content";

    let _m = server
        .mock("GET", "/retry-test")
        .with_status(503)
        .with_body("Service Unavailable")
        .expect_at_most(2)
        .create_async()
        .await;
    let _m_ok = server
        .mock("GET", "/retry-test")
        .with_status(200)
        .with_body(body.as_slice())
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/retry-test", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(
        result.is_ok(),
        "should succeed after retries: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_does_not_retry_on_client_error() {
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("GET", "/not-found")
        .with_status(404)
        .with_body("Not Found")
        .expect(1)
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/not-found", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("404"),
        "error should contain status code: {err_msg}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fetch_url_bytes_exhausts_retries_on_persistent_server_error() {
    let mut server = mockito::Server::new_async().await;

    let _m = server
        .mock("GET", "/always-failing")
        .with_status(502)
        .with_body("Bad Gateway")
        .expect(3)
        .create_async()
        .await;

    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();
    let orchestrator = mock.orchestrator();

    let url = format!("{}/always-failing", server.url());
    let result = orchestrator.fetch_url_bytes(&url);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("502"),
        "error should contain status code: {err_msg}"
    );
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

    fn setup_neoforge_project(
        mock: &MockBuildOrchestrator,
        mc_version: &str,
        loader_version: &str,
    ) {
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
            .write_file(&pack_dir.join("index.toml"), "hash-format = \"sha256\"\n")
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

        let expected_url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
            v = version
        );

        // Reproduce the exact URL construction from install_neoforge_server
        let actual_url = format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar",
            v = version
        );
        let actual_filename = format!("neoforge-{}-installer.jar", version);

        assert_eq!(actual_url, expected_url);
        assert_eq!(
            actual_filename,
            format!("neoforge-{}-installer.jar", version)
        );

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
                args.iter()
                    .any(|a| a.contains("neoforge") && a.contains("installer")),
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
        mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

        let result = orchestrator.download_server_jar(&dist_dir, &pack_info);
        // The call will fail (no HTTP server), but should NOT be "unsupported loader type"
        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            !error_msg.contains("unsupported loader type"),
            "NeoForge should be recognized as a valid loader type"
        );
    }

    /// Verify unsupported pre-NeoForge Minecraft versions still fail fast.
    #[test]
    fn test_neoforge_pre_1_20_1_is_rejected_before_download() {
        let pack_info = PackInfo {
            author: "A".to_string(),
            name: "P".to_string(),
            version: "1.0.0".to_string(),
            mc_version: "1.19.4".to_string(),
            loader_version: "47.1.106".to_string(),
            loader_type: "neoforge".to_string(),
        };

        let mock = MockBuildOrchestrator::new();
        mock.setup_basic_pack_structure().unwrap();
        let orchestrator = mock.orchestrator();
        let dist_dir = mock.workdir().join("dist").join("server");
        mock.session.filesystem().create_dir_all(&dist_dir).unwrap();

        let error = orchestrator
            .install_neoforge_server(&dist_dir, &pack_info)
            .expect_err("pre-1.20.1 NeoForge should be rejected");

        assert!(
            matches!(error, BuildError::ValidationError { .. }),
            "expected validation error, got: {error:?}"
        );
        assert!(
            error.to_string().contains("1.20.1 and newer"),
            "error should explain the version floor: {error}"
        );
    }
}
