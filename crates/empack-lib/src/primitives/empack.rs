use serde::{Deserialize, Serialize};
use std::fmt;

/// Project types for empack dependency specifications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    /// Mod project
    Mod,
    /// Datapack project
    Datapack,
    /// Resource pack project
    ResourcePack,
    /// Shader project
    Shader,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModpackState {
    /// No empack.yml or pack/ directory exists
    Uninitialized,
    /// empack.yml exists, pack/ may be initialized
    Configured,
    /// Built artifacts exist in .empack/dist/
    Built,
}

impl fmt::Display for ModpackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModpackState::Uninitialized => write!(f, "uninitialized"),
            ModpackState::Configured => write!(f, "configured"),
            ModpackState::Built => write!(f, "built"),
        }
    }
}

/// State transition operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateTransition {
    /// Initialize: Uninitialized -> Configured
    Initialize,
    /// Sync: Configured -> Configured (reconcile configs)
    Synchronize,
    /// Build: Configured -> Built
    Build(Vec<BuildTarget>),
    /// Clean: Built -> Configured
    Clean,
}

impl fmt::Display for StateTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateTransition::Initialize => write!(f, "initialize"),
            StateTransition::Synchronize => write!(f, "synchronize"),
            StateTransition::Build(targets) => {
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
