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
use crate::networking::cache::HttpCache;
use crate::networking::rate_limit::RateLimiterManager;
use crate::terminal::TerminalCapabilities;
use anyhow::Context;
use indicatif::MultiProgress;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub trait FileSystemProvider {
    fn current_dir(&self) -> Result<PathBuf>;

    fn config_manager(&self, workdir: PathBuf) -> ConfigManager<'_>;

    fn read_to_string(&self, path: &Path) -> Result<String>;

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>>;

    fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    fn write_bytes(&self, path: &Path, content: &[u8]) -> Result<()>;

    fn exists(&self, path: &Path) -> bool;

    fn metadata_exists(&self, path: &Path) -> bool;

    fn is_directory(&self, path: &Path) -> bool;

    fn create_dir_all(&self, path: &Path) -> Result<()>;

    fn get_file_list(&self, path: &Path) -> Result<HashSet<PathBuf>>;

    fn has_build_artifacts(&self, dist_dir: &Path) -> Result<bool>;

    fn remove_file(&self, path: &Path) -> Result<()>;

    fn remove_dir_all(&self, path: &Path) -> Result<()>;
}

/// Provider trait for network operations
pub trait NetworkProvider {
    fn http_client(&self) -> Result<Client>;

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

impl ProcessOutput {
    /// Returns the most informative error text on failure.
    ///
    /// Prefers stderr; falls back to stdout when stderr is empty.
    /// Some tools (notably packwiz) write error messages to stdout.
    pub fn error_output(&self) -> &str {
        let stderr = self.stderr.trim();
        if stderr.is_empty() {
            self.stdout.trim()
        } else {
            stderr
        }
    }
}

pub trait ProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput>;

    /// Returns the program path if found. Uses platform-appropriate lookup.
    fn find_program(&self, program: &str) -> Option<String>;
}

pub trait ConfigProvider {
    fn app_config(&self) -> &AppConfig;
}

/// Provider trait for archive operations (zip extraction, archive creation)
pub trait ArchiveProvider {
    fn extract_zip(&self, archive_path: &Path, dest_dir: &Path) -> Result<()>;

    fn create_archive(
        &self,
        source_dir: &Path,
        dest_path: &Path,
        format: crate::empack::archive::ArchiveFormat,
    ) -> Result<()>;
}

pub struct LiveArchiveProvider;

impl ArchiveProvider for LiveArchiveProvider {
    fn extract_zip(&self, archive_path: &Path, dest_dir: &Path) -> Result<()> {
        crate::empack::archive::extract_zip(archive_path, dest_dir)
            .with_context(|| format!("Failed to extract zip: {}", archive_path.display()))
    }

    fn create_archive(
        &self,
        source_dir: &Path,
        dest_path: &Path,
        format: crate::empack::archive::ArchiveFormat,
    ) -> Result<()> {
        crate::empack::archive::create_archive(source_dir, dest_path, format)
            .with_context(|| format!("Failed to create archive: {}", dest_path.display()))
    }
}

pub trait InteractiveProvider {
    fn text_input(&self, prompt: &str, default: String) -> Result<String>;

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;

    fn select(&self, prompt: &str, options: &[&str]) -> Result<usize>;

    /// Returns Some(index) if user selected, None if user pressed ESC.
    fn fuzzy_select(&self, prompt: &str, options: &[String]) -> Result<Option<usize>>;
}

pub trait Session {
    fn display(&self) -> &dyn DisplayProvider;

    fn filesystem(&self) -> &dyn FileSystemProvider;

    fn network(&self) -> &dyn NetworkProvider;

    fn process(&self) -> &dyn ProcessProvider;

    fn config(&self) -> &dyn ConfigProvider;

    fn interactive(&self) -> &dyn InteractiveProvider;

    fn terminal(&self) -> &TerminalCapabilities;

    fn archive(&self) -> &dyn ArchiveProvider;

    fn packwiz(&self) -> Box<dyn PackwizOps + '_>;

    fn state(&self) -> Result<PackStateManager<'_, dyn FileSystemProvider + '_>>;

    /// Resolved path to the packwiz-tx binary for process execution.
    ///
    /// Resolved once at session construction via `resolve_packwiz_binary()`.
    /// All callsites that execute packwiz-tx should use this instead of the
    /// bare `PACKWIZ_BIN` constant, which only works when the binary is in PATH.
    fn packwiz_bin(&self) -> &str;
}

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
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))
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

        let entries = std::fs::read_dir(path)
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;
        for entry in entries {
            let entry = entry
                .with_context(|| format!("Failed to read directory entry: {}", path.display()))?;
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
            let entry = entry.with_context(|| {
                format!("Failed to read directory entry: {}", dist_dir.display())
            })?;
            let path = entry.path();

            // Look for common build artifacts (files)
            if path.is_file()
                && path.extension().is_some()
                && let Some(extension) = path.extension()
            {
                match extension.to_str() {
                    Some("mrpack") | Some("zip") | Some("jar") | Some("gz") | Some("7z") => {
                        return Ok(true);
                    }
                    _ => continue,
                }
            }

            // Also consider build target directories as evidence of build state
            if path.is_dir()
                && path.file_name().and_then(|n| n.to_str()).is_some()
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

pub struct LiveNetworkProvider {
    client: Client,
    cache: Arc<HttpCache>,
    rate_limiter: Arc<RateLimiterManager>,
    #[cfg(feature = "test-utils")]
    modrinth_base_url: Option<String>,
    #[cfg(feature = "test-utils")]
    curseforge_base_url: Option<String>,
}

impl LiveNetworkProvider {
    pub fn new() -> Self {
        Self::with_timeout(30)
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        let cache_dir = std::env::temp_dir().join("empack").join("http_cache");
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            client: client.clone(),
            cache: Arc::new(HttpCache::new(cache_dir)),
            rate_limiter: Arc::new(RateLimiterManager::new(client)),
            #[cfg(feature = "test-utils")]
            modrinth_base_url: None,
            #[cfg(feature = "test-utils")]
            curseforge_base_url: None,
        }
    }

    #[cfg(feature = "test-utils")]
    pub fn new_for_test(
        modrinth_base_url: Option<String>,
        curseforge_base_url: Option<String>,
    ) -> Self {
        let cache_dir = std::env::temp_dir().join("empack-test").join("http_cache");
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            client: client.clone(),
            cache: Arc::new(HttpCache::new(cache_dir)),
            rate_limiter: Arc::new(RateLimiterManager::new(client)),
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
        Ok(self.client.clone())
    }

    fn project_resolver(
        &self,
        client: Client,
        curseforge_api_key: Option<String>,
    ) -> Box<dyn ProjectResolverTrait + Send + Sync> {
        #[cfg(feature = "test-utils")]
        {
            Box::new(ProjectResolver::new_with_base_urls_and_networking(
                client,
                curseforge_api_key,
                self.modrinth_base_url.clone(),
                self.curseforge_base_url.clone(),
                self.cache.clone(),
                self.rate_limiter.clone(),
            ))
        }

        #[cfg(not(feature = "test-utils"))]
        {
            Box::new(ProjectResolver::with_networking(
                client,
                curseforge_api_key,
                self.cache.clone(),
                self.rate_limiter.clone(),
            ))
        }
    }
}

/// Default timeout for child process execution (5 minutes).
/// Prevents indefinite hangs from packwiz or java processes.
const PROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub struct LiveProcessProvider {
    custom_path: Option<String>,
}

impl LiveProcessProvider {
    pub fn new() -> Self {
        Self { custom_path: None }
    }

    pub fn with_custom_path(path: String) -> Self {
        Self {
            custom_path: Some(path),
        }
    }

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

        // Share the Child between the wait thread and the timeout handler.
        // Uses try_wait() polling so the mutex is NOT held while blocking,
        // allowing the timeout handler to acquire the lock and call kill().
        let child_handle = Arc::new(Mutex::new(child));
        let child_for_timeout = Arc::clone(&child_handle);
        let cmd_name = command.to_string();

        // Take stdout/stderr pipes up front so we can drain them on
        // separate threads (avoids deadlock if pipe buffers fill).
        let stdout_pipe = child_handle.lock().unwrap().stdout.take();
        let stderr_pipe = child_handle.lock().unwrap().stderr.take();

        let stdout_thread = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut pipe) = stdout_pipe {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut s);
            }
            s
        });
        let stderr_thread = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut pipe) = stderr_pipe {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut s);
            }
            s
        });

        let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<ProcessOutput>>();
        std::thread::spawn(move || {
            loop {
                {
                    let mut guard = child_handle.lock().unwrap();
                    match guard.try_wait() {
                        Ok(Some(status)) => {
                            drop(guard);
                            let stdout = stdout_thread.join().unwrap_or_default();
                            let stderr = stderr_thread.join().unwrap_or_default();
                            let _ = tx.send(Ok(ProcessOutput {
                                stdout,
                                stderr,
                                success: status.success(),
                            }));
                            return;
                        }
                        Ok(None) => {
                            // Still running -- release lock and poll again
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.into()));
                            return;
                        }
                    }
                } // Lock released here so timeout handler can acquire it
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        match rx.recv_timeout(PROCESS_TIMEOUT) {
            Ok(result) => {
                result.with_context(|| format!("Failed to execute command: {}", cmd_name))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Kill via Child handle -- child is still in the mutex
                if let Ok(mut guard) = child_for_timeout.lock() {
                    let _ = guard.kill();
                    let _ = guard.wait(); // Reap zombie process
                }
                anyhow::bail!(
                    "Command '{}' timed out after {} seconds (process killed)",
                    cmd_name,
                    PROCESS_TIMEOUT.as_secs()
                )
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Kill orphan child process before returning
                if let Ok(mut guard) = child_for_timeout.lock() {
                    let _ = guard.kill();
                    let _ = guard.wait();
                }
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
        crate::logger::global_shutdown();

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

        match Select::new().with_prompt(prompt).items(options).interact() {
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

pub struct CommandSession<F, N, P, C, I>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
{
    multi_progress: Arc<MultiProgress>,
    display_provider: LiveDisplayProvider,
    terminal_capabilities: TerminalCapabilities,
    filesystem_provider: F,
    network_provider: N,
    process_provider: P,
    config_provider: C,
    interactive_provider: I,
    archive_provider: LiveArchiveProvider,
    /// Resolved packwiz-tx binary path for process execution.
    /// Computed once at session construction via `resolve_packwiz_binary()`.
    packwiz_bin_path: String,
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
    pub fn new(app_config: AppConfig) -> Self {
        // Initialize display and logger systems
        let terminal_capabilities = match TerminalCapabilities::detect_from_config(app_config.color)
        {
            Ok(caps) => {
                crate::display::Display::init_or_get(caps.clone());
                let logger_config = app_config.to_logger_config(&caps);
                if let Err(e) = crate::logger::Logger::init(logger_config) {
                    eprintln!("empack: logger init failed: {e}");
                }
                caps
            }
            Err(_) => TerminalCapabilities::minimal(),
        };

        let multi_progress = Arc::new(MultiProgress::new());
        let display_provider = LiveDisplayProvider::new_with_arc(multi_progress.clone());

        // Resolve packwiz-tx binary once at session construction.
        // This is a blocking call that may download the binary on first use.
        let packwiz_bin_path = crate::platform::packwiz_bin::resolve_packwiz_binary()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| crate::empack::packwiz::PACKWIZ_BIN.to_string());

        Self {
            multi_progress,
            display_provider,
            terminal_capabilities,
            filesystem_provider: LiveFileSystemProvider,
            network_provider: LiveNetworkProvider::with_timeout(app_config.net_timeout),
            process_provider: LiveProcessProvider::new(),
            config_provider: LiveConfigProvider::new(app_config.clone()),
            interactive_provider: LiveInteractiveProvider::new(
                app_config.yes,
                app_config.workdir.clone(),
            ),
            archive_provider: LiveArchiveProvider,
            packwiz_bin_path,
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
            terminal_capabilities: TerminalCapabilities::minimal(),
            filesystem_provider,
            network_provider,
            process_provider,
            config_provider,
            interactive_provider,
            archive_provider: LiveArchiveProvider,
            packwiz_bin_path: crate::empack::packwiz::PACKWIZ_BIN.to_string(),
        }
    }

    pub fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }

    pub fn filesystem(&self) -> &dyn FileSystemProvider {
        &self.filesystem_provider
    }

    pub fn network(&self) -> &dyn NetworkProvider {
        &self.network_provider
    }

    pub fn process(&self) -> &dyn ProcessProvider {
        &self.process_provider
    }

    pub fn config(&self) -> &dyn ConfigProvider {
        &self.config_provider
    }

    pub fn interactive(&self) -> &dyn InteractiveProvider {
        &self.interactive_provider
    }

    pub fn terminal(&self) -> &TerminalCapabilities {
        &self.terminal_capabilities
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

    fn terminal(&self) -> &TerminalCapabilities {
        &self.terminal_capabilities
    }

    fn archive(&self) -> &dyn ArchiveProvider {
        &self.archive_provider
    }

    fn packwiz_bin(&self) -> &str {
        &self.packwiz_bin_path
    }

    fn packwiz(&self) -> Box<dyn PackwizOps + '_> {
        Box::new(LivePackwizOps::new(
            self.process(),
            self.filesystem(),
            &self.packwiz_bin_path,
        ))
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
