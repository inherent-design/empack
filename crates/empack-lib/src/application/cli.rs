use crate::primitives::ConfigError;
use clap::{Args, Parser, Subcommand};

use super::config::AppConfig;

/// empack CLI - Minecraft modpack management
#[derive(Debug, Clone, Parser, Default)]
#[command(name = "empack")]
#[command(about = "Minecraft modpack manager")]
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

/// Arguments for the `init` subcommand.
#[derive(Args, Debug, Default, Clone)]
pub struct InitArgs {
    /// Target directory for the modpack project
    #[arg(help = "Directory for the modpack project (created if needed)")]
    pub dir: Option<String>,

    /// Force overwrite existing files
    #[arg(short, long, help = "Force overwrite existing modpack files")]
    pub force: bool,

    /// Mod loader (neoforge, fabric, forge, quilt, none)
    #[arg(
        long,
        short = 'm',
        env = "EMPACK_MODLOADER",
        help = "Mod loader to use (none for vanilla; skips interactive prompt)"
    )]
    pub modloader: Option<String>,

    /// Minecraft version
    #[arg(
        long,
        env = "EMPACK_MC_VERSION",
        help = "Minecraft version (skips interactive prompt)"
    )]
    pub mc_version: Option<String>,

    /// Author name
    #[arg(
        long,
        short = 'A',
        env = "EMPACK_AUTHOR",
        help = "Author name (skips interactive prompt)"
    )]
    pub author: Option<String>,

    /// Modpack display name
    #[arg(
        long,
        short = 'n',
        env = "EMPACK_NAME",
        help = "Modpack display name (default: directory basename)"
    )]
    pub pack_name: Option<String>,

    /// Loader version (e.g., "0.15.0" for Fabric, "21.1.172" for NeoForge)
    #[arg(
        long,
        env = "EMPACK_LOADER_VERSION",
        help = "Loader version (skips interactive prompt)"
    )]
    pub loader_version: Option<String>,

    /// Pack version string (e.g., "1.0.0")
    #[arg(
        long,
        env = "EMPACK_PACK_VERSION",
        help = "Pack version (skips interactive prompt)"
    )]
    pub pack_version: Option<String>,

    /// Folder for datapacks relative to pack root
    #[arg(long, env = "EMPACK_DATAPACK_FOLDER")]
    pub datapack_folder: Option<String>,

    /// Additional accepted MC versions (comma-separated)
    #[arg(long, env = "EMPACK_GAME_VERSIONS", value_delimiter = ',')]
    pub game_versions: Option<Vec<String>>,

    /// Import modpack from a source (file path or URL)
    #[arg(long = "from", value_name = "SOURCE")]
    pub from_source: Option<String>,
}

/// Arguments for the `build` subcommand.
#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// Build targets to execute
    #[arg(help = "Build targets: mrpack, client, server, client-full, server-full, all")]
    pub targets: Vec<String>,

    /// Clean before building
    #[arg(short, long, help = "Clean build directories before building")]
    pub clean: bool,

    /// Archive format for distribution packages
    #[arg(long, value_enum, default_value = "zip")]
    pub format: CliArchiveFormat,

    /// Directory to scan for manually downloaded restricted mods
    #[arg(long, env = "EMPACK_DOWNLOADS_DIR")]
    pub downloads_dir: Option<String>,
}

impl Default for BuildArgs {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        }
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
    Init(InitArgs),

    /// Synchronize empack.yml dependencies with pack.toml reality
    Sync {},

    /// Build modpack targets
    Build(BuildArgs),

    /// Add projects to the modpack
    Add {
        /// Mod names, URLs, or project IDs to add
        #[arg(help = "Mod names, URLs, or project IDs")]
        mods: Vec<String>,

        /// Force add even if conflicts exist
        #[arg(
            short,
            long,
            help = "Force add projects even if version conflicts exist"
        )]
        force: bool,

        /// Search platform preference
        #[arg(long, value_enum, help = "Preferred platform for project resolution")]
        platform: Option<SearchPlatform>,

        /// Project type to search for (skips tiered search when specified)
        #[arg(long = "type", value_enum)]
        project_type: Option<CliProjectType>,

        /// Pin a specific Modrinth version ID (skips version selection)
        #[arg(long, value_name = "ID")]
        version_id: Option<String>,

        /// Pin a specific CurseForge file ID (skips version selection)
        #[arg(long, value_name = "ID")]
        file_id: Option<String>,
    },

    /// Remove projects from the modpack
    #[command(alias = "rm")]
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

/// Archive format for distribution packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CliArchiveFormat {
    Zip,
    #[value(name = "tar.gz")]
    TarGz,
    #[value(name = "7z")]
    SevenZ,
}

impl CliArchiveFormat {
    pub fn to_archive_format(&self) -> crate::empack::archive::ArchiveFormat {
        match self {
            CliArchiveFormat::Zip => crate::empack::archive::ArchiveFormat::Zip,
            CliArchiveFormat::TarGz => crate::empack::archive::ArchiveFormat::TarGz,
            CliArchiveFormat::SevenZ => crate::empack::archive::ArchiveFormat::SevenZ,
        }
    }
}

/// Project type filter for the add command.
///
/// When specified, skips tiered type guessing and searches for the given
/// project type directly.
#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum CliProjectType {
    Mod,
    #[value(name = "datapack")]
    Datapack,
    #[value(name = "resourcepack")]
    ResourcePack,
    Shader,
}

impl CliProjectType {
    pub fn to_project_type(&self) -> crate::primitives::ProjectType {
        match self {
            CliProjectType::Mod => crate::primitives::ProjectType::Mod,
            CliProjectType::Datapack => crate::primitives::ProjectType::Datapack,
            CliProjectType::ResourcePack => crate::primitives::ProjectType::ResourcePack,
            CliProjectType::Shader => crate::primitives::ProjectType::Shader,
        }
    }
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
            Commands::Init(..) => false,
            Commands::Sync { .. } => true,
            Commands::Build(..) => true,
            Commands::Add { .. } => true,
            Commands::Remove { .. } => true,
            Commands::Clean { .. } => true,
        }
    }

    /// Get execution order for command
    pub fn execution_order(&self) -> u8 {
        match self {
            Commands::Requirements => 0,
            Commands::Version => 0,
            Commands::Init(..) => 1,
            Commands::Clean { .. } => 2,
            Commands::Sync { .. } => 5,
            Commands::Add { .. } => 6,
            Commands::Remove { .. } => 7,
            Commands::Build(..) => 10,
        }
    }
}
