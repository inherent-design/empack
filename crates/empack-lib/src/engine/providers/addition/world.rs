//! Provider worlds become exact member inventories before an addition group is available.
use super::*;
use crate::engine::{
    acquisition::{DownloadRequest, HttpAcquisition},
    addition::{DirectFileLimits, world},
    content::{InitialObservation, SourceEvidencePolicy},
    dependency_content::{DependencyContent, DependencyContents},
    mrpack::{AcquiredBuildFile, LockedFileKey},
};
use empack_core::files::FilePermissions;

pub struct ProviderArchiveAddition {
    current: ResolvedProject,
    closure: ProviderClosure,
    choices: BTreeMap<ProviderProjectId, ProviderAddInput>,
    documents: AdmissionPermit,
}
pub(super) struct WorldSelection {
    pub files: NonEmpty<ResolvedFile>,
    pub roots: NonEmpty<Placement>,
    content: BTreeMap<FileSlot, AcquiredBuildFile>,
}
struct Selection {
    id: ProviderProjectId,
    archive: ProviderArchiveSource,
    roots: NonEmpty<Placement>,
}
impl ProviderArchiveAddition {
    pub(super) fn new(
        current: ResolvedProject,
        closure: ProviderClosure,
        choices: BTreeMap<ProviderProjectId, ProviderAddInput>,
        documents: AdmissionPermit,
    ) -> Self {
        Self {
            current,
            closure,
            choices,
            documents,
        }
    }

    /// Payload acquisition is a separate host choice. No project writer or publication is used.
    pub async fn acquire(
        self,
        scope: &mut WorkScope,
        transport: &HttpAcquisition,
        evidence: SourceEvidencePolicy,
        limits: DirectFileLimits,
    ) -> Result<Box<ProviderAddition>> {
        let mut selections = Vec::new();
        let mut requests = Vec::new();
        for (id, selected) in &self.closure.selections {
            if selected.kind != ContentKind::World {
                continue;
            }
            let Some(choice) = self.choices.get(id) else {
                ensure!(
                    self.current
                        .lock()
                        .dependencies
                        .values()
                        .any(|dep| dep.identity == ResolvedIdentity::Provider(id.clone())),
                    "A required world needs an explicit destination root"
                );
                continue;
            };
            let declared = selected.resolution.files.as_slice();
            let primary = declared
                .iter()
                .find(|file| file.primary)
                .unwrap_or(&declared[0]);
            let file = match &choice.files {
                ProviderFiles::Primary | ProviderFiles::PrimaryPlaced(_) => primary,
                ProviderFiles::Named(names) => {
                    ensure!(names.len() == 1, "Select one provider world archive");
                    declared
                        .iter()
                        .find(|file| names.contains(&file.filename))
                        .context("Selected world archive is missing")?
                }
                ProviderFiles::Placed(places) => {
                    ensure!(
                        places.len() == 1,
                        "Select one provider world archive and its destination roots"
                    );
                    declared
                        .iter()
                        .find(|file| places.contains_key(&file.filename))
                        .context("Selected world archive is missing")?
                }
                ProviderFiles::All => {
                    ensure!(declared.len() == 1, "Select one provider world archive");
                    primary
                }
            };
            let roots = match &choice.files {
                ProviderFiles::PrimaryPlaced(roots) => roots.clone(),
                ProviderFiles::Placed(places) => places[&file.filename].clone(),
                _ => {
                    let folder = choice.folder.as_ref().map(PortableRelPath::as_str).or_else(||self.current.intent().content_folder(ContentKind::World)).context("Provider worlds require an explicit destination folder or world layout")?;
                    NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(&format!(
                            "{}/{}",
                            folder, selected.resolution.project.slug
                        ))?,
                        layer: ContentLayer::Common,
                        requirements: choice.requirements.clone(),
                    }])?
                }
            };
            let archive = ProviderArchiveSource {
                pin: selected.resolution.pin.clone(),
                slot: FileSlot::parse(&file.filename)?,
                expected: file.expected.clone(),
                alternatives: file.persistent_alternatives(),
            };
            let alternatives = selected
                .resolution
                .download_alternatives(&archive.slot, &archive.expected)?;
            ensure!(
                !alternatives.is_empty(),
                "Provider world archive requires verified supplied content before member interpretation"
            );
            requests.push(DownloadRequest {
                alternatives: NonEmpty::new(alternatives)?,
                expected: archive.expected.clone(),
                evidence,
                initial: InitialObservation::RequireEvidence,
                limits: crate::engine::acquisition::TransferLimits {
                    file_bytes: limits
                        .transfer
                        .file_bytes
                        .min(limits.archive.compressed_bytes),
                    ..limits.transfer
                },
            });
            selections.push(Selection {
                id: id.clone(),
                archive,
                roots,
            });
        }
        ensure!(
            selections.len() <= limits.files,
            "Provider world count exceeds limit"
        );
        let acquired = transport
            .acquire_batch(scope, requests, limits.transfer)
            .await?;
        let mut worlds = BTreeMap::new();
        let mut permits = Vec::new();
        let mut expanded = 0u64;
        for (selection, content) in selections.into_iter().zip(acquired) {
            let members = world::read(
                scope,
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
                crate::engine::artifacts::ArchiveLimits {
                    total_bytes: limits.archive.total_bytes.min(
                        limits
                            .transfer
                            .transfer_bytes
                            .checked_sub(expanded)
                            .context("World expansion exceeds batch limit")?,
                    ),
                    ..limits.archive
                },
                evidence,
            )
            .await?;
            let (members, permit) = members.into_parts();
            permits.push(permit);
            let mut files = Vec::new();
            let mut content = BTreeMap::new();
            for member in members {
                let bytes = member.file.content.lease().len();
                expanded = expanded
                    .checked_add(bytes)
                    .context("World expansion size overflow")?;
                let slot = FileSlot::parse(member.relative.as_str())?;
                files.push(ResolvedFile {
                    slot: slot.clone(),
                    expected: ExpectedContent {
                        digests: None,
                        size: Some(bytes),
                        accepted_observation: Some(member.file.content.lease().id()),
                    },
                    acquisition: AcquisitionSpec::ProviderArchiveMember {
                        archive: selection.archive.clone(),
                        member: member.member,
                    },
                    provenance: Provenance {
                        source: "provider-world-archive".into(),
                        location: Some(selection.id.to_string()),
                        declared_digests: None,
                        conversions: vec![],
                    },
                    placements: NonEmpty::new(
                        selection
                            .roots
                            .as_slice()
                            .iter()
                            .map(|root| {
                                Ok(Placement {
                                    destination: InstallDestination::parse(&format!(
                                        "{}/{}",
                                        root.destination.relative().as_str(),
                                        member.relative.as_str()
                                    ))?,
                                    layer: root.layer,
                                    requirements: root.requirements.clone(),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?,
                    )?,
                });
                content.insert(slot, member.file);
            }
            worlds.insert(
                selection.id,
                WorldSelection {
                    files: NonEmpty::new(files)?,
                    roots: selection.roots,
                    content,
                },
            );
        }
        let project = normalize(&self.current, &self.closure, &self.choices, &worlds)?;
        let group = AdditionGroup::from_resolved(&project)?;
        let mut materialized = DependencyContents::new();
        for (key, dependency) in &project.lock().dependencies {
            if let ResolvedIdentity::Provider(id) = &dependency.identity
                && let Some(world) = worlds.get(id)
            {
                for (slot, file) in &world.content {
                    materialized.insert(
                        LockedFileKey {
                            dependency: key.clone(),
                            slot: slot.clone(),
                        },
                        DependencyContent::Materialized(file.clone()),
                    );
                }
            }
        }
        Ok(Box::new(ProviderAddition {
            project,
            group,
            evidence: self.closure,
            _documents: self.documents,
            materialized,
            _members: permits,
        }))
    }
}

#[cfg(test)]
mod tests;
