//! Invocation context and terminal interaction for native engine hosts.
//! Project filesystem, networking and publication belong to engine capabilities.
use crate::Result;
use crate::application::config::AppConfig;
use crate::display::{DisplayProvider, LiveDisplayProvider};
use crate::terminal::TerminalCapabilities;
use anyhow::Context;
use indicatif::MultiProgress;
use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

pub trait InvocationProvider {
    fn current_dir(&self) -> Result<PathBuf>;
}
pub struct LiveInvocationProvider;
impl InvocationProvider for LiveInvocationProvider {
    fn current_dir(&self) -> Result<PathBuf> {
        Ok(env::current_dir()?)
    }
}
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
    /// Some tools write error messages to stdout.
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

pub trait ProcessProvider {
    fn check_cancelled(&self) -> Result<()> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ProcessOutput>> + Send>>;

    /// Returns the program path if found. Uses platform-appropriate lookup.
    fn find_program(&self, program: &str) -> Option<String>;
}

pub trait ConfigProvider {
    fn app_config(&self) -> &AppConfig;
}

/// Host prompts and deliberate selection; no project mutation authority.
pub trait InteractiveProvider {
    /// Whether this host can obtain a deliberate choice. Defaulted answers are not choices.
    fn can_choose(&self) -> bool {
        false
    }

    fn text_input(&self, prompt: &str, default: String) -> Result<String>;

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;

    fn select(&self, prompt: &str, options: &[&str]) -> Result<usize>;

    /// Returns Some(index) if user selected, None if user pressed ESC.
    fn fuzzy_select(&self, prompt: &str, options: &[String]) -> Result<Option<usize>>;
}

pub trait Session {
    fn display(&self) -> &dyn DisplayProvider;
    fn invocation(&self) -> &dyn InvocationProvider;
    fn process(&self) -> &dyn ProcessProvider;
    fn config(&self) -> &dyn ConfigProvider;
    fn interactive(&self) -> &dyn InteractiveProvider;
    fn terminal(&self) -> &TerminalCapabilities;
}

/// Default timeout for child process execution (5 minutes).
/// Bounds child exit and inherited output-pipe lifetimes.
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
    cancellation: super::process_runtime::Cancellation,
}

impl LiveProcessProvider {
    pub fn new() -> Self {
        Self {
            custom_path: None,
            cancellation: Default::default(),
        }
    }

    pub fn with_cancellation(mut self, cancellation: super::process_runtime::Cancellation) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_custom_path(path: String) -> Self {
        Self {
            custom_path: Some(path),
            cancellation: Default::default(),
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

pub(super) fn decode_process_output_chunk(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        return decode_process_output_chunk_windows(bytes);
    }

    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
fn decode_process_output_chunk_windows(bytes: &[u8]) -> String {
    decode_process_output_chunk_windows_with_codepages(bytes, &windows_process_output_codepages())
}

#[cfg(windows)]
fn decode_process_output_chunk_windows_with_codepages(bytes: &[u8], codepages: &[u32]) -> String {
    if let Ok(valid_utf8) = std::str::from_utf8(bytes) {
        return valid_utf8.to_string();
    }

    for codepage in codepages {
        if let Some(decoded) = decode_windows_codepage(bytes, *codepage) {
            return decoded;
        }
    }

    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
fn windows_process_output_codepages() -> Vec<u32> {
    use windows_sys::Win32::Globalization::{GetACP, GetOEMCP};
    use windows_sys::Win32::System::Console::GetConsoleOutputCP;

    let mut codepages = Vec::new();
    for codepage in unsafe { [GetConsoleOutputCP(), GetACP(), GetOEMCP()] } {
        if codepage != 0 && !codepages.contains(&codepage) {
            codepages.push(codepage);
        }
    }
    codepages
}

#[cfg(windows)]
fn decode_windows_codepage(bytes: &[u8], codepage: u32) -> Option<String> {
    use windows_sys::Win32::Globalization::MultiByteToWideChar;

    if bytes.is_empty() {
        return Some(String::new());
    }
    if codepage == 0 {
        return None;
    }

    let input_len = i32::try_from(bytes.len()).ok()?;
    let required_len = unsafe {
        MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if required_len <= 0 {
        return None;
    }

    let mut wide = vec![0u16; required_len as usize];
    let converted_len = unsafe {
        MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            wide.as_mut_ptr(),
            required_len,
        )
    };
    if converted_len <= 0 {
        return None;
    }

    String::from_utf16(&wide[..converted_len as usize]).ok()
}

impl ProcessProvider for LiveProcessProvider {
    fn check_cancelled(&self) -> Result<()> {
        self.cancellation.check()
    }

    fn execute(
        &self,
        command: &str,
        args: &[&str],
        working_dir: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ProcessOutput>> + Send>> {
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

        Box::pin(super::process_runtime::execute_async(
            cmd,
            process_timeout(),
            self.cancellation.clone(),
            None,
        ))
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
            use std::os::unix::fs::PermissionsExt;
            let cwd = std::env::current_dir().ok()?;
            let path = Path::new(program);
            let candidates = if path.components().count() > 1 || path.is_absolute() {
                vec![cwd.join(path)]
            } else {
                let search = self
                    .effective_path()
                    .unwrap_or_else(|| "/usr/bin:/bin".into());
                std::env::split_paths(&search)
                    .map(|directory| cwd.join(directory).join(program))
                    .collect()
            };
            candidates
                .into_iter()
                .find(|candidate| {
                    std::fs::metadata(candidate).is_ok_and(|metadata| {
                        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                    })
                })
                .map(|path| path.to_string_lossy().into_owned())
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
}

impl LiveInteractiveProvider {
    pub fn new(yes_mode: bool, _workdir: Option<PathBuf>) -> Self {
        Self { yes_mode }
    }

    /// Check if we're in a TTY environment suitable for interactive prompts
    fn is_tty() -> bool {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    }

    fn handle_interrupt<T>(&self) -> Result<T> {
        Err(super::process_runtime::Interrupted.into())
    }
}

impl InteractiveProvider for LiveInteractiveProvider {
    fn can_choose(&self) -> bool {
        !self.yes_mode && Self::is_tty()
    }

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

pub struct CommandSession {
    multi_progress: Arc<MultiProgress>,
    display_provider: LiveDisplayProvider,
    terminal_capabilities: TerminalCapabilities,
    invocation_provider: LiveInvocationProvider,
    process_provider: LiveProcessProvider,
    config_provider: LiveConfigProvider,
    interactive_provider: LiveInteractiveProvider,
}
impl CommandSession {
    pub fn new(app_config: AppConfig) -> Self {
        let terminal_capabilities = TerminalCapabilities::detect_from_config(app_config.color)
            .unwrap_or_else(|_| TerminalCapabilities::minimal());
        let multi_progress = Arc::new(MultiProgress::new());
        let display_provider = LiveDisplayProvider::new_with_arc(multi_progress.clone())
            .with_capabilities(terminal_capabilities.clone());
        Self {
            multi_progress,
            display_provider,
            terminal_capabilities,
            invocation_provider: LiveInvocationProvider,
            process_provider: LiveProcessProvider::new(),
            interactive_provider: LiveInteractiveProvider::new(
                app_config.yes,
                app_config.workdir.clone(),
            ),
            config_provider: LiveConfigProvider::new(app_config),
        }
    }
    pub fn with_cancellation(mut self, cancellation: super::process_runtime::Cancellation) -> Self {
        self.process_provider.cancellation = cancellation;
        self
    }
}
impl Session for CommandSession {
    fn display(&self) -> &dyn DisplayProvider {
        &self.display_provider
    }
    fn invocation(&self) -> &dyn InvocationProvider {
        &self.invocation_provider
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
}
impl Drop for CommandSession {
    fn drop(&mut self) {
        let _ = self.multi_progress.clear();
        crate::terminal::cursor::force_show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use tempfile::TempDir;
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

    #[cfg(unix)]
    fn write_empack_boundary(root: &Path) {
        std::fs::create_dir_all(root.join("pack")).expect("create pack dir");
        std::fs::write(root.join("empack.yml"), "name: test-pack\n").expect("write empack.yml");
        std::fs::write(
            root.join("pack").join("pack.toml"),
            "name = \"test-pack\"\n",
        )
        .expect("write pack.toml");
    }

    #[cfg(unix)]
    fn write_state_marker(root: &Path) -> std::path::PathBuf {
        let marker = root.join("operation-evidence");
        std::fs::write(&marker, "active\n").expect("write state marker");
        marker
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
    fn process_output_decoder_prefers_utf8_when_valid() {
        let decoded = decode_process_output_chunk("§6No Enchant Glint 1.20.1.zip\n".as_bytes());
        assert_eq!(decoded, "§6No Enchant Glint 1.20.1.zip\n");
    }

    #[cfg(windows)]
    #[test]
    fn windows_process_output_decoder_preserves_section_sign_from_cp1252() {
        let decoded = decode_process_output_chunk_windows_with_codepages(
            &[
                0xA7, b'6', b'N', b'o', b' ', b'E', b'n', b'c', b'h', b'a', b'n', b't', b' ', b'G',
                b'l', b'i', b'n', b't', b' ', b'1', b'.', b'2', b'0', b'.', b'1', b'.', b'z', b'i',
                b'p',
            ],
            &[1252],
        );
        assert_eq!(decoded, "§6No Enchant Glint 1.20.1.zip");
    }

    #[cfg(windows)]
    #[test]
    fn windows_process_output_decoder_falls_back_lossily_when_unmappable() {
        let decoded = decode_process_output_chunk_windows_with_codepages(&[0xFF, 0xFE], &[0]);
        assert!(
            !decoded.is_empty(),
            "lossy fallback should still return replacement text"
        );
    }

    #[tokio::test]
    async fn live_process_provider_execute_and_find_program_work() {
        let provider = LiveProcessProvider::default();

        let output = provider
            .execute("rustc", &["--version"], Path::new("."))
            .await
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

    #[tokio::test]
    async fn live_process_provider_reports_spawn_failure_for_missing_command() {
        let provider = LiveProcessProvider::new();
        let error = provider
            .execute("definitely-not-a-real-program-empack", &[], Path::new("."))
            .await
            .expect_err("missing command should fail");

        assert!(error.to_string().contains("Failed to spawn command"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_process_provider_uses_custom_path_for_execution_and_lookup() {
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("hello-tool");
        write_script(&command, "#!/bin/sh\nprintf 'custom path works'\n");

        let provider =
            LiveProcessProvider::new_for_test(Some(temp.path().to_string_lossy().into_owned()));
        let output = provider
            .execute("hello-tool", &[], temp.path())
            .await
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
    #[tokio::test]
    async fn live_process_provider_uses_custom_path_for_execution_and_lookup() {
        let _guard = crate::test_support::env_lock().lock_async().await;
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("hello-tool.cmd");
        write_script(&command, "@echo off\r\necho custom path works\r\n");
        let _pathext = unsafe { EnvVarGuard::set("PATHEXT", ".CMD;.EXE;.BAT;.COM") };

        let provider =
            LiveProcessProvider::new_for_test(Some(temp.path().to_string_lossy().into_owned()));
        let output = provider
            .execute("hello-tool", &[], temp.path())
            .await
            .expect("execute custom tool");

        assert!(output.stdout.contains("custom path works"));
        // Windows command lookup is case-insensitive and may reflect PATHEXT casing.
        let found = provider
            .find_program("hello-tool")
            .expect("lookup hello-tool");
        assert!(found.eq_ignore_ascii_case(&command.to_string_lossy()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_process_provider_times_out_with_override() {
        let _guard = crate::test_support::env_lock().lock_async().await;
        let temp = TempDir::new().expect("temp dir");
        let command = temp.path().join("sleepy");
        write_script(&command, "#!/bin/sh\nsleep 2\n");

        let _timeout = unsafe { EnvVarGuard::set("EMPACK_PROCESS_TIMEOUT_SECS", "1") };
        let provider = LiveProcessProvider::new();

        let error = provider
            .execute(command.to_str().expect("command path"), &[], temp.path())
            .await
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

    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_deadline_covers_closed_and_inherited_pipes() {
        let _guard = crate::test_support::env_lock().lock_async().await;
        let previous = std::env::var_os("EMPACK_PROCESS_TIMEOUT_SECS");
        unsafe {
            std::env::set_var("EMPACK_PROCESS_TIMEOUT_SECS", "1");
        }
        let mut results = Vec::new();
        for script in ["exec 1>&-; exec 2>&-; sleep 4", "sleep 4 & wait"] {
            let start = std::time::Instant::now();
            let result = LiveProcessProvider::new()
                .execute("sh", &["-c", script], Path::new("."))
                .await;
            results.push((result, start.elapsed()));
        }
        unsafe {
            match previous {
                Some(v) => std::env::set_var("EMPACK_PROCESS_TIMEOUT_SECS", v),
                None => std::env::remove_var("EMPACK_PROCESS_TIMEOUT_SECS"),
            }
        }
        for (result, elapsed) in results {
            assert!(result.unwrap_err().to_string().contains("timed out"));
            assert!(
                elapsed < std::time::Duration::from_secs(3),
                "deadline took {elapsed:?}"
            );
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_one_process_preserves_markers_and_other_sessions() {
        let dir = TempDir::new().unwrap();
        write_empack_boundary(dir.path());
        let marker = write_state_marker(dir.path());
        let cancellation = super::super::process_runtime::Cancellation::default();
        let token = cancellation.clone();
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            token.cancel();
        });
        let start = std::time::Instant::now();
        let result = LiveProcessProvider::new()
            .with_cancellation(cancellation)
            .execute("sh", &["-c", "sleep 10 & wait"], dir.path())
            .await;
        trigger.join().unwrap();
        assert_eq!(
            crate::application::classify_error(&result.unwrap_err()),
            crate::application::EmpackExitCode::Interrupted
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        assert!(marker.exists());
        assert!(
            LiveProcessProvider::new()
                .execute("sh", &["-c", "exit 0"], dir.path())
                .await
                .unwrap()
                .success
        );
    }
    #[cfg(unix)]
    #[test]
    fn program_lookup_checks_files_without_running_a_locator() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempDir::new().unwrap();
        let marker = root.path().join("unexpected");
        let trap = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
        write_script(&root.path().join("which"), &trap);
        write_script(&root.path().join("selected"), "#!/bin/sh\nexit 0\n");
        let not_executable = root.path().join("data");
        std::fs::write(&not_executable, "data").unwrap();
        std::fs::set_permissions(&not_executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::create_dir(root.path().join("directory")).unwrap();
        let provider =
            LiveProcessProvider::with_custom_path(root.path().to_string_lossy().into_owned());
        assert_eq!(
            provider.find_program("selected"),
            Some(root.path().join("selected").to_string_lossy().into_owned())
        );
        assert_eq!(
            provider.find_program(root.path().join("selected").to_str().unwrap()),
            provider.find_program("selected")
        );
        for name in ["data", "directory", "absent"] {
            assert!(provider.find_program(name).is_none());
        }
        assert!(!marker.exists());
    }
}
