//! Three-way instance file reconciliation. Callers establish root and release authority.
use crate::files::{FileCapabilities, FileContent, ObservedPath};

/// Authority retained over a file after initial installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePolicy {
    /// Replace or retire unchanged installed files; changed bytes require a decision.
    Managed,
    /// Supply initial content without authority to replace subsequent user content.
    Seed,
}

/// The baseline of one file in a completed installation, not the next release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFile {
    /// Exact installed bytes and portable attributes.
    pub baseline: FileContent,
    /// File ownership policy recorded when installation completed.
    pub policy: FilePolicy,
}

/// Desired content after release authentication and side/choice selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFile {
    /// Independently verified expected bytes and portable attributes.
    pub content: FileContent,
    /// Requested installation ownership policy.
    pub policy: FilePolicy,
}

/// A file decision requiring user input or a new safe destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileConflict {
    /// Existing bytes have no previous installation ownership, even if they match.
    UnownedDestination,
    /// An owned file differs from both its baseline and the incoming postcondition.
    ModifiedManagedFile,
    /// A selected file destination is a directory rather than a regular file.
    WrongKind,
    /// A release requests managed ownership of an initial-only user file.
    SeedOwnership,
}

/// A pure decision; it is not authorization to mutate a native path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDecision {
    /// No bytes change; the instance planner may update or retire the record.
    Unchanged,
    /// Preserve user content without claiming it matches the incoming release.
    Preserve,
    /// Write verified content to an observed absent destination.
    Create(FileContent),
    /// Replace the exact observed baseline with verified incoming content.
    Replace(FileContent),
    /// Remove an unchanged owned regular file, never a directory or tree.
    Remove,
    /// Leave the destination unchanged and expose the named conflict.
    Conflict(FileConflict),
}

/// Reconcile one selected destination without I/O or implicit ownership adoption.
///
/// `previous` comes from a completed installation record, `current` from a native
/// observation, and `incoming` from the selected release. Links must be rejected by
/// the native reader before reaching this function. The caller checks collisions,
/// mutable-data exclusions, root identity and complete inventory coverage, then
/// binds any mutation to these observations and an execution grant.
pub fn plan_file(
    previous: Option<&InstalledFile>,
    current: &ObservedPath,
    incoming: Option<&ReleaseFile>,
    capabilities: FileCapabilities,
) -> FileDecision {
    let Some(incoming) = incoming else {
        return match previous {
            None => FileDecision::Preserve,
            Some(previous) if previous.policy == FilePolicy::Seed => FileDecision::Preserve,
            Some(previous) => match current {
                ObservedPath::Absent => FileDecision::Unchanged,
                ObservedPath::Directory => FileDecision::Conflict(FileConflict::WrongKind),
                ObservedPath::File(actual)
                    if actual.equivalent(&previous.baseline, capabilities) =>
                {
                    FileDecision::Remove
                }
                ObservedPath::File(_) => FileDecision::Conflict(FileConflict::ModifiedManagedFile),
            },
        };
    };

    if matches!(current, ObservedPath::Directory) {
        return FileDecision::Conflict(FileConflict::WrongKind);
    }
    if incoming.policy == FilePolicy::Seed {
        return match current {
            ObservedPath::Absent => FileDecision::Create(incoming.content.clone()),
            ObservedPath::File(_) => FileDecision::Preserve,
            ObservedPath::Directory => unreachable!("directory checked above"),
        };
    }
    if previous.is_some_and(|previous| previous.policy == FilePolicy::Seed) {
        return FileDecision::Conflict(FileConflict::SeedOwnership);
    }
    match (previous, current) {
        (_, ObservedPath::Absent) => FileDecision::Create(incoming.content.clone()),
        (None, ObservedPath::File(_)) => FileDecision::Conflict(FileConflict::UnownedDestination),
        (Some(previous), ObservedPath::File(actual)) => {
            if actual.equivalent(&incoming.content, capabilities) {
                FileDecision::Unchanged
            } else if actual.equivalent(&previous.baseline, capabilities) {
                FileDecision::Replace(incoming.content.clone())
            } else {
                FileDecision::Conflict(FileConflict::ModifiedManagedFile)
            }
        }
        (_, ObservedPath::Directory) => unreachable!("directory checked above"),
    }
}

#[cfg(test)]
mod tests;
