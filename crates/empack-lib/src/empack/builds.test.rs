use super::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// Mock structure for testing build orchestrator without external dependencies
struct MockBuildOrchestrator {
    orchestrator: BuildOrchestrator,
    temp_dir: TempDir,
    mock_commands: Vec<String>,
}

impl MockBuildOrchestrator {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let orchestrator = BuildOrchestrator::new(temp_dir.path().to_path_buf());
        Self {
            orchestrator,
            temp_dir,
            mock_commands: Vec::new(),
        }
    }

    fn setup_basic_pack_structure(&self) -> Result<(), BuildError> {
        // Create pack directory
        let pack_dir = self.temp_dir.path().join("pack");
        fs::create_dir_all(&pack_dir)?;

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
        fs::write(&pack_toml, toml_content)?;

        // Create basic index.toml
        let index_toml = pack_dir.join("index.toml");
        let index_content = r#"
hash-format = "sha1"

[[files]]
file = "mods/test-mod.pw.toml"
hash = "abcd1234"
"#;
        fs::write(&index_toml, index_content)?;

        // Create mods directory
        let mods_dir = pack_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

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
        fs::write(&mod_file, mod_content)?;

        Ok(())
    }

    fn setup_installer_structure(&self) -> Result<(), BuildError> {
        let installer_dir = self.temp_dir.path().join("installer");
        fs::create_dir_all(&installer_dir)?;

        // Create mock installer jar
        let installer_jar = installer_dir.join("packwiz-installer-bootstrap.jar");
        fs::write(&installer_jar, "mock installer jar content")?;

        Ok(())
    }

    fn setup_templates(&self) -> Result<(), BuildError> {
        let templates_dir = self.temp_dir.path().join("templates").join("client");
        fs::create_dir_all(&templates_dir)?;

        // Create a test template file
        let template_file = templates_dir.join("launcher.json.template");
        let template_content = r#"{
    "name": "{{NAME}}",
    "version": "{{VERSION}}",
    "author": "{{AUTHOR}}",
    "mcVersion": "{{MC_VERSION}}",
    "fabricVersion": "{{FABRIC_VERSION}}"
}"#;
        fs::write(&template_file, template_content)?;

        Ok(())
    }

    fn create_mock_mrpack(&self) -> Result<(), BuildError> {
        let dist_dir = self.temp_dir.path().join("dist");
        fs::create_dir_all(&dist_dir)?;

        // Create mock mrpack file
        let mrpack_file = dist_dir.join("TestPack-v1.0.0.mrpack");
        fs::write(&mrpack_file, "mock mrpack content")?;

        Ok(())
    }

    fn workdir(&self) -> &Path {
        self.temp_dir.path()
    }
}

fn create_test_orchestrator() -> (TempDir, BuildOrchestrator) {
    let temp_dir = TempDir::new().unwrap();
    let orchestrator = BuildOrchestrator::new(temp_dir.path().to_path_buf());
    (temp_dir, orchestrator)
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
    let (_temp, orchestrator) = create_test_orchestrator();

    // Should fail without pack directory
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::ConfigError { reason } => {
            assert!(reason.contains("pack/ directory not found"));
        }
        _ => panic!("Expected ConfigError"),
    }

    // Create pack directory
    std::fs::create_dir_all(orchestrator.workdir.join("pack")).unwrap();

    // Should fail because packwiz is not available in test environment
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_err());
    match result.unwrap_err() {
        BuildError::MissingTool { tool } => {
            assert!(tool.contains("packwiz"));
        }
        _ => panic!("Expected MissingTool error"),
    }
}

#[test]
fn test_load_pack_info() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let mut orchestrator = mock.orchestrator;
    let pack_info = orchestrator.load_pack_info().unwrap();

    assert_eq!(pack_info.name, "TestPack");
    assert_eq!(pack_info.author, "TestAuthor");
    assert_eq!(pack_info.version, "1.0.0");
    assert_eq!(pack_info.mc_version, "1.21");
    assert_eq!(pack_info.fabric_version, "0.15.11");
}

#[test]
fn test_load_pack_info_missing_file() {
    let (_temp, mut orchestrator) = create_test_orchestrator();
    
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
    let pack_dir = mock.workdir().join("pack");
    fs::create_dir_all(&pack_dir).unwrap();
    let pack_toml = pack_dir.join("pack.toml");
    fs::write(&pack_toml, "invalid toml content [ unclosed bracket").unwrap();

    let mut orchestrator = mock.orchestrator;
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

    let mut orchestrator = mock.orchestrator;
    
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
    let mut orchestrator = mock.orchestrator;
    
    // Create target directory
    let target_dir = workdir.join("test-target");
    fs::create_dir_all(&target_dir).unwrap();

    // Process templates
    let result = orchestrator.process_build_templates("templates/client", &target_dir);
    assert!(result.is_ok());

    // Check that template was processed
    let output_file = target_dir.join("launcher.json");
    assert!(output_file.exists());
    
    let content = fs::read_to_string(&output_file).unwrap();
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
    let mut orchestrator = mock.orchestrator;
    
    let target_dir = workdir.join("test-target");
    fs::create_dir_all(&target_dir).unwrap();

    // Process templates from non-existent directory (should not error)
    let result = orchestrator.process_build_templates("templates/nonexistent", &target_dir);
    assert!(result.is_ok());
}

#[test]
fn test_copy_dir_contents() {
    let mock = MockBuildOrchestrator::new();
    mock.setup_basic_pack_structure().unwrap();

    let workdir = mock.workdir().to_path_buf();
    let orchestrator = mock.orchestrator;
    
    // Create source directory with content
    let src_dir = workdir.join("test-src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("file1.txt"), "content1").unwrap();
    fs::write(src_dir.join("file2.txt"), "content2").unwrap();
    
    let sub_dir = src_dir.join("subdir");
    fs::create_dir_all(&sub_dir).unwrap();
    fs::write(sub_dir.join("file3.txt"), "content3").unwrap();

    // Copy to destination
    let dst_dir = workdir.join("test-dst");
    let result = orchestrator.copy_dir_contents(&src_dir, &dst_dir);
    assert!(result.is_ok());

    // Verify files were copied
    assert!(dst_dir.join("file1.txt").exists());
    assert!(dst_dir.join("file2.txt").exists());
    assert!(dst_dir.join("subdir").join("file3.txt").exists());
    
    let content1 = fs::read_to_string(dst_dir.join("file1.txt")).unwrap();
    assert_eq!(content1, "content1");
}

#[test]
fn test_create_artifact() {
    let mock = MockBuildOrchestrator::new();
    let workdir = mock.workdir().to_path_buf();
    let orchestrator = mock.orchestrator;
    
    // Create test file
    let test_file = workdir.join("test.txt");
    let content = "test content";
    fs::write(&test_file, content).unwrap();

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
    let mut orchestrator = mock.orchestrator;
    
    // Load pack info to enable zip file cleaning
    orchestrator.load_pack_info().unwrap();
    
    // Create target directory with files
    let target_dir = workdir.join("dist").join("client");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("test.txt"), "content").unwrap();
    fs::write(target_dir.join(".gitkeep"), "").unwrap();
    
    // Create zip file
    let zip_file = workdir.join("dist").join("TestPack-v1.0.0-client.zip");
    fs::write(&zip_file, "mock zip content").unwrap();

    // Clean target
    let result = orchestrator.clean_target(BuildTarget::Client);
    assert!(result.is_ok());
    
    // Verify files were cleaned but .gitkeep preserved
    assert!(!target_dir.join("test.txt").exists());
    assert!(target_dir.join(".gitkeep").exists());
    assert!(!zip_file.exists());
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
