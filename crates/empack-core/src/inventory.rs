//! Independent expected build content, including environment precedence and optional choices.
use crate::{
    digest::{ContentId, DigestSet},
    files::FilePermissions,
    model::{ContentLayer, DependencyKey, ExpectedContent, FileSlot, NonEmpty},
    path::InstallDestination,
    projection::BuildTarget,
    requirements::{OptionalChoice, Requirement, Requirements},
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::fmt;

/// Why an expected output exists. Identity never comes from an exporter's observed files.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentOwner {
    /// A normalized exact selection and file slot.
    Dependency {
        /// Logical owner.
        key: DependencyKey,
        /// Exact file role.
        slot: FileSlot,
    },
    /// Explicit source, override or template input label.
    Source(String),
    /// A resolved runtime producer obligation.
    Runtime(String),
}
/// How an included file must appear in the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Representation {
    /// An exact input whose bytes are not acquired yet. Selection can eliminate it;
    /// a completed inventory cannot contain this planning-only representation.
    Unacquired {
        /// Original byte assertions, never invented placeholder hashes or URLs.
        expected: ExpectedContent,
    },
    /// Verified bytes must be included with their declared archive permissions.
    Embedded {
        /// Observed address.
        content: ContentId,
        /// Exact byte count.
        bytes: u64,
        /// Output attributes, independent of host filesystem capabilities.
        permissions: FilePermissions,
    },
    /// A reference format must retain the exact byte assertions and approved locators.
    Download {
        /// Expected source/output hashes.
        digests: DigestSet,
        /// Exact reference byte length.
        bytes: u64,
        /// Allowed download alternatives in preference order.
        urls: NonEmpty<String>,
    },
}
/// One input obligation before target/environment projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryInput {
    /// Semantic owner.
    pub owner: ContentOwner,
    /// Relative installation path.
    pub destination: InstallDestination,
    /// Overlay precedence.
    pub layer: ContentLayer,
    /// Independent side requirements.
    pub requirements: Requirements,
    /// Expected bytes or exact download reference.
    pub representation: Representation,
}
/// Optional-file behavior is an explicit build choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionalPolicy {
    /// Keep selectable requirements in reference/bootstrap output.
    Preserve,
    /// Materialize selections; explicit values override declared defaults.
    Resolve {
        /// Values keyed by logical choice label.
        choices: BTreeMap<String, bool>,
        /// Whether unspecified choices may use their declared defaults.
        use_defaults: bool,
    },
}
/// An expected entry after target/environment projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedEntry {
    /// Semantic owner retained for verification diagnostics.
    pub owner: ContentOwner,
    /// Final target-relative path.
    pub destination: InstallDestination,
    /// Mrpack retains layers; standalone outputs flatten the selected view.
    pub layer: ContentLayer,
    /// Mrpack retains both environments; standalone entries describe only the selected side.
    pub requirements: Requirements,
    /// Required target representation.
    pub representation: Representation,
}
/// Deliberate side-layer replacement, recorded instead of silently discarding an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precedence {
    /// Final installation destination.
    pub destination: InstallDestination,
    /// Common-layer owner replaced for this target.
    pub replaced: ContentOwner,
    /// Selected side-layer owner.
    pub replacement: ContentOwner,
}
/// Chosen optional participation with its original choice description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceDecision {
    /// Original choice metadata.
    pub choice: OptionalChoice,
    /// Effective selection.
    pub enabled: bool,
    /// True only when a declared default supplied this selection.
    pub used_default: bool,
}
/// Immutable expected content; it grants no writer or publication authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInventory {
    target: BuildTarget,
    entries: Vec<ProjectedEntry>,
    precedence: Vec<Precedence>,
    choices: Vec<ChoiceDecision>,
}
/// A projection cannot represent declared semantics without new input or acquisition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// Two inputs claim the same destination in one layer.
    DuplicateDestination(String),
    /// One key declares conflicting choice metadata.
    ConflictingChoice(String),
    /// A selected optional item needs a user/default policy.
    ChoiceRequired(String),
    /// A provided choice has no matching declared optional input.
    UnknownChoice(String),
    /// Per-side layering or unsupported-both requirements are invalid.
    InvalidEnvironment(String),
    /// A full distribution needs acquired bytes instead of a download-only reference.
    MaterializationRequired(String),
    /// Reference output must preserve optional semantics instead of baking in local selections.
    ReferenceChoicesMustBePreserved,
    /// A flat selectable override would erase its common-layer fallback.
    OptionalOverlayNeedsSelection(String),
}
impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDestination(path) => {
                write!(f, "Two inputs occupy layer destination {path}")
            }
            Self::ConflictingChoice(key) => {
                write!(f, "Conflicting optional choice metadata: {key}")
            }
            Self::ChoiceRequired(key) => write!(f, "Optional choice needs a selection: {key}"),
            Self::UnknownChoice(key) => write!(f, "Unknown optional choice: {key}"),
            Self::InvalidEnvironment(path) => write!(f, "Invalid layer environment: {path}"),
            Self::MaterializationRequired(path) => {
                write!(f, "Full distribution needs verified bytes: {path}")
            }
            Self::OptionalOverlayNeedsSelection(path) => write!(
                f,
                "Optional side override needs a selection before flattening: {path}"
            ),
            Self::ReferenceChoicesMustBePreserved => {
                f.write_str("Mrpack output preserves optional choices")
            }
        }
    }
}
impl core::error::Error for InventoryError {}
impl BuildInventory {
    /// Plan and require target representations to be complete.
    pub fn project(
        inputs: &[InventoryInput],
        target: BuildTarget,
        policy: &OptionalPolicy,
    ) -> Result<Self, InventoryError> {
        BuildSelection::select(inputs, target, policy)?.finish()
    }
}
/// Side and optional selection before acquisition. It is not a complete output inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSelection(BuildInventory);
impl BuildSelection {
    /// Select surviving obligations before deciding which missing bytes must be acquired.
    pub fn select(
        inputs: &[InventoryInput],
        target: BuildTarget,
        policy: &OptionalPolicy,
    ) -> Result<Self, InventoryError> {
        let mut by_layer = BTreeMap::new();
        let mut declared: BTreeMap<String, &OptionalChoice> = BTreeMap::new();
        for input in inputs {
            let path = input.destination.relative().as_str();
            if by_layer
                .insert((input.layer, input.destination.relative().clone()), input)
                .is_some()
            {
                return Err(InventoryError::DuplicateDestination(path.into()));
            }
            if (input.requirements.client == Requirement::Unsupported
                && input.requirements.server == Requirement::Unsupported)
                || (input.layer == ContentLayer::Client
                    && input.requirements.server != Requirement::Unsupported)
                || (input.layer == ContentLayer::Server
                    && input.requirements.client != Requirement::Unsupported)
            {
                return Err(InventoryError::InvalidEnvironment(path.into()));
            }
            for requirement in [&input.requirements.client, &input.requirements.server] {
                if let Requirement::Optional(choice) = requirement {
                    let key = choice.key.as_str();
                    if let Some(previous) = declared.insert(key.into(), choice)
                        && previous != choice
                    {
                        return Err(InventoryError::ConflictingChoice(key.into()));
                    }
                }
            }
        }
        if let OptionalPolicy::Resolve { choices, .. } = policy {
            for key in choices.keys() {
                if !declared.contains_key(key) {
                    return Err(InventoryError::UnknownChoice(key.clone()));
                }
            }
        }
        if target == BuildTarget::Mrpack {
            if !matches!(policy, OptionalPolicy::Preserve) {
                return Err(InventoryError::ReferenceChoicesMustBePreserved);
            }
            return Ok(Self(BuildInventory {
                target,
                entries: by_layer
                    .values()
                    .map(|input| ProjectedEntry {
                        owner: input.owner.clone(),
                        destination: input.destination.clone(),
                        layer: input.layer,
                        requirements: input.requirements.clone(),
                        representation: input.representation.clone(),
                    })
                    .collect(),
                precedence: Vec::new(),
                choices: Vec::new(),
            }));
        }
        let client = matches!(target, BuildTarget::Client | BuildTarget::ClientFull);
        let full = matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull);
        let side = if client {
            ContentLayer::Client
        } else {
            ContentLayer::Server
        };
        let mut selected: BTreeMap<_, ProjectedEntry> = BTreeMap::new();
        let mut precedence = Vec::new();
        let mut choices: BTreeMap<String, ChoiceDecision> = BTreeMap::new();
        let paths: BTreeSet<_> = by_layer.keys().map(|(_, path)| path.clone()).collect();
        for path in paths {
            let common = by_layer.get(&(ContentLayer::Common, path.clone())).copied();
            let specific = by_layer.get(&(side, path.clone())).copied();
            let side_requirement = specific.map(|input| {
                if client {
                    &input.requirements.client
                } else {
                    &input.requirements.server
                }
            });
            let common_requirement = common.map(|input| {
                if client {
                    &input.requirements.client
                } else {
                    &input.requirements.server
                }
            });
            if matches!(side_requirement, Some(Requirement::Optional(_)))
                && matches!(policy, OptionalPolicy::Preserve)
                && common_requirement.is_some_and(|value| *value != Requirement::Unsupported)
            {
                return Err(InventoryError::OptionalOverlayNeedsSelection(
                    path.as_str().into(),
                ));
            }
            let selected_side = side_requirement
                .map(|requirement| select_requirement(requirement, policy, full, &mut choices))
                .transpose()?
                .flatten();
            let (input, requirement) = if let Some(requirement) = selected_side {
                let input = specific.unwrap();
                if let Some(previous) = common
                    && common_requirement != Some(&Requirement::Unsupported)
                {
                    precedence.push(Precedence {
                        destination: input.destination.clone(),
                        replaced: previous.owner.clone(),
                        replacement: input.owner.clone(),
                    });
                }
                (input, requirement)
            } else {
                let Some(input) = common else {
                    continue;
                };
                let Some(requirement) =
                    select_requirement(common_requirement.unwrap(), policy, full, &mut choices)?
                else {
                    continue;
                };
                (input, requirement)
            };
            selected.insert(
                path,
                ProjectedEntry {
                    owner: input.owner.clone(),
                    destination: input.destination.clone(),
                    layer: ContentLayer::Common,
                    requirements: if client {
                        Requirements {
                            client: requirement,
                            server: Requirement::Unsupported,
                        }
                    } else {
                        Requirements {
                            client: Requirement::Unsupported,
                            server: requirement,
                        }
                    },
                    representation: input.representation.clone(),
                },
            );
        }
        Ok(Self(BuildInventory {
            target,
            entries: selected.into_values().collect(),
            precedence,
            choices: choices.into_values().collect(),
        }))
    }
    /// Surviving obligations, which may still need content acquisition.
    pub fn entries(&self) -> &[ProjectedEntry] {
        &self.0.entries
    }
    /// Complete only after every included entry has an allowed target representation.
    pub fn finish(self) -> Result<BuildInventory, InventoryError> {
        let full = matches!(
            self.0.target,
            BuildTarget::ClientFull | BuildTarget::ServerFull
        );
        for entry in &self.0.entries {
            if matches!(entry.representation, Representation::Unacquired { .. })
                || (full && matches!(entry.representation, Representation::Download { .. }))
            {
                return Err(InventoryError::MaterializationRequired(
                    entry.destination.relative().as_str().into(),
                ));
            }
        }
        Ok(self.0)
    }
}
impl BuildInventory {
    /// Requested target.
    pub fn target(&self) -> BuildTarget {
        self.target
    }
    /// Complete included content obligations.
    pub fn entries(&self) -> &[ProjectedEntry] {
        &self.entries
    }
    /// Explicitly replaced common inputs.
    pub fn precedence(&self) -> &[Precedence] {
        &self.precedence
    }
    /// Materialized optional selections.
    pub fn choices(&self) -> &[ChoiceDecision] {
        &self.choices
    }
}

fn select_requirement(
    requirement: &Requirement,
    policy: &OptionalPolicy,
    full: bool,
    decisions: &mut BTreeMap<String, ChoiceDecision>,
) -> Result<Option<Requirement>, InventoryError> {
    Ok(match requirement {
        Requirement::Unsupported => None,
        Requirement::Required => Some(Requirement::Required),
        Requirement::Optional(choice) => match policy {
            OptionalPolicy::Preserve if !full => Some(requirement.clone()),
            OptionalPolicy::Preserve => {
                return Err(InventoryError::ChoiceRequired(choice.key.as_str().into()));
            }
            OptionalPolicy::Resolve {
                choices,
                use_defaults,
            } => {
                let explicit = choices.get(choice.key.as_str());
                let enabled = match explicit {
                    Some(value) => *value,
                    None if *use_defaults => choice.default_enabled,
                    None => return Err(InventoryError::ChoiceRequired(choice.key.as_str().into())),
                };
                decisions.insert(
                    choice.key.as_str().into(),
                    ChoiceDecision {
                        choice: choice.clone(),
                        enabled,
                        used_default: explicit.is_none(),
                    },
                );
                enabled.then_some(Requirement::Required)
            }
        },
    })
}
