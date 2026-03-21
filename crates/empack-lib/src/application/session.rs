//! Command session architecture
//!
//! Implements the Session-Scoped Dependency Injection Pattern.
//! Each command execution creates a session that owns all ephemeral state.

use crate::Result;
use crate::application::config::AppConfig;
use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::empack::config::ConfigManager;
use crate::empack::packwiz::{LivePackwizOps, PackwizOps};
use crate::empack::search::{ProjectResolver, ProjectResolverTrait};
use crate::empack::state::PackStateManager;
use anyhow::Context;
use indicatif::MultiProgress;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Abstract interface for state management operations.
// StateManager trait removed - using concrete PackStateManager type instead.

/// Provider trait for filesystem operations
pub trait FileSystemProvider {
    /// Get current working directory
    fn current_dir(&self) -> Result<PathBuf>;

    /// Create a config manager for the given directory
    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_>;

    // Core file I/O operations for dependency injection
    /// Read entire file contents as string
    fn read_to_string(&self, path: &Path) -> Result<String>;

    /// Read entire file contents as bytes
    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write string content to file
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Write binary content to file
    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// Check if path exists
    fn exists(&self, path: &Path) -> bool;

    /// Check whether metadata can be read for a path
    fn metadata_exists(&self, path: &Path) -> bool;

    /// Check if path is a directory
    fn is_directory(&self, path: &Path) -> bool;

    /// Create directory and all parent directories
    fn create_dir_all(&self, path: &Path) -> Result<()>;

    // Additional methods for state management
    /// Get list of files and directories in a path
    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>>;

    /// Check if directory has build artifacts (mrpack, zip, jar files or build target dirs)
    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool>;

    /// Remove a file
    fn remove_file(&self, path: &Path) -> Result<()>;

    /// Remove a directory and all its contents
    fn remove_dir_all(&self, path: &Path) -> Result<()>;
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

    /// Check if a program is available in PATH. Returns the program path if found.
    /// Cross-platform: uses platform-appropriate lookup (which on Unix, where on Windows).
    fn find_program(&self, program: &str) -> Option<String>;
}

/// Provider trait for configuration access
pub trait ConfigProvider {
    /// Get the application configuration
    fn app_config(&self) -> &AppConfig;
}

/// Provider trait for interactive user input operations
pub trait InteractiveProvider {
    /// Prompt for text input with optional default value
    fn text_input(&self, prompt: &str, default: String) -> Result<String>;

    /// Prompt for confirmation (yes/no)
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;

    /// Prompt for selection from a list of options
    fn select(&self, prompt: &str, options: &[&str]) -> Result<usize>;

    /// Prompt for fuzzy selection from a list of options
    /// Returns Some(index) if user selected, None if user pressed ESC
    fn fuzzy_select(&self, prompt: &str, options: &[String]) -> Result<Option<usize>>;
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

    /// Get the interactive provider for this session
    fn interactive(&self) -> &dyn InteractiveProvider;

    /// Get the packwiz operations provider for this session
    fn packwiz(&self) -> Box<dyn PackwizOps + '_>;

    /// Get the state manager for this session
    fn state(&self) -> Result<PackStateManager<'_, dyn FileSystemProvider + '_>>;
}

/// Live implementation of FileSystemProvider
pub struct LiveFileSystemProvider;

impl FileSystemProvider for LiveFileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        env::current_dir().context("Failed to get current directory")
    }

    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_> {
        ConfigManager::new(workdir, self)
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))
    }

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write file: {}", path.display()))
    }

    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write file: {}", path.display()))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn metadata_exists(&self, path: &Path) -> bool {
        std::fs::metadata(path).is_ok()
    }

    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))
    }

    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>> {
        let mut files = HashSet::new();

        if !path.exists() {
            return Ok(files);
        }

        let entries =
            std::fs::read_dir(path).with_context(|| format!("Failed to read directory: {}", path.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("Failed to read directory entry: {}", path.display()))?;
            files.insert(entry.path());
        }

        Ok(files)
    }

    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool> {
        if !dist_dir.exists() {
            return Ok(false);
        }

        let entries = std::fs::read_dir(dist_dir)
            .with_context(|| format!("Failed to read directory: {}", dist_dir.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("Failed to read directory entry: {}", dist_dir.display()))?;
            let path = entry.path();

            // Look for common build artifacts (files)
            if path.is_file() && path.extension().is_some()
                && let Some(extension) = path.extension()
            {
                match extension.to_str() {
                    Some("mrpack") | Some("zip") | Some("jar") => return Ok(true),
                    _ => continue,
                }
            }

            // Also consider build target directories as evidence of build state
            if path.is_dir() && path.file_name().and_then(|n| n.to_str()).is_some()
                && let Some(dir_name) = path.file_name().and_then(|n| n.to_str())
            {
                match dir_name {
                    "mrpack" | "client" | "server" | "client-full" | "server-full" => {
                        return Ok(true);
                    }
                    _ => continue,
                }
            }
        }

        Ok(false)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove file: {}", path.display()))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove directory: {}", path.display()))
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
}

impl Default for LiveNetworkProvider {
    fn default() -> Self {
        Self::new()
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

/// Default timeout for child process execution (5 minutes).
/// Prevents indefinite hangs from packwiz or java processes.
const PROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

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
                // Use platform-specific PATH separator
                #[cfg(windows)]
                let path_sep = ";";
                #[cfg(not(windows))]
                let path_sep = ":";
                let custom_path = format!("{}{}{}", bin_path, path_sep, current_path);
                Self::with_custom_path(custom_path)
            }
            None => Self::new(),
        }
    }
}

impl Default for LiveProcessProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessProvider for LiveProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput> {
        use std::process::Command;

        let mut cmd = Command::new(command);
        cmd.args(args).current_dir(working_dir);

        if let Some(custom_path) = &self.custom_path {
            cmd.env("PATH", custom_path);
        }

        let child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn command: {}", command))?;

        let child_id = child.id();
        let cmd_name = command.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });

        match rx.recv_timeout(PROCESS_TIMEOUT) {
            Ok(result) => {
                let output = result
                    .with_context(|| format!("Failed to execute command: {}", cmd_name))?;
                Ok(ProcessOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    success: output.status.success(),
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Best-effort kill of the hung child process
                #[cfg(unix)]
                {
                    // SAFETY: libc::kill sends a signal to a process by PID.
                    // child_id is a valid PID from a process we just spawned.
                    unsafe { libc::kill(child_id as libc::pid_t, libc::SIGKILL); }
                }
                #[cfg(windows)]
                {
                    // On Windows, we cannot easily kill by PID without opening the process.
                    // The background thread will clean up when the process exits or parent dies.
                    let _ = child_id;
                }
                #[cfg(unix)]
                let kill_status = "process killed";
                #[cfg(windows)]
                let kill_status = "process may still be running";
                anyhow::bail!(
                    "Command '{}' timed out after {} seconds ({})",
                    cmd_name,
                    PROCESS_TIMEOUT.as_secs(),
                    kill_status
                )
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!(
                    "Command '{}' execution thread terminated unexpectedly",
                    cmd_name
                )
            }
        }
    }

    fn find_program(&self, program: &str) -> Option<String> {
        #[cfg(windows)]
        let locate_cmd = "where";
        #[cfg(not(windows))]
        let locate_cmd = "which";

        let cwd = std::env::current_dir().ok()?;
        let output = self.execute(locate_cmd, &[program], &cwd).ok()?;
        if output.success {
            let path = output.stdout.trim().lines().next()?.to_string();
            if path.is_empty() { None } else { Some(path) }
        } else {
            None
        }
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

/// Live implementation of InteractiveProvider
pub struct LiveInteractiveProvider {
    yes_mode: bool,
    workdir: Option<PathBuf>,
}

impl LiveInteractiveProvider {
    pub fn new(yes_mode: bool, workdir: Option<PathBuf>) -> Self {
        Self { yes_mode, workdir }
    }

    /// Check if we're in a TTY environment suitable for interactive prompts
    fn is_tty() -> bool {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    }

    fn handle_interrupt(&self) -> ! {
        crate::terminal::cursor::force_show_cursor();

        // Best-effort state marker cleanup
        let marker_dir = self
            .workdir
            .as_ref()
            .cloned()
            .or_else(|| std::env::current_dir().ok());
        if let Some(dir) = &marker_dir {
            let marker = dir.join(crate::empack::state::STATE_MARKER_FILE);
            let _ = std::fs::remove_file(marker);
        }

        std::process::exit(130)
    }
}

impl InteractiveProvider for LiveInteractiveProvider {
    fn text_input(&self, prompt: &str, default: String) -> Result<String> {
        // Check yes_mode first (--yes flag), then TTY
        if self.yes_mode || !Self::is_tty() {
            // Non-interactive mode: return default
            return Ok(default);
        }

        use dialoguer::Input;

        match Input::new()
            .with_prompt(prompt)
            .default(default.clone())
            .interact_text()
        {
            Ok(val) => Ok(val),
            Err(dialoguer::Error::IO(ref io_err))
                if io_err.kind() == std::io::ErrorKind::Interrupted =>
            {
                self.handle_interrupt()
            }
            Err(e) => Err(e).context("Failed to read text input"),
        }
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        // Check yes_mode first (--yes flag), then TTY
        if self.yes_mode || !Self::is_tty() {
            // Non-interactive mode: return default
            return Ok(default);
        }

        use dialoguer::Confirm;

        match Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()
        {
            Ok(val) => Ok(val),
            Err(dialoguer::Error::IO(ref io_err))
                if io_err.kind() == std::io::ErrorKind::Interrupted =>
            {
                self.handle_interrupt()
            }
            Err(e) => Err(e).context("Failed to read confirmation"),
        }
    }

    fn select(&self, prompt: &str, options: &[&str]) -> Result<usize> {
        // Check yes_mode first (--yes flag), then TTY
        if self.yes_mode || !Self::is_tty() {
            // Non-interactive mode: return first option (index 0)
            return Ok(0);
        }

        use dialoguer::Select;

        match Select::new()
            .with_prompt(prompt)
            .items(options)
            .interact()
        {
            Ok(val) => Ok(val),
            Err(dialoguer::Error::IO(ref io_err))
                if io_err.kind() == std::io::ErrorKind::Interrupted =>
            {
                self.handle_interrupt()
            }
            Err(e) => Err(e).context("Failed to read selection"),
        }
    }

    fn fuzzy_select(&self, prompt: &str, options: &[String]) -> Result<Option<usize>> {
        // Check yes_mode first (--yes flag), then TTY
        if self.yes_mode || !Self::is_tty() {
            // Non-interactive mode: return first option (index 0)
            return Ok(Some(0));
        }

        use dialoguer::FuzzySelect;

        match FuzzySelect::new()
            .with_prompt(prompt)
            .items(options)
            .max_length(6) // Show 6 items per page (enables pagination)
            .interact_opt()
        {
            Ok(val) => Ok(val),
            Err(dialoguer::Error::IO(ref io_err))
                if io_err.kind() == std::io::ErrorKind::Interrupted =>
            {
                self.handle_interrupt()
            }
            Err(e) => Err(e).context("Failed to read fuzzy selection"),
        }
    }
}

/// CommandSession owns all ephemeral state for a single command execution
pub struct CommandSession<F, N, P, C, I>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
{
    /// Shared progress display infrastructure (also held by display_provider)
    multi_progress: Arc<MultiProgress>,
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
    /// Interactive input provider
    interactive_provider: I,
}

impl
    CommandSession<
        LiveFileSystemProvider,
        LiveNetworkProvider,
        LiveProcessProvider,
        LiveConfigProvider,
        LiveInteractiveProvider,
    >
{
    /// Create a new command session with owned state (production composition)
    pub fn new(app_config: AppConfig) -> Self {
        // Initialize display and logger systems
        if let Ok(terminal_caps) =
            crate::terminal::TerminalCapabilities::detect_from_config(&app_config)
        {
            crate::display::Display::init_or_get(terminal_caps.clone());
            let logger_config = app_config.to_logger_config(&terminal_caps);
            if let Err(e) = crate::logger::Logger::init(logger_config) {
                eprintln!("empack: logger init failed: {e}");
            }
        }

        let multi_progress = Arc::new(MultiProgress::new());
        let display_provider = LiveDisplayProvider::new_with_arc(multi_progress.clone());

        Self {
            multi_progress,
            display_provider,
            filesystem_provider: LiveFileSystemProvider,
            network_provider: LiveNetworkProvider::new(),
            process_provider: LiveProcessProvider::new(),
            config_provider: LiveConfigProvider::new(app_config.clone()),
            interactive_provider: LiveInteractiveProvider::new(app_config.yes, app_config.workdir.clone()),
        }
    }
}

impl<F, N, P, C, I> CommandSession<F, N, P, C, I>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
{
    /// Create a new generic command session with custom providers (for testing)
    #[cfg(feature = "test-utils")]
    pub fn new_with_providers(
        filesystem_provider: F,
        network_provider: N,
        process_provider: P,
        config_provider: C,
        interactive_provider: I,
    ) -> Self {
        let multi_progress = Arc::new(MultiProgress::new());
        let display_provider = LiveDisplayProvider::new_with_arc(multi_progress.clone());

        Self {
            multi_progress,
            display_provider,
            filesystem_provider,
            network_provider,
            process_provider,
            config_provider,
            interactive_provider,
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

    /// Get the interactive provider for this session
    pub fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }
}

impl<F, N, P, C, I> Session for CommandSession<F, N, P, C, I>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
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

    fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }

    fn packwiz(&self) -> Box<dyn PackwizOps + '_> {
        Box::new(LivePackwizOps::new(self.process(), self.filesystem()))
    }

    fn state(&self) -> Result<PackStateManager<'_, dyn FileSystemProvider + '_>> {
        let workdir = match self.config().app_config().workdir.as_ref().cloned() {
            Some(dir) => dir,
            None => self.filesystem().current_dir()?,
        };
        Ok(PackStateManager::new(workdir, self.filesystem()))
    }
}

impl<F, N, P, C, I> Drop for CommandSession<F, N, P, C, I>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
{
    fn drop(&mut self) {
        // Ordered teardown:
        // 1. Clear any lingering progress bars from the shared MultiProgress
        let _ = self.multi_progress.clear();
        // 2. Restore cursor visibility (defense-in-depth, complements panic hook + signal handler)
        crate::terminal::cursor::force_show_cursor();
    }
}
