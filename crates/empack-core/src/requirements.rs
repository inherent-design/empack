//! Environment requirements preserve optional choices independently of side selection.
use alloc::string::String;
use core::fmt;

/// Stable, nonempty logical choice label. It is not a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceKey(String);
impl ChoiceKey {
    /// Preserve spelling; reject blank labels and control characters.
    pub fn parse(value: &str) -> Result<Self, RequirementError> {
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(RequirementError::InvalidChoiceKey);
        }
        Ok(Self(value.into()))
    }
    /// Return the logical label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A user-selectable installation choice, independent of its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalChoice {
    /// Stable choice identity.
    pub key: ChoiceKey,
    /// Default only; an explicit user choice takes precedence.
    pub default_enabled: bool,
    /// Description preserved by formats that can express it.
    pub description: Option<String>,
}

/// Whether a file participates in a selected environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    /// Never installed in this environment.
    Unsupported,
    /// Always installed in this environment.
    Required,
    /// Participation follows an explicit choice.
    Optional(OptionalChoice),
}

/// Client and server requirements are independent dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirements {
    /// Client requirement.
    pub client: Requirement,
    /// Server requirement.
    pub server: Requirement,
}

/// Environments with supported content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environments {
    /// Client only.
    Client,
    /// Server only.
    Server,
    /// Both client and server.
    Both,
}

/// Lossless projection for a format with one optional choice for all supported sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniformRequirements<'a> {
    /// Sides on which content participates.
    pub environments: Environments,
    /// Optional choice, retaining its identity, default and description.
    pub choice: Option<&'a OptionalChoice>,
}

/// A requirement cannot be represented without changing its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementError {
    /// No supported environment remains.
    NoEnvironment,
    /// One side is required while the other is optional.
    MixedRequirement,
    /// Optional sides refer to different choices or choice metadata.
    DifferentChoices,
    /// Choice identity is empty or contains control characters.
    InvalidChoiceKey,
}
impl fmt::Display for RequirementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoEnvironment => "File is unsupported on both environments",
            Self::MixedRequirement => {
                "Mixed required/optional environments cannot share one optional choice"
            }
            Self::DifferentChoices => {
                "Different environment choices cannot share one optional choice"
            }
            Self::InvalidChoiceKey => {
                "Optional choice requires a nonempty label without control characters"
            }
        })
    }
}
impl core::error::Error for RequirementError {}

impl Requirements {
    /// Refuse lossy conversion into a format with one shared optional choice.
    pub fn uniform(&self) -> Result<UniformRequirements<'_>, RequirementError> {
        use Requirement::*;
        let (environments, choice) = match (&self.client, &self.server) {
            (Unsupported, Unsupported) => return Err(RequirementError::NoEnvironment),
            (Required, Optional(_)) | (Optional(_), Required) => {
                return Err(RequirementError::MixedRequirement);
            }
            (Optional(a), Optional(b)) if a != b => return Err(RequirementError::DifferentChoices),
            (Optional(choice), Unsupported) => (Environments::Client, Some(choice)),
            (Unsupported, Optional(choice)) => (Environments::Server, Some(choice)),
            (Optional(choice), Optional(_)) => (Environments::Both, Some(choice)),
            (Required, Unsupported) => (Environments::Client, None),
            (Unsupported, Required) => (Environments::Server, None),
            (Required, Required) => (Environments::Both, None),
        };
        Ok(UniformRequirements {
            environments,
            choice,
        })
    }
}
