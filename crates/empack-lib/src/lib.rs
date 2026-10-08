//! # empack Library
//!
//! Minecraft modpack management library.
//!
//! ## Core Modules
//!
//! - [`primitives`] - Foundation types, errors, and shared coordination
//! - [`terminal`] - Cross-platform terminal capability detection
//! - [`logger`] - Structured logging with progress tracking
//! - [`networking`] - Async HTTP client with concurrency management
//! - [`platform`] - System resource detection and optimization
//! - [`engine`] - Verified planning, provider resolution and recoverable publication
//! - [`empack`] - Domain-specific modpack management types
//! - [`application`] - CLI interface and configuration management
//!
//! ## Quick Start
//!
//! ```no_run
//! #[tokio::main]
//! async fn main() {
//! // Initialize and run empack
//! empack_lib::main().await.unwrap();
//! }
//! ```

pub mod application;
pub mod display;
pub mod empack;
pub mod engine;
pub mod logger;
pub mod networking;
pub mod platform;
pub mod primitives;
pub mod terminal;

pub mod testing;

pub use application::{AppConfig, Cli, CliLoad, Commands, EmpackExitCode, execute_command};
pub use logger::Logger;
pub use networking::{NetworkingConfig, NetworkingManager};
pub use platform::SystemResources;
pub use primitives::{
    ConfigError, LogFormat, LogLevel, LogOutput, LoggerError, TerminalCapsDetectIntent,
    TerminalColorCaps,
};
pub use terminal::TerminalCapabilities;

pub type Result<T> = anyhow::Result<T>;

use application::CliConfig;
use std::future::Future;
pub async fn main() -> Result<()> {
    let config = CliConfig::load()?;
    run_with_config(config).await
}

pub async fn process_main() -> std::process::ExitCode {
    display::clear_error_rendered();
    match CliConfig::load_for_process() {
        Ok(CliLoad::Ready(config)) => match run_with_config(*config).await {
            Ok(()) => EmpackExitCode::Success.as_process_exit_code(),
            Err(error) => {
                let exit_code = application::classify_error(&error);
                if let Some(message) = fallback_process_error_message(&error) {
                    eprintln!("{message}");
                }
                exit_code.as_process_exit_code()
            }
        },
        Ok(CliLoad::Display(message)) => {
            print!("{message}");
            EmpackExitCode::Success.as_process_exit_code()
        }
        Err(ConfigError::ParseError { reason, .. }) => {
            eprint!("{reason}");
            EmpackExitCode::Usage.as_process_exit_code()
        }
        Err(error) => {
            eprintln!("Error: {error}");
            EmpackExitCode::Usage.as_process_exit_code()
        }
    }
}

pub async fn run_with_config(mut config: CliConfig) -> Result<()> {
    config.app_config.validate()?;
    execute_command(config).await
}

fn fallback_process_error_message(error: &anyhow::Error) -> Option<String> {
    if display::take_error_rendered() {
        None
    } else {
        Some(format!("Error: {error:#}"))
    }
}

/// Await a library command without installing process-wide signal handlers.
pub async fn run_main_loop<F>(_workdir: Option<std::path::PathBuf>, command: F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    command.await
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::convert::Infallible;
    use std::ffi::OsString;
    use std::sync::OnceLock;

    const CLI_ENV_VARS: [&str; 20] = [
        "EMPACK_WORKDIR",
        "EMPACK_CPU_JOBS",
        "EMPACK_NET_TIMEOUT",
        "EMPACK_ID_MODRINTH",
        "EMPACK_KEY_MODRINTH",
        "EMPACK_KEY_CURSEFORGE",
        "EMPACK_LOG_LEVEL",
        "EMPACK_LOG_FORMAT",
        "EMPACK_LOG_OUTPUT",
        "EMPACK_COLOR",
        "EMPACK_YES",
        "EMPACK_DRY_RUN",
        "EMPACK_MODLOADER",
        "EMPACK_MC_VERSION",
        "EMPACK_AUTHOR",
        "EMPACK_NAME",
        "EMPACK_LOADER_VERSION",
        "EMPACK_PACK_VERSION",
        "EMPACK_DATAPACK_FOLDER",
        "EMPACK_GAME_VERSIONS",
    ];

    pub struct CliEnvGuard {
        saved: [(&'static str, Option<OsString>); CLI_ENV_VARS.len()],
    }

    pub type EnvLockGuard<'a> = tokio::sync::MutexGuard<'a, ()>;

    pub struct EnvLock {
        inner: tokio::sync::Mutex<()>,
    }

    impl EnvLock {
        pub fn lock(&'static self) -> Result<EnvLockGuard<'static>, Infallible> {
            Ok(self.inner.blocking_lock())
        }

        pub async fn lock_async(&'static self) -> EnvLockGuard<'static> {
            self.inner.lock().await
        }
    }

    pub fn env_lock() -> &'static EnvLock {
        static LOCK: OnceLock<EnvLock> = OnceLock::new();
        LOCK.get_or_init(|| EnvLock {
            inner: tokio::sync::Mutex::new(()),
        })
    }

    pub fn isolate_cli_env() -> CliEnvGuard {
        let saved = CLI_ENV_VARS.map(|key| (key, std::env::var_os(key)));
        unsafe {
            for key in CLI_ENV_VARS {
                std::env::remove_var(key);
            }
        }
        CliEnvGuard { saved }
    }

    impl Drop for CliEnvGuard {
        fn drop(&mut self) {
            unsafe {
                for (key, value) in &self.saved {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn error_render_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn fallback_process_error_message_is_suppressed_after_rendered_error() {
        let _guard = error_render_lock().lock().expect("error render lock");
        crate::display::clear_error_rendered();
        crate::display::mark_error_rendered();

        assert!(fallback_process_error_message(&anyhow::anyhow!("boom")).is_none());
    }

    #[test]
    fn fallback_process_error_message_formats_when_no_error_was_rendered() {
        let _guard = error_render_lock().lock().expect("error render lock");
        crate::display::clear_error_rendered();

        let message =
            fallback_process_error_message(&anyhow::anyhow!("boom")).expect("fallback message");
        assert_eq!(message, "Error: boom");
    }

    #[tokio::test]
    async fn run_main_loop_completes_with_ready_command() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        run_main_loop(
            Some(temp_dir.path().to_path_buf()),
            std::future::ready(Ok::<(), anyhow::Error>(())),
        )
        .await
        .expect("run main loop");
    }

    #[tokio::test]
    async fn run_main_loop_propagates_command_error() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let error = run_main_loop(
            Some(temp_dir.path().to_path_buf()),
            std::future::ready(Err::<(), anyhow::Error>(anyhow::anyhow!("boom"))),
        )
        .await
        .expect_err("run main loop should propagate command errors");

        assert!(error.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn live_uninitialized_build_errors_classify_as_usage() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let error = execute_command(CliConfig {
            app_config: AppConfig {
                workdir: Some(temp_dir.path().to_path_buf()),
                ..AppConfig::default()
            },
            command: Some(Commands::Build(application::BuildArgs {
                targets: vec!["mrpack".to_string()],
                ..Default::default()
            })),
        })
        .await
        .expect_err("uninitialized build should fail");

        assert_eq!(
            application::classify_error(&error),
            EmpackExitCode::Usage,
            "error chain: {}",
            error
                .chain()
                .map(|cause| cause.to_string())
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
}
