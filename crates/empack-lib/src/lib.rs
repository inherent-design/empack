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
//! - [`api`] - Platform API abstraction and dependency resolution
//! - [`empack`] - Domain-specific modpack management types
//! - [`application`] - CLI interface and configuration management
//!
//! ## Quick Start
//!
//! ```no_run
//! # tokio_test::block_on(async {
//! // Initialize and run empack
//! empack_lib::main().await.unwrap();
//! # })
//! ```

pub mod api;
pub mod application;
pub mod display;
pub mod empack;
pub mod logger;
pub mod networking;
pub mod platform;
pub mod primitives;
pub mod terminal;

pub mod testing;

pub use api::{DependencyGraph, DependencyGraphError, DependencyNode};
pub use application::{AppConfig, Cli, Commands, execute_command};
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

pub async fn run_with_config(config: CliConfig) -> Result<()> {
    let workdir = config.app_config.workdir.clone();
    run_main_loop(workdir, execute_command(config)).await
}

pub async fn run_main_loop<F>(workdir: Option<std::path::PathBuf>, command: F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    // Recover cursor from prior crashed runs
    terminal::cursor::force_show_cursor();
    terminal::cursor::install_panic_hook();

    // Run command with signal handling
    tokio::select! {
        biased;
        result = command => {
            terminal::cursor::force_show_cursor();
            logger::global_shutdown();
            result
        }
        _ = tokio::signal::ctrl_c() => {
            terminal::cursor::force_show_cursor();
            logger::global_shutdown();

            // Best-effort state marker cleanup using configured workdir
            let marker_dir = workdir.or_else(|| std::env::current_dir().ok());
            if let Some(dir) = &marker_dir {
                let marker = dir.join(empack::state::STATE_MARKER_FILE);
                let _ = std::fs::remove_file(marker);
            }

            std::process::exit(130)
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

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

    pub fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
}
