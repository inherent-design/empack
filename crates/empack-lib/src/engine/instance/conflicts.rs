//! Explicit local deviations stay distinct from publisher-selected content.
use super::*;
use empack_core::{digest::ContentId, files::FilePermissions, instance::FileConflict};

/// Observed conflict facts for presentation. Available choices are not grants;
/// preparation must still bind and validate every explicit decision.
#[derive(Debug)]
pub struct InstanceConflict {
    pub path: String,
    pub destination: String,
    pub reason: FileConflict,
    pub existing_file: bool,
    pub incoming_policy: Option<empack_core::instance::FilePolicy>,
}
impl InstanceConflict {
    pub fn explanation(&self) -> &'static str {
        match self.reason {
            FileConflict::UnownedDestination => {
                "A file already exists here without an empack ownership record."
            }
            FileConflict::ModifiedManagedFile => "A pack-managed file has local changes.",
            FileConflict::WrongKind => "A directory is blocking a file destination.",
            FileConflict::SeedOwnership => {
                "The release asks to manage a previously user-owned file."
            }
        }
    }
    pub fn can_preserve(&self) -> bool {
        self.reason != FileConflict::WrongKind && self.existing_file
    }
    pub fn can_replace(&self) -> bool {
        self.reason != FileConflict::WrongKind
            && (self.existing_file || self.incoming_policy.is_some())
    }
    pub fn can_merge(&self) -> bool {
        self.can_preserve()
            && self.incoming_policy == Some(empack_core::instance::FilePolicy::Managed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictChoice {
    Preserve,
    Replace,
    /// Explicit local merge result; no automatic text or world merging.
    Merge {
        file: std::path::PathBuf,
    },
}
/// A release-relative destination. Preparation binds the decision to its exact observation.
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    pub destination: String,
    pub choice: ConflictChoice,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalOverride {
    pub destination: String,
    pub original: FileIdentity,
    pub accepted: FileIdentity,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub sha256: String,
    pub bytes: u64,
    pub readonly: bool,
    pub executable: bool,
}
impl FileIdentity {
    fn from_content(content: &FileContent) -> Self {
        Self {
            sha256: empack_core::digest::ExpectedDigest::Sha256(*content.content.bytes()).hex(),
            bytes: content.bytes,
            readonly: content.permissions.readonly,
            executable: content.permissions.executable,
        }
    }
    fn content(&self) -> Result<FileContent> {
        Ok(FileContent {
            content: ContentId::from_sha256(super::super::release::decode_hex(&self.sha256)?),
            bytes: self.bytes,
            permissions: FilePermissions {
                readonly: self.readonly,
                executable: self.executable,
            },
        })
    }
}
impl LocalOverride {
    pub(super) fn validate(&self) -> Result<()> {
        path(&self.destination)?;
        self.original.content()?;
        self.accepted.content()?;
        Ok(())
    }
}
pub(super) fn resolutions(
    values: Vec<ConflictResolution>,
    layout: InstanceLayout,
) -> Result<BTreeMap<ManagedPath, ConflictChoice>> {
    let mut selected = BTreeMap::new();
    for value in values {
        ensure!(
            selected
                .insert(layout.target(path(&value.destination)?), value.choice)
                .is_none(),
            "Duplicate conflict decision: {}",
            value.destination
        );
    }
    Ok(selected)
}
/// Return a retained local deviation only when the selected release still asks for the same file.
pub(super) fn retained(
    value: &LocalOverride,
    previous: &InstalledFile,
    current: &ObservedPath,
    incoming: Option<&DesiredFile>,
) -> Result<Option<FileDecision>> {
    let capabilities = verification::native_capabilities();
    ensure!(
        previous.policy == empack_core::instance::FilePolicy::Managed
            && value
                .original
                .content()?
                .equivalent(&previous.baseline, capabilities),
        "Local override does not match the retained release"
    );
    if matches!(current, ObservedPath::Directory) {
        return Ok(None);
    }
    if incoming.is_none() {
        // Local content is user-owned; removing its release entry cannot delete it.
        return Ok(Some(FileDecision::Preserve));
    }
    let incoming = incoming.unwrap();
    if let ObservedPath::File(actual) = current
        && incoming.policy == empack_core::instance::FilePolicy::Managed
        && incoming
            .content
            .equivalent(&previous.baseline, capabilities)
        && value.accepted.content()?.equivalent(actual, capabilities)
    {
        return Ok(Some(FileDecision::Preserve));
    }
    Ok(None)
}
pub(super) fn resolve(
    choice: &ConflictChoice,
    merged: Option<&FileContent>,
    conflict: FileConflict,
    destination: &str,
    current: &ObservedPath,
    incoming: Option<&DesiredFile>,
) -> Result<(FileDecision, Option<LocalOverride>)> {
    ensure!(
        conflict != FileConflict::WrongKind,
        "Conflict decisions cannot replace a directory or link"
    );
    match choice {
        ConflictChoice::Preserve => {
            let ObservedPath::File(actual) = current else {
                anyhow::bail!("Preserve requires an existing regular file")
            };
            let local = incoming
                .filter(|file| file.policy == empack_core::instance::FilePolicy::Managed)
                .map(|file| LocalOverride {
                    destination: destination.into(),
                    original: FileIdentity::from_content(&file.content),
                    accepted: FileIdentity::from_content(actual),
                });
            Ok((FileDecision::Preserve, local))
        }
        ConflictChoice::Merge { .. } => {
            let incoming = incoming
                .filter(|file| file.policy == empack_core::instance::FilePolicy::Managed)
                .context("Merge requires a selected managed file")?;
            ensure!(
                matches!(current, ObservedPath::File(_)),
                "Merge requires an existing regular file"
            );
            let content = merged.context("Merge content was not captured")?.clone();
            Ok((
                FileDecision::Replace(content.clone()),
                Some(LocalOverride {
                    destination: destination.into(),
                    original: FileIdentity::from_content(&incoming.content),
                    accepted: FileIdentity::from_content(&content),
                }),
            ))
        }
        ConflictChoice::Replace => {
            let decision = match (current, incoming) {
                (ObservedPath::File(_), Some(file)) => FileDecision::Replace(file.content.clone()),
                (ObservedPath::Absent, Some(file)) => FileDecision::Create(file.content.clone()),
                (ObservedPath::File(_), None) => FileDecision::Remove,
                _ => anyhow::bail!("Replace requires an exact regular file or absent destination"),
            };
            Ok((decision, None))
        }
    }
}

/// Observe a user-selected merge result without staging or modifying either source.
pub(super) fn capture_merge(
    file: &Path,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<FileContent> {
    ensure!(
        file.is_absolute(),
        "Merge result must be an absolute host path"
    );
    let root = ProjectReadRoot::open(file.parent().context("Merge result has no parent")?)?;
    let relative = path(
        file.file_name()
            .and_then(|value| value.to_str())
            .context("Merge result requires a portable filename")?,
    )?;
    let (parent, leaf) = super::super::native::parent(&root.directory, &relative)?;
    let input = super::super::native::open_file(&parent, &leaf)?;
    ensure!(
        input.metadata()?.is_file(),
        "Merge result must be a regular file"
    );
    drop(input);
    let snapshot = root.capture(std::slice::from_ref(&relative), limits, cancel)?;
    let Some(super::super::snapshot::Observation::File(observed)) =
        snapshot.entries().get(&relative)
    else {
        anyhow::bail!("Merge result must be a regular file")
    };
    root.revalidate(&snapshot, cancel)?;
    Ok(verification::content(observed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use empack_core::instance::FilePolicy;

    #[test]
    fn reported_choices_match_the_actual_resolver_for_observed_conflicts() {
        let content = |id| FileContent {
            content: ContentId::from_sha256([id; 32]),
            bytes: 1,
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
        };
        for previous_policy in [None, Some(FilePolicy::Managed), Some(FilePolicy::Seed)] {
            let previous = previous_policy.map(|policy| InstalledFile {
                baseline: content(1),
                policy,
            });
            for current in [
                ObservedPath::Absent,
                ObservedPath::Directory,
                ObservedPath::File(content(2)),
            ] {
                for incoming_policy in [None, Some(FilePolicy::Managed), Some(FilePolicy::Seed)] {
                    let incoming = incoming_policy.map(|policy| DesiredFile {
                        content: content(3),
                        policy,
                    });
                    let FileDecision::Conflict(reason) = plan_file(
                        previous.as_ref(),
                        &current,
                        incoming.as_ref(),
                        verification::native_capabilities(),
                    ) else {
                        continue;
                    };
                    let report = InstanceConflict {
                        path: "game/config/settings".into(),
                        destination: "config/settings".into(),
                        reason,
                        existing_file: matches!(&current, ObservedPath::File(_)),
                        incoming_policy,
                    };
                    for (choice, advertised) in [
                        (ConflictChoice::Preserve, report.can_preserve()),
                        (ConflictChoice::Replace, report.can_replace()),
                        (
                            ConflictChoice::Merge {
                                file: "prepared-local-input".into(),
                            },
                            report.can_merge(),
                        ),
                    ] {
                        let outcome = resolve(
                            &choice,
                            Some(&content(4)),
                            reason,
                            &report.destination,
                            &current,
                            incoming.as_ref(),
                        );
                        assert_eq!(advertised, outcome.is_ok(), "{report:?} / {choice:?}");
                    }
                }
            }
        }
    }
}
