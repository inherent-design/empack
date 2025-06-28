//! Application layer modules
//!
//! Organizes CLI interface, configuration management, and application initialization.

pub mod cli;
pub mod commands;
pub mod config;
pub mod env;
pub mod loader;

// Re-export main types for convenience
pub use cli::{Cli, CliConfig, Commands};
pub use commands::execute_command;
pub use config::AppConfig;
pub use loader::*;
