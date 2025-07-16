use super::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use crate::application::session_mocks::MockCommandSession;

// Mock structure for testing build orchestrator without external dependencies
struct MockBuildOrchestrator {
    temp_dir: TempDir,
    mock_commands: Vec<String>,
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
            mock_commands: Vec::new(),
            session,
        }
    }
    
    fn orchestrator(&self) -> BuildOrchestrator {
        BuildOrchestrator::new(&self.session).expect("Failed to create orchestrator")
    }

    fn setup_basic_pack_structure(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        
        // Use the mock filesystem to create the pack structure
        let filesystem = self.session.filesystem();
        
        // Create pack directory
        let pack_dir = workdir.join("pack");
        filesystem.create_dir_all(&pack_dir).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

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
        filesystem.write_file(&pack_toml, toml_content).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        // Create basic index.toml
        let index_toml = pack_dir.join("index.toml");
        let index_content = r#"
hash-format = "sha1"

[[files]]
file = "mods/test-mod.pw.toml"
hash = "abcd1234"
"#;
        filesystem.write_file(&index_toml, index_content).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        // Create mods directory
        let mods_dir = pack_dir.join("mods");
        filesystem.create_dir_all(&mods_dir).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

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
        filesystem.write_file(&mod_file, mod_content).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        Ok(())
    }

    fn setup_installer_structure(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        let filesystem = self.session.filesystem();
        
        let installer_dir = workdir.join("installer");
        filesystem.create_dir_all(&installer_dir).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        // Create mock installer jar
        let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
        filesystem.write_file(&installer_jar, "mock installer jar content").map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        Ok(())
    }

    fn setup_templates(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        let filesystem = self.session.filesystem();
        
        let templates_dir = workdir.join("templates").join("client");
        filesystem.create_dir_all(&templates_dir).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        // Create a test template file
        let template_file = templates_dir.join("launcher.json.template");
        let template_content = r#"{
    "name": "{{NAME}}",
    "version": "{{VERSION}}",
    "author": "{{AUTHOR}}",
    "mcVersion": "{{MC_VERSION}}",
    "fabricVersion": "{{FABRIC_VERSION}}"
}"#;
        filesystem.write_file(&template_file, template_content).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        Ok(())
    }

    fn create_mock_mrpack(&self) -> Result<(), BuildError> {
        let workdir = self.temp_dir.path().to_path_buf();
        let filesystem = self.session.filesystem();
        
        let dist_dir = workdir.join("dist");
        filesystem.create_dir_all(&dist_dir).map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

        // Create mock mrpack file
        let mrpack_file = dist_dir.join("TestPack-v1.0.0.mrpack");
        filesystem.write_file(&mrpack_file, "mock mrpack content").map_err(|e| BuildError::ConfigError { reason: e.to_string() })?;

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
    let (temp_dir, session) = create_test_orchestrator();
    let orchestrator = BuildOrchestrator::new(&session).expect("Failed to create orchestrator");

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
    assert_eq!(pack_info.fabric_version, "0.15.11");
}

#[test]
fn test_load_pack_info_missing_file() {
    let (temp_dir, session) = create_test_orchestrator();
    let mut orchestrator = BuildOrchestrator::new(&session).expect("Failed to create orchestrator");
    
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
    assert!(content.contains("\"fabricVersion\": \"0.15.11\""));
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
    
    let content1 = filesystem.read_to_string(&dst_dir.join("file1.txt")).unwrap();
    assert_eq!(content1, "content1");
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

// Note: The following tests would require mocking external commands (packwiz, zip, unzip)
// For now, we test the logic that can be tested without external dependencies
// In a real implementation, these would use a process mock or dependency injection

#[test]
fn test_build_result_structure() {
    // Test that BuildResult has the expected structure
    let result = BuildResult {
        target: BuildTarget::Mrpack,
        success: true,
        output_path: Some(PathBuf::from("/test/path")),
        artifacts: vec![],
        warnings: vec![],
    };
    
    assert_eq!(result.target, BuildTarget::Mrpack);
    assert!(result.success);
    assert!(result.output_path.is_some());
    assert!(result.artifacts.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_build_artifact_structure() {
    let artifact = BuildArtifact {
        name: "test.mrpack".to_string(),
        path: PathBuf::from("/test/test.mrpack"),
        size: 1024,
    };
    
    assert_eq!(artifact.name, "test.mrpack");
    assert_eq!(artifact.path, PathBuf::from("/test/test.mrpack"));
    assert_eq!(artifact.size, 1024);
}

#[test]
fn test_pack_info_structure() {
    let pack_info = PackInfo {
        author: "TestAuthor".to_string(),
        name: "TestPack".to_string(),
        version: "1.0.0".to_string(),
        mc_version: "1.21".to_string(),
        fabric_version: "0.15.11".to_string(),
    };
    
    assert_eq!(pack_info.author, "TestAuthor");
    assert_eq!(pack_info.name, "TestPack");
    assert_eq!(pack_info.version, "1.0.0");
    assert_eq!(pack_info.mc_version, "1.21");
    assert_eq!(pack_info.fabric_version, "0.15.11");
}
