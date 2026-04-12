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
use crate::networking::rate_budget::HostBudgetRegistry;
use crate::networking::rate_limit::RateLimiterManager;
use crate::terminal::TerminalCapabilities;
use anyhow::Context;
use indicatif::MultiProgress;
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    /// Per-host adaptive rate budget registry.
    fn rate_budgets(&self) -> &HostBudgetRegistry;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStream {
    Stdout,
    Stderr,
}

pub trait ProcessObserver {
    fn on_line(&self, stream: ProcessStream, line: &str);
}

pub trait ProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput>;

    fn execute_streaming(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &Path,
        observer: &dyn ProcessObserver,
    ) -> Result<ProcessOutput> {
        let _ = observer;
        self.execute(command, args, working_dir)
    }

    /// Returns the program path if found. Uses platform-appropriate lookup.
    fn find_program(&self, program: &str) -> Option<String>;
}

pub struct IssueStreamObserver<'a> {
    display: &'a dyn DisplayProvider,
    label: &'a str,
}

impl<'a> IssueStreamObserver<'a> {
    pub fn new(display: &'a dyn DisplayProvider, label: &'a str) -> Self {
        Self { display, label }
    }

    fn should_echo_stdout(line: &str) -> bool {
        let lower = line.trim_start().to_ascii_lowercase();
        lower.starts_with("error:")
            || lower.starts_with("caused by:")
            || lower.starts_with("warning:")
            || lower.starts_with("failed to ")
    }

    fn should_echo_stderr(line: &str) -> bool {
        let trimmed = line.trim_start();
        !Self::looks_like_java_stack_frame(trimmed) && !trimmed.starts_with("... ")
    }

    fn looks_like_java_stack_frame(line: &str) -> bool {
        let Some(frame) = line.strip_prefix("at ") else {
            return false;
        };

        (frame.contains('(')
            && frame.ends_with(')')
            && (frame.contains(".java:")
                || frame.ends_with("(Native Method)")
                || frame.ends_with("(Unknown Source)")))
            || {
                let mut parts = frame.split_whitespace();
                let symbol = parts.next().unwrap_or_default();
                parts.next().is_none()
                    && symbol.contains('.')
                    && symbol.chars().any(|c| c.is_ascii_uppercase())
            }
    }
}

impl ProcessObserver for IssueStreamObserver<'_> {
    fn on_line(&self, stream: ProcessStream, line: &str) {
        let clean = line.trim();
        if clean.is_empty() {
            return;
        }

        match stream {
            ProcessStream::Stderr if Self::should_echo_stderr(clean) => self
                .display
                .status()
                .warning(&format!("{}: {}", self.label, clean)),
            ProcessStream::Stderr => {}
            ProcessStream::Stdout if Self::should_echo_stdout(clean) => self
                .display
                .status()
                .message(&format!("{}: {}", self.label, clean)),
            ProcessStream::Stdout => {}
        }
    }
}

pub fn execute_process_with_live_issues(
    session: &dyn Session,
    command: &str,
    args: &[&str],
    working_dir: &Path,
) -> Result<ProcessOutput> {
    let label = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .to_string();
    let observer = IssueStreamObserver::new(session.display(), &label);
    session
        .process()
        .execute_streaming(command, args, working_dir, &observer)
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
    rate_budgets: Arc<HostBudgetRegistry>,
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
        let cache_dir = crate::platform::cache::http_cache_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("empack").join("http_cache"));
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build HTTP client");
        let rate_budgets = Arc::new(HostBudgetRegistry::new());
        Self {
            client: client.clone(),
            cache: Arc::new(HttpCache::new(cache_dir)),
            rate_limiter: Arc::new(RateLimiterManager::new_with_budgets(client, &rate_budgets)),
            rate_budgets,
            #[cfg(feature = "test-utils")]
            modrinth_base_url: None,
            #[cfg(feature = "test-utils")]
            curseforge_base_url: None,
        }
    }

    pub async fn new_async(timeout_secs: u64) -> Self {
        let provider = Self::with_timeout(timeout_secs);
        if let Err(error) = provider.cache.load_from_disk().await {
            tracing::warn!(
                error = %error,
                cache_dir = %provider.cache.cache_dir().display(),
                "failed to load persisted HTTP cache; continuing with empty cache"
            );
        }
        provider
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
        let rate_budgets = Arc::new(HostBudgetRegistry::new());
        Self {
            client: client.clone(),
            cache: Arc::new(HttpCache::new(cache_dir)),
            rate_limiter: Arc::new(RateLimiterManager::new_with_budgets(client, &rate_budgets)),
            rate_budgets,
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

    fn rate_budgets(&self) -> &HostBudgetRegistry {
        &self.rate_budgets
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
const DEFAULT_PROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

fn process_timeout() -> std::time::Duration {
    if let Ok(value) = std::env::var("EMPACK_PROCESS_TIMEOUT_SECS")
        && let Ok(secs) = value.parse::<u64>()
    {
        return std::time::Duration::from_secs(secs);
    }

    DEFAULT_PROCESS_TIMEOUT
}

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

    fn effective_path(&self) -> Option<std::ffi::OsString> {
        self.custom_path
            .as_ref()
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("PATH"))
    }

    #[cfg(windows)]
    fn effective_pathext(&self) -> std::ffi::OsString {
        std::env::var_os("PATHEXT")
            .unwrap_or_else(|| std::ffi::OsString::from(".COM;.EXE;.BAT;.CMD"))
    }

    #[cfg(windows)]
    fn resolve_command_path(&self, command: &str) -> Option<PathBuf> {
        let command_path = Path::new(command);
        let command_has_parent = command_path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
        let has_extension = command_path.extension().is_some();

        let candidate_paths = if command_path.is_absolute() || command_has_parent {
            vec![command_path.to_path_buf()]
        } else if let Some(path) = self.effective_path() {
            std::env::split_paths(&path)
                .map(|dir| dir.join(command))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let pathexts = self
            .effective_pathext()
            .to_string_lossy()
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| {
                if ext.starts_with('.') {
                    ext.to_string()
                } else {
                    format!(".{ext}")
                }
            })
            .collect::<Vec<_>>();

        for candidate in candidate_paths {
            if has_extension {
                if candidate.is_file() {
                    return Some(candidate);
                }
                continue;
            }

            for ext in &pathexts {
                let with_ext = candidate.with_extension(ext.trim_start_matches('.'));
                if with_ext.is_file() {
                    return Some(with_ext);
                }
            }
        }

        None
    }
}

impl Default for LiveProcessProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessProvider for LiveProcessProvider {
    fn execute(&self, command: &str, args: &[&str], working_dir: &Path) -> Result<ProcessOutput> {
        struct NoopProcessObserver;

        impl ProcessObserver for NoopProcessObserver {
            fn on_line(&self, _stream: ProcessStream, _line: &str) {}
        }

        self.execute_streaming(command, args, working_dir, &NoopProcessObserver)
    }

    fn execute_streaming(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &Path,
        observer: &dyn ProcessObserver,
    ) -> Result<ProcessOutput> {
        use std::process::Command;

        #[cfg(windows)]
        let resolved_command = self
            .resolve_command_path(command)
            .unwrap_or_else(|| PathBuf::from(command));
        #[cfg(not(windows))]
        let resolved_command = PathBuf::from(command);

        let mut cmd = Command::new(&resolved_command);
        cmd.args(args).current_dir(working_dir);

        if let Some(path) = self.effective_path() {
            cmd.env("PATH", path);
        }

        #[cfg(windows)]
        {
            cmd.env("PATHEXT", self.effective_pathext());
        }

        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn command: {}", command))?;
        let cmd_name = command.to_string();

        enum ProcessEvent {
            Chunk(ProcessStream, String),
            ReaderFailed(ProcessStream, String),
        }

        fn spawn_reader(
            pipe: impl std::io::Read + Send + 'static,
            stream: ProcessStream,
            tx: std::sync::mpsc::Sender<ProcessEvent>,
        ) -> std::thread::JoinHandle<()> {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(pipe);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) => break,
                        Ok(_) => {
                            let chunk = String::from_utf8_lossy(&buf).into_owned();
                            if tx.send(ProcessEvent::Chunk(stream, chunk)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(ProcessEvent::ReaderFailed(stream, error.to_string()));
                            break;
                        }
                    }
                }
            })
        }

        let stdout_pipe = child
            .stdout
            .take()
            .context("Failed to capture command stdout")?;
        let stderr_pipe = child
            .stderr
            .take()
            .context("Failed to capture command stderr")?;

        let (tx, rx) = std::sync::mpsc::channel::<ProcessEvent>();
        let stdout_thread = spawn_reader(stdout_pipe, ProcessStream::Stdout, tx.clone());
        let stderr_thread = spawn_reader(stderr_pipe, ProcessStream::Stderr, tx);

        let start = std::time::Instant::now();
        let process_timeout = process_timeout();
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut stdout_partial = String::new();
        let mut stderr_partial = String::new();
        let mut exit_status = None;

        fn handle_chunk(
            full_output: &mut String,
            partial: &mut String,
            chunk: &str,
            stream: ProcessStream,
            observer: &dyn ProcessObserver,
        ) {
            full_output.push_str(chunk);
            partial.push_str(chunk);

            while let Some(pos) = partial.find('\n') {
                let line = partial[..pos].trim_end_matches('\r').to_string();
                observer.on_line(stream, &line);
                partial.drain(..=pos);
            }
        }

        loop {
            if start.elapsed() > process_timeout {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                anyhow::bail!(
                    "Command '{}' timed out after {} seconds (process killed)",
                    cmd_name,
                    process_timeout.as_secs()
                )
            }

            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(ProcessEvent::Chunk(ProcessStream::Stdout, chunk)) => {
                    handle_chunk(
                        &mut stdout,
                        &mut stdout_partial,
                        &chunk,
                        ProcessStream::Stdout,
                        observer,
                    );
                }
                Ok(ProcessEvent::Chunk(ProcessStream::Stderr, chunk)) => {
                    handle_chunk(
                        &mut stderr,
                        &mut stderr_partial,
                        &chunk,
                        ProcessStream::Stderr,
                        observer,
                    );
                }
                Ok(ProcessEvent::ReaderFailed(stream, error)) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    anyhow::bail!("Failed to read {:?} from '{}': {}", stream, cmd_name, error);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if exit_status.is_none() {
                        exit_status =
                            Some(child.wait().with_context(|| {
                                format!("Failed to execute command: {}", cmd_name)
                            })?);
                    }
                    break;
                }
            }

            if exit_status.is_none()
                && let Some(status) = child
                    .try_wait()
                    .with_context(|| format!("Failed to execute command: {}", cmd_name))?
            {
                exit_status = Some(status);
            }
        }

        let _ = stdout_thread.join();
        let _ = stderr_thread.join();

        if !stdout_partial.is_empty() {
            observer.on_line(ProcessStream::Stdout, stdout_partial.trim_end_matches('\r'));
        }
        if !stderr_partial.is_empty() {
            observer.on_line(ProcessStream::Stderr, stderr_partial.trim_end_matches('\r'));
        }

        let status = match exit_status {
            Some(status) => status,
            None => child
                .wait()
                .with_context(|| format!("Failed to execute command: {}", cmd_name))?,
        };

        Ok(ProcessOutput {
            stdout,
            stderr,
            success: status.success(),
        })
    }

    fn find_program(&self, program: &str) -> Option<String> {
        #[cfg(windows)]
        {
            return self
                .resolve_command_path(program)
                .map(|path| path.to_string_lossy().into_owned());
        }

        #[cfg(not(windows))]
        {
            let cwd = std::env::current_dir().ok()?;
            let output = self.execute("which", &[program], &cwd).ok()?;
            if output.success {
                let path = output.stdout.trim().lines().next()?.to_string();
                if path.is_empty() { None } else { Some(path) }
            } else {
                None
            }
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

pub struct CommandSession<F, N, P, C, I, A = LiveArchiveProvider>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
    A: ArchiveProvider,
{
    multi_progress: Arc<MultiProgress>,
    display_provider: LiveDisplayProvider,
    terminal_capabilities: TerminalCapabilities,
    filesystem_provider: F,
    network_provider: N,
    process_provider: P,
    config_provider: C,
    interactive_provider: I,
    archive_provider: A,
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
        LiveArchiveProvider,
    >
{
    fn resolve_packwiz_bin_path() -> String {
        crate::platform::packwiz_bin::resolve_packwiz_binary()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "packwiz-tx binary resolution failed; falling back to PATH lookup");
                crate::empack::packwiz::PACKWIZ_BIN.to_string()
            })
    }

    fn build_live_session(
        app_config: AppConfig,
        packwiz_bin_path: String,
        network_provider: LiveNetworkProvider,
    ) -> Self {
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

        Self {
            multi_progress,
            display_provider,
            terminal_capabilities,
            filesystem_provider: LiveFileSystemProvider,
            network_provider,
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

    pub fn new(app_config: AppConfig) -> Self {
        let packwiz_bin_path = Self::resolve_packwiz_bin_path();
        let network_provider = LiveNetworkProvider::with_timeout(app_config.net_timeout);
        Self::build_live_session(app_config, packwiz_bin_path, network_provider)
    }

    pub async fn new_async(app_config: AppConfig) -> Self {
        let packwiz_bin_path = tokio::task::spawn_blocking(Self::resolve_packwiz_bin_path)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "packwiz-tx binary resolution task failed; falling back to PATH lookup");
                crate::empack::packwiz::PACKWIZ_BIN.to_string()
            });
        let network_provider = LiveNetworkProvider::new_async(app_config.net_timeout).await;
        Self::build_live_session(app_config, packwiz_bin_path, network_provider)
    }
}

impl<F, N, P, C, I, A> CommandSession<F, N, P, C, I, A>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
    A: ArchiveProvider,
{
    #[cfg(feature = "test-utils")]
    pub fn new_with_providers_and_archive(
        filesystem_provider: F,
        network_provider: N,
        process_provider: P,
        config_provider: C,
        interactive_provider: I,
        archive_provider: A,
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
            archive_provider,
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

impl<F, N, P, C, I> CommandSession<F, N, P, C, I, LiveArchiveProvider>
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
        Self::new_with_providers_and_archive(
            filesystem_provider,
            network_provider,
            process_provider,
            config_provider,
            interactive_provider,
            LiveArchiveProvider,
        )
    }
}

impl<F, N, P, C, I, A> Session for CommandSession<F, N, P, C, I, A>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
    A: ArchiveProvider,
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

impl<F, N, P, C, I, A> Drop for CommandSession<F, N, P, C, I, A>
where
    F: FileSystemProvider,
    N: NetworkProvider,
    P: ProcessProvider,
    C: ConfigProvider,
    I: InteractiveProvider,
    A: ArchiveProvider,
{
    fn drop(&mut self) {
        // Ordered teardown:
        // 1. Clear any lingering progress bars from the shared MultiProgress
        let _ = self.multi_progress.clear();
        // 2. Restore cursor visibility (defense-in-depth, complements panic hook + signal handler)
        crate::terminal::cursor::force_show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{
        DisplayProvider, MultiProgressProvider, ProgressProvider, ProgressTracker, StatusProvider,
        StructuredProvider,
    };
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Clone, Default)]
    struct RecordingDisplay {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingDisplay {
        fn events(&self) -> Vec<String> {
            self.events.lock().expect("events").clone()
        }
    }

    struct RecordingStatus {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingStatus {
        fn push(&self, kind: &str, message: String) {
            self.events
                .lock()
                .expect("events")
                .push(format!("{kind}:{message}"));
        }
    }

    impl StatusProvider for RecordingStatus {
        fn checking(&self, task: &str) {
            self.push("checking", task.to_string());
        }

        fn success(&self, item: &str, details: &str) {
            self.push("success", format!("{item}:{details}"));
        }

        fn error(&self, item: &str, details: &str) {
            self.push("error", format!("{item}:{details}"));
        }

        fn warning(&self, message: &str) {
            self.push("warning", message.to_string());
        }

        fn info(&self, message: &str) {
            self.push("info", message.to_string());
        }

        fn message(&self, text: &str) {
            self.push("message", text.to_string());
        }

        fn emphasis(&self, text: &str) {
            self.push("emphasis", text.to_string());
        }

        fn subtle(&self, text: &str) {
            self.push("subtle", text.to_string());
        }

        fn list(&self, items: &[&str]) {
            self.push("list", items.join(","));
        }

        fn complete(&self, task: &str) {
            self.push("complete", task.to_string());
        }

        fn tool_check(&self, tool: &str, available: bool, version: &str) {
            self.push("tool_check", format!("{tool}:{available}:{version}"));
        }

        fn section(&self, title: &str) {
            self.push("section", title.to_string());
        }

        fn step(&self, current: usize, total: usize, description: &str) {
            self.push("step", format!("{current}/{total}:{description}"));
        }
    }

    struct NoopProgressTracker;

    impl ProgressTracker for NoopProgressTracker {
        fn set_position(&self, _pos: u64) {}

        fn inc(&self) {}

        fn inc_by(&self, _n: u64) {}

        fn set_message(&self, _message: &str) {}

        fn tick(&self, _item: &str) {}

        fn finish(&self, _message: &str) {}

        fn abandon(&self, _message: &str) {}

        fn finish_clear(&self) {}
    }

    struct NoopMultiProgress;

    impl MultiProgressProvider for NoopMultiProgress {
        fn add_bar(&self, _total: u64, _message: &str) -> Box<dyn ProgressTracker> {
            Box::new(NoopProgressTracker)
        }

        fn add_spinner(&self, _message: &str) -> Box<dyn ProgressTracker> {
            Box::new(NoopProgressTracker)
        }

        fn clear(&self) {}
    }

    struct NoopProgress;

    impl ProgressProvider for NoopProgress {
        fn bar(&self, _total: u64) -> Box<dyn ProgressTracker> {
            Box::new(NoopProgressTracker)
        }

        fn spinner(&self, _message: &str) -> Box<dyn ProgressTracker> {
            Box::new(NoopProgressTracker)
        }

        fn multi(&self) -> Box<dyn MultiProgressProvider> {
            Box::new(NoopMultiProgress)
        }
    }

    struct NoopStructured;

    impl StructuredProvider for NoopStructured {
        fn table(&self, _headers: &[&str], _rows: &[Vec<&str>]) {}

        fn list(&self, _items: &[&str]) {}

        fn properties(&self, _pairs: &[(&str, &str)]) {}
    }

    impl DisplayProvider for RecordingDisplay {
        fn status(&self) -> Box<dyn StatusProvider> {
            Box::new(RecordingStatus {
                events: self.events.clone(),
            })
        }

        fn progress(&self) -> Box<dyn ProgressProvider> {
            Box::new(NoopProgress)
        }

        fn table(&self) -> Box<dyn StructuredProvider> {
            Box::new(NoopStructured)
        }
    }

    struct TestSession {
        display: RecordingDisplay,
        process: LiveProcessProvider,
        filesystem: LiveFileSystemProvider,
        network: LiveNetworkProvider,
        config: LiveConfigProvider,
        interactive: LiveInteractiveProvider,
        terminal: TerminalCapabilities,
        archive: LiveArchiveProvider,
    }

    impl TestSession {
        fn new(display: RecordingDisplay, process: LiveProcessProvider) -> Self {
            Self {
                display,
                process,
                filesystem: LiveFileSystemProvider,
                network: LiveNetworkProvider::with_timeout(1),
                config: LiveConfigProvider::new(crate::application::config::AppConfig::default()),
                interactive: LiveInteractiveProvider::new(true, None),
                terminal: TerminalCapabilities::minimal(),
                archive: LiveArchiveProvider,
            }
        }
    }

    impl Session for TestSession {
        fn display(&self) -> &dyn DisplayProvider {
            &self.display
        }

        fn filesystem(&self) -> &dyn FileSystemProvider {
            &self.filesystem
        }

        fn network(&self) -> &dyn NetworkProvider {
            &self.network
        }

        fn process(&self) -> &dyn ProcessProvider {
            &self.process
        }

        fn config(&self) -> &dyn ConfigProvider {
            &self.config
        }

        fn interactive(&self) -> &dyn InteractiveProvider {
            &self.interactive
        }

        fn terminal(&self) -> &TerminalCapabilities {
            &self.terminal
        }

        fn archive(&self) -> &dyn ArchiveProvider {
            &self.archive
        }

        fn packwiz(&self) -> Box<dyn PackwizOps + '_> {
            Box::new(crate::empack::packwiz::LivePackwizOps::new(
                self.process(),
                self.filesystem(),
                self.packwiz_bin(),
            ))
        }

        fn state(&self) -> Result<PackStateManager<'_, dyn FileSystemProvider + '_>> {
            let workdir = self.filesystem.current_dir()?;
            Ok(PackStateManager::new(workdir, self.filesystem()))
        }

        fn packwiz_bin(&self) -> &str {
            crate::empack::packwiz::PACKWIZ_BIN
        }
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        unsafe fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match self.previous.as_ref() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn write_script(path: &Path, contents: &str) {
        std::fs::write(path, contents).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("set executable");
        }
    }

    #[test]
    fn recording_display_status_records_all_event_kinds() {
        let display = RecordingDisplay::default();
        let status = display.status();

        status.checking("dependencies");
        status.success("init", "done");
        status.error("build", "failed");
        status.warning("warning message");
        status.info("info message");
        status.message("plain message");
        status.emphasis("important");
        status.subtle("quiet");
        status.list(&["one", "two"]);
        status.complete("cleanup");
        status.tool_check("java", true, "21");
        status.section("Section");
        status.step(2, 5, "continue");

        assert_eq!(
            display.events(),
            vec![
                "checking:dependencies",
                "success:init:done",
                "error:build:failed",
                "warning:warning message",
                "info:info message",
                "message:plain message",
                "emphasis:important",
                "subtle:quiet",
                "list:one,two",
                "complete:cleanup",
                "tool_check:java:true:21",
                "section:Section",
                "step:2/5:continue",
            ]
        );
    }

    #[test]
    fn issue_stream_observer_matches_issue_prefixes_only() {
        assert!(IssueStreamObserver::should_echo_stdout(
            "Error: failed to resolve dependency"
        ));
        assert!(IssueStreamObserver::should_echo_stdout(
            "  Caused by: network timeout"
        ));
        assert!(IssueStreamObserver::should_echo_stdout(
            "Warning: using fallback loader"
        ));
        assert!(IssueStreamObserver::should_echo_stdout(
            "failed to download manifest"
        ));
    }

    #[test]
    fn issue_stream_observer_ignores_false_positive_substrings() {
        assert!(!IssueStreamObserver::should_echo_stdout(
            "Downloading error-handling-lib.jar: 100%"
        ));
        assert!(!IssueStreamObserver::should_echo_stdout(
            "3 warnings suppressed"
        ));
        assert!(!IssueStreamObserver::should_echo_stdout(
            "resolved without errors"
        ));
    }

    #[test]
    fn issue_stream_observer_suppresses_stack_frame_stderr() {
        assert!(IssueStreamObserver::should_echo_stderr(
            "java.lang.IllegalStateException: boom"
        ));
        assert!(IssueStreamObserver::should_echo_stderr(
            "Caused by: network timeout"
        ));
        assert!(!IssueStreamObserver::should_echo_stderr(
            "at com.example.Main.main(Main.java:42)"
        ));
        assert!(!IssueStreamObserver::should_echo_stderr(
            "at com.example.Frame"
        ));
        assert!(!IssueStreamObserver::should_echo_stderr("... 12 more"));
    }

    #[test]
    fn issue_stream_observer_keeps_non_stack_at_prefix_stderr() {
        assert!(IssueStreamObserver::should_echo_stderr(
            "at line 5 of config.toml"
        ));
        assert!(IssueStreamObserver::should_echo_stderr(
            "at request completion the cache was invalid"
        ));
    }

    #[test]
    fn issue_stream_observer_suppresses_native_method_and_unknown_source_frames() {
        assert!(!IssueStreamObserver::should_echo_stderr(
            "at com.example.Main.main(Native Method)"
        ));
        assert!(!IssueStreamObserver::should_echo_stderr(
            "at com.example.Main.main(Unknown Source)"
        ));
    }

    #[test]
    fn issue_stream_observer_ignores_empty_and_non_issue_lines() {
        let display = RecordingDisplay::default();
        let observer = IssueStreamObserver::new(&display, "packwiz-tx");

        observer.on_line(ProcessStream::Stderr, "   ");
        observer.on_line(ProcessStream::Stdout, "Downloaded manifest successfully");

        assert!(display.events().is_empty());
    }

    #[test]
    fn process_output_error_output_prefers_stderr_and_falls_back_to_stdout() {
        let stderr_preferred = ProcessOutput {
            stdout: "stdout failure".to_string(),
            stderr: "   stderr failure   ".to_string(),
            success: false,
        };
        assert_eq!(stderr_preferred.error_output(), "stderr failure");

        let stdout_fallback = ProcessOutput {
            stdout: "  stdout failure  ".to_string(),
            stderr: "   ".to_string(),
            success: false,
        };
        assert_eq!(stdout_fallback.error_output(), "stdout failure");
    }

    #[test]
    fn live_archive_provider_creates_and_extracts_zip() {
        let temp = TempDir::new().expect("temp dir");
        let source_dir = temp.path().join("source");
        let archive_path = temp.path().join("bundle.zip");
        let extract_dir = temp.path().join("extract");
        std::fs::create_dir_all(source_dir.join("nested")).expect("create source tree");
        std::fs::write(source_dir.join("nested").join("hello.txt"), "world")
            .expect("write source file");

        let provider = LiveArchiveProvider;
        provider
            .create_archive(
                &source_dir,
                &archive_path,
                crate::empack::archive::ArchiveFormat::Zip,
            )
            .expect("create archive");
        provider
            .extract_zip(&archive_path, &extract_dir)
            .expect("extract archive");

        assert_eq!(
            std::fs::read_to_string(extract_dir.join("nested").join("hello.txt"))
                .expect("read extracted file"),
            "world"
        );
    }

    #[test]
    fn live_archive_provider_wraps_errors_with_context() {
        let temp = TempDir::new().expect("temp dir");
        let provider = LiveArchiveProvider;

        let extract_err = provider
            .extract_zip(
                &temp.path().join("missing.zip"),
                &temp.path().join("extract"),
            )
            .expect_err("missing zip should fail");
        assert!(extract_err.to_string().contains("Failed to extract zip:"));

        let create_err = provider
            .create_archive(
                &temp.path().join("missing-source"),
                &temp.path().join("output.zip"),
                crate::empack::archive::ArchiveFormat::Zip,
            )
            .expect_err("missing source should fail");
        assert!(create_err.to_string().contains("Failed to create archive:"));
    }

    #[test]
    fn live_filesystem_provider_round_trips_files_and_directory_ops() {
        let provider = LiveFileSystemProvider;
        let temp = TempDir::new().expect("temp dir");
        let nested = temp.path().join("nested");
        let text_file = nested.join("hello.txt");
        let bytes_file = nested.join("payload.bin");
        let removable_dir = temp.path().join("remove-me");

        assert_eq!(
            provider.current_dir().expect("current dir"),
            std::env::current_dir().unwrap()
        );
        provider.create_dir_all(&nested).expect("create nested dir");
        assert!(provider.metadata_exists(&nested));
        assert!(provider.is_directory(&nested));

        provider
            .write_file(&text_file, "hello")
            .expect("write text file");
        provider
            .write_bytes(&bytes_file, b"payload")
            .expect("write bytes file");

        assert_eq!(
            provider.read_to_string(&text_file).expect("read text file"),
            "hello"
        );
        assert_eq!(
            provider.read_bytes(&bytes_file).expect("read bytes file"),
            b"payload"
        );

        let files = provider.get_file_list(&nested).expect("list nested files");
        assert!(files.contains(&text_file));
        assert!(files.contains(&bytes_file));

        let _manager = provider.config_manager(temp.path().to_path_buf());

        provider.remove_file(&text_file).expect("remove text file");
        assert!(!provider.exists(&text_file));

        provider
            .create_dir_all(&removable_dir)
            .expect("create removable dir");
        provider
            .remove_dir_all(&removable_dir)
            .expect("remove removable dir");
        assert!(!provider.exists(&removable_dir));
    }

    #[test]
    fn live_filesystem_provider_detects_artifacts_and_ignores_noise() {
        let provider = LiveFileSystemProvider;
        let temp = TempDir::new().expect("temp dir");
        let missing = temp.path().join("missing");
        let noisy_dist = temp.path().join("dist-noisy");
        let file_dist = temp.path().join("dist-file");
        let dir_dist = temp.path().join("dist-dir");

        assert!(
            !provider
                .has_build_artifacts(&missing)
                .expect("missing dist should be false")
        );

        provider
            .create_dir_all(&noisy_dist)
            .expect("create noisy dist");
        provider
            .write_file(&noisy_dist.join("notes.txt"), "noise")
            .expect("write noise file");
        provider
            .create_dir_all(&noisy_dist.join("scratch"))
            .expect("create noise dir");
        assert!(
            !provider
                .has_build_artifacts(&noisy_dist)
                .expect("noise should not count as artifacts")
        );

        provider
            .create_dir_all(&file_dist)
            .expect("create file dist");
        provider
            .write_file(&file_dist.join("pack.mrpack"), "artifact")
            .expect("write artifact file");
        assert!(
            provider
                .has_build_artifacts(&file_dist)
                .expect("mrpack should count as artifact")
        );

        provider.create_dir_all(&dir_dist).expect("create dir dist");
        provider
            .create_dir_all(&dir_dist.join("client-full"))
            .expect("create artifact dir");
        assert!(
            provider
                .has_build_artifacts(&dir_dist)
                .expect("client-full dir should count as artifact")
        );
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn live_network_provider_exposes_client_budget_and_resolver() {
        let default_provider = LiveNetworkProvider::default();
        assert!(default_provider.http_client().is_ok());
        assert!(
            default_provider
                .rate_budgets()
                .for_host("api.modrinth.com")
                .is_some()
        );

        let provider = LiveNetworkProvider::new_for_test(
            Some("https://modrinth.example".to_string()),
            Some("https://curseforge.example".to_string()),
        );
        let client = provider.http_client().expect("http client");
        let _resolver = provider.project_resolver(client, Some("cf-key".to_string()));
        assert!(
            provider
                .rate_budgets()
                .for_host("api.curseforge.com")
                .is_some()
        );
    }

    #[test]
    fn live_process_provider_execute_and_find_program_work() {
        let provider = LiveProcessProvider::default();

        let output = provider
            .execute("rustc", &["--version"], Path::new("."))
            .expect("execute rustc");
        assert!(output.success);
        assert!(output.stdout.contains("rustc"));
        assert!(provider.find_program("rustc").is_some());
        assert!(
            LiveProcessProvider::new_for_test(None)
                .find_program("definitely-not-a-real-program-empack")
                .is_none()
        );
    }

    #[test]
    fn live_process_provider_reports_spawn_failure_for_missing_command() {
        let provider = LiveProcessProvider::new();
        let error = provider
            .execute("definitely-not-a-real-program-empack", &[], Path::new("."))
            .expect_err("missing command should fail");

        assert!(error.to_string().contains("Failed to spawn command"));
    }

    #[cfg(unix)]
    #[test]
    fn live_process_provider_uses_custom_path_for_execution_and_lookup() {
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("hello-tool");
        write_script(&command, "#!/bin/sh\nprintf 'custom path works'\n");

        let provider =
            LiveProcessProvider::new_for_test(Some(temp.path().to_string_lossy().into_owned()));
        let output = provider
            .execute("hello-tool", &[], temp.path())
            .expect("execute custom tool");

        assert_eq!(output.stdout, "custom path works");
        assert_eq!(
            provider
                .find_program("hello-tool")
                .expect("lookup hello-tool"),
            command.to_string_lossy()
        );
    }

    #[cfg(windows)]
    #[test]
    fn live_process_provider_uses_custom_path_for_execution_and_lookup() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("hello-tool.cmd");
        write_script(&command, "@echo off\r\necho custom path works\r\n");
        let _pathext = unsafe { EnvVarGuard::set("PATHEXT", ".CMD;.EXE;.BAT;.COM") };

        let provider =
            LiveProcessProvider::new_for_test(Some(temp.path().to_string_lossy().into_owned()));
        let output = provider
            .execute("hello-tool", &[], temp.path())
            .expect("execute custom tool");

        assert!(output.stdout.contains("custom path works"));
        // Windows command lookup is case-insensitive and may reflect PATHEXT casing.
        let found = provider
            .find_program("hello-tool")
            .expect("lookup hello-tool");
        assert!(found.eq_ignore_ascii_case(&command.to_string_lossy()));
    }

    #[cfg(unix)]
    #[test]
    fn execute_process_with_live_issues_flushes_partial_output_and_issue_lines() {
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("packwiz-tx");
        write_script(
            &command,
            "#!/bin/sh\nprintf 'error: partial stdout'\nprintf 'Caused by: boom\\n' >&2\nprintf 'at com.example.Frame\\n' >&2\n",
        );

        let display = RecordingDisplay::default();
        let session = TestSession::new(display.clone(), LiveProcessProvider::new());

        let output = execute_process_with_live_issues(
            &session,
            command.to_str().expect("command path"),
            &[],
            temp.path(),
        )
        .expect("execute command");

        assert_eq!(output.stdout, "error: partial stdout");
        assert!(output.stderr.contains("Caused by: boom"));

        let events = display.events();
        assert!(
            events
                .iter()
                .any(|event| event == "message:packwiz-tx: error: partial stdout")
        );
        assert!(
            events
                .iter()
                .any(|event| event == "warning:packwiz-tx: Caused by: boom")
        );
        assert!(
            events
                .iter()
                .all(|event| !event.contains("com.example.Frame"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn execute_process_with_live_issues_flushes_partial_stderr_without_newline() {
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("packwiz-tx");
        write_script(
            &command,
            "#!/bin/sh\nprintf 'warning: partial stderr' >&2\n",
        );

        let display = RecordingDisplay::default();
        let session = TestSession::new(display.clone(), LiveProcessProvider::new());
        let output = execute_process_with_live_issues(
            &session,
            command.to_str().expect("command path"),
            &[],
            temp.path(),
        )
        .expect("execute command");

        assert_eq!(output.stderr, "warning: partial stderr");
        assert!(
            display
                .events()
                .contains(&"warning:packwiz-tx: warning: partial stderr".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn live_process_provider_times_out_with_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("sleepy");
        write_script(&command, "#!/bin/sh\nsleep 2\n");

        let _timeout = unsafe { EnvVarGuard::set("EMPACK_PROCESS_TIMEOUT_SECS", "1") };
        let provider = LiveProcessProvider::new();

        let error = provider
            .execute(command.to_str().expect("command path"), &[], temp.path())
            .expect_err("command should time out");

        assert!(error.to_string().contains("timed out after 1 seconds"));
        assert!(error.to_string().contains("sleepy"));
    }

    #[test]
    fn live_interactive_provider_uses_defaults_in_non_interactive_mode() {
        let provider = LiveInteractiveProvider::new(true, None);

        assert_eq!(
            provider
                .text_input("prompt", "fallback".to_string())
                .expect("text input"),
            "fallback"
        );
        assert!(!provider.confirm("prompt", false).expect("confirm"));
        assert_eq!(
            provider.select("prompt", &["one", "two"]).expect("select"),
            0
        );
        assert_eq!(
            provider
                .fuzzy_select("prompt", &[String::from("one"), String::from("two")])
                .expect("fuzzy select"),
            Some(0)
        );
    }

    #[test]
    fn command_session_resolve_packwiz_bin_path_falls_back_on_invalid_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let missing = temp.path().join("missing-packwiz");
        let _override = unsafe { EnvVarGuard::set("EMPACK_PACKWIZ_BIN", missing.as_os_str()) };

        let resolved = CommandSession::<
            LiveFileSystemProvider,
            LiveNetworkProvider,
            LiveProcessProvider,
            LiveConfigProvider,
            LiveInteractiveProvider,
        >::resolve_packwiz_bin_path();

        assert_eq!(resolved, crate::empack::packwiz::PACKWIZ_BIN);
    }

    #[test]
    fn command_session_new_uses_packwiz_env_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("packwiz-tx");
        write_script(&command, "#!/bin/sh\nprintf 'packwiz stub'\n");
        let _override = unsafe { EnvVarGuard::set("EMPACK_PACKWIZ_BIN", command.as_os_str()) };

        let session = CommandSession::new(crate::application::config::AppConfig::default());
        assert_eq!(session.packwiz_bin(), command.to_string_lossy());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn command_session_new_async_uses_packwiz_env_override() {
        let _guard = crate::test_support::env_lock().lock_async().await;
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("packwiz-tx");
        write_script(&command, "#!/bin/sh\nprintf 'packwiz stub'\n");
        let _override = unsafe { EnvVarGuard::set("EMPACK_PACKWIZ_BIN", command.as_os_str()) };

        let session =
            CommandSession::new_async(crate::application::config::AppConfig::default()).await;
        assert_eq!(session.packwiz_bin(), command.to_string_lossy());
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn command_session_new_with_providers_wires_packwiz_and_interactive_defaults() {
        let session = CommandSession::new_with_providers_and_archive(
            LiveFileSystemProvider,
            LiveNetworkProvider::with_timeout(1),
            LiveProcessProvider::new(),
            LiveConfigProvider::new(crate::application::config::AppConfig::default()),
            LiveInteractiveProvider::new(true, None),
            LiveArchiveProvider,
        );

        assert_eq!(session.packwiz_bin(), crate::empack::packwiz::PACKWIZ_BIN);
        assert_eq!(
            session.terminal().unicode,
            crate::primitives::TerminalUnicodeCaps::Ascii
        );
        assert!(!session.terminal().is_tty);
        assert_eq!(
            session
                .interactive()
                .text_input("prompt", "fallback".to_string())
                .expect("text input"),
            "fallback"
        );
        assert!(!session.config().app_config().yes);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn command_session_accessors_and_state_are_structured() {
        let temp = TempDir::new().expect("temp dir");
        let source_dir = temp.path().join("source");
        let archive_path = temp.path().join("source.zip");
        std::fs::create_dir_all(&source_dir).expect("create source dir");
        std::fs::write(source_dir.join("file.txt"), "hello").expect("write source file");

        let app_config = crate::application::config::AppConfig {
            workdir: Some(temp.path().to_path_buf()),
            ..crate::application::config::AppConfig::default()
        };
        let session = CommandSession::new_with_providers_and_archive(
            LiveFileSystemProvider,
            LiveNetworkProvider::new(),
            LiveProcessProvider::new(),
            LiveConfigProvider::new(app_config),
            LiveInteractiveProvider::new(true, None),
            LiveArchiveProvider,
        );

        session.display().status().message("hello");
        assert!(session.network().http_client().is_ok());
        assert_eq!(
            session.terminal().unicode,
            crate::primitives::TerminalUnicodeCaps::Ascii
        );
        assert!(
            !session
                .interactive()
                .confirm("prompt", false)
                .expect("confirm")
        );

        let session_ref: &dyn Session = &session;
        assert!(session_ref.filesystem().current_dir().is_ok());
        assert!(
            session_ref
                .network()
                .rate_budgets()
                .for_host("api.modrinth.com")
                .is_some()
        );
        assert!(session_ref.process().find_program("rustc").is_some());
        assert_eq!(
            session_ref.config().app_config().workdir.as_deref(),
            Some(temp.path())
        );
        assert!(
            !session_ref
                .interactive()
                .confirm("prompt", false)
                .expect("confirm")
        );

        session_ref
            .archive()
            .create_archive(
                &source_dir,
                &archive_path,
                crate::empack::archive::ArchiveFormat::Zip,
            )
            .expect("create archive");
        assert!(archive_path.exists());
        let _packwiz = session_ref.packwiz();
        let state = session_ref.state().expect("state manager");
        assert_eq!(state.workdir, temp.path());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn live_network_provider_new_async_uses_http_cache_dir_override_and_loads_cache() {
        let _guard = crate::test_support::env_lock().lock_async().await;
        let temp = TempDir::new().expect("temp dir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp.path().as_os_str()) };

        let provider = LiveNetworkProvider::new_async(1).await;
        assert_eq!(
            provider.cache.cache_dir(),
            &crate::platform::cache::http_cache_dir().expect("http cache dir")
        );

        provider
            .cache
            .put(
                "https://example.invalid/cache".to_string(),
                crate::networking::cache::CachedResponse {
                    data: b"cached".to_vec(),
                    etag: None,
                    expires: std::time::SystemTime::now() + std::time::Duration::from_secs(60),
                    status: 200,
                },
            )
            .await;

        let reloaded = LiveNetworkProvider::new_async(1).await;
        let cached = reloaded
            .cache
            .get("https://example.invalid/cache")
            .await
            .expect("cached response should reload from disk");
        assert_eq!(cached.data, b"cached");
    }
}
