use serde::{Deserialize, Serialize};
use std::fmt;

/// Project types for empack dependency specifications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Mod,
    /// Datapack project
    Datapack,
    /// Resource pack project
    ResourcePack,
    /// Shader project
    Shader,
}

impl ProjectType {
    /// Whether this project type uses the pack's modloader as a search facet.
    /// Only mods are loader-specific; resource packs and shaders have their
    /// own loader taxonomies on Modrinth (e.g. "minecraft" for resource packs,
    /// "iris"/"optifine" for shaders).
    pub fn uses_loader_facet(&self) -> bool {
        matches!(self, ProjectType::Mod)
    }

    /// Modrinth project_type facet value.
    pub fn modrinth_facet_name(&self) -> &'static str {
        match self {
            ProjectType::Mod => "mod",
            ProjectType::ResourcePack => "resourcepack",
            ProjectType::Shader => "shader",
            ProjectType::Datapack => "datapack",
        }
    }

    /// CurseForge classId for this project type.
    ///
    /// Shaders fall back to classId 6 (Mods) because most CurseForge shader
    /// packs are distributed as mods. CurseForge does have classId 6552 for
    /// shaders but it is unverified against the live API.
    pub fn curseforge_class_id(&self) -> u32 {
        match self {
            ProjectType::Mod => 6,
            ProjectType::ResourcePack => 12,
            ProjectType::Shader => 6,
            ProjectType::Datapack => 17,
        }
    }
}

/// Build targets for empack distribution formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildTarget {
    /// Build a Modrinth-compatible '.mrpack' file
    Mrpack,
    /// Build a bootstrapped client installer
    Client,
    /// Build a bootstrapped server installer
    Server,
    /// Build a non-redistributable client (embeds content)
    ClientFull,
    /// Build a non-redistributable server (embeds content)
    ServerFull,
}

impl fmt::Display for BuildTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildTarget::Mrpack => write!(f, "mrpack"),
            BuildTarget::Client => write!(f, "client"),
            BuildTarget::Server => write!(f, "server"),
            BuildTarget::ClientFull => write!(f, "client-full"),
            BuildTarget::ServerFull => write!(f, "server-full"),
        }
    }
}

impl std::str::FromStr for BuildTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mrpack" => Ok(BuildTarget::Mrpack),
            "client" => Ok(BuildTarget::Client),
            "server" => Ok(BuildTarget::Server),
            "client-full" => Ok(BuildTarget::ClientFull),
            "server-full" => Ok(BuildTarget::ServerFull),
            _ => Err(format!("Invalid build target: {}", s)),
        }
    }
}

/// Filesystem state machine states for modpack development
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackState {
    /// No valid empack modpack layout is present
    Uninitialized,
    /// Both empack.yml and pack/pack.toml exist
    Configured,
    /// Built artifacts exist in the project-local dist/ artifact root
    Built,
    /// Currently building
    Building,
    /// Currently cleaning
    Cleaning,
    /// A previous Building or Cleaning operation was interrupted
    Interrupted { was: Box<PackState> },
}

impl fmt::Display for PackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackState::Uninitialized => write!(f, "uninitialized"),
            PackState::Configured => write!(f, "configured"),
            PackState::Built => write!(f, "built"),
            PackState::Building => write!(f, "building"),
            PackState::Cleaning => write!(f, "cleaning"),
            PackState::Interrupted { was } => write!(f, "interrupted (was: {})", was),
        }
    }
}

/// Pure identity of a state transition, without execution payload.
/// Used by the validation whitelist to determine if a transition is legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionKind {
    /// Uninitialized -> Configured
    Initialize,
    /// Configured -> Configured (refresh metadata)
    RefreshIndex,
    /// Configured/Built -> Built (full build)
    Build,
    /// Non-destructive build-artifact cleanup; idempotent when nothing is built
    Clean,
}

impl fmt::Display for TransitionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionKind::Initialize => write!(f, "initialize"),
            TransitionKind::RefreshIndex => write!(f, "refresh-index"),
            TransitionKind::Build => write!(f, "build"),
            TransitionKind::Clean => write!(f, "clean"),
        }
    }
}

/// Internal marker transitions for intermediate build/clean states.
/// These are `pub(crate)` because marker transitions should only be
/// initiated from within the crate (by build/clean pipelines), never
/// by external consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerKind {
    /// Configured/Built -> Building (intermediate marker state)
    Building,
    /// Built -> Cleaning (intermediate marker state)
    Cleaning,
}

impl fmt::Display for MarkerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarkerKind::Building => write!(f, "building"),
            MarkerKind::Cleaning => write!(f, "cleaning"),
        }
    }
}

/// Initialization parameters for packwiz init
#[derive(Debug, Clone, Copy)]
pub struct InitializationConfig<'a> {
    pub name: &'a str,
    pub author: &'a str,
    pub version: &'a str,
    pub modloader: &'a str,
    pub mc_version: &'a str,
    pub loader_version: &'a str,
}

/// State transition operations
#[derive(Debug)]
pub enum StateTransition<'a> {
    /// Initialize: Uninitialized -> Configured
    Initialize(InitializationConfig<'a>),
    /// Refresh packwiz metadata inside an already configured project
    RefreshIndex,
    /// Build: Configured -> Built
    Build(
        Box<crate::empack::builds::BuildOrchestrator<'a>>,
        Vec<BuildTarget>,
    ),
    /// Clean: Built -> Configured
    Clean,
}

impl<'a> StateTransition<'a> {
    /// Extract the pure transition identity, discarding execution payload.
    pub fn kind(&self) -> TransitionKind {
        match self {
            StateTransition::Initialize(_) => TransitionKind::Initialize,
            StateTransition::RefreshIndex => TransitionKind::RefreshIndex,
            StateTransition::Build(_, _) => TransitionKind::Build,
            StateTransition::Clean => TransitionKind::Clean,
        }
    }
}

impl<'a> fmt::Display for StateTransition<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateTransition::Initialize(_) => write!(f, "initialize"),
            StateTransition::RefreshIndex => write!(f, "refresh-index"),
            StateTransition::Build(_, targets) => {
                write!(
                    f,
                    "build [{}]",
                    targets
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            StateTransition::Clean => write!(f, "clean"),
        }
    }
}

/// Execution order for build targets (matches V1's command registry)
impl BuildTarget {
    pub fn execution_order(&self) -> u8 {
        match self {
            BuildTarget::Mrpack => 10,
            BuildTarget::Client => 11,
            BuildTarget::Server => 12,
            BuildTarget::ClientFull => 13,
            BuildTarget::ServerFull => 14,
        }
    }

    /// Expand 'all' meta-target to concrete targets
    pub fn expand_all() -> Vec<BuildTarget> {
        vec![
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::Server,
        ]
    }

    /// Sort targets by execution order
    pub fn sort_by_execution_order(targets: &mut Vec<BuildTarget>) {
        targets.sort_by_key(|t| t.execution_order());
        targets.dedup();
    }
}
