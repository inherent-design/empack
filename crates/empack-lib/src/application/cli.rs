use crate::primitives::ConfigError;
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

use super::config::AppConfig;

/// Explicit source decisions for adopting previously untracked installed content.
#[derive(Debug, Clone, Args, Default)]
pub struct AdoptionSourceArgs {
    /// Provider for a project selector or supplied-file identification
    #[arg(long, requires = "from", conflicts_with = "dependencies")]
    pub platform: Option<SearchPlatform>,
    /// Kind of the selected installed content
    #[arg(long = "type", requires = "from", conflicts_with = "dependencies")]
    pub project_type: Option<CliProjectType>,
    /// Exact Modrinth selection; adoption never requests latest
    #[arg(long, requires = "from", conflicts_with_all = ["dependencies", "file_id"])]
    pub version_id: Option<String>,
    /// Exact CurseForge selection; adoption never requests latest
    #[arg(long, requires = "from", conflicts_with_all = ["dependencies", "version_id"])]
    pub file_id: Option<String>,
    /// Explicit provider file roles, placements and environment requirements
    #[arg(long, requires = "from", conflicts_with = "dependencies")]
    pub file_plan: Option<PathBuf>,
}

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
    Ready(Box<CliConfig>),
    Display(String),
}

impl CliConfig {
    /// Load configuration from command line arguments
    pub fn load() -> Result<Self, ConfigError> {
        let dotenv = super::loader::load_dotenv_files();
        let cli = Cli::parse();
        dotenv?;
        Ok(Self {
            app_config: cli.config,
            command: cli.command,
        })
    }

    /// Load CLI configuration for process entrypoints without letting clap exit
    /// the process directly.
    pub fn load_for_process() -> Result<CliLoad, ConfigError> {
        let args: Vec<_> = std::env::args_os().collect();
        if let Ok(display @ CliLoad::Display(_)) = Self::load_for_process_from(&args) {
            return Ok(display);
        }
        super::loader::load_dotenv_files()?;
        Self::load_for_process_from(args)
    }

    /// Load explicit command line arguments for process entrypoints.
    pub fn load_for_process_from<I, T>(args: I) -> Result<CliLoad, ConfigError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Cli::try_parse_from(args) {
            Ok(cli) => Ok(CliLoad::Ready(Box::new(Self {
                app_config: cli.config,
                command: cli.command,
            }))),
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
#[command(group(clap::ArgGroup::new("import_origin").args(["from_source", "continue_import"]).multiple(false)))]
pub struct InitArgs {
    /// Target directory for the modpack project
    #[arg(help = "Directory for the modpack project (created if needed)")]
    pub dir: Option<String>,

    /// Force overwrite existing files
    #[arg(
        short,
        long,
        help = "Replace existing managed project content after preparation succeeds"
    )]
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

    /// Parent folder for interpreted world directories, relative to the pack root
    #[arg(long, env = "EMPACK_WORLD_FOLDER")]
    pub world_folder: Option<String>,

    /// Additional accepted MC versions (comma-separated)
    #[arg(long, env = "EMPACK_GAME_VERSIONS", value_delimiter = ',')]
    pub game_versions: Option<Vec<String>>,

    /// Import modpack from a source (file path or URL)
    #[arg(long = "from", value_name = "SOURCE")]
    pub from_source: Option<String>,

    /// Resume the saved source archive and verified associations for this destination.
    #[arg(long = "continue", conflicts_with = "from_source")]
    pub continue_import: bool,

    /// Default for optional imported files; participation remains optional.
    #[arg(long, requires = "import_origin", value_name = "BOOL")]
    pub import_optional_default: Option<bool>,

    /// Explicitly exclude archive members outside recognized content namespaces.
    #[arg(long, requires = "import_origin")]
    pub exclude_auxiliary: bool,

    /// Retain imported downloads as local files instead of durable URL references.
    #[arg(long, requires = "import_origin")]
    pub import_local_files: bool,

    /// Associate a selected file with one exact imported download obligation.
    #[arg(
        long = "import-file",
        requires = "import_origin",
        value_name = "SELECTOR=PATH"
    )]
    pub import_files: Vec<String>,
}

/// Arguments for the `build` subcommand.
#[derive(Args, Debug, Clone, Default)]
#[command(after_help = "Consumers:
  modrinth    Modrinth archive with exact references and overrides
  curseforge  CurseForge client ZIP with exact references and overrides
  prism       Launcher instance; bundled pack content by default
  server      Prepared server runtime; bundled pack content by default
  empack      Exact native release and immutable assets

Without CONSUMERS, use distribution.recipes from empack.yml.
Use --delivery references for a Prism/server snapshot installed by empack.
Reference consumers need distribution.native identity and Java settings.
Bundled content does not include client game binaries or imply offline launch.
Update authority is independent of dependency delivery; snapshots are the default.
All requested outputs are verified before combined publication.

Examples:
  empack build --dry-run modrinth prism
  empack build prism server --delivery references
  empack build --continue")]
pub struct BuildArgs {
    /// Consumers to build with explicit policy overrides
    #[arg(
        help = "Consumers to build (default: distribution.recipes in empack.yml)",
        value_name = "CONSUMERS",
        value_parser = ["modrinth", "curseforge", "prism", "server", "empack"],
        conflicts_with = "continue_build"
    )]
    pub targets: Vec<String>,

    /// How dependency bytes reach the consumer; independent of update authority
    #[arg(long, value_parser = ["references", "bundled"], conflicts_with = "continue_build")]
    pub delivery: Option<String>,

    /// Environment to project; each consumer validates the selected side
    #[arg(long, value_parser = ["client", "server", "both"], conflicts_with = "continue_build")]
    pub environment: Option<String>,

    /// Who may select future releases; requires a bound platform or publisher association
    #[arg(long, value_parser = ["snapshot", "platform", "empack"], conflicts_with = "continue_build")]
    pub updates: Option<String>,

    /// Continue a previously blocked restricted-mod build
    #[arg(
        long = "continue",
        help = "Resume a saved build after supplying missing downloads",
        conflicts_with = "clean"
    )]
    pub continue_build: bool,

    /// Remove obsolete artifacts when the verified build is published
    #[arg(
        short,
        long,
        help = "Remove obsolete artifacts when the verified build is published"
    )]
    pub clean: bool,

    /// Archive format override for distributions; mrpack and curseforge always use ZIP
    #[arg(long, value_enum, conflicts_with = "continue_build")]
    pub format: Option<CliArchiveFormat>,

    /// Directory to scan for manually downloaded restricted mods
    #[arg(long, env = "EMPACK_DOWNLOADS_DIR")]
    pub downloads_dir: Option<String>,

    /// Open verified provider pages for missing files after saving continuation
    #[arg(long)]
    pub open_downloads: bool,

    /// Wait for verified downloads, then present a fresh build plan
    #[arg(long, value_name = "SECONDS", requires = "downloads_dir", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub wait_downloads: Option<u64>,

    /// Explicitly associate a local download with a pending filename
    #[arg(
        long = "associate-download",
        value_name = "FILENAME=PATH",
        requires = "continue_build"
    )]
    pub associate_downloads: Vec<String>,

    /// Use authored defaults for optional files in materialized distributions.
    #[arg(long, conflicts_with = "continue_build")]
    pub optional_defaults: bool,

    /// Decide one optional choice explicitly.
    #[arg(
        long = "optional",
        value_name = "CHOICE=true|false",
        conflicts_with = "continue_build"
    )]
    pub optional_choices: Vec<String>,

    /// Allow reference archives to omit optional choice keys, defaults and descriptions.
    #[arg(long, conflicts_with = "continue_build")]
    pub allow_optional_metadata_loss: bool,
}

/// An explicit user choice is needed before the command can execute.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CommandInputRequired(pub &'static str);

/// Native instance commands are separate from author dependency updates.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct InstanceConflictArgs {
    /// Keep edited bytes as an explicit local deviation (release-relative path)
    #[arg(long = "preserve", value_name = "PATH")]
    pub preserve: Vec<String>,
    /// Replace an exact conflicting file with selected release content, or retire it
    #[arg(long = "replace", value_name = "PATH")]
    pub replace: Vec<String>,
    /// Apply a separately prepared merge result as a local deviation
    #[arg(long = "merge", value_name = "PATH=FILE")]
    pub merge: Vec<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum InstanceCommand {
    /// Resume an exact pending release with verified manual file associations.
    Continue {
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },
    /// Discard a selected pending instance recipe; keep the completed installation.
    DiscardPending,
    /// Clear interrupted runtime evidence only after all remaining processes have stopped
    RecoverRuntime {
        #[arg(long, required = true)]
        acknowledge_stopped: bool,
    },
    /// Run a locally selected runtime while preventing concurrent managed updates
    Launch {
        /// Runtime program and arguments after --; no shell expansion or remote commands
        #[arg(last = true, required = true, num_args = 1..)]
        command: Vec<std::ffi::OsString>,
    },
    /// Apply the exact signed release selected by the saved authenticated channel observation
    Update {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        /// Local signed release envelope; omit to fetch the saved channel's exact release
        release: Option<std::path::PathBuf>,
        #[arg(long, value_parser = ["client", "server"], default_value = "client")]
        side: String,
        #[arg(long, value_parser = ["game", "prism"])]
        layout: Option<String>,
        #[arg(long = "choice", value_name = "KEY=VALUE")]
        choices: Vec<String>,
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },

    /// Enroll an explicit publisher and HTTPS channel; does not install or update content
    Subscribe {
        #[arg(long)]
        pack: String,
        #[arg(long)]
        channel: String,
        url: String,
        /// Trusted Ed25519 public key as 64 lowercase hexadecimal characters
        #[arg(long = "key", required = true)]
        keys: Vec<String>,
    },
    /// Replace enrolled publisher keys locally, preserving the saved anti-replay floor
    Trust {
        #[arg(
            long = "key",
            required_unless_present = "revoke_all",
            conflicts_with = "revoke_all"
        )]
        keys: Vec<String>,
        /// Disable updates without discarding the subscription or sequence floor
        #[arg(long)]
        revoke_all: bool,
    },
    /// Verify a signed channel envelope and save its anti-replay floor without installing content
    ObserveChannel {
        /// Local signed envelope; omit to fetch the enrolled HTTPS channel
        envelope: Option<std::path::PathBuf>,
    },

    /// Inspect choices, or apply explicit alternatives within the installed release
    Options {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        #[arg(long = "choice", value_name = "KEY=VALUE")]
        choices: Vec<String>,
        #[arg(long)]
        assets: Option<std::path::PathBuf>,
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },
    /// Show the completed release, saved choices and retained rollback releases
    Inspect,
    /// Restore the installed release without changing its choices or selecting newer content
    Repair {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        /// Directory containing the retained release's immutable asset paths
        #[arg(long)]
        assets: Option<std::path::PathBuf>,
        /// Associate exact missing content with its release file key
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },
    /// Restore a retained release's managed content while preserving edited user files
    Rollback {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        /// Exact SHA-256 of a retained completed release payload
        release: String,
        #[arg(long)]
        assets: Option<std::path::PathBuf>,
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
        /// Resolve alternatives no longer present in the current saved choices
        #[arg(long = "choice", value_name = "KEY=VALUE")]
        choices: Vec<String>,
    },

    /// Install the initial release or repair the active release without reverting updates
    Prepare {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        /// Immutable JSON release payload, not an author manifest
        release: std::path::PathBuf,
        /// Expected SHA-256 of the exact release payload bytes
        #[arg(long)]
        sha256: String,
        /// Environment to install; an existing instance cannot switch sides
        #[arg(long, value_parser = ["client", "server"], default_value = "client")]
        side: String,
        /// Fixed content directory; retained on updates (new instances default to game)
        #[arg(long, value_parser = ["game", "prism"])]
        layout: Option<String>,
        /// Stable release choice and selected alternative
        #[arg(long = "choice", value_name = "KEY=VALUE")]
        choices: Vec<String>,
        /// Associate exact content with a release file key, including manual downloads
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },

    /// Install an exact local release, preserving seeds and rejecting edited managed files
    Install {
        #[command(flatten)]
        conflicts: InstanceConflictArgs,
        /// Immutable JSON release payload, not an author manifest
        release: std::path::PathBuf,
        /// Expected SHA-256 of the exact release payload bytes
        #[arg(long)]
        sha256: String,
        /// Environment to install; an existing instance cannot switch sides
        #[arg(long, value_parser = ["client", "server"], default_value = "client")]
        side: String,
        /// Fixed content directory; retained on updates (new instances default to game)
        #[arg(long, value_parser = ["game", "prism"])]
        layout: Option<String>,
        /// Stable release choice and selected alternative
        #[arg(long = "choice", value_name = "KEY=VALUE")]
        choices: Vec<String>,
        /// Associate exact content with a release file key, including manual downloads
        #[arg(long = "file", value_name = "KEY=PATH")]
        files: Vec<String>,
    },
}

/// Available empack commands
#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Install exact native release content into a separate game instance
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },

    /// Check tool dependencies and show setup guidance
    Requirements,

    /// Show version information
    Version,

    /// Create a project or import a modpack archive
    Init(InitArgs),

    /// Reconcile installed content with recorded intent and exact selections
    Sync {
        /// Acquire and verify all remote references before publishing the complete batch
        #[arg(long)]
        materialize: bool,
        /// Resume exact saved selections and verified manual downloads
        #[arg(long = "continue", conflicts_with = "materialize")]
        continue_sync: bool,
        /// Associate a local file with the exact dependency/slot printed by sync
        #[arg(
            long = "file",
            value_name = "DEPENDENCY/SLOT=PATH",
            requires = "continue_sync"
        )]
        files: Vec<String>,
    },

    /// Refresh selected installed dependencies while retaining intent and pins
    Update {
        /// Publish verified independent groups and report failures without discarding prior content
        #[arg(long)]
        continue_independent: bool,
        /// Exact logical keys from empack.yml or empack.lock
        #[arg(required = true)]
        dependencies: Vec<String>,
    },

    /// Accept verified installed changes without rewriting payloads
    Adopt {
        /// Exact logical keys already declared or tracked by the project
        #[arg(required_unless_present = "from", conflicts_with = "from")]
        dependencies: Vec<String>,
        /// Describe new installed content using local files, URLs, or provider selectors
        #[arg(long, num_args = 1.., conflicts_with = "dependencies")]
        from: Vec<String>,
        #[command(flatten)]
        selection: AdoptionSourceArgs,
    },

    /// Build verified distribution archives
    Build(BuildArgs),

    /// Add projects to the modpack
    Add {
        /// Publish verified independent groups after resolution; report blocked groups as failure
        #[arg(long)]
        continue_independent: bool,
        /// Mod names, URLs, or project IDs to add
        #[arg(help = "Names, provider URLs/IDs, HTTPS files, or local paths")]
        mods: Vec<String>,

        /// Update an existing selection of the same identity
        #[arg(
            short,
            long,
            help = "Update an existing selection of the same identity"
        )]
        force: bool,

        /// Search platform preference
        #[arg(
            long,
            value_enum,
            help = "Provider for project resolution or supplied-file identification"
        )]
        platform: Option<SearchPlatform>,

        /// Project type to search for (skips tiered search when specified)
        #[arg(long = "type", visible_alias = "project-type", value_enum)]
        project_type: Option<CliProjectType>,

        /// Pin a specific Modrinth version ID (skips version selection)
        #[arg(long, value_name = "ID")]
        version_id: Option<String>,

        /// Pin a specific CurseForge file ID (skips version selection)
        #[arg(long, value_name = "ID")]
        file_id: Option<String>,

        /// YAML file-role destinations and environment choices for one provider project
        #[arg(long, value_name = "PATH")]
        file_plan: Option<PathBuf>,

        /// Track an HTTPS download as local content instead of retaining its URL
        #[arg(long)]
        download_as_local: bool,
    },

    /// Remove projects from the modpack
    #[command(alias = "rm")]
    Remove {
        /// Mod names to remove
        #[arg(help = "Dependency keys, titles, or provider-qualified IDs")]
        mods: Vec<String>,

        /// Remove dependencies as well
        #[arg(
            short,
            long,
            help = "Reserved: automatic orphan cleanup requires complete dependency metadata"
        )]
        deps: bool,

        /// Remove authoring roots while retaining their exact installed selections.
        #[arg(long)]
        forget: bool,

        /// Accept unknown dependents; known requirements still prevent deletion.
        #[arg(long, conflicts_with = "forget")]
        acknowledge_unknown: bool,
    },

    /// Inspect or recover an interrupted engine operation
    Recover {
        #[arg(value_enum, default_value = "inspect")]
        action: CliRecoveryAction,
        /// Require the exact operation reported by inspection
        #[arg(long)]
        operation: Option<String>,
    },

    /// Remove generated builds, caches or saved continuation data
    Clean {
        /// What to clean
        #[arg(
            help = "What to clean: builds, cache, continuation, import, sync, retained, all (builds and cache)"
        )]
        targets: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CliRecoveryAction {
    /// Show interrupted publication and available recovery actions
    Inspect,
    /// Finish publishing the verified candidate
    Finish,
    /// Restore changes owned by the interrupted operation
    Restore,
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
    World,
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
            Commands::Instance { .. } => false,
            Commands::Recover { .. } => false,
            Commands::Requirements => false,
            Commands::Version => false,
            Commands::Init(..) => false,
            Commands::Sync { .. } => true,
            Commands::Build(..) => true,
            Commands::Add { .. } | Commands::Update { .. } | Commands::Adopt { .. } => true,
            Commands::Remove { .. } => true,
            Commands::Clean { .. } => true,
        }
    }

    /// Get execution order for command
    pub fn execution_order(&self) -> u8 {
        match self {
            Commands::Instance { .. } => 1,
            Commands::Recover { .. } => 0,
            Commands::Requirements => 0,
            Commands::Version => 0,
            Commands::Init(..) => 1,
            Commands::Clean { .. } => 2,
            Commands::Sync { .. } => 5,
            Commands::Add { .. } | Commands::Update { .. } | Commands::Adopt { .. } => 6,
            Commands::Remove { .. } => 7,
            Commands::Build(..) => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_launch_preserves_native_argument_boundaries() {
        assert!(Cli::try_parse_from(["empack", "instance", "launch"]).is_err());
        assert!(Cli::try_parse_from(["empack", "instance", "recover-runtime"]).is_err());
        assert!(
            Cli::try_parse_from([
                "empack",
                "instance",
                "recover-runtime",
                "--acknowledge-stopped"
            ])
            .is_ok()
        );
        let cli = Cli::try_parse_from([
            "empack",
            "instance",
            "launch",
            "--",
            "/path with spaces/java",
            "-Xmx2G",
            "literal $value",
        ])
        .unwrap();
        let Some(Commands::Instance {
            command: InstanceCommand::Launch { command },
        }) = cli.command
        else {
            panic!()
        };
        assert_eq!(
            command,
            vec![
                std::ffi::OsString::from("/path with spaces/java"),
                "-Xmx2G".into(),
                "literal $value".into()
            ]
        );
    }

    #[test]
    fn instance_install_requires_a_digest_and_keeps_exact_input_associations() {
        assert!(Cli::try_parse_from(["empack", "instance", "install", "release.json"]).is_err());
        let cli = Cli::try_parse_from([
            "empack",
            "instance",
            "install",
            "release.json",
            "--sha256",
            &"00".repeat(32),
            "--side",
            "server",
            "--choice",
            "extra=yes",
            "--file",
            "one=some=file.jar",
        ])
        .unwrap();
        let Some(Commands::Instance {
            command:
                InstanceCommand::Install {
                    side,
                    choices,
                    files,
                    ..
                },
        }) = cli.command
        else {
            panic!("not instance install")
        };
        assert_eq!(side, "server");
        assert_eq!(choices, ["extra=yes"]);
        assert_eq!(files, ["one=some=file.jar"]);
    }

    use clap::CommandFactory;
    use std::str::FromStr;

    #[test]
    fn help_never_displays_provider_credential_values() {
        let help = Cli::command().render_long_help().to_string();
        if let Some(key) = AppConfig::default().curseforge_api_client_key {
            assert!(!help.contains(&key));
        }
        let command = Cli::command();
        let argument = command
            .get_arguments()
            .find(|arg| arg.get_id() == "curseforge_api_client_key")
            .unwrap();
        assert!(argument.is_hide_default_value_set());
        assert!(argument.is_hide_env_values_set());
    }

    #[test]
    fn sync_continuation_requires_exact_file_association_mode() {
        assert!(Cli::try_parse_from(["empack", "sync", "--file", "one/primary=file"]).is_err());
        assert!(Cli::try_parse_from(["empack", "sync", "--continue", "--materialize"]).is_err());
        let cli = Cli::try_parse_from([
            "empack",
            "sync",
            "--continue",
            "--file",
            "one/primary=some=file",
        ])
        .unwrap();
        let Some(Commands::Sync {
            materialize,
            continue_sync,
            files,
        }) = cli.command
        else {
            panic!("not synchronization");
        };
        assert!(continue_sync);
        assert!(!materialize);
        assert_eq!(files, ["one/primary=some=file"]);
    }

    #[test]
    fn import_file_associations_require_an_archive_and_preserve_paths() {
        assert!(
            Cli::try_parse_from(["empack", "init", "--import-file", "declared:0=file"]).is_err()
        );
        let cli = Cli::try_parse_from([
            "empack",
            "init",
            "--from",
            "pack.mrpack",
            "--import-file",
            "declared:0=some=file.zip",
            "--import-file",
            "declared:1=second.zip",
        ])
        .unwrap();
        let Some(Commands::Init(args)) = cli.command else {
            panic!("not initialization")
        };
        assert_eq!(
            args.import_files,
            ["declared:0=some=file.zip", "declared:1=second.zip"]
        );
    }

    #[test]
    fn import_continuation_accepts_decisions_and_excludes_a_new_source() {
        let cli = Cli::try_parse_from([
            "empack",
            "init",
            "--continue",
            "--import-file",
            "exact=file.jar",
            "--import-optional-default",
            "false",
            "--import-local-files",
            "--exclude-auxiliary",
            "destination",
        ])
        .unwrap();
        let Some(Commands::Init(args)) = cli.command else {
            panic!("not initialization")
        };
        assert!(args.continue_import);
        assert_eq!(args.dir.as_deref(), Some("destination"));
        assert_eq!(args.import_files, ["exact=file.jar"]);
        assert!(
            Cli::try_parse_from(["empack", "init", "--continue", "--from", "archive.zip"]).is_err()
        );
        for flag in ["--import-local-files", "--exclude-auxiliary"] {
            assert!(Cli::try_parse_from(["empack", "init", flag]).is_err());
        }
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
        assert!(
            Commands::Sync {
                materialize: false,
                continue_sync: false,
                files: vec![]
            }
            .requires_modpack()
        );
        assert!(Commands::Build(BuildArgs::default()).requires_modpack());
        assert_eq!(Commands::Requirements.execution_order(), 0);
        assert_eq!(Commands::Version.execution_order(), 0);
        assert_eq!(Commands::Init(InitArgs::default()).execution_order(), 1);
        assert_eq!(Commands::Clean { targets: vec![] }.execution_order(), 2);
        assert_eq!(
            Commands::Sync {
                materialize: false,
                continue_sync: false,
                files: vec![]
            }
            .execution_order(),
            5
        );
        assert_eq!(
            Commands::Add {
                mods: vec![],
                force: false,
                platform: None,
                project_type: None,
                version_id: None,
                file_id: None,
                file_plan: None,
                download_as_local: false,
                continue_independent: false,
            }
            .execution_order(),
            6
        );
        assert_eq!(
            Commands::Remove {
                mods: vec![],
                deps: false,
                forget: false,
                acknowledge_unknown: false,
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
        assert_eq!(args.format, None);
        assert_eq!(args.downloads_dir, None);
    }

    #[test]
    fn build_archive_override_distinguishes_absence_from_explicit_zip() {
        for (arguments, expected) in [
            (vec!["empack", "build", "prism"], None),
            (
                vec!["empack", "build", "prism", "--format", "zip"],
                Some(CliArchiveFormat::Zip),
            ),
            (
                vec!["empack", "build", "prism", "--format", "tar.gz"],
                Some(CliArchiveFormat::TarGz),
            ),
            (
                vec!["empack", "build", "prism", "--format", "7z"],
                Some(CliArchiveFormat::SevenZ),
            ),
        ] {
            let cli = Cli::try_parse_from(arguments).unwrap();
            let Some(Commands::Build(args)) = cli.command else {
                panic!("Expected build command")
            };
            assert_eq!(args.format, expected);
        }
        assert!(Cli::try_parse_from(["empack", "build", "--continue", "--format", "zip"]).is_err());
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
        let result = CliConfig::load_from(["empack", "build", "--continue", "prism"]);

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
    fn explicit_download_association_requires_continuation() {
        assert!(
            CliConfig::load_from([
                "empack",
                "build",
                "--associate-download",
                "mod.jar=chosen.jar"
            ])
            .is_err()
        );
        let parsed = CliConfig::load_from([
            "empack",
            "build",
            "--continue",
            "--associate-download",
            "mod.jar=chosen.jar",
        ])
        .unwrap();
        let Some(Commands::Build(args)) = parsed.command else {
            panic!("expected build");
        };
        assert_eq!(args.associate_downloads, ["mod.jar=chosen.jar"]);
    }

    #[test]
    fn cli_config_load_from_supports_remove_alias() {
        let config = CliConfig::load_from(["empack", "rm", "sodium"]).expect("parse remove alias");

        let Some(Commands::Remove { mods, deps, .. }) = config.command else {
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

        let config = CliConfig::load_from(["empack", "build", "prism"]).expect("parse build");

        let Some(Commands::Build(args)) = config.command else {
            panic!("expected build command");
        };

        assert_eq!(args.targets, vec!["prism"]);
        assert_eq!(args.downloads_dir.as_deref(), Some("/tmp/from-env"));
    }
}

#[cfg(test)]
#[test]
fn provider_file_plan_flag_selects_an_explicit_document() {
    let cli = Cli::try_parse_from([
        "empack",
        "add",
        "--platform",
        "modrinth",
        "renderer",
        "--file-plan",
        "choices.yml",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Some(Commands::Add { file_plan: Some(path), .. }) if path == std::path::Path::new("choices.yml"))
    );
}

#[test]
fn adoption_source_choices_are_explicit_and_cannot_mix_with_tracked_keys() {
    for args in [
        vec!["empack", "adopt", "--from", "project/pack/mods/example.jar"],
        vec![
            "empack",
            "adopt",
            "--from",
            "renderer",
            "--platform",
            "modrinth",
            "--version-id",
            "RootVer1",
            "--file-plan",
            "files.yml",
        ],
        vec!["empack", "adopt", "tracked-key"],
    ] {
        Cli::try_parse_from(args).unwrap();
    }
    for args in [
        vec!["empack", "adopt"],
        vec!["empack", "adopt", "tracked-key", "--from", "other.jar"],
        vec!["empack", "adopt", "tracked-key", "--platform", "modrinth"],
        vec![
            "empack",
            "adopt",
            "--from",
            "renderer",
            "--version-id",
            "RootVer1",
            "--file-id",
            "123",
        ],
    ] {
        assert!(Cli::try_parse_from(&args).is_err(), "accepted {args:?}");
    }
}

#[cfg(test)]
mod consumer_cli_tests {
    use super::*;
    #[test]
    fn consumer_delivery_is_explicit_and_cannot_change_a_continuation() {
        let config = CliConfig::load_from([
            "empack",
            "build",
            "prism",
            "server",
            "--delivery",
            "references",
        ])
        .unwrap();
        let Commands::Build(args) = config.command.unwrap() else {
            panic!("expected build")
        };
        assert_eq!(args.targets, ["prism", "server"]);
        assert_eq!(args.delivery.as_deref(), Some("references"));
        for args in [
            vec!["empack", "build", "client-full"],
            vec!["empack", "build", "--continue", "--delivery", "bundled"],
        ] {
            assert!(CliConfig::load_from(args).is_err());
        }
    }
}
