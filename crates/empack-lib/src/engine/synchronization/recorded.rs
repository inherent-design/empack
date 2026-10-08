//! Captured sources satisfy recorded slots without resolving new versions or downloading content.
use super::{SynchronizationCandidate, native::SynchronizationPreparation};
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        archive_source::ZipContentSource,
        artifacts::ArchiveLimits,
        content::{ContentPool, InitialObservation, SourceEvidencePolicy},
        dependency_content::{DependencyContent, DependencyContents, validate_reference},
        layout::ProjectLayout,
        mrpack::{AcquiredBuildFile, LockedFileKey},
        project::MutationSnapshot,
        snapshot::Observation,
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::ManagedPath,
    model::{AcquisitionSpec, ExpectedContent, ResolvedProject},
    path::PortableRelPath,
};
use std::collections::BTreeMap;

struct FileInput {
    key: LockedFileKey,
    path: PortableRelPath,
    expected: ExpectedContent,
}
pub(in crate::engine) struct RecordedInputs {
    snapshot: MutationSnapshot,
    resolution: Option<ResolvedProject>,
    content: DependencyContents,
    local: Vec<FileInput>,
    archives: BTreeMap<PortableRelPath, Vec<FileInput>>,
    limits: ArchiveLimits,
    bytes: u64,
}
impl RecordedInputs {
    pub(in crate::engine) fn new(
        snapshot: MutationSnapshot,
        resolution: Option<ResolvedProject>,
        limits: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let workspace = snapshot.workspace();
        let candidate = SynchronizationCandidate::prepare_input(
            workspace.intent(),
            workspace.prior_lock(),
            resolution.as_ref(),
        )?;
        let mut content = BTreeMap::new();
        let mut local = Vec::new();
        let mut archives = BTreeMap::<PortableRelPath, Vec<FileInput>>::new();
        let mut bytes = 0u64;
        for (key, dependency) in &candidate.project().lock().dependencies {
            for file in dependency.files.as_slice() {
                cancel.check()?;
                let key = LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                };
                let path = match &file.acquisition {
                    AcquisitionSpec::Local(path) => path.clone(),
                    AcquisitionSpec::Embedded { archive, member } => {
                        if matches!(
                            workspace.observations().entries().get(archive),
                            Some(Observation::File(_))
                        ) {
                            archives
                                .entry(archive.clone())
                                .or_default()
                                .push(FileInput {
                                    key,
                                    path: member.clone(),
                                    expected: file.expected.clone(),
                                });
                            continue;
                        }
                        // Imports may retain exact installed members after discarding their archive.
                        // Their captured bytes must still satisfy every original source assertion.
                        file.placements
                            .as_slice()
                            .iter()
                            .map(|placement| {
                                ProjectLayout::path(&ManagedPath::Content {
                                    layer: placement.layer,
                                    path: placement.destination.relative().clone(),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?
                            .into_iter()
                            .find(|path| {
                                matches!(
                                    workspace.observations().entries().get(path),
                                    Some(Observation::File(_))
                                )
                            })
                            .context("Recorded archive and installed member are both missing")?
                    }
                    _ => {
                        validate_reference(file)?;
                        content.insert(key, DependencyContent::Reference);
                        continue;
                    }
                };
                let Some(Observation::File(observed)) =
                    workspace.observations().entries().get(&path)
                else {
                    anyhow::bail!(
                        "Recorded local source is missing or not a file: {}",
                        path.as_str()
                    );
                };
                bytes = bytes
                    .checked_add(observed.bytes)
                    .context("Recorded source size overflow")?;
                local.push(FileInput {
                    key,
                    path,
                    expected: file.expected.clone(),
                });
            }
        }
        for (archive, members) in &archives {
            let Some(Observation::File(observed)) = workspace.observations().entries().get(archive)
            else {
                unreachable!("selected captured archive")
            };
            ensure!(
                observed.bytes <= limits.compressed_bytes,
                "Recorded archive exceeds compressed byte limit"
            );
            let expanded = members
                .iter()
                .try_fold(0u64, |total, input| {
                    let maximum = input.expected.size.unwrap_or(limits.file_bytes);
                    ensure!(
                        maximum <= limits.file_bytes,
                        "Recorded member exceeds file byte limit"
                    );
                    total
                        .checked_add(maximum)
                        .context("Recorded member size overflow")
                })?
                .min(limits.total_bytes);
            bytes = bytes
                .checked_add(observed.bytes)
                .and_then(|n| n.checked_add(expanded))
                .context("Recorded archive size overflow")?;
        }
        let bytes = bytes
            .checked_mul(2)
            .context("Recorded acquisition staging overflow")?;
        Ok(Self {
            snapshot,
            resolution,
            content,
            local,
            archives,
            limits,
            bytes,
        })
    }
    pub(in crate::engine) fn bytes(&self) -> u64 {
        self.bytes
    }
    pub(in crate::engine) fn memory(&self) -> Result<u64> {
        if self.archives.is_empty() {
            return Ok(128 << 10);
        }
        (self.limits.entries as u64)
            .checked_mul(2048)
            .and_then(|n| n.checked_add(128 << 10))
            .context("Recorded archive memory estimate overflow")
    }
    pub(in crate::engine) fn open_files(&self) -> u64 {
        // Packed content retains one backing; current archive/member readers are short-lived.
        10
    }
    pub(in crate::engine) fn prepare(
        mut self,
        evidence: SourceEvidencePolicy,
        cancel: &Cancellation,
    ) -> Result<SynchronizationPreparation> {
        let workspace = self.snapshot.workspace();
        let mut pool = ContentPool::new(self.bytes / 2)?;
        for input in self.local {
            cancel.check()?;
            let (content, permissions) =
                workspace.acquire_file(&input.path, Some(&input.expected), evidence, cancel)?;
            let content = pool.insert(content, cancel)?;
            self.content.insert(
                input.key,
                AcquiredBuildFile {
                    content,
                    permissions,
                }
                .into(),
            );
        }
        // One archive reader at a time bounds parser memory while reusing it for every selected member.
        for (archive, members) in self.archives {
            let (source, _) = workspace.acquire_file(
                &archive,
                None,
                SourceEvidencePolicy::Compatibility,
                cancel,
            )?;
            let mut archive = ZipContentSource::open(&source, self.limits, cancel)?;
            for input in members {
                let (content, permissions) = archive.acquire(
                    &input.path,
                    &input.expected,
                    evidence,
                    InitialObservation::RequireEvidence,
                    cancel,
                )?;
                let content = pool.insert(content, cancel)?;
                self.content.insert(
                    input.key,
                    AcquiredBuildFile {
                        content,
                        permissions,
                    }
                    .into(),
                );
            }
        }
        super::native::plan_synchronization_with_resolution(
            self.snapshot,
            self.content,
            self.resolution.as_ref(),
            cancel,
        )
    }
}
