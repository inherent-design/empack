//! Pure file-change planning. Absence from desired roots never implies deletion.
use crate::{digest::ContentId, model::ContentLayer, path::PortableRelPath};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::fmt;

/// Logical project destinations. Native layout and authority remain in the engine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManagedPath {
    /// Normalized authoring document.
    IntentDocument,
    /// Exact selection document.
    LockDocument,
    /// Content in an explicit environment layer.
    Content {
        /// Environment layer.
        layer: ContentLayer,
        /// Destination within that layer.
        path: PortableRelPath,
    },
    /// User-owned template selected for an explicit operation.
    UserTemplate(PortableRelPath),
    /// Editable authoring scaffold; initialization may only seed an absent file.
    Scaffold(ProjectScaffold),
    /// Generated distribution output.
    Artifact(PortableRelPath),
    /// Native installed game content, separate from author sources.
    InstanceFile(PortableRelPath),
    /// Native instance content inside Prism's fixed game directory.
    PrismFile(PortableRelPath),
    /// Fixed layout marker keeps an empty Prism game directory present.
    PrismLayoutMarker,
    /// Keeps an empty native game directory present without claiming user content.
    InstanceLayoutMarker,
    /// Completed native installation ownership and choices.
    InstanceRecord,
    /// Enrolled publisher keys and channel anti-replay state.
    InstanceSubscription,
    /// Retained immutable native release descriptor.
    InstanceRelease(PortableRelPath),
}
/// The bounded set of authoring helpers outside the content and template roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectScaffold {
    /// Repository ignore policy.
    GitIgnore,
    /// Pull-request and branch validation.
    ValidationWorkflow,
    /// Tagged distribution publication.
    ReleaseWorkflow,
}
/// Portable permission intent. Native adapters additionally preserve appropriate source modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePermissions {
    /// The file should resist ordinary writes.
    pub readonly: bool,
    /// Executable by its owner on systems with executable permission bits.
    pub executable: bool,
}
/// Permission capabilities of the representation being planned or verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileCapabilities {
    /// POSIX executable bits can be observed and enforced in this representation.
    pub executable_bits: bool,
}

/// A byte-level postcondition, not source-authentication or artifact-structure evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    /// Content address of independently selected expected bytes.
    pub content: ContentId,
    /// Exact byte count.
    pub bytes: u64,
    /// Required portable permissions.
    pub permissions: FilePermissions,
}
impl FileContent {
    /// Compare content and supported attributes without discarding declared output intent.
    pub fn equivalent(&self, other: &Self, capabilities: FileCapabilities) -> bool {
        self.content == other.content
            && self.bytes == other.bytes
            && self.permissions.readonly == other.permissions.readonly
            && (!capabilities.executable_bits
                || self.permissions.executable == other.permissions.executable)
    }
}
/// Native observations are converted to these values for pure planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservedPath {
    /// Destination and relevant absent ancestors were observed.
    Absent,
    /// Existing regular file.
    File(FileContent),
    /// An existing directory cannot be implicitly replaced or recursively deleted.
    Directory,
}
/// An explicit bounded change with its expected prior state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// Create or replace one regular file.
    Replace {
        /// Exact logical target.
        target: ManagedPath,
        /// The prior state must still match at publication.
        before: ObservedPath,
        /// The frozen candidate must satisfy this postcondition.
        after: FileContent,
    },
    /// Remove one selected regular file; never a tree.
    Remove {
        /// Exact logical target.
        target: ManagedPath,
        /// Required prior content.
        before: FileContent,
    },
}
impl FileChange {
    /// Exact target of this change.
    pub fn target(&self) -> &ManagedPath {
        match self {
            Self::Replace { target, .. } | Self::Remove { target, .. } => target,
        }
    }
}
/// An immutable, deterministic file plan. It grants no native write authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    capabilities: FileCapabilities,
    changes: Vec<FileChange>,
    expected: BTreeMap<ManagedPath, FileContent>,
}
impl FilePlan {
    /// Reconcile explicit desired files and explicitly selected removals.
    /// Unlisted observed files are retained and remain expected in the candidate.
    pub fn prepare(
        observed: &BTreeMap<ManagedPath, ObservedPath>,
        desired: &BTreeMap<ManagedPath, FileContent>,
        removals: &BTreeSet<ManagedPath>,
    ) -> Result<Self, FilePlanError> {
        Self::prepare_for(
            observed,
            desired,
            removals,
            FileCapabilities {
                executable_bits: true,
            },
        )
    }

    /// Plan for explicit representation capabilities. Unsupported native bits remain in expected intent.
    pub fn prepare_for(
        observed: &BTreeMap<ManagedPath, ObservedPath>,
        desired: &BTreeMap<ManagedPath, FileContent>,
        removals: &BTreeSet<ManagedPath>,
        capabilities: FileCapabilities,
    ) -> Result<Self, FilePlanError> {
        let mut expected: BTreeMap<_, _> = observed
            .iter()
            .filter_map(|(path, state)| {
                if let ObservedPath::File(content) = state {
                    Some((path.clone(), content.clone()))
                } else {
                    None
                }
            })
            .collect();
        let mut changes = Vec::new();
        for (target, after) in desired {
            if removals.contains(target) {
                return Err(FilePlanError::ContradictoryTarget(target.clone()));
            }
            let before = observed
                .get(target)
                .ok_or_else(|| FilePlanError::UnobservedTarget(target.clone()))?;
            match before {
                ObservedPath::Directory => {
                    return Err(FilePlanError::DirectoryTarget(target.clone()));
                }
                ObservedPath::File(content) if content.equivalent(after, capabilities) => {}
                _ => changes.push(FileChange::Replace {
                    target: target.clone(),
                    before: before.clone(),
                    after: after.clone(),
                }),
            }
            expected.insert(target.clone(), after.clone());
        }
        for target in removals {
            match observed
                .get(target)
                .ok_or_else(|| FilePlanError::UnobservedTarget(target.clone()))?
            {
                ObservedPath::File(before) => changes.push(FileChange::Remove {
                    target: target.clone(),
                    before: before.clone(),
                }),
                ObservedPath::Absent => {}
                ObservedPath::Directory => {
                    return Err(FilePlanError::DirectoryTarget(target.clone()));
                }
            }
            expected.remove(target);
        }
        changes.sort_by(|left, right| left.target().cmp(right.target()));
        Ok(Self {
            capabilities,
            changes,
            expected,
        })
    }
    /// The representation capabilities used for attribute comparisons.
    pub fn capabilities(&self) -> FileCapabilities {
        self.capabilities
    }
    /// Deterministic explicit changes.
    pub fn changes(&self) -> &[FileChange] {
        &self.changes
    }
    /// Complete selected regular-file inventory after the operation, including retained files.
    pub fn expected(&self) -> &BTreeMap<ManagedPath, FileContent> {
        &self.expected
    }
}
/// Planning failures cannot be downgraded into a partial default batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePlanError {
    /// The target was not observed, including its absence.
    UnobservedTarget(ManagedPath),
    /// A file operation selected a directory.
    DirectoryTarget(ManagedPath),
    /// One request simultaneously writes and removes a file.
    ContradictoryTarget(ManagedPath),
}
impl fmt::Display for FilePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnobservedTarget(_) => f.write_str("File target has no prior observation"),
            Self::DirectoryTarget(_) => {
                f.write_str("File operation cannot replace or remove a directory")
            }
            Self::ContradictoryTarget(_) => {
                f.write_str("File target cannot be both written and removed")
            }
        }
    }
}
impl core::error::Error for FilePlanError {}
