use crate::primitives::ConfigError;
use clap::{Parser, Subcommand};

use super::config::AppConfig;

/// empack CLI - Minecraft modpack management
#[derive(Debug, Clone, Parser)]
#[command(name = "empack")]
#[command(about = "A smarter Minecraft modpack manager")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Global configuration options
    #[command(flatten)]
    pub config: AppConfig,

    /// empack commands
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Configuration loaded from CLI
pub struct CliConfig {
    pub app_config: AppConfig,
    pub command: Option<Commands>,
}

impl CliConfig {
    /// Load configuration from command line arguments
    pub fn load() -> Result<Self, ConfigError> {
        let cli = Cli::parse();
        Ok(Self {
            app_config: cli.config,
            command: cli.command,
        })
    }
}

/// Available empack commands
#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Check tool dependencies and show setup guidance
    Requirements,

    /// Show version information
    Version,

    /// Initialize modpack development environment
    Init {
        /// Modpack name
        #[arg(help = "Name of the modpack to initialize")]
        name: Option<String>,

        /// Force overwrite existing files
        #[arg(short, long, help = "Force overwrite existing modpack files")]
        force: bool,
    },

    /// Synchronize empack.yml dependencies with pack.toml reality
    Sync {
        /// Dry run - show what would be changed without applying
        #[arg(long, help = "Show planned changes without applying them")]
        dry_run: bool,
    },

    /// Build modpack targets
    Build {
        /// Build targets to execute
        #[arg(help = "Build targets: mrpack, client, server, client-full, server-full, all")]
        targets: Vec<String>,

        /// Clean before building
        #[arg(short, long, help = "Clean build directories before building")]
        clean: bool,

        /// Parallel build processes
        #[arg(short = 'j', long, help = "Number of parallel build processes")]
        jobs: Option<usize>,
    },

    /// Add projects to the modpack
    Add {
        /// Mod names, URLs, or project IDs to add
        #[arg(help = "Mod names, URLs, or project IDs")]
        mods: Vec<String>,

        /// Force add even if conflicts exist
        #[arg(short, long, help = "Force add projects even if version conflicts exist")]
        force: bool,

        /// Search platform preference
        #[arg(long, value_enum, help = "Preferred platform for project resolution")]
        platform: Option<SearchPlatform>,
    },

    /// Remove projects from the modpack
    Remove {
        /// Mod names to remove
        #[arg(help = "Mod names to remove")]
        mods: Vec<String>,

        /// Remove dependencies as well
        #[arg(
            short,
            long,
            help = "Also remove dependencies that are no longer needed"
        )]
        deps: bool,
    },

    /// Clean build directories
    Clean {
        /// What to clean
        #[arg(help = "What to clean: builds, cache, all")]
        targets: Vec<String>,
    },
}

/// Search platform preference for project resolution
#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum SearchPlatform {
    /// Prefer Modrinth
    Modrinth,
    /// Prefer CurseForge
    Curseforge,
    /// Search both platforms
    Both,
}

impl std::str::FromStr for SearchPlatform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "modrinth" => Ok(SearchPlatform::Modrinth),
            "curseforge" => Ok(SearchPlatform::Curseforge),
            "both" => Ok(SearchPlatform::Both),
            _ => Err(format!("Invalid search platform: {}", s)),
        }
    }
}

impl Commands {
    /// Check if command requires an initialized modpack directory
    pub fn requires_modpack(&self) -> bool {
        match self {
            Commands::Requirements => false,
            Commands::Version => false,
            Commands::Init { .. } => false,
            Commands::Sync { .. } => true,
            Commands::Build { .. } => true,
            Commands::Add { .. } => true,
            Commands::Remove { .. } => true,
            Commands::Clean { .. } => true,
        }
    }

    /// Get execution order for command (matches V1's command registry)
    pub fn execution_order(&self) -> u8 {
        match self {
            Commands::Requirements => 0,
            Commands::Version => 0,
            Commands::Init { .. } => 1,
            Commands::Clean { .. } => 2,
            Commands::Sync { .. } => 5,
            Commands::Add { .. } => 6,
            Commands::Remove { .. } => 7,
            Commands::Build { .. } => 10,
        }
    }
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            config: AppConfig::default(),
            command: None,
        }
    }
}
