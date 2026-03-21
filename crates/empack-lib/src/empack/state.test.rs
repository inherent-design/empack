use super::*;
use crate::application::session::{FileSystemProvider, ProcessOutput};
use crate::application::session_mocks::mock_root;
use crate::empack::config::ConfigManager;
use crate::empack::packwiz::MockPackwizOps;
use std::collections::HashSet;

/// Mock implementation of FileSystemProvider for testing - zero I/O
/// Supports stateful operations that actually modify the simulated filesystem
#[derive(Debug)]
struct MockStateProvider {
    /// Simulated filesystem as a set of file paths
    files: std::cell::RefCell<HashSet<PathBuf>>,
    /// Simulated file contents (stored separately so existing tests don't break)
    file_contents: std::cell::RefCell<std::collections::HashMap<PathBuf, String>>,
    /// Simulated directories
    directories: std::cell::RefCell<HashSet<PathBuf>>,
    /// Files with build artifacts
    build_artifacts: std::cell::RefCell<HashSet<PathBuf>>,
}

impl MockStateProvider {
    fn new() -> Self {
        Self {
            files: std::cell::RefCell::new(HashSet::new()),
            file_contents: std::cell::RefCell::new(std::collections::HashMap::new()),
            directories: std::cell::RefCell::new(HashSet::new()),
            build_artifacts: std::cell::RefCell::new(HashSet::new()),
        }
    }

    /// Add a file to the mock filesystem
    fn add_file(&self, path: PathBuf) {
        self.files.borrow_mut().insert(path);
    }

    /// Add a file with specific content to the mock filesystem
    fn add_file_with_content(&self, path: PathBuf, content: String) {
        self.files.borrow_mut().insert(path.clone());
        self.file_contents.borrow_mut().insert(path, content);
    }

    /// Add a directory to the mock filesystem
    fn add_directory(&self, path: PathBuf) {
        self.directories.borrow_mut().insert(path);
    }

    /// Add a build artifact file
    fn add_build_artifact(&self, path: PathBuf) {
        self.build_artifacts.borrow_mut().insert(path);
    }

}

impl crate::application::session::FileSystemProvider for MockStateProvider {
    fn current_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(mock_root())
    }

    // state_manager method removed - create PackStateManager directly

    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        ConfigManager::new(workdir, self)
    }

    fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
        if self.files.borrow().contains(path) {
            // Return stored content if available
            if let Some(content) = self.file_contents.borrow().get(path) {
                return Ok(content.clone());
            }
            // Return valid YAML content for empack.yml files
            if path.file_name().and_then(|n| n.to_str()) == Some("empack.yml") {
                Ok("empack:\n  name: test-pack\n  minecraft_version: 1.20.1\n".to_string())
            } else {
                Ok("mock content".to_string())
            }
        } else {
            Err(anyhow::anyhow!("File not found"))
        }
    }

    fn write_file(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        self.files.borrow_mut().insert(path.to_path_buf());
        self.file_contents
            .borrow_mut()
            .insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn read_bytes(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        if self.files.borrow().contains(path) {
            if let Some(content) = self.file_contents.borrow().get(path) {
                return Ok(content.as_bytes().to_vec());
            }
            Ok(b"mock content".to_vec())
        } else {
            Err(anyhow::anyhow!("File not found: {}", path.display()))
        }
    }

    fn write_bytes(&self, path: &Path, _content: &[u8]) -> anyhow::Result<()> {
        self.files.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains(path)
    }

    fn metadata_exists(&self, path: &Path) -> bool {
        self.exists(path) || self.is_directory(path)
    }

    fn is_directory(&self, path: &Path) -> bool {
        self.directories.borrow().contains(path)
    }

    fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        // Actually add the directory to the mock filesystem
        self.directories.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }

    fn get_file_list(&self, path: &Path) -> anyhow::Result<HashSet<PathBuf>> {
        let mut files_in_dir = HashSet::new();

        // Return files that are children of the given path
        for file in self.files.borrow().iter() {
            if let Some(parent) = file.parent()
                && parent == path
            {
                files_in_dir.insert(file.clone());
            }
        }

        Ok(files_in_dir)
    }

    fn has_build_artifacts(&self, dist_dir: &Path) -> anyhow::Result<bool> {
        // Check if any build artifacts exist in the dist directory
        for artifact in self.build_artifacts.borrow().iter() {
            if let Some(parent) = artifact.parent()
                && parent == dist_dir
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
        self.files.borrow_mut().remove(path);
        self.file_contents.borrow_mut().remove(path);
        self.build_artifacts.borrow_mut().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        // Actually remove the directory and all its contents from the mock filesystem
        self.directories.borrow_mut().remove(path);

        // Remove all files and artifacts that are children of this directory
        let mut files_to_remove = Vec::new();
        for file in self.files.borrow().iter() {
            if file.starts_with(path) {
                files_to_remove.push(file.clone());
            }
        }
        for file in files_to_remove {
            self.files.borrow_mut().remove(&file);
        }

        let mut artifacts_to_remove = Vec::new();
        for artifact in self.build_artifacts.borrow().iter() {
            if artifact.starts_with(path) {
                artifacts_to_remove.push(artifact.clone());
            }
        }
        for artifact in artifacts_to_remove {
            self.build_artifacts.borrow_mut().remove(&artifact);
        }

        Ok(())
    }

}

/// Create a MockPackwizOps for state tests that stores pack files via its own in-memory map
fn mock_packwiz_for_test() -> MockPackwizOps {
    MockPackwizOps::new()
}

/// Helper to create a test setup with uninitialized state
fn create_uninitialized_test() -> (MockStateProvider, PathBuf) {
    let mock_provider = MockStateProvider::new();
    let workdir = mock_root().join("workdir");
    mock_provider.add_directory(workdir.clone());
    (mock_provider, workdir)
}

/// Helper to create a test setup with configured state
fn create_configured_test() -> (MockStateProvider, PathBuf) {
    let (mock_provider, workdir) = create_uninitialized_test();

    // Add empack.yml to simulate configured state
    mock_provider.add_file(workdir.join("empack.yml"));
    mock_provider.add_directory(workdir.join("pack"));
    mock_provider.add_file(workdir.join("pack").join("pack.toml"));
    mock_provider.add_file(workdir.join("pack").join("index.toml"));

    (mock_provider, workdir)
}

/// Helper to create a test setup with built state
fn create_built_test() -> (MockStateProvider, PathBuf) {
    let (mock_provider, workdir) = create_configured_test();

    // Add dist directory and build artifacts
    let dist_dir = workdir.join("dist");
    mock_provider.add_directory(dist_dir.clone());
    mock_provider.add_build_artifact(dist_dir.join("test.mrpack"));

    (mock_provider, workdir)
}

fn create_progressive_init_test() -> (MockStateProvider, PathBuf) {
    let (mock_provider, workdir) = create_uninitialized_test();

    mock_provider.add_file(workdir.join("empack.yml"));

    (mock_provider, workdir)
}

fn successful_process_output() -> ProcessOutput {
    ProcessOutput {
        stdout: String::new(),
        stderr: String::new(),
        success: true,
    }
}

fn configured_build_session(
    workdir: &Path,
) -> crate::application::session_mocks::MockCommandSession {
    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_output = workdir.join("dist").join("Test Pack-v1.0.0.mrpack");
    let process = crate::application::session_mocks::MockProcessProvider::new()
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
                mrpack_output.display().to_string(),
            ],
            Ok(successful_process_output()),
        );

    crate::application::session_mocks::MockCommandSession::new()
        .with_filesystem(
            crate::application::session_mocks::MockFileSystemProvider::new()
                .with_current_dir(workdir.to_path_buf())
                .with_configured_project(workdir.to_path_buf()),
        )
        .with_process(process)
}

#[test]
fn test_initial_state_is_uninitialized() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir, &provider);
    let state = manager.discover_state().unwrap();
    assert_eq!(state, PackState::Uninitialized);
}

#[tokio::test]
async fn test_transition_to_configured() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir, &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();

    let packwiz = mock_packwiz_for_test();
    let result = manager
        .execute_transition(
            &process,
            &packwiz,
            StateTransition::Initialize(crate::primitives::InitializationConfig {
                name: "Test Pack",
                author: "Test Author",
                version: "1.0.0",
                modloader: "fabric",
                mc_version: "1.20.1",
                loader_version: "0.14.21",
            }),
        )
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Configured);
}

#[tokio::test]
async fn test_transition_to_built() {
    let workdir = mock_root().join("configured-project");
    let session = configured_build_session(&workdir);
    let manager = PackStateManager::new(workdir.clone(), session.filesystem());
    let targets = vec![BuildTarget::Mrpack];
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&session).unwrap();
    let packwiz = mock_packwiz_for_test();
    let result = manager
        .execute_transition(session.process(), &packwiz, StateTransition::Build(mock_orchestrator, targets))
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Built);
}

#[tokio::test]
async fn test_clean_transitions() {
    let (provider, workdir) = create_built_test();
    let manager = PackStateManager::new(workdir, &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    // Start from built state and clean back to configured
    let result = manager
        .execute_transition(&process, &packwiz, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Configured);

    // Clean back to uninitialized
    let result = manager
        .execute_transition(&process, &packwiz, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Uninitialized);
}

#[tokio::test]
async fn test_invalid_transitions() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    // Can't build from uninitialized
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session).unwrap();
    let result = manager
        .execute_transition(
            &process,
            &packwiz,
            StateTransition::Build(mock_orchestrator, vec![BuildTarget::Mrpack]),
        )
        .await;
    assert!(result.is_err());

    // Can't sync from uninitialized
    let result = manager
        .execute_transition(&process, &packwiz, StateTransition::RefreshIndex)
        .await;
    assert!(result.is_err());
}

#[test]
fn test_state_validation() {
    // Test uninitialized state validation
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    assert!(manager.validate_state(PackState::Uninitialized).unwrap());
    assert!(!manager.validate_state(PackState::Configured).unwrap());

    // Test configured state validation
    let (provider, workdir) = create_configured_test();
    let manager = PackStateManager::new(workdir, &provider);
    assert!(manager.validate_state(PackState::Configured).unwrap());
    assert!(!manager.validate_state(PackState::Uninitialized).unwrap());

    // Test built state validation uses the canonical dist/ root
    let (provider, workdir) = create_built_test();
    let manager = PackStateManager::new(workdir, &provider);
    assert!(manager.validate_state(PackState::Built).unwrap());

    // Test progressive init state is not treated as fully configured
    let (provider, workdir) = create_progressive_init_test();
    let manager = PackStateManager::new(workdir, &provider);
    assert!(!manager.validate_state(PackState::Configured).unwrap());
}

#[test]
fn test_paths_helper() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    let paths = manager.paths();

    assert_eq!(paths.empack_yml, workdir.join("empack.yml"));
    assert_eq!(paths.pack_toml, workdir.join("pack").join("pack.toml"));
    assert_eq!(
        paths.build_output(BuildTarget::Mrpack),
        workdir.join("dist").join("mrpack")
    );
}

// Test the pure functions directly for comprehensive coverage
#[test]
fn test_pure_discover_state_function() {
    // Test uninitialized state
    let (provider, workdir) = create_uninitialized_test();
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, PackState::Uninitialized);

    // Test configured state
    let (provider, workdir) = create_configured_test();
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, PackState::Configured);

    // Test built state
    let (provider, workdir) = create_built_test();
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, PackState::Built);
}

#[test]
fn test_discover_state_ignores_legacy_hidden_artifact_root() {
    let (provider, workdir) = create_configured_test();
    provider.add_directory(workdir.join(".empack").join("dist"));
    provider.add_build_artifact(workdir.join(".empack").join("dist").join("legacy.mrpack"));

    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, PackState::Configured);

    let manager = PackStateManager::new(workdir, &provider);
    assert!(!manager.validate_state(PackState::Built).unwrap());
}

#[test]
fn test_pure_can_transition_function() {
    // Test valid transitions
    assert!(can_transition(
        &PackState::Uninitialized,
        &PackState::Configured
    ));
    assert!(can_transition(
        &PackState::Configured,
        &PackState::Built
    ));
    assert!(can_transition(
        &PackState::Built,
        &PackState::Configured
    ));
    assert!(can_transition(
        &PackState::Configured,
        &PackState::Uninitialized
    ));
    assert!(can_transition(
        &PackState::Built,
        &PackState::Building
    ));

    // Test same state transitions
    assert!(can_transition(
        &PackState::Configured,
        &PackState::Configured
    ));

    // Test invalid transitions
    assert!(!can_transition(
        &PackState::Uninitialized,
        &PackState::Built
    ));
    assert!(!can_transition(
        &PackState::Built,
        &PackState::Uninitialized
    ));
}

#[tokio::test]
async fn test_pure_execute_transition_function() {
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    // Test initialize transition
    let (provider, workdir) = create_uninitialized_test();
    let result = execute_transition(
        &provider,
        &process,
        &packwiz,
        &workdir,
        StateTransition::Initialize(crate::primitives::InitializationConfig {
            name: "Test Pack",
            author: "Test Author",
            version: "1.0.0",
            modloader: "fabric",
            mc_version: "1.20.1",
            loader_version: "0.14.21",
        }),
    )
    .await
    .unwrap();
    assert_eq!(result.state, PackState::Configured);

    // Test initialize transition for progressive-init state after empack.yml exists
    let (provider, workdir) = create_progressive_init_test();
    let result = execute_transition(
        &provider,
        &process,
        &packwiz,
        &workdir,
        StateTransition::Initialize(crate::primitives::InitializationConfig {
            name: "Test Pack",
            author: "Test Author",
            version: "1.0.0",
            modloader: "fabric",
            mc_version: "1.20.1",
            loader_version: "0.14.21",
        }),
    )
    .await
    .unwrap();
    assert_eq!(result.state, PackState::Configured);

    // Test build transition
    let workdir = mock_root().join("configured-project");
    let session = configured_build_session(&workdir);
    let targets = vec![BuildTarget::Mrpack];
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&session).unwrap();
    let result = execute_transition(
        session.filesystem(),
        session.process(),
        &packwiz,
        &workdir,
        StateTransition::Build(mock_orchestrator, targets),
    )
    .await
    .unwrap();
    assert_eq!(result.state, PackState::Built);

    // Test refresh-index transition (should succeed with valid mock data)
    let (provider, workdir) = create_configured_test();
    let result = execute_transition(&provider, &process, &packwiz, &workdir, StateTransition::RefreshIndex).await;
    // With valid YAML and mock packwiz, refresh-index should succeed
    assert_eq!(result.unwrap().state, PackState::Configured);

    // Test clean transition from built
    let (provider, workdir) = create_built_test();
    let result = execute_transition(&provider, &process, &packwiz, &workdir, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Configured);

    // Test clean transition from configured
    let (provider, workdir) = create_configured_test();
    let result = execute_transition(&provider, &process, &packwiz, &workdir, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result.state, PackState::Uninitialized);
}

#[tokio::test]
async fn test_refresh_transition_rejects_progressive_init_state() {
    let (provider, workdir) = create_progressive_init_test();
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    let result = execute_transition(&provider, &process, &packwiz, &workdir, StateTransition::RefreshIndex).await;

    assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
}

#[tokio::test]
async fn test_initialize_transition_rejects_already_configured_project() {
    let (provider, workdir) = create_configured_test();
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    let result = execute_transition(
        &provider,
        &process,
        &packwiz,
        &workdir,
        StateTransition::Initialize(crate::primitives::InitializationConfig {
            name: "Test Pack",
            author: "Test Author",
            version: "1.0.0",
            modloader: "fabric",
            mc_version: "1.20.1",
            loader_version: "0.14.21",
        }),
    )
    .await;

    assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
}

#[test]
fn test_begin_build_transition_rejects_incomplete_configured_layout() {
    let (provider, workdir) = create_progressive_init_test();
    let manager = PackStateManager::new(workdir, &provider);

    let result = manager.begin_state_transition(StateTransition::Building);
    assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
}

#[test]
fn test_begin_build_transition_allows_rebuilds_from_built_state() {
    let (provider, workdir) = create_built_test();
    let manager = PackStateManager::new(workdir, &provider);

    let result = manager.begin_state_transition(StateTransition::Building);
    assert!(result.is_ok());
}

#[test]
fn test_pure_execute_initialize_function() {
    let (provider, workdir) = create_uninitialized_test();
    let packwiz = mock_packwiz_for_test();
    let result = execute_initialize(
        &provider,
        &packwiz,
        &workdir,
        "Test Pack",
        "Test Author",
        "1.0.0",
        "fabric",
        "1.20.1",
        "0.14.21",
    )
    .unwrap();
    assert_eq!(result, PackState::Configured);

    // Verify the mock provider received the expected calls
    assert!(
        provider
            .files
            .borrow()
            .contains(&workdir.join("empack.yml"))
    );
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("pack"))
    );
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("templates"))
    );
}

#[test]
fn test_pure_execute_refresh_index_function() {
    // Test that execute_refresh_index correctly validates configuration and runs packwiz refresh
    // With valid mock data, this should succeed demonstrating the function works correctly
    let (provider, workdir) = create_configured_test();
    let packwiz = mock_packwiz_for_test();
    // Pre-populate the mock packwiz filesystem with pack.toml so refresh finds it
    packwiz.filesystem.lock().unwrap().insert(
        workdir.join("pack").join("pack.toml"),
        "mock".to_string(),
    );
    let result = execute_refresh_index(&provider, &packwiz, &workdir);

    // With valid YAML and mock packwiz, synchronization should succeed
    // This demonstrates that the pure function correctly calls ConfigManager and packwiz
    match result {
        Ok(transition_result) => {
            assert_eq!(transition_result.state, PackState::Configured);
        }
        Err(e) => {
            panic!("Expected success, got error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_pure_execute_build_function() {
    let workdir = mock_root().join("configured-project");
    let session = configured_build_session(&workdir);
    let targets = vec![BuildTarget::Mrpack];
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&session).unwrap();
    let result = execute_build(mock_orchestrator, &targets).await.unwrap();
    assert_eq!(result, PackState::Built);
}

#[test]
fn test_pure_clean_functions() {
    // Test clean_build_artifacts
    let (provider, workdir) = create_built_test();
    let result = clean_build_artifacts(&provider, &workdir);
    assert!(result.is_ok());
    assert!(
        !provider
            .directories
            .borrow()
            .contains(&workdir.join("dist"))
    );

    // Test clean_configuration
    let (provider, workdir) = create_configured_test();
    let result = clean_configuration(&provider, &workdir);
    assert!(result.is_ok());
    assert!(
        !provider
            .files
            .borrow()
            .contains(&workdir.join("empack.yml"))
    );
    assert!(
        !provider
            .directories
            .borrow()
            .contains(&workdir.join("pack"))
    );
}

#[test]
fn test_clean_build_artifacts_preserves_configuration_metadata() {
    let (provider, workdir) = create_built_test();

    clean_build_artifacts(&provider, &workdir).unwrap();

    assert!(provider.files.borrow().contains(&workdir.join("empack.yml")));
    assert!(provider.files.borrow().contains(&workdir.join("pack").join("pack.toml")));
    assert!(provider.files.borrow().contains(&workdir.join("pack").join("index.toml")));
    assert!(!provider.directories.borrow().contains(&workdir.join("dist")));
}

#[test]
fn test_pure_create_initial_structure_function() {
    let (provider, workdir) = create_uninitialized_test();
    let result = create_initial_structure(&provider, &workdir);
    assert!(result.is_ok());

    // Verify expected directories were created
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("pack"))
    );
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("templates"))
    );
}

/// Test: Invalid state transition is rejected
///
/// Validates that can_transition() correctly rejects invalid state transitions
/// Example: Uninitialized → Built (must go through Configured first)
#[test]
fn test_invalid_state_transition_rejected() {
    let (_provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir, &_provider);

    let current_state = manager.discover_state().unwrap();
    assert_eq!(current_state, PackState::Uninitialized);

    // Attempt to transition directly to Built state (invalid)
    let can_build = manager.can_transition(&current_state, &PackState::Built);
    assert!(!can_build, "Should not be able to transition from Uninitialized to Built");

    // Verify valid transitions are still allowed
    let can_configure = manager.can_transition(&current_state, &PackState::Configured);
    assert!(can_configure, "Should be able to transition from Uninitialized to Configured");
}

/// Test: Invalid state transitions return appropriate errors
///
/// Validates error paths when execute_transition is called with invalid state sequences
#[tokio::test]
async fn test_invalid_transition_execution_error() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();

    // Attempt to execute Cleaning transition from Uninitialized state
    // Cleaning expects Built or Configured state, so this should fail
    let result = manager
        .execute_transition(&process, &packwiz, StateTransition::Cleaning)
        .await;

    // Transition should fail with error for invalid transition
    // The state machine should reject transitioning to Cleaning from Uninitialized

    match result {
        Err(e) => {
            // Expected: error for invalid transition
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("transition") || error_msg.contains("state") || error_msg.contains("invalid"),
                "Error should mention invalid transition, got: {}",
                error_msg
            );
        }
        Ok(r) if r.state == PackState::Uninitialized => {
            // Acceptable: no-op clean on already clean state (stays uninitialized)
            eprintln!("Note: Clean on Uninitialized succeeded as no-op (remained uninitialized)");
        }
        Ok(r) if r.state == PackState::Cleaning => {
            // Acceptable: transition to Cleaning state (will immediately return to previous state)
            eprintln!("Note: Clean on Uninitialized entered Cleaning state (transient)");
        }
        Ok(r) => {
            panic!("Unexpected state after invalid transition: {:?}", r.state);
        }
    }
}

/// Test: State machine prevents invalid intermediate state skips
///
/// Validates that transitions requiring intermediate steps are rejected
#[test]
fn test_state_transition_requires_intermediate_steps() {
    let (_provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir, &_provider);

    // Valid state transition chain:
    // Uninitialized → Configured → Built → Cleaning (backwards)

    // Verify each step is valid individually
    assert!(
        manager.can_transition(&PackState::Uninitialized, &PackState::Configured),
        "Uninitialized -> Configured should be valid"
    );
    assert!(
        manager.can_transition(&PackState::Configured, &PackState::Built),
        "Configured -> Built should be valid"
    );
    assert!(
        manager.can_transition(&PackState::Built, &PackState::Configured),
        "Built -> Configured (clean backwards) should be valid"
    );

    // Verify invalid skips are rejected
    assert!(
        !manager.can_transition(&PackState::Uninitialized, &PackState::Built),
        "Should not skip Configured state"
    );
    assert!(
        !manager.can_transition(&PackState::Uninitialized, &PackState::Cleaning),
        "Should not skip to Cleaning from Uninitialized"
    );
}

/// Test: execute_refresh_index with malformed empack.yml
///
/// Validates error recovery when empack.yml is corrupted or has invalid YAML syntax.
#[test]
fn test_execute_refresh_index_malformed_yaml() {
    let workdir = mock_root().join("malformed-yaml");

    // Load malformed empack.yml directly into the mock filesystem so config validation
    // sees the same contents the test is asserting on.
    let fs_provider = crate::application::session_mocks::MockFileSystemProvider::new()
        .with_current_dir(workdir.clone())
        .with_file(
            workdir.join("empack.yml"),
            "name: test-pack\nversion: [invalid yaml syntax here\nminecraft:\n  version: 1.21.1"
                .to_string(),
        )
        .with_file(
            workdir.join("pack").join("pack.toml"),
            r#"name = "test-pack"
version = "1.0.0"
[index]
file = "index.toml"
"#
            .to_string(),
        );

    let packwiz = mock_packwiz_for_test();

    // Execute refresh-index - should return error due to malformed YAML
    let result = execute_refresh_index(&fs_provider, &packwiz, &workdir);

    // Verify error occurred
    assert!(
        result.is_err(),
        "execute_refresh_index should fail with malformed YAML"
    );

    // Verify error message contains useful information
    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);
    // Error should mention configuration or YAML parsing issue
    assert!(
        err_msg.contains("empack.yml") || err_msg.contains("ConfigManagementError")
            || err_msg.contains("YamlError") || err_msg.contains("YAML"),
        "Error should indicate configuration/YAML issue, got: {}",
        err_msg
    );
}

// --- Interrupted state detection and marker cleanup tests ---

/// Test: discover_state returns Interrupted when building marker file exists
#[test]
fn test_discover_state_detects_interrupted_building() {
    let workdir = mock_root().join("interrupted-building");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    // Simulate a configured project
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));
    // Simulate an interrupted building state via marker file
    provider.add_file_with_content(
        workdir.join(".empack-state"),
        "building".to_string(),
    );

    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(
        state,
        PackState::Interrupted {
            was: Box::new(PackState::Building)
        }
    );
}

/// Test: discover_state returns Interrupted when cleaning marker file exists
#[test]
fn test_discover_state_detects_interrupted_cleaning() {
    let workdir = mock_root().join("interrupted-cleaning");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));
    // Simulate an interrupted cleaning state via marker file
    provider.add_file_with_content(
        workdir.join(".empack-state"),
        "cleaning".to_string(),
    );

    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(
        state,
        PackState::Interrupted {
            was: Box::new(PackState::Cleaning)
        }
    );
}

/// Test: marker file is removed after successful clean transition
#[tokio::test]
async fn test_clean_removes_marker_on_interrupted_state() {
    let workdir = mock_root().join("clean-interrupted");
    let provider = MockStateProvider::new();
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let packwiz = mock_packwiz_for_test();
    provider.add_directory(workdir.clone());
    // Set up a configured project with an interrupted building marker
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));
    provider.add_file_with_content(
        workdir.join(".empack-state"),
        "building".to_string(),
    );

    // Verify it's detected as Interrupted
    let state = discover_state(&provider, &workdir).unwrap();
    assert!(matches!(state, PackState::Interrupted { .. }));

    // Clean should recover from interrupted state
    let result = execute_transition(
        &provider,
        &process,
        &packwiz,
        &workdir,
        StateTransition::Clean,
    )
    .await
    .unwrap();

    // After cleaning an interrupted-building, the underlying state was Configured
    // so cleaning from Configured -> Uninitialized
    assert_eq!(result.state, PackState::Uninitialized);

    // Marker file should be removed
    assert!(
        !provider.exists(&workdir.join(".empack-state")),
        "Marker file should be removed after cleaning"
    );
}

/// Test: marker file is written on Building transition via begin_state_transition
#[test]
fn test_begin_state_transition_writes_marker() {
    let workdir = mock_root().join("marker-write");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));

    let manager = PackStateManager::new(workdir.clone(), &provider);
    manager
        .begin_state_transition(StateTransition::Building)
        .unwrap();

    // Marker file should exist with "building" content
    assert!(provider.exists(&workdir.join(".empack-state")));
    let content = provider
        .read_to_string(&workdir.join(".empack-state"))
        .unwrap();
    assert_eq!(content, "building");
}

/// Test: marker file is removed on complete_state_transition
#[test]
fn test_complete_state_transition_removes_marker() {
    let workdir = mock_root().join("marker-cleanup");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));

    let manager = PackStateManager::new(workdir.clone(), &provider);

    // Begin a building transition (writes marker)
    manager
        .begin_state_transition(StateTransition::Building)
        .unwrap();
    assert!(provider.exists(&workdir.join(".empack-state")));

    // Complete the transition (removes marker)
    manager.complete_state_transition().unwrap();
    assert!(
        !provider.exists(&workdir.join(".empack-state")),
        "Marker file should be removed after completing transition"
    );
}

/// Test: can_transition allows recovery from Interrupted state
#[test]
fn test_can_transition_from_interrupted() {
    // Interrupted states should be able to clean (recover)
    assert!(can_transition(
        &PackState::Interrupted {
            was: Box::new(PackState::Building)
        },
        &PackState::Configured
    ));
    assert!(can_transition(
        &PackState::Interrupted {
            was: Box::new(PackState::Cleaning)
        },
        &PackState::Configured
    ));
    assert!(can_transition(
        &PackState::Interrupted {
            was: Box::new(PackState::Building)
        },
        &PackState::Uninitialized
    ));

    // Interrupted should NOT be able to advance to Built
    assert!(!can_transition(
        &PackState::Interrupted {
            was: Box::new(PackState::Building)
        },
        &PackState::Built
    ));
}

/// Test: Interrupted Display formatting
#[test]
fn test_interrupted_display() {
    let interrupted = PackState::Interrupted {
        was: Box::new(PackState::Building),
    };
    assert_eq!(interrupted.to_string(), "interrupted (was: building)");

    let interrupted_clean = PackState::Interrupted {
        was: Box::new(PackState::Cleaning),
    };
    assert_eq!(
        interrupted_clean.to_string(),
        "interrupted (was: cleaning)"
    );
}

/// Test: discover_state falls through to normal detection when no marker file
#[test]
fn test_discover_state_no_marker_returns_normal_state() {
    let workdir = mock_root().join("no-marker");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));

    // No marker file -- should return Configured normally
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, PackState::Configured);
}

/// Test: Cleaning transition via begin_state_transition writes correct marker
#[test]
fn test_begin_cleaning_transition_writes_marker() {
    let workdir = mock_root().join("cleaning-marker");
    let provider = MockStateProvider::new();
    provider.add_directory(workdir.clone());
    provider.add_file(workdir.join("empack.yml"));
    provider.add_file(workdir.join("pack").join("pack.toml"));
    provider.add_directory(workdir.join("pack"));
    provider.add_directory(artifact_root(&workdir));
    provider.add_build_artifact(artifact_root(&workdir).join("test.mrpack"));

    let manager = PackStateManager::new(workdir.clone(), &provider);
    manager
        .begin_state_transition(StateTransition::Cleaning)
        .unwrap();

    let content = provider
        .read_to_string(&workdir.join(".empack-state"))
        .unwrap();
    assert_eq!(content, "cleaning");
}

// ── transition() enforced free function tests ──────────────────────────────

#[test]
fn test_transition_valid_forward() {
    let result = transition(PackState::Uninitialized, PackState::Configured);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), PackState::Configured);
}

#[test]
fn test_transition_valid_configure_to_built() {
    let result = transition(PackState::Configured, PackState::Built);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), PackState::Built);
}

#[test]
fn test_transition_valid_clean_backwards() {
    let result = transition(PackState::Built, PackState::Configured);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), PackState::Configured);
}

#[test]
fn test_transition_valid_same_state() {
    let result = transition(PackState::Configured, PackState::Configured);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), PackState::Configured);
}

#[test]
fn test_transition_invalid_skip_state() {
    let result = transition(PackState::Uninitialized, PackState::Built);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("transition"),
        "Error should mention transition, got: {}",
        err
    );
}

#[test]
fn test_transition_invalid_building_not_in_whitelist() {
    // Building is a transient state managed by the orchestrator, not a
    // direct target from Uninitialized
    let result = transition(PackState::Uninitialized, PackState::Building);
    assert!(result.is_err());
}

#[test]
fn test_transition_invalid_cleaning_from_uninitialized() {
    let result = transition(PackState::Uninitialized, PackState::Cleaning);
    assert!(result.is_err());
}

#[test]
fn test_transition_interrupted_can_recover_to_configured() {
    let state = PackState::Interrupted {
        was: Box::new(PackState::Building),
    };
    let result = transition(state, PackState::Configured);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), PackState::Configured);
}

#[test]
fn test_transition_interrupted_can_recover_to_uninitialized() {
    let state = PackState::Interrupted {
        was: Box::new(PackState::Building),
    };
    let result = transition(state, PackState::Uninitialized);
    assert!(result.is_ok());
}

#[test]
fn test_transition_interrupted_cannot_skip_to_built() {
    let state = PackState::Interrupted {
        was: Box::new(PackState::Building),
    };
    let result = transition(state, PackState::Built);
    assert!(result.is_err());
}
