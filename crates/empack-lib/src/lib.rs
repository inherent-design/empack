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

pub mod application;
pub mod empack;
pub mod logger;
pub mod networking;
pub mod platform;
pub mod primitives;
pub mod terminal;

// Re-export commonly used types for convenience
pub use application::{AppConfig, Cli, Commands, execute_command};
pub use logger::Logger;
pub use networking::{NetworkingConfig, NetworkingManager};
pub use platform::SystemResources;
pub use primitives::{
    ConfigError, LogFormat, LogLevel, LogOutput, LoggerError, TerminalCapsDetectIntent,
    TerminalColorCaps,
};
pub use terminal::TerminalCapabilities;

// Private imports for the main function
use anyhow::Result;
use application::CliConfig;

pub async fn main() -> Result<()> {
    // Load CLI configuration
    let config = CliConfig::load()?;

    // Execute the command
    execute_command(config).await
}
