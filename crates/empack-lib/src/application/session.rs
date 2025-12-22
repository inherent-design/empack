//! Command session architecture
//!
//! Implements the Session-Scoped Dependency Injection Pattern.
//! Each command execution creates a session that owns all ephemeral state.

use crate::application::config::AppConfig;
use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::empack::config::ConfigManager;
use crate::empack::search::{ProjectResolver, ProjectResolverTrait};
use crate::empack::state::ModpackStateManager;
use anyhow::{Context, Result};
use indicatif::MultiProgress;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

/// Abstract interface for state management operations
// StateManager trait removed - using concrete ModpackStateManager type instead

/// Provider trait for filesystem operations
pub trait FileSystemProvider {
    /// Get current working directory
    fn current_dir(&self) -> Result<PathBuf>;

    // state_manager method removed - create ModpackStateManager directly

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
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), crate::empack::state::StateError>;

    /// Run packwiz refresh command
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), crate::empack::state::StateError>;

    /// Get the expected cache path for packwiz-installer-bootstrap.jar
    fn get_bootstrap_jar_cache_path(&self) -> Result<PathBuf>;
}

/// Provider trait for network operations
pub trait NetworkProvider {
    /// Create an HTTP client with appropriate timeout
    fn http_client(&self) -> Result<Client>;

    /// Create a project resolver with HTTP client and API keys
    fn project_resolver(
        &self,
        client: Client,
        curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync>;
}

/// Process execution output
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Provider trait for process execution
pub trait ProcessProvider {
    /// Execute a command with given arguments in working directory
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput>;

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

    /// Get the state manager for this session
    fn state(&self) -> ModpackStateManager<'_, dyn FileSystemProvider + '_>;
}

/// Live implementation of FileSystemProvider
pub struct LiveFileSystemProvider;

impl FileSystemProvider for LiveFileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        env::current_dir().context("Failed to get current directory")
    }

    // state_manager method removed - create ModpackStateManager directly

    fn get_installed_mods(&self) -> Result<HashSet<String>> {
        let pack_dir = self.current_dir()?.join("pack");
        let output = std::process::Command::new("packwiz")
            .arg("list")
            .current_dir(&pack_dir)
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
            let normalized_name = mod_name.to_lowercase().replace(' ', "_").replace('-', "_");

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

    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), crate::empack::state::StateError> {
        use std::process::Command;

        #[cfg(test)]
        {
            // Mock packwiz init - create expected files
            let pack_dir = workdir.join("pack");
            self.create_dir_all(&pack_dir).map_err(|e| {
                crate::empack::state::StateError::IoError {
                    message: e.to_string(),
                }
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
            self.write_file(&pack_file, default_pack_toml)
                .map_err(|e| crate::empack::state::StateError::IoError {
                    message: e.to_string(),
                })?;

            // Also create index.toml
            let index_file = pack_dir.join("index.toml");
            let default_index = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;
            self.write_file(&index_file, default_index).map_err(|e| {
                crate::empack::state::StateError::IoError {
                    message: e.to_string(),
                }
            })?;
            return Ok(());
        }

        #[cfg(not(test))]
        {
            let pack_dir = workdir.join("pack");

            // Ensure pack directory exists before running packwiz
            if !pack_dir.exists() {
                return Err(crate::empack::state::StateError::MissingFile {
                    file: "pack directory".to_string(),
                });
            }

            // Build packwiz init command with all required parameters
            let mut args = vec![
                "init",
                "--name",
                name,
                "--author",
                author,
                "--version",
                version,
                "--mc-version",
                mc_version,
                "--modloader",
                modloader,
                "-y", // Non-interactive mode
            ];

            // Add modloader-specific version arguments
            match modloader {
                "neoforge" => {
                    args.push("--neoforge-version");
                    args.push(loader_version);
                }
                "fabric" => {
                    args.push("--fabric-version");
                    args.push(loader_version);
                }
                "quilt" => {
                    args.push("--quilt-version");
                    args.push(loader_version);
                }
                "forge" => {
                    args.push("--forge-version");
                    args.push(loader_version);
                }
                _ => {
                    // For vanilla or unknown modloaders, don't add version args
                }
            }

            let status = Command::new("packwiz")
                .args(&args)
                .current_dir(&pack_dir)
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

    fn get_bootstrap_jar_cache_path(&self) -> Result<PathBuf> {
        // First check for local installer JAR (for development/testing)
        let local_jar = std::env::current_dir()
            .context("Failed to get current directory")?
            .join("installer")
            .join("packwiz-installer-bootstrap.jar");

        if local_jar.exists() {
            return Ok(local_jar);
        }

        // Return cache directory path
        let cache_dir = dirs::cache_dir()
            .context("Failed to determine cache directory")?
            .join("empack")
            .join("jars");

        Ok(cache_dir.join("packwiz-installer-bootstrap.jar"))
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
    pub fn new_for_test(
        modrinth_base_url: Option<String>,
        curseforge_base_url: Option<String>,
    ) -> Self {
        Self {
            modrinth_base_url,
            curseforge_base_url,
        }
    }

    /// Integration test constructor with custom base URLs (for external test crates)
    #[cfg(feature = "integration-tests")]
    pub fn new_with_base_urls(
        modrinth_base_url: Option<String>,
        curseforge_base_url: Option<String>,
    ) -> Self {
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

    fn project_resolver(
        &self,
        client: Client,
        curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync> {
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
pub struct LiveProcessProvider {
    /// Custom PATH override for hermetic testing
    custom_path: Option<String>,
}

impl LiveProcessProvider {
    /// Create a new LiveProcessProvider with system PATH
    pub fn new() -> Self {
        Self { custom_path: None }
    }

    /// Create a LiveProcessProvider with custom PATH for hermetic testing
    pub fn with_custom_path(path: String) -> Self {
        Self {
            custom_path: Some(path),
        }
    }

    /// Create a LiveProcessProvider configured for testing with test environment
    pub fn new_for_test(test_bin_path: Option<String>) -> Self {
        match test_bin_path {
            Some(bin_path) => {
                let current_path = std::env::var("PATH").unwrap_or_default();
                let custom_path = format!("{}:{}", bin_path, current_path);
                Self::with_custom_path(custom_path)
            }
            None => Self::new(),
        }
    }

    /// Get the PATH environment variable to use for this provider
    fn get_path_env(&self) -> String {
        match &self.custom_path {
            Some(path) => path.clone(),
            None => std::env::var("PATH").unwrap_or_default(),
        }
    }
}

impl ProcessProvider for LiveProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput> {
        use std::process::Command;

        let mut cmd = Command::new(command);
        cmd.args(args).current_dir(working_dir);

        // Set custom PATH if specified
        if let Some(custom_path) = &self.custom_path {
            cmd.env("PATH", custom_path);
        }

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute command: {}", command))?;

        Ok(ProcessOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
        })
    }

    fn check_packwiz(&self) -> Result<(bool, String)> {
        // Check if packwiz is available in PATH
        let mut cmd = std::process::Command::new("which");
        cmd.arg("packwiz");

        // Set custom PATH if specified
        if let Some(custom_path) = &self.custom_path {
            cmd.env("PATH", custom_path);
        }

        match cmd.output() {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                let version = self
                    .get_packwiz_version()
                    .unwrap_or_else(|| "unknown".to_string());
                Ok((true, version))
            }
            _ => Ok((false, "not found".to_string())),
        }
    }

    fn get_packwiz_version(&self) -> Option<String> {
        // First, find the absolute path to packwiz
        let mut cmd = std::process::Command::new("which");
        cmd.arg("packwiz");

        // Set custom PATH if specified
        if let Some(custom_path) = &self.custom_path {
            cmd.env("PATH", custom_path);
        }

        let packwiz_path_output = cmd.output().ok()?;

        if !packwiz_path_output.status.success() || packwiz_path_output.stdout.is_empty() {
            return None;
        }

        let path_str = String::from_utf8_lossy(&packwiz_path_output.stdout)
            .trim()
            .to_string();

        // Use go version -m to inspect the binary's module information
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

        // Parse the output to find the version
        // Looking for the line that starts with "mod" and extract the third field
        for line in version_output.lines() {
            if line.starts_with("mod") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 3 {
                    return Some(fields[2].to_string());
                }
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

impl
    CommandSession<
        LiveFileSystemProvider,
        LiveNetworkProvider,
        LiveProcessProvider,
        LiveConfigProvider,
    >
{
    /// Create a new command session with owned state (production composition)
    pub fn new(app_config: AppConfig) -> Self {
        // Initialize display system if not already done
        if let Ok(terminal_caps) =
            crate::terminal::TerminalCapabilities::detect_from_config(&app_config)
        {
            let _ = crate::display::Display::init(terminal_caps);
        }

        let multi_progress = MultiProgress::new();
        let display_provider = LiveDisplayProvider::new_with_multi_progress(&multi_progress);

        Self {
            multi_progress,
            display_provider,
            filesystem_provider: LiveFileSystemProvider,
            network_provider: LiveNetworkProvider::new(),
            process_provider: LiveProcessProvider::new(),
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

    fn state(&self) -> ModpackStateManager<'_, dyn FileSystemProvider + '_> {
        let workdir = self
            .config()
            .app_config()
            .workdir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| {
                self.filesystem()
                    .current_dir()
                    .expect("Failed to get current directory")
            });
        ModpackStateManager::new(workdir, self.filesystem())
    }
}
