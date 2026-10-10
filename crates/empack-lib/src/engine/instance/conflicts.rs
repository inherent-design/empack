//! Explicit local deviations stay distinct from publisher-selected content.
use super::*;
use empack_core::{digest::ContentId, files::FilePermissions, instance::FileConflict};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    Preserve,
    Replace,
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
    choice: ConflictChoice,
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
