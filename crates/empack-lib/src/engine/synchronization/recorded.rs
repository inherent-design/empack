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
        project::{MutationSnapshot, WorkspaceSnapshot},
        snapshot::Observation,
    },
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::ManagedPath,
    model::{AcquisitionSpec, ExpectedContent, ResolvedFile, ResolvedProject},
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
    retained_bytes: u64,
}
impl RecordedInputs {
    pub(in crate::engine) fn metadata_memory(
        snapshot: &MutationSnapshot,
        resolution: Option<&ResolvedProject>,
        limits: ArchiveLimits,
    ) -> Result<u64> {
        let workspace = snapshot.workspace();
        let lock = resolution
            .map(ResolvedProject::lock)
            .or_else(|| workspace.prior_lock().map(|lock| lock.lock()))
            .context("Recorded synchronization has no resolution")?;
        let archives = lock
            .dependencies
            .values()
            .flat_map(|dependency| dependency.files.as_slice())
            .any(|file| match &file.acquisition {
                AcquisitionSpec::Embedded { archive, .. } => matches!(
                    workspace.observations().entries().get(archive),
                    Some(Observation::File(_))
                ),
                _ => false,
            });
        parser_memory(limits, archives)
    }

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
        let mut largest = 0u64;
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
                        verified_placement(workspace, file, cancel)?
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
                largest = largest.max(observed.bytes);
                local.push(FileInput {
                    key,
                    path,
                    expected: file.expected.clone(),
                });
            }
        }
        for (archive, members) in &archives {
            let source = ZipContentSource::open_captured(workspace, archive, limits, cancel)?;
            let mut expanded = 0u64;
            for input in members {
                let member = source
                    .member(&input.path)
                    .context("Declared archive member is missing")?;
                ensure!(
                    input
                        .expected
                        .size
                        .is_none_or(|bytes| bytes == member.bytes),
                    "Recorded member size differs from declaration"
                );
                expanded = expanded
                    .checked_add(member.bytes)
                    .context("Recorded member size overflow")?;
                largest = largest.max(member.bytes);
            }
            ensure!(
                expanded <= limits.total_bytes,
                "Recorded members exceed archive read allowance"
            );
            bytes = bytes
                .checked_add(expanded)
                .context("Recorded archive size overflow")?;
        }
        let retained_bytes = bytes;
        // The pool retains selected bytes; only one current input is copied into it at a time.
        let bytes = bytes
            .checked_add(largest)
            .context("Recorded acquisition staging overflow")?;
        Ok(Self {
            snapshot,
            resolution,
            content,
            local,
            archives,
            limits,
            bytes,
            retained_bytes,
        })
    }
    pub(in crate::engine) fn bytes(&self) -> u64 {
        self.bytes
    }
    pub(in crate::engine) fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }
    pub(in crate::engine) fn memory(&self) -> Result<u64> {
        parser_memory(self.limits, !self.archives.is_empty())
    }
    pub(in crate::engine) fn open_files(&self) -> u64 {
        // Packed content retains one backing; current archive/member readers are short-lived.
        10
    }
    pub(in crate::engine) fn prepare_with_references(
        mut self,
        evidence: SourceEvidencePolicy,
        references: BTreeMap<LockedFileKey, AcquiredBuildFile>,
        cancel: &Cancellation,
    ) -> Result<SynchronizationPreparation> {
        for (key, acquired) in references {
            ensure!(
                matches!(self.content.get(&key), Some(DependencyContent::Reference)),
                "Acquired synchronization content does not select a recorded remote reference"
            );
            self.content.insert(key, acquired.into());
        }
        let workspace = self.snapshot.workspace();
        let mut pool = ContentPool::new(self.retained_bytes)?;
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
            let mut archive =
                ZipContentSource::open_captured(workspace, &archive, self.limits, cancel)?;
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

fn verified_placement(
    workspace: &WorkspaceSnapshot,
    file: &ResolvedFile,
    cancel: &Cancellation,
) -> Result<PortableRelPath> {
    let mut last_error = None;
    for placement in file.placements.as_slice() {
        cancel.check()?;
        let path = ProjectLayout::path(&ManagedPath::Content {
            layer: placement.layer,
            path: placement.destination.relative().clone(),
        })?;
        if !matches!(
            workspace.observations().entries().get(&path),
            Some(Observation::File(_))
        ) {
            continue;
        }
        match workspace.verify_file(&path, &file.expected, cancel) {
            Ok(_) => return Ok(path),
            Err(error) => last_error = Some(error),
        }
    }
    cancel.check()?;
    Err(last_error.unwrap_or_else(|| {
        anyhow::anyhow!("Recorded archive and installed member are both missing")
    }))
    .context("No installed placement satisfies the recorded member")
}

fn parser_memory(limits: ArchiveLimits, archives: bool) -> Result<u64> {
    if !archives {
        return Ok(128 << 10);
    }
    (limits.entries as u64)
        .checked_mul(2048)
        .and_then(|n| n.checked_add(128 << 10))
        .context("Recorded archive memory estimate overflow")
}
