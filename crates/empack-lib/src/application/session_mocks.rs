//! Mock implementations of session providers for testing
//!
//! These mocks enable comprehensive testing of command handlers without
//! requiring external dependencies or filesystem operations.

use crate::application::session::{Session, ProcessOutput, *};
use crate::empack::state::ModpackStateManager;
use crate::empack::config::ConfigManager;
use crate::empack::search::{ProjectResolver, ProjectResolverTrait, ProjectInfo, SearchError};
use crate::application::config::AppConfig;
use crate::display::{DisplayProvider, LiveDisplayProvider};
use indicatif::MultiProgress;
use anyhow::Result;
use std::path::PathBuf;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;
use std::cell::RefCell;
use reqwest::Client;

/// Mock filesystem provider for testing
pub struct MockFileSystemProvider {
    pub current_dir: PathBuf,
    pub installed_mods: HashSet<String>,
    pub state_manager_calls: Arc<Mutex<Vec<PathBuf>>>,
    pub config_manager_calls: Arc<Mutex<Vec<PathBuf>>>,
    /// In-memory filesystem: path -> content
    pub files: Arc<Mutex<HashMap<PathBuf, String>>>,
    /// Track directories that exist
    pub directories: Arc<Mutex<HashSet<PathBuf>>>,
}

impl MockFileSystemProvider {
    pub fn new() -> Self {
        Self {
            current_dir: PathBuf::from("/test/workdir"),
            installed_mods: HashSet::new(),
            state_manager_calls: Arc::new(Mutex::new(Vec::new())),
            config_manager_calls: Arc::new(Mutex::new(Vec::new())),
            files: Arc::new(Mutex::new(HashMap::new())),
            directories: Arc::new(Mutex::new(HashSet::new())),
        }
    }
    
    pub fn with_current_dir(mut self, dir: PathBuf) -> Self {
        self.current_dir = dir.clone();
        self.directories.lock().unwrap().insert(dir);
        self
    }
    
    pub fn with_installed_mods(mut self, mods: HashSet<String>) -> Self {
        self.installed_mods = mods;
        self
    }
    
    pub fn with_file(self, path: PathBuf, content: String) -> Self {
        // Add parent directory to directories set
        if let Some(parent) = path.parent() {
            self.directories.lock().unwrap().insert(parent.to_path_buf());
        }
        self.files.lock().unwrap().insert(path, content);
        self
    }
    
    pub fn with_files(self, files: HashMap<PathBuf, String>) -> Self {
        *self.files.lock().unwrap() = files;
        self
    }
    
    /// Helper method to create a typical empack project structure
    pub fn with_empack_project(self, workdir: PathBuf, name: &str, minecraft_version: &str, loader: &str) -> Self {
        let empack_yml = format!(r#"empack:
  dependencies:
    - fabric_api: "Fabric API|mod"
    - sodium: "Sodium|mod"  
  minecraft_version: "{}"
  loader: {}
  name: "{}"
  author: "Test Author"
  version: "1.0.0"
"#, minecraft_version, loader, name);

        let pack_toml = format!(r#"name = "{}"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{}"
{} = "0.14.21"
"#, name, minecraft_version, loader);

        let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;

        self.with_file(workdir.join("empack.yml"), empack_yml)
            .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string())
            .with_file(workdir.join("pack").join("index.toml"), index_toml.to_string())
    }

    /// Standard mock setup for a configured project (empack.yml + pack.toml)
    pub fn with_configured_project(self, workdir: PathBuf) -> Self {
        let empack_yml = r#"empack:
  dependencies:
    - fabric_api: "Fabric API|mod"
    - sodium: "Sodium|mod"
  minecraft_version: "1.21.1"
  loader: fabric
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;

        let pack_toml = r#"name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21.1"
fabric = "0.15.0"
"#;

        let index_toml = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;

        self.with_file(workdir.join("empack.yml"), empack_yml.to_string())
            .with_file(workdir.join("pack").join("pack.toml"), pack_toml.to_string())
            .with_file(workdir.join("pack").join("index.toml"), index_toml.to_string())
    }

    /// Standard mock setup for a built project (configured + build artifacts)
    pub fn with_built_project(self, workdir: PathBuf) -> Self {
        let configured = self.with_configured_project(workdir.clone());
        
        // Add build artifacts
        let dist_dir = workdir.join("dist");
        let mrpack_content = "mock mrpack content";
        let zip_content = "mock zip content";
        
        configured
            .with_file(dist_dir.join("test-pack.mrpack"), mrpack_content.to_string())
            .with_file(dist_dir.join("test-pack.zip"), zip_content.to_string())
    }
}

impl FileSystemProvider for MockFileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        Ok(self.current_dir.clone())
    }
    
    // state_manager method removed - create ModpackStateManager directly
    
    fn get_installed_mods(&self) -> Result<HashSet<String>> {
        Ok(self.installed_mods.clone())
    }
    
    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        self.config_manager_calls.lock().unwrap().push(workdir.clone());
        ConfigManager::new(workdir, self)
    }
    
    fn read_to_string(&self, path: &std::path::Path) -> Result<String> {
        let files = self.files.lock().unwrap();
        if let Some(content) = files.get(path) {
            Ok(content.clone())
        } else {
            Err(anyhow::anyhow!("File not found: {}", path.display()))
        }
    }
    
    fn write_file(&self, path: &std::path::Path, content: &str) -> Result<()> {
        self.files.lock().unwrap().insert(path.to_path_buf(), content.to_string());
        Ok(())
    }
    
    fn exists(&self, path: &std::path::Path) -> bool {
        // Check both files and directories
        self.files.lock().unwrap().contains_key(path) || 
        self.directories.lock().unwrap().contains(path) ||
        self.is_directory(path)
    }
    
    fn is_directory(&self, path: &std::path::Path) -> bool {
        // Check if the path is explicitly tracked as a directory
        let directories = self.directories.lock().unwrap();
        if directories.contains(path) {
            return true;
        }
        
        // Check if the path is the current directory
        if path == self.current_dir {
            return true;
        }
        
        // No fallback pattern matching - if it's not explicitly tracked, it doesn't exist
        false
    }
    
    fn create_dir_all(&self, path: &std::path::Path) -> Result<()> {
        // Track the directory creation in the mock filesystem
        self.directories.lock().unwrap().insert(path.to_path_buf());
        // Also track all parent directories as existing
        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            self.directories.lock().unwrap().insert(parent.to_path_buf());
            current = parent.to_path_buf();
        }
        Ok(())
    }
    
    fn get_file_list(&self, path: &std::path::Path) -> Result<HashSet<PathBuf>, std::io::Error> {
        let files = self.files.lock().unwrap();
        let directories = self.directories.lock().unwrap();
        let mut result = HashSet::new();
        
        // Add files that are direct children of the path
        for file_path in files.keys() {
            if file_path.parent() == Some(path) {
                result.insert(file_path.clone());
            }
        }
        
        // Add directories that are direct children of the path
        for dir_path in directories.iter() {
            if dir_path.parent() == Some(path) {
                result.insert(dir_path.clone());
            }
        }
        
        Ok(result)
    }
    
    fn has_build_artifacts(&self, dist_dir: &std::path::Path) -> Result<bool, std::io::Error> {
        let files = self.files.lock().unwrap();
        
        for path in files.keys() {
            if path.starts_with(dist_dir) {
                if let Some(extension) = path.extension() {
                    match extension.to_str() {
                        Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                        _ => continue,
                    }
                }
            }
        }
        
        Ok(false)
    }
    
    fn remove_file(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
    
    fn remove_dir_all(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let mut files = self.files.lock().unwrap();
        let paths_to_remove: Vec<PathBuf> = files.keys()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect();
        
        for path in paths_to_remove {
            files.remove(&path);
        }
        
        Ok(())
    }
    
    fn run_packwiz_init(&self, workdir: &std::path::Path) -> Result<(), crate::empack::state::StateError> {
        // Mock packwiz init - create expected files in memory
        let pack_dir = workdir.join("pack");
        let pack_file = pack_dir.join("pack.toml");
        let default_pack_toml = r#"name = "Test Modpack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
        self.write_file(&pack_file, default_pack_toml).map_err(|e| 
            crate::empack::state::StateError::IoError {
                message: e.to_string(),
            }
        )?;

        // Also create index.toml
        let index_file = pack_dir.join("index.toml");
        let default_index = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
        self.write_file(&index_file, default_index).map_err(|e| 
            crate::empack::state::StateError::IoError {
                message: e.to_string(),
            }
        )?;
        
        Ok(())
    }
    
    fn run_packwiz_refresh(&self, workdir: &std::path::Path) -> Result<(), crate::empack::state::StateError> {
        // Mock packwiz refresh - verify pack.toml exists
        let pack_file = workdir.join("pack").join("pack.toml");
        if !self.exists(&pack_file) {
            return Err(crate::empack::state::StateError::MissingFile {
                file: "pack.toml".to_string(),
            });
        }
        Ok(())
    }
    
    fn get_bootstrap_jar_cache_path(&self) -> Result<PathBuf> {
        // For testing, return a path in the test directory
        let jar_path = self.current_dir.join("cache").join("packwiz-installer-bootstrap.jar");
        
        // Ensure the mock JAR file exists to prevent network download attempts
        if !self.exists(&jar_path) {
            // Create cache directory
            let cache_dir = jar_path.parent().unwrap().to_path_buf();
            self.directories.lock().unwrap().insert(cache_dir);
            
            // Create mock JAR file
            self.files.lock().unwrap().insert(jar_path.clone(), "mock bootstrap jar content".to_string());
        }
        
        Ok(jar_path)
    }
}

/// Mock network provider for testing
pub struct MockNetworkProvider {
    pub client_calls: Arc<Mutex<usize>>,
    pub resolver_calls: Arc<Mutex<Vec<(Client, Option<String>)>>>,
    pub mock_resolver: Arc<MockProjectResolver>,
}

impl MockNetworkProvider {
    pub fn new() -> Self {
        Self {
            client_calls: Arc::new(Mutex::new(0)),
            resolver_calls: Arc::new(Mutex::new(Vec::new())),
            mock_resolver: Arc::new(MockProjectResolver::new()),
        }
    }
    
    pub fn with_project_response(mut self, query: String, project_info: ProjectInfo) -> Self {
        self.mock_resolver.responses.lock().unwrap().insert(query, Ok(project_info));
        self
    }
    
    pub fn with_error_response(mut self, query: String, error_message: String) -> Self {
        self.mock_resolver.responses.lock().unwrap().insert(query, Err(error_message));
        self
    }
}

impl NetworkProvider for MockNetworkProvider {
    fn http_client(&self) -> Result<Client> {
        *self.client_calls.lock().unwrap() += 1;
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))
    }
    
    fn project_resolver(&self, client: Client, curseforge_api_key: Option<String>) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        self.resolver_calls.lock().unwrap().push((client.clone(), curseforge_api_key.clone()));
        Box::new(self.mock_resolver.as_ref().clone())
    }
}

/// Mock project resolver for testing
#[derive(Clone)]
pub struct MockProjectResolver {
    pub responses: Arc<Mutex<HashMap<String, Result<ProjectInfo, String>>>>,
}

impl MockProjectResolver {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub fn with_response(mut self, query: String, response: Result<ProjectInfo, String>) -> Self {
        self.responses.lock().unwrap().insert(query, response);
        self
    }
    
    pub fn with_project_response(mut self, query: String, project_info: ProjectInfo) -> Self {
        self.responses.lock().unwrap().insert(query, Ok(project_info));
        self
    }
    
    pub fn with_error_response(mut self, query: String, error_message: String) -> Self {
        self.responses.lock().unwrap().insert(query, Err(error_message));
        self
    }
}

impl ProjectResolverTrait for MockProjectResolver {
    fn resolve_project(
        &self,
        title: &str,
        _project_type: Option<&str>,
        _minecraft_version: Option<&str>,
        _mod_loader: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectInfo, SearchError>> + Send + '_>> {
        let responses = self.responses.clone();
        let query = title.to_string();
        
        Box::pin(async move {
            let responses = responses.lock().unwrap();
            match responses.get(&query).cloned() {
                Some(Ok(project_info)) => Ok(project_info),
                Some(Err(error_message)) => Err(SearchError::NoResults { query: error_message }),
                None => Err(SearchError::NoResults { query }),
            }
        })
    }
}

/// Process call record for spy pattern
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessCall {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

/// Mock process provider for testing with spy pattern
pub struct MockProcessProvider {
    pub packwiz_available: bool,
    pub packwiz_version: String,
    pub calls: RefCell<Vec<ProcessCall>>,
    pub results: HashMap<(String, Vec<String>), Result<ProcessOutput, String>>,
}

impl MockProcessProvider {
    pub fn new() -> Self {
        Self {
            packwiz_available: true,
            packwiz_version: "1.0.0".to_string(),
            calls: RefCell::new(Vec::new()),
            results: HashMap::new(),
        }
    }
    
    pub fn with_packwiz_unavailable(mut self) -> Self {
        self.packwiz_available = false;
        self
    }
    
    pub fn with_packwiz_version(mut self, version: String) -> Self {
        self.packwiz_version = version;
        self
    }
    
    pub fn with_result(mut self, command: String, args: Vec<String>, result: Result<ProcessOutput, String>) -> Self {
        self.results.insert((command, args), result);
        self
    }
    
    pub fn with_packwiz_result(mut self, args: Vec<String>, result: Result<ProcessOutput, String>) -> Self {
        self.results.insert(("packwiz".to_string(), args), result);
        self
    }
    
    /// Get all recorded process calls for verification
    pub fn get_calls(&self) -> Vec<ProcessCall> {
        self.calls.borrow().clone()
    }
    
    /// Get calls for a specific command
    pub fn get_calls_for_command(&self, command: &str) -> Vec<ProcessCall> {
        self.calls.borrow().iter()
            .filter(|call| call.command == command)
            .cloned()
            .collect()
    }
    
    /// Verify that a specific command was called with expected arguments
    pub fn verify_call(&self, command: &str, args: &[&str], working_dir: &std::path::Path) -> bool {
        let expected_call = ProcessCall {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            working_dir: working_dir.to_path_buf(),
        };
        
        self.calls.borrow().contains(&expected_call)
    }
}

impl ProcessProvider for MockProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &std::path::Path) -> Result<ProcessOutput> {
        // Record the call for spy pattern verification
        let call = ProcessCall {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            working_dir: working_dir.to_path_buf(),
        };
        self.calls.borrow_mut().push(call);
        
        // Check if we have a specific result for this command
        let key = (command.to_string(), args.iter().map(|s| s.to_string()).collect());
        if let Some(result) = self.results.get(&key) {
            match result {
                Ok(output) => Ok(output.clone()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            }
        } else {
            // Default behavior: succeed with empty output
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: String::new(),
                success: true,
            })
        }
    }
    
    fn check_packwiz(&self) -> Result<(bool, String)> {
        Ok((self.packwiz_available, self.packwiz_version.clone()))
    }
    
    fn get_packwiz_version(&self) -> Option<String> {
        if self.packwiz_available {
            Some(self.packwiz_version.clone())
        } else {
            None
        }
    }
}

/// Mock config provider for testing
pub struct MockConfigProvider {
    pub app_config: AppConfig,
}

impl MockConfigProvider {
    pub fn new(app_config: AppConfig) -> Self {
        Self { app_config }
    }
}

impl ConfigProvider for MockConfigProvider {
    fn app_config(&self) -> &AppConfig {
        &self.app_config
    }
}

/// Mock command session for testing
pub struct MockCommandSession {
    pub multi_progress: MultiProgress,
    pub display_provider: LiveDisplayProvider,
    pub filesystem_provider: MockFileSystemProvider,
    pub network_provider: MockNetworkProvider,
    pub process_provider: MockProcessProvider,
    pub config_provider: MockConfigProvider,
}

impl MockCommandSession {
    pub fn new() -> Self {
        // Initialize display system for tests
        use crate::display::Display;
        use crate::terminal::capabilities::TerminalCapabilities;
        let capabilities = TerminalCapabilities::detect_from_config(&AppConfig::default())
            .expect("Failed to detect terminal capabilities for testing");
        let _ = Display::init(capabilities);
        
        let multi_progress = MultiProgress::new();
        let display_provider = LiveDisplayProvider::new_with_multi_progress(&multi_progress);
        
        Self {
            multi_progress,
            display_provider,
            filesystem_provider: MockFileSystemProvider::new(),
            network_provider: MockNetworkProvider::new(),
            process_provider: MockProcessProvider::new(),
            config_provider: MockConfigProvider::new(AppConfig::default()),
        }
    }
    
    pub fn with_filesystem(mut self, filesystem: MockFileSystemProvider) -> Self {
        self.filesystem_provider = filesystem;
        self
    }
    
    pub fn with_network(mut self, network: MockNetworkProvider) -> Self {
        self.network_provider = network;
        self
    }
    
    pub fn with_process(mut self, process: MockProcessProvider) -> Self {
        self.process_provider = process;
        self
    }
    
    pub fn with_config(mut self, config: MockConfigProvider) -> Self {
        self.config_provider = config;
        self
    }
    
    /// Get the display provider for this session
    pub fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }
    
    /// Get the filesystem provider for this session
    pub fn filesystem(&self) -> &dyn FileSystemProvider {
        &self.filesystem_provider
    }
    
    /// Get the network provider for this session
    pub fn network(&self) -> &dyn NetworkProvider {
        &self.network_provider
    }
    
    /// Get the process provider for this session
    pub fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }
    
    /// Get the config provider for this session
    pub fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }
}

impl Session for MockCommandSession {
    fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }
    
    fn filesystem(&self) -> &dyn FileSystemProvider {
        &self.filesystem_provider
    }
    
    fn network(&self) -> &dyn NetworkProvider {
        &self.network_provider
    }
    
    fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }
    
    fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }
    
    fn state(&self) -> crate::empack::state::ModpackStateManager<'_, dyn FileSystemProvider + '_> {
        let workdir = self.filesystem().current_dir().expect("Failed to get current directory");
        crate::empack::state::ModpackStateManager::new(workdir, self.filesystem())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    
    #[test]
    fn test_mock_filesystem_provider() {
        let mut mods = HashSet::new();
        mods.insert("test_mod".to_string());
        
        let provider = MockFileSystemProvider::new()
            .with_current_dir(PathBuf::from("/custom/path"))
            .with_installed_mods(mods.clone());
        
        assert_eq!(provider.current_dir().unwrap(), PathBuf::from("/custom/path"));
        assert_eq!(provider.get_installed_mods().unwrap(), mods);
    }
    
    #[test]
    fn test_mock_process_provider() {
        let working_dir = PathBuf::from("/test/workdir");
        let provider = MockProcessProvider::new()
            .with_packwiz_version("2.0.0".to_string())
            .with_result(
                "packwiz".to_string(),
                vec!["add".to_string(), "test-mod".to_string()],
                Err("Mock error".to_string())
            );
        
        assert_eq!(provider.check_packwiz().unwrap(), (true, "2.0.0".to_string()));
        assert_eq!(provider.get_packwiz_version().unwrap(), "2.0.0");
        
        // Test successful command (uses default behavior)
        let result = provider.execute("packwiz", &["list"], &working_dir);
        assert!(result.is_ok());
        assert!(result.unwrap().success);
        
        // Test command with specific result
        let result = provider.execute("packwiz", &["add", "test-mod"], &working_dir);
        assert!(result.is_err());
        
        // Test spy pattern - verify calls were recorded
        let calls = provider.get_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].command, "packwiz");
        assert_eq!(calls[0].args, vec!["list"]);
        assert_eq!(calls[1].command, "packwiz");
        assert_eq!(calls[1].args, vec!["add", "test-mod"]);
        
        // Test verification helper
        assert!(provider.verify_call("packwiz", &["list"], &working_dir));
        assert!(provider.verify_call("packwiz", &["add", "test-mod"], &working_dir));
        assert!(!provider.verify_call("packwiz", &["remove", "test-mod"], &working_dir));
    }
    
    #[test]
    fn test_mock_command_session() {
        let session = MockCommandSession::new()
            .with_process(MockProcessProvider::new().with_packwiz_unavailable());
        
        assert_eq!(session.process().check_packwiz().unwrap(), (false, "1.0.0".to_string()));
    }
}