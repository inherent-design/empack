use super::*;
use std::collections::HashMap;

/// Mock implementation of StateProvider for testing - zero I/O
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

impl StateProvider for MockStateProvider {
    fn is_directory(&self, path: &Path) -> Result<bool, std::io::Error> {
        Ok(self.directories.borrow().contains(path))
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
    
    fn create_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        // Actually add the directory to the mock filesystem
        self.directories.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }
    
    fn write_file(&self, path: &Path, _content: &str) -> Result<(), std::io::Error> {
        // Actually add the file to the mock filesystem
        self.files.borrow_mut().insert(path.to_path_buf());
        Ok(())
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
    
    fn run_packwiz_init(&self, _workdir: &Path) -> Result<(), StateError> {
        match self.packwiz_results.get("init") {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
    }
    
    fn run_packwiz_refresh(&self, _workdir: &Path) -> Result<(), StateError> {
        match self.packwiz_results.get("refresh") {
            Some(result) => result.clone(),
            None => Ok(()), // Default success
        }
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
    let manager = ModpackStateManager::new(workdir, &provider);
    let state = manager.discover_state().unwrap();
    assert_eq!(state, ModpackState::Uninitialized);
}

#[test] 
fn test_transition_to_configured() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = ModpackStateManager::new(workdir, &provider);

    let result = manager
        .execute_transition(StateTransition::Initialize)
        .unwrap();
    assert_eq!(result, ModpackState::Configured);

    // Note: In the new architecture, we verify the logic without checking actual files
    // The MockStateProvider simulates successful file operations
}

#[test]
fn test_transition_to_built() {
    let (provider, workdir) = create_configured_test();
    let manager = ModpackStateManager::new(workdir, &provider);

    // Build from configured state
    let targets = vec![BuildTarget::Mrpack, BuildTarget::Client];
    let result = manager
        .execute_transition(StateTransition::Build(targets))
        .unwrap();
    assert_eq!(result, ModpackState::Built);

    // In the new architecture, we test that the transition logic works correctly
    // The MockStateProvider handles all I/O operations
}

#[test]
fn test_clean_transitions() {
    let (provider, workdir) = create_built_test();
    let manager = ModpackStateManager::new(workdir, &provider);

    // Start from built state and clean back to configured
    let result = manager.execute_transition(StateTransition::Clean).unwrap();
    assert_eq!(result, ModpackState::Configured);

    // Clean back to uninitialized
    let result = manager.execute_transition(StateTransition::Clean).unwrap();
    assert_eq!(result, ModpackState::Uninitialized);

    // The MockStateProvider simulates successful cleanup operations
}

#[test]
fn test_invalid_transitions() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = ModpackStateManager::new(workdir, &provider);

    // Can't build from uninitialized
    let result = manager.execute_transition(StateTransition::Build(vec![BuildTarget::Mrpack]));
    assert!(result.is_err());

    // Can't sync from uninitialized
    let result = manager.execute_transition(StateTransition::Synchronize);
    assert!(result.is_err());
}

#[test]
fn test_state_validation() {
    // Test uninitialized state validation
    let (provider, workdir) = create_uninitialized_test();
    let manager = ModpackStateManager::new(workdir.clone(), &provider);
    assert!(manager.validate_state(ModpackState::Uninitialized).unwrap());
    assert!(!manager.validate_state(ModpackState::Configured).unwrap());

    // Test configured state validation
    let (provider, workdir) = create_configured_test();
    let manager = ModpackStateManager::new(workdir, &provider);
    assert!(manager.validate_state(ModpackState::Configured).unwrap());
    assert!(!manager.validate_state(ModpackState::Uninitialized).unwrap());
}

#[test]
fn test_paths_helper() {
    let (provider, workdir) = create_uninitialized_test();
    let manager = ModpackStateManager::new(workdir.clone(), &provider);
    let paths = manager.paths();

    assert_eq!(paths.empack_yml, workdir.join("empack.yml"));
    assert_eq!(
        paths.pack_toml,
        workdir.join("pack").join("pack.toml")
    );
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
    assert_eq!(state, ModpackState::Uninitialized);

    // Test configured state
    let (provider, workdir) = create_configured_test();
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, ModpackState::Configured);

    // Test built state
    let (provider, workdir) = create_built_test();
    let state = discover_state(&provider, &workdir).unwrap();
    assert_eq!(state, ModpackState::Built);
}

#[test]
fn test_pure_can_transition_function() {
    // Test valid transitions
    assert!(can_transition(ModpackState::Uninitialized, ModpackState::Configured));
    assert!(can_transition(ModpackState::Configured, ModpackState::Built));
    assert!(can_transition(ModpackState::Built, ModpackState::Configured));
    assert!(can_transition(ModpackState::Configured, ModpackState::Uninitialized));
    
    // Test same state transitions
    assert!(can_transition(ModpackState::Configured, ModpackState::Configured));
    
    // Test invalid transitions
    assert!(!can_transition(ModpackState::Uninitialized, ModpackState::Built));
    assert!(!can_transition(ModpackState::Built, ModpackState::Uninitialized));
}
