//! Command session architecture
//! 
//! Implements the Session-Scoped Dependency Injection Pattern.
//! Each command execution creates a session that owns all ephemeral state.

use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::empack::state::{ModpackStateManager, StateError};
use crate::empack::config::ConfigManager;
use crate::empack::search::{ProjectResolver, ProjectResolverTrait};
use crate::application::config::AppConfig;
use crate::primitives::{ModpackState, StateTransition};
use indicatif::MultiProgress;
use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use std::env;
use std::collections::HashSet;
use reqwest::Client;

/// Abstract interface for state management operations
pub trait StateManager {
    /// Discover current state from filesystem
    fn discover_state(&self) -> Result<ModpackState, StateError>;
    
    /// Execute a state transition
    fn execute_transition(&self, transition: StateTransition) -> Result<ModpackState, StateError>;
}

/// Provider trait for filesystem operations
pub trait FileSystemProvider {
    /// Get current working directory
    fn current_dir(&self) -> Result<PathBuf>;
    
    /// Create a modpack state manager for the given directory
    fn state_manager(&self, workdir: PathBuf) -> Box<dyn StateManager + '_>;
    
    /// Get list of currently installed mods from packwiz
    fn get_installed_mods(&self) -> Result<HashSet<String>>;
    
    /// Create a config manager for the given directory
    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_>;
    
    // Core file I/O operations for dependency injection
    /// Read entire file contents as string
    fn read_to_string(&self, path: &Path) -> Result<String>;
    
    /// Write string content to file
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;
    
    /// Check if path exists
    fn exists(&self, path: &Path) -> bool;
    
    /// Check if path is a directory
    fn is_directory(&self, path: &Path) -> bool;
    
    /// Create directory and all parent directories
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    
    // Additional methods for state management
    /// Get list of files and directories in a path
    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>, std::io::Error>;
    
    /// Check if directory has build artifacts (mrpack, zip, jar files or build target dirs)
    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, std::io::Error>;
    
    /// Remove a file
    fn remove_file(&self, path: &Path) -> Result<(), std::io::Error>;
    
    /// Remove a directory and all its contents
    fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error>;
    
    /// Run packwiz init command
    fn run_packwiz_init(&self, workdir: &Path) -> Result<(), crate::empack::state::StateError>;
    
    /// Run packwiz refresh command
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), crate::empack::state::StateError>;
}

/// Provider trait for network operations
pub trait NetworkProvider {
    /// Create an HTTP client with appropriate timeout
    fn http_client(&self) -> Result<Client>;
    
    /// Create a project resolver with HTTP client and API keys
    fn project_resolver(&self, client: Client, curseforge_api_key: Option<String>) -> Box<dyn ProjectResolverTrait + Send + Sync>;
}

/// Provider trait for process execution
pub trait ProcessProvider {
    /// Execute a packwiz command with given arguments
    fn execute_packwiz(&self, args: &[&str]) -> Result<()>;
    
    /// Check if packwiz is available and return version info
    fn check_packwiz(&self) -> Result<(bool, String)>;
    
    /// Get packwiz version using go toolchain
    fn get_packwiz_version(&self) -> Option<String>;
}

/// Provider trait for configuration access
pub trait ConfigProvider {
    /// Get the application configuration
    fn app_config(&self) -> &AppConfig;
}

/// Session trait that both CommandSession and MockCommandSession can implement
pub trait Session {
    /// Get the display provider for this session
    fn display(&self) -> &dyn DisplayProvider;
    
    /// Get the filesystem provider for this session
    fn filesystem(&self) -> &dyn FileSystemProvider;
    
    /// Get the network provider for this session
    fn network(&self) -> &dyn NetworkProvider;
    
    /// Get the process provider for this session
    fn process(&self) -> &dyn ProcessProvider;
    
    /// Get the config provider for this session
    fn config(&self) -> &dyn ConfigProvider;
}

/// Live implementation of FileSystemProvider
pub struct LiveFileSystemProvider;

impl FileSystemProvider for LiveFileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        env::current_dir().context("Failed to get current directory")
    }
    
    fn state_manager(&self, workdir: PathBuf) -> Box<dyn StateManager + '_> {
        Box::new(ModpackStateManager::new(workdir, self))
    }
    
    fn get_installed_mods(&self) -> Result<HashSet<String>> {
        let output = std::process::Command::new("packwiz")
            .arg("list")
            .output()
            .context("Failed to execute packwiz list command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Packwiz list command failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut installed_mods = HashSet::new();

        // Parse packwiz list output - each line should contain a mod name
        // The format varies, but we're looking for .toml files or project names
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Mods:") || line.starts_with("Total:") {
                continue;
            }

            // Extract mod name from various formats packwiz might use
            // Common formats: "- modname" or "modname.pw.toml" or just "modname"
            let mod_name = if line.starts_with("- ") {
                line.trim_start_matches("- ").trim()
            } else if line.ends_with(".pw.toml") {
                line.trim_end_matches(".pw.toml")
            } else {
                line
            };

            // Convert to a normalized key format (lowercase, replace spaces/dashes with underscores)
            let normalized_name = mod_name
                .to_lowercase()
                .replace(' ', "_")
                .replace('-', "_");

            installed_mods.insert(normalized_name);
        }

        Ok(installed_mods)
    }
    
    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        ConfigManager::new(workdir, self)
    }
    
    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))
    }
    
    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write file: {}", path.display()))
    }
    
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    
    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }
    
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))
    }
    
    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>, std::io::Error> {
        let mut files = HashSet::new();

        if !path.exists() {
            return Ok(files);
        }

        let entries = std::fs::read_dir(path)?;
        for entry in entries {
            let entry = entry?;
            files.insert(entry.path());
        }

        Ok(files)
    }
    
    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool, std::io::Error> {
        if !dist_dir.exists() {
            return Ok(false);
        }

        let entries = std::fs::read_dir(dist_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Look for common build artifacts (files)
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    match extension.to_str() {
                        Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                        _ => continue,
                    }
                }
            }

            // Also consider build target directories as evidence of build state
            if path.is_dir() {
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    match dir_name {
                        "mrpack" | "client" | "server" | "client-full" | "server-full" => {
                            return Ok(true);
                        }
                        _ => continue,
                    }
                }
            }
        }

        Ok(false)
    }
    
    fn remove_file(&self, path: &Path) -> Result<(), std::io::Error> {
        std::fs::remove_file(path)
    }
    
    fn remove_dir_all(&self, path: &Path) -> Result<(), std::io::Error> {
        std::fs::remove_dir_all(path)
    }
    
    fn run_packwiz_init(&self, workdir: &Path) -> Result<(), crate::empack::state::StateError> {
        use std::process::Command;
        
        #[cfg(test)]
        {
            // Mock packwiz init - create expected files
            let pack_dir = workdir.join("pack");
            self.create_dir_all(&pack_dir).map_err(|e| crate::empack::state::StateError::IoError {
                message: e.to_string(),
            })?;

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
            self.write_file(&pack_file, default_pack_toml).map_err(|e| crate::empack::state::StateError::IoError {
                message: e.to_string(),
            })?;

            // Also create index.toml
            let index_file = pack_dir.join("index.toml");
            let default_index = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
            self.write_file(&index_file, default_index).map_err(|e| crate::empack::state::StateError::IoError {
                message: e.to_string(),
            })?;
            return Ok(());
        }

        #[cfg(not(test))]
        {
            let pack_file = workdir.join("pack").join("pack.toml");

            let status = Command::new("packwiz")
                .args(&["init", "--pack-file", pack_file.to_str().unwrap()])
                .current_dir(workdir)
                .status()
                .map_err(|e| crate::empack::state::StateError::CommandFailed {
                    command: format!("packwiz init failed: {}", e),
                })?;

            if !status.success() {
                return Err(crate::empack::state::StateError::CommandFailed {
                    command: "packwiz init returned non-zero".to_string(),
                });
            }

            Ok(())
        }
    }
    
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), crate::empack::state::StateError> {
        use std::process::Command;
        
        #[cfg(test)]
        {
            // Mock packwiz refresh - verify pack.toml exists
            let pack_file = workdir.join("pack").join("pack.toml");
            if !pack_file.exists() {
                return Err(crate::empack::state::StateError::MissingFile {
                    file: "pack.toml".to_string(),
                });
            }
            return Ok(());
        }

        #[cfg(not(test))]
        {
            let pack_file = workdir.join("pack").join("pack.toml");

            let status = Command::new("packwiz")
                .args(&["--pack-file", pack_file.to_str().unwrap(), "refresh"])
                .current_dir(workdir)
                .status()
                .map_err(|e| crate::empack::state::StateError::CommandFailed {
                    command: format!("packwiz refresh failed: {}", e),
                })?;

            if !status.success() {
                return Err(crate::empack::state::StateError::CommandFailed {
                    command: "packwiz refresh returned non-zero".to_string(),
                });
            }

            Ok(())
        }
    }
}

/// Live implementation of NetworkProvider
pub struct LiveNetworkProvider {
    #[cfg(feature = "test-utils")]
    modrinth_base_url: Option<String>,
    #[cfg(feature = "test-utils")]
    curseforge_base_url: Option<String>,
}

impl LiveNetworkProvider {
    /// Production constructor - uses default API URLs
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "test-utils")]
            modrinth_base_url: None,
            #[cfg(feature = "test-utils")]
            curseforge_base_url: None,
        }
    }
    
    /// Test-only constructor with custom base URLs
    #[cfg(feature = "test-utils")]
    pub fn new_for_test(modrinth_base_url: Option<String>, curseforge_base_url: Option<String>) -> Self {
        Self {
            modrinth_base_url,
            curseforge_base_url,
        }
    }
    
    /// Integration test constructor with custom base URLs (for external test crates)
    #[cfg(feature = "integration-tests")]
    pub fn new_with_base_urls(modrinth_base_url: Option<String>, curseforge_base_url: Option<String>) -> Self {
        Self {
            #[cfg(test)]
            modrinth_base_url,
            #[cfg(test)]
            curseforge_base_url,
        }
    }
}

impl NetworkProvider for LiveNetworkProvider {
    fn http_client(&self) -> Result<Client> {
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")
    }
    
    fn project_resolver(&self, client: Client, curseforge_api_key: Option<String>) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        #[cfg(feature = "test-utils")]
        {
            Box::new(ProjectResolver::new_with_base_urls(
                client,
                curseforge_api_key,
                self.modrinth_base_url.clone(),
                self.curseforge_base_url.clone(),
            ))
        }
        
        #[cfg(not(feature = "test-utils"))]
        {
            Box::new(ProjectResolver::new(client, curseforge_api_key))
        }
    }
}

/// Live implementation of ProcessProvider
pub struct LiveProcessProvider;

impl ProcessProvider for LiveProcessProvider {
    fn execute_packwiz(&self, args: &[&str]) -> Result<()> {
        execute_packwiz_command(args)
    }
    
    fn check_packwiz(&self) -> Result<(bool, String)> {
        match std::process::Command::new("packwiz").arg("--help").output() {
            Ok(output) if output.status.success() => {
                let version = self.get_packwiz_version().unwrap_or_else(|| "unknown".to_string());
                Ok((true, version))
            },
            _ => Ok((false, "not found".to_string())),
        }
    }
    
    fn get_packwiz_version(&self) -> Option<String> {
        let packwiz_path = std::process::Command::new("which")
            .arg("packwiz")
            .output()
            .ok()?
            .stdout;
        
        if packwiz_path.is_empty() {
            return None;
        }
        
        let path_str = String::from_utf8_lossy(&packwiz_path).trim().to_string();
        
        let output = std::process::Command::new("go")
            .arg("version")
            .arg("-m")
            .arg(&path_str)
            .output()
            .ok()?;
        
        if !output.status.success() {
            return None;
        }
        
        let version_output = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = version_output.lines().collect();
        if lines.len() >= 3 {
            let third_line = lines[2];
            let fields: Vec<&str> = third_line.split_whitespace().collect();
            if fields.len() >= 3 {
                return Some(fields[2].to_string());
            }
        }
        
        None
    }
}

/// Live implementation of ConfigProvider
pub struct LiveConfigProvider {
    app_config: AppConfig,
}

impl LiveConfigProvider {
    pub fn new(app_config: AppConfig) -> Self {
        Self { app_config }
    }
}

impl ConfigProvider for LiveConfigProvider {
    fn app_config(&self) -> &AppConfig {
        &self.app_config
    }
}

/// CommandSession owns all ephemeral state for a single command execution
pub struct CommandSession<F, N, P, C>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
{
    /// Owns the progress display infrastructure
    multi_progress: MultiProgress,
    /// Display provider for this session
    display_provider: LiveDisplayProvider,
    /// Filesystem operations provider
    filesystem_provider: F,
    /// Network operations provider
    network_provider: N,
    /// Process execution provider
    process_provider: P,
    /// Configuration provider
    config_provider: C,
}

impl CommandSession<LiveFileSystemProvider, LiveNetworkProvider, LiveProcessProvider, LiveConfigProvider> {
    /// Create a new command session with owned state (production composition)
    pub fn new(app_config: AppConfig) -> Self {
        let multi_progress = MultiProgress::new();
        let display_provider = LiveDisplayProvider::new_with_multi_progress(&multi_progress);
        
        Self {
            multi_progress,
            display_provider,
            filesystem_provider: LiveFileSystemProvider,
            network_provider: LiveNetworkProvider::new(),
            process_provider: LiveProcessProvider,
            config_provider: LiveConfigProvider::new(app_config),
        }
    }
}

impl<F, N, P, C> CommandSession<F, N, P, C>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
{
    /// Create a new generic command session with custom providers (for testing)
    #[cfg(feature = "test-utils")]
    pub fn new_with_providers(
        filesystem_provider: F,
        network_provider: N,
        process_provider: P,
        config_provider: C,
    ) -> Self {
        let multi_progress = MultiProgress::new();
        let display_provider = LiveDisplayProvider::new_with_multi_progress(&multi_progress);
        
        Self {
            multi_progress,
            display_provider,
            filesystem_provider,
            network_provider,
            process_provider,
            config_provider,
        }
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

impl<F, N, P, C> Session for CommandSession<F, N, P, C>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
{
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
}

// Helper function for packwiz execution - will be moved to appropriate module later
fn execute_packwiz_command(args: &[&str]) -> Result<()> {
    use std::process::Command;
    
    let output = Command::new("packwiz")
        .args(args)
        .output()
        .context("Failed to execute packwiz command")?;
    
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Packwiz command failed: {}", stderr))
    }
}