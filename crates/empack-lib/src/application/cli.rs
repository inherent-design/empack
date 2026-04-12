use crate::primitives::ConfigError;
use clap::{Args, Parser, Subcommand};
use std::ffi::OsString;

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

pub enum CliLoad {
    Ready(CliConfig),
    Display(String),
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

    /// Load CLI configuration for process entrypoints without letting clap exit
    /// the process directly.
    pub fn load_for_process() -> Result<CliLoad, ConfigError> {
        Self::load_for_process_from(std::env::args_os())
    }

    /// Load explicit command line arguments for process entrypoints.
    pub fn load_for_process_from<I, T>(args: I) -> Result<CliLoad, ConfigError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Cli::try_parse_from(args) {
            Ok(cli) => Ok(CliLoad::Ready(Self {
                app_config: cli.config,
                command: cli.command,
            })),
            Err(error) => match error.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    Ok(CliLoad::Display(error.to_string()))
                }
                _ => Err(ConfigError::ParseError {
                    value: "command line".to_string(),
                    reason: error.to_string(),
                }),
            },
        }
    }

    /// Load configuration from explicit command line arguments.
    pub fn load_from<I, T>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let cli = Cli::try_parse_from(args).map_err(|e| ConfigError::ParseError {
            value: "command line".to_string(),
            reason: e.to_string(),
        })?;
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
    #[arg(
        help = "Build targets: mrpack, client, server, client-full, server-full, all",
        conflicts_with = "continue_build"
    )]
    pub targets: Vec<String>,

    /// Continue a previously blocked restricted-mod build
    #[arg(
        long = "continue",
        help = "Continue a pending restricted-mod build",
        conflicts_with = "clean"
    )]
    pub continue_build: bool,

    /// Clean before building
    #[arg(short, long, help = "Clean build directories before building")]
    pub clean: bool,

    /// Archive format for distribution packages
    #[arg(
        long,
        value_enum,
        default_value = "zip",
        conflicts_with = "continue_build"
    )]
    pub format: CliArchiveFormat,

    /// Directory to scan for manually downloaded restricted mods
    #[arg(long, env = "EMPACK_DOWNLOADS_DIR")]
    pub downloads_dir: Option<String>,
}

impl Default for BuildArgs {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            continue_build: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empack::archive::ArchiveFormat;
    use crate::primitives::ProjectType;
    use clap::CommandFactory;
    use std::str::FromStr;

    #[test]
    fn cli_archive_format_to_archive_format_maps_variants() {
        assert_eq!(
            CliArchiveFormat::Zip.to_archive_format(),
            ArchiveFormat::Zip
        );
        assert_eq!(
            CliArchiveFormat::TarGz.to_archive_format(),
            ArchiveFormat::TarGz
        );
        assert_eq!(
            CliArchiveFormat::SevenZ.to_archive_format(),
            ArchiveFormat::SevenZ
        );
    }

    #[test]
    fn cli_project_type_to_project_type_maps_variants() {
        assert_eq!(CliProjectType::Mod.to_project_type(), ProjectType::Mod);
        assert_eq!(
            CliProjectType::Datapack.to_project_type(),
            ProjectType::Datapack
        );
        assert_eq!(
            CliProjectType::ResourcePack.to_project_type(),
            ProjectType::ResourcePack
        );
        assert_eq!(
            CliProjectType::Shader.to_project_type(),
            ProjectType::Shader
        );
    }

    #[test]
    fn search_platform_from_str_supports_known_aliases() {
        assert_eq!(
            SearchPlatform::from_str("modrinth").unwrap(),
            SearchPlatform::Modrinth
        );
        assert_eq!(
            SearchPlatform::from_str("curseforge").unwrap(),
            SearchPlatform::Curseforge
        );
        assert_eq!(
            SearchPlatform::from_str("both").unwrap(),
            SearchPlatform::Both
        );
        assert!(SearchPlatform::from_str("unknown").is_err());
    }

    #[test]
    fn commands_surface_metadata_matches_expected_values() {
        assert!(!Commands::Requirements.requires_modpack());
        assert!(!Commands::Version.requires_modpack());
        assert!(Commands::Sync {}.requires_modpack());
        assert!(Commands::Build(BuildArgs::default()).requires_modpack());
        assert_eq!(Commands::Requirements.execution_order(), 0);
        assert_eq!(Commands::Version.execution_order(), 0);
        assert_eq!(Commands::Init(InitArgs::default()).execution_order(), 1);
        assert_eq!(Commands::Clean { targets: vec![] }.execution_order(), 2);
        assert_eq!(Commands::Sync {}.execution_order(), 5);
        assert_eq!(
            Commands::Add {
                mods: vec![],
                force: false,
                platform: None,
                project_type: None,
                version_id: None,
                file_id: None,
            }
            .execution_order(),
            6
        );
        assert_eq!(
            Commands::Remove {
                mods: vec![],
                deps: false
            }
            .execution_order(),
            7
        );
        assert_eq!(Commands::Build(BuildArgs::default()).execution_order(), 10);
    }

    #[test]
    fn build_args_default_matches_expected_defaults() {
        let args = BuildArgs::default();
        assert!(args.targets.is_empty());
        assert!(!args.continue_build);
        assert!(!args.clean);
        assert_eq!(args.format, CliArchiveFormat::Zip);
        assert_eq!(args.downloads_dir, None);
    }

    #[test]
    fn cli_config_load_from_parses_arguments_and_config() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        crate::display::test_utils::clean_test_env();
        let _cli_env = crate::test_support::isolate_cli_env();

        let config = CliConfig::load_from([
            "empack",
            "--color",
            "always",
            "--log-level",
            "4",
            "--net-timeout",
            "45",
            "-j",
            "8",
            "--yes",
            "--dry-run",
            "init",
            "--force",
        ])
        .expect("parse cli config");

        assert_eq!(
            config.app_config.color,
            crate::primitives::TerminalCapsDetectIntent::Always
        );
        assert_eq!(config.app_config.log_level, 4);
        assert_eq!(config.app_config.net_timeout, 45);
        assert_eq!(config.app_config.cpu_jobs, 8);
        assert!(config.app_config.yes);
        assert!(config.app_config.dry_run);
        assert!(matches!(config.command, Some(Commands::Init(_))));
    }

    #[test]
    fn cli_load_for_process_from_returns_display_for_help() {
        let result = CliConfig::load_for_process_from(["empack", "--help"])
            .expect("help should not be treated as a parse failure");

        match result {
            CliLoad::Display(message) => {
                assert!(message.contains("Minecraft modpack manager"));
                assert!(message.contains("Usage:"));
            }
            CliLoad::Ready(_) => panic!("help should return a display payload"),
        }
    }

    #[test]
    fn cli_load_for_process_from_returns_parse_error_for_invalid_args() {
        let result = CliConfig::load_for_process_from(["empack", "--definitely-invalid-flag"]);

        match result {
            Err(ConfigError::ParseError { reason, .. }) => {
                assert!(reason.contains("--definitely-invalid-flag"));
            }
            Ok(CliLoad::Display(_)) => panic!("invalid args should not be treated as help/version"),
            Ok(CliLoad::Ready(_)) => panic!("invalid args should not parse successfully"),
            Err(other) => panic!("unexpected error type: {other}"),
        }
    }

    #[test]
    fn cli_command_graph_is_structurally_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_command_graph_exposes_build_continue_contract() {
        let command = Cli::command();
        let build = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "build")
            .expect("build subcommand");

        let continue_arg = build
            .get_arguments()
            .find(|arg| arg.get_id().as_str() == "continue_build")
            .expect("build --continue arg");
        assert_eq!(continue_arg.get_long(), Some("continue"));

        let downloads_arg = build
            .get_arguments()
            .find(|arg| arg.get_id().as_str() == "downloads_dir")
            .expect("build --downloads-dir arg");
        assert_eq!(downloads_arg.get_long(), Some("downloads-dir"));
        assert_eq!(
            downloads_arg
                .get_env()
                .map(|value| value.to_string_lossy().to_string())
                .as_deref(),
            Some("EMPACK_DOWNLOADS_DIR")
        );
    }

    #[test]
    fn cli_command_graph_exposes_remove_alias() {
        let command = Cli::command();
        let remove = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "remove")
            .expect("remove subcommand");

        assert!(
            remove.get_all_aliases().any(|alias| alias == "rm"),
            "remove command should expose rm alias"
        );
    }

    #[test]
    fn cli_config_load_from_rejects_build_continue_with_targets() {
        let result = CliConfig::load_from(["empack", "build", "--continue", "client-full"]);

        let err = match result {
            Ok(_) => panic!("continue build with targets should fail at parse time"),
            Err(err) => err,
        };
        let rendered = err.to_string();
        assert!(rendered.contains("cannot be used with"));
        assert!(rendered.contains("--continue"));
    }

    #[test]
    fn cli_config_load_from_rejects_build_continue_with_clean() {
        let result = CliConfig::load_from(["empack", "build", "--continue", "--clean"]);

        let err = match result {
            Ok(_) => panic!("continue build with clean should fail at parse time"),
            Err(err) => err,
        };
        let rendered = err.to_string();
        assert!(rendered.contains("cannot be used with"));
        assert!(rendered.contains("--clean"));
    }

    #[test]
    fn cli_config_load_from_rejects_build_continue_with_format() {
        let result = CliConfig::load_from(["empack", "build", "--continue", "--format", "tar.gz"]);

        let err = match result {
            Ok(_) => panic!("continue build with format should fail at parse time"),
            Err(err) => err,
        };
        let rendered = err.to_string();
        assert!(rendered.contains("cannot be used with"));
        assert!(rendered.contains("--format"));
    }

    #[test]
    fn cli_config_load_from_parses_build_continue_with_downloads_dir() {
        let config = CliConfig::load_from([
            "empack",
            "build",
            "--continue",
            "--downloads-dir",
            "/tmp/downloads",
        ])
        .expect("parse continue build");

        let Some(Commands::Build(args)) = config.command else {
            panic!("expected build command");
        };

        assert!(args.continue_build);
        assert!(args.targets.is_empty());
        assert_eq!(args.downloads_dir.as_deref(), Some("/tmp/downloads"));
    }

    #[test]
    fn cli_config_load_from_supports_remove_alias() {
        let config = CliConfig::load_from(["empack", "rm", "sodium"]).expect("parse remove alias");

        let Some(Commands::Remove { mods, deps }) = config.command else {
            panic!("expected remove command");
        };

        assert_eq!(mods, vec!["sodium"]);
        assert!(!deps);
    }

    #[test]
    fn cli_config_load_from_reads_build_downloads_dir_from_env() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        crate::display::test_utils::clean_test_env();
        let _cli_env = crate::test_support::isolate_cli_env();
        unsafe {
            std::env::set_var("EMPACK_DOWNLOADS_DIR", "/tmp/from-env");
        }

        let config = CliConfig::load_from(["empack", "build", "client-full"]).expect("parse build");

        let Some(Commands::Build(args)) = config.command else {
            panic!("expected build command");
        };

        assert_eq!(args.targets, vec!["client-full"]);
        assert_eq!(args.downloads_dir.as_deref(), Some("/tmp/from-env"));
    }
}
