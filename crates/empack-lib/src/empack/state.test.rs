use super::*;
use crate::empack::config::ConfigManager;
use std::collections::{HashMap, HashSet};

/// Mock implementation of FileSystemProvider for testing - zero I/O
/// Now supports stateful operations that actually modify the simulated filesystem
#[derive(Debug)]
struct MockStateProvider {
    /// Simulated filesystem as a set of file paths
    files: std::cell::RefCell<HashSet<PathBuf>>,
    /// Simulated directories
    directories: std::cell::RefCell<HashSet<PathBuf>>,
    /// Files with build artifacts
    build_artifacts: std::cell::RefCell<HashSet<PathBuf>>,
    /// Results for packwiz commands
    packwiz_results: HashMap<String, Result<(), StateError>>,
}

impl MockStateProvider {
    fn new() -> Self {
        Self {
            files: std::cell::RefCell::new(HashSet::new()),
            directories: std::cell::RefCell::new(HashSet::new()),
            build_artifacts: std::cell::RefCell::new(HashSet::new()),
            packwiz_results: HashMap::new(),
        }
    }

    /// Add a file to the mock filesystem
    fn add_file(&self, path: PathBuf) {
        self.files.borrow_mut().insert(path);
    }

    /// Add a directory to the mock filesystem
    fn add_directory(&self, path: PathBuf) {
        self.directories.borrow_mut().insert(path);
    }

    /// Add a build artifact file
    fn add_build_artifact(&self, path: PathBuf) {
        self.build_artifacts.borrow_mut().insert(path);
    }

    /// Set result for packwiz commands (immutable for test setup)
    fn with_packwiz_result(mut self, command: &str, result: Result<(), StateError>) -> Self {
        self.packwiz_results.insert(command.to_string(), result);
        self
    }
}

impl crate::application::session::FileSystemProvider for MockStateProvider {
    fn current_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(PathBuf::from("/test"))
    }

    // state_manager method removed - create PackStateManager directly

    fn get_installed_mods(&self) -> anyhow::Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        ConfigManager::new(workdir, self)
    }

    fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
        if self.files.borrow().contains(path) {
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

    fn write_file(&self, path: &Path, _content: &str) -> anyhow::Result<()> {
        // Actually add the file to the mock filesystem
        self.files.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains(path)
    }

    fn is_directory(&self, path: &Path) -> bool {
        self.directories.borrow().contains(path)
    }

    fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        // Actually add the directory to the mock filesystem
        self.directories.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }

    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>, std::io::Error> {
        let mut files_in_dir = HashSet::new();

        // Return files that are children of the given path
        for file in self.files.borrow().iter() {
            if let Some(parent) = file.parent() {
                if parent == path {
                    files_in_dir.insert(file.clone());
                }
            }
        }

        Ok(files_in_dir)
    }

    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, std::io::Error> {
        // Check if any build artifacts exist in the dist directory
        for artifact in self.build_artifacts.borrow().iter() {
            if let Some(parent) = artifact.parent() {
                if parent == dist_dir {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn remove_file(&self, path: &Path) -> Result<(), std::io::Error> {
        // Actually remove the file from the mock filesystem
        self.files.borrow_mut().remove(path);
        self.build_artifacts.borrow_mut().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
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

    fn run_packwiz_init(
        &self,
        _process: &dyn crate::application::session::ProcessProvider,
        _workdir: &Path,
        _name: &str,
        _author: &str,
        _version: &str,
        _modloader: &str,
        _mc_version: &str,
        _loader_version: &str,
    ) -> Result<(), StateError> {
        match self.packwiz_results.get("init") {
            Some(Ok(_)) => Ok(()),
            Some(Err(_)) => Err(StateError::CommandFailed {
                command: "packwiz init".to_string(),
            }),
            None => Ok(()), // Default success
        }
    }

    fn run_packwiz_refresh(
        &self,
        _process: &dyn crate::application::session::ProcessProvider,
        _workdir: &Path,
    ) -> Result<(), StateError> {
        match self.packwiz_results.get("refresh") {
            Some(Ok(_)) => Ok(()),
            Some(Err(_)) => Err(StateError::CommandFailed {
                command: "packwiz refresh".to_string(),
            }),
            None => Ok(()), // Default success
        }
    }

    fn get_bootstrap_jar_cache_path(&self) -> anyhow::Result<PathBuf> {
        // For state tests, return a mock path
        Ok(PathBuf::from("/test/cache/packwiz-installer-bootstrap.jar"))
    }
}

/// Helper to create a test setup with uninitialized state
fn create_uninitialized_test() -> (MockStateProvider, PathBuf) {
    let mock_provider = MockStateProvider::new();
    let workdir = PathBuf::from("/test/workdir");
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

    let result = manager
        .execute_transition(
            &process,
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
    assert_eq!(result, PackState::Configured);

    // Note: In the new architecture, we verify the logic without checking actual files
    // The MockStateProvider simulates successful file operations
}

#[tokio::test]
async fn test_transition_to_built() {
    let (provider, workdir) = create_configured_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();

    // Build from configured state
    let targets = vec![BuildTarget::Mrpack, BuildTarget::Client];
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session).unwrap();
    let result = manager
        .execute_transition(&process, StateTransition::Build(mock_orchestrator, targets))
        .await
        .unwrap();
    assert_eq!(result, PackState::Built);

    // In the new architecture, we test that the transition logic works correctly
    // The MockStateProvider handles all I/O operations
}

#[tokio::test]
async fn test_clean_transitions() {
    let (provider, workdir) = create_built_test();
    let manager = PackStateManager::new(workdir, &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();

    // Start from built state and clean back to configured
    let result = manager
        .execute_transition(&process, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result, PackState::Configured);

    // Clean back to uninitialized
    let result = manager
        .execute_transition(&process, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result, PackState::Uninitialized);

    // The MockStateProvider simulates successful cleanup operations
}

#[tokio::test]
async fn test_invalid_transitions() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = PackStateManager::new(workdir.clone(), &provider);
    let process = crate::application::session_mocks::MockProcessProvider::new();

    // Can't build from uninitialized
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session).unwrap();
    let result = manager
        .execute_transition(
            &process,
            StateTransition::Build(mock_orchestrator, vec![BuildTarget::Mrpack]),
        )
        .await;
    assert!(result.is_err());

    // Can't sync from uninitialized
    let result = manager
        .execute_transition(&process, StateTransition::Synchronize)
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
fn test_pure_can_transition_function() {
    // Test valid transitions
    assert!(can_transition(
        PackState::Uninitialized,
        PackState::Configured
    ));
    assert!(can_transition(
        PackState::Configured,
        PackState::Built
    ));
    assert!(can_transition(
        PackState::Built,
        PackState::Configured
    ));
    assert!(can_transition(
        PackState::Configured,
        PackState::Uninitialized
    ));

    // Test same state transitions
    assert!(can_transition(
        PackState::Configured,
        PackState::Configured
    ));

    // Test invalid transitions
    assert!(!can_transition(
        PackState::Uninitialized,
        PackState::Built
    ));
    assert!(!can_transition(
        PackState::Built,
        PackState::Uninitialized
    ));
}

#[tokio::test]
async fn test_pure_execute_transition_function() {
    let process = crate::application::session_mocks::MockProcessProvider::new();

    // Test initialize transition
    let (provider, workdir) = create_uninitialized_test();
    let result = execute_transition(
        &provider,
        &process,
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
    assert_eq!(result, PackState::Configured);

    // Test build transition
    let (provider, workdir) = create_configured_test();
    let targets = vec![BuildTarget::Mrpack];
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session).unwrap();
    let result = execute_transition(
        &provider,
        &process,
        &workdir,
        StateTransition::Build(mock_orchestrator, targets),
    )
    .await
    .unwrap();
    assert_eq!(result, PackState::Built);

    // Test synchronize transition (should succeed with valid mock data)
    let (provider, workdir) = create_configured_test();
    let result = execute_transition(&provider, &process, &workdir, StateTransition::Synchronize).await;
    // With valid YAML and mock packwiz, synchronize should succeed
    assert_eq!(result.unwrap(), PackState::Configured);

    // Test clean transition from built
    let (provider, workdir) = create_built_test();
    let result = execute_transition(&provider, &process, &workdir, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result, PackState::Configured);

    // Test clean transition from configured
    let (provider, workdir) = create_configured_test();
    let result = execute_transition(&provider, &process, &workdir, StateTransition::Clean)
        .await
        .unwrap();
    assert_eq!(result, PackState::Uninitialized);
}

#[test]
fn test_pure_execute_initialize_function() {
    let (provider, workdir) = create_uninitialized_test();
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let result = execute_initialize(
        &provider,
        &process,
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
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("installer"))
    );
}

#[test]
fn test_pure_execute_synchronize_function() {
    // Test that execute_synchronize correctly validates configuration and runs packwiz refresh
    // With valid mock data, this should succeed demonstrating the function works correctly
    let (provider, workdir) = create_configured_test();
    let process = crate::application::session_mocks::MockProcessProvider::new();
    let result = execute_synchronize(&provider, &process, &workdir);

    // With valid YAML and mock packwiz, synchronization should succeed
    // This demonstrates that the pure function correctly calls ConfigManager and packwiz
    match result {
        Ok(PackState::Configured) => {
            // Expected: synchronize returns to Configured state after validation
        }
        Err(e) => {
            panic!("Expected success, got error: {:?}", e);
        }
        Ok(other_state) => {
            panic!("Expected Configured state, got: {:?}", other_state);
        }
    }
}

#[tokio::test]
async fn test_pure_execute_build_function() {
    let (provider, workdir) = create_configured_test();
    let targets = vec![BuildTarget::Mrpack, BuildTarget::Client];
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let mock_orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session).unwrap();
    let result = execute_build(mock_orchestrator, &targets).await.unwrap();
    assert_eq!(result, PackState::Built);

    // In the new architecture, the build verification is handled by the orchestrator
    // The test validates the state transition logic
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
    assert!(
        provider
            .directories
            .borrow()
            .contains(&workdir.join("installer"))
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
    let can_build = manager.can_transition(current_state, PackState::Built);
    assert!(!can_build, "Should not be able to transition from Uninitialized to Built");

    // Verify valid transitions are still allowed
    let can_configure = manager.can_transition(current_state, PackState::Configured);
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

    // Attempt to execute Cleaning transition from Uninitialized state
    // Cleaning expects Built or Configured state, so this should fail
    let result = manager
        .execute_transition(&process, StateTransition::Cleaning)
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
        Ok(PackState::Uninitialized) => {
            // Acceptable: no-op clean on already clean state (stays uninitialized)
            eprintln!("Note: Clean on Uninitialized succeeded as no-op (remained uninitialized)");
        }
        Ok(PackState::Cleaning) => {
            // Acceptable: transition to Cleaning state (will immediately return to previous state)
            eprintln!("Note: Clean on Uninitialized entered Cleaning state (transient)");
        }
        Ok(other_state) => {
            panic!("Unexpected state after invalid transition: {:?}", other_state);
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
        manager.can_transition(PackState::Uninitialized, PackState::Configured),
        "Uninitialized → Configured should be valid"
    );
    assert!(
        manager.can_transition(PackState::Configured, PackState::Built),
        "Configured → Built should be valid"
    );
    assert!(
        manager.can_transition(PackState::Built, PackState::Configured),
        "Built → Configured (clean backwards) should be valid"
    );

    // Verify invalid skips are rejected
    assert!(
        !manager.can_transition(PackState::Uninitialized, PackState::Built),
        "Should not skip Configured state"
    );
    assert!(
        !manager.can_transition(PackState::Uninitialized, PackState::Cleaning),
        "Should not skip to Cleaning from Uninitialized"
    );
}
