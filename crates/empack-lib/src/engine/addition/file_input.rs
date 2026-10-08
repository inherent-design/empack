//! Acquired direct files enter the same canonical addition and native publication path.
use super::{AdditionGroup, DocumentCodec};
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, validate_expectation},
    dependency_content::{DependencyContent, DependencyContents},
    layout::ProjectLayout,
    mrpack::{AcquiredBuildFile, LockedFileKey},
    resources::{AdmissionPermit, ResourceRequest},
    runtime::WorkScope,
};
use anyhow::{Context, Result, ensure};
use empack_core::{files::ManagedPath, model::*, requirements::Requirements};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub enum AcquiredFileSource {
    /// The first selected placement becomes the tracked local source. The original host path
    /// does not become a project read/write capability or an absolute path in the manifest.
    Local,
    /// Preserve an explicit project-relative authoring source.
    TrackedLocal(empack_core::path::PortableRelPath),
    /// Explicit download-as-local conversion. No transient locator enters durable state.
    DownloadedLocal,
    /// Expanded archive members retain their observed archive identity as provenance.
    ArchiveMember {
        archive: empack_core::digest::ContentId,
        member: empack_core::path::PortableRelPath,
    },
    Url(NonEmpty<String>),
    /// Explicitly accepted installed bytes; durable ownership remains URL-backed.
    ObservedUrl(NonEmpty<String>),
}
#[derive(Clone)]
pub enum FileEvidence {
    Declared(ExpectedContent),
    /// Explicit acceptance of these initial bytes, never manufactured source authenticity.
    AcceptObserved,
}
/// A standalone file role is distinct from membership in a multi-file local identity.
#[derive(Clone)]
pub enum FileInputRole {
    Primary,
    Named(FileSlot),
    Member(FileSlot),
}
impl FileInputRole {
    pub(super) fn member(&self) -> Option<&FileSlot> {
        match self {
            Self::Member(slot) => Some(slot),
            _ => None,
        }
    }
    pub(super) fn slot(&self) -> FileSlot {
        match self {
            Self::Primary => FileSlot::parse("primary").expect("static primary role"),
            Self::Named(slot) | Self::Member(slot) => slot.clone(),
        }
    }
}
#[derive(Clone)]
pub struct AcquiredFileInput {
    pub role: FileInputRole,
    pub key: DependencyKey,
    pub title: String,
    pub kind: ContentKind,
    pub source: AcquiredFileSource,
    pub evidence: FileEvidence,
    pub requirements: Requirements,
    pub placements: NonEmpty<Placement>,
    pub file: AcquiredBuildFile,
}
pub struct FileAddition {
    project: ResolvedProject,
    group: AdditionGroup,
    content: DependencyContents,
    _documents: AdmissionPermit,
}
impl FileAddition {
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn group(&self) -> &AdditionGroup {
        &self.group
    }
    pub fn content(&self) -> &DependencyContents {
        &self.content
    }

    /// Normalize an explicitly selected file inventory after acquisition. There is no network,
    /// project reader or writer here. Type identification and archive interpretation precede
    /// this boundary; an arbitrary ZIP is not assumed to be an installed world or modpack.
    pub fn from_acquired(
        scope: &WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<AcquiredFileInput>,
        policy: SourceEvidencePolicy,
    ) -> Result<Self> {
        let mut memory = 0u64;
        for input in inputs.as_slice() {
            scope.cancellation().check()?;
            let mut bytes = input
                .title
                .len()
                .checked_add(input.key.as_str().len())
                .context("File input size overflow")?;
            for placement in input.placements.as_slice() {
                bytes = bytes
                    .checked_add(placement.destination.relative().as_str().len())
                    .context("File input size overflow")?;
            }
            if let AcquiredFileSource::Url(urls) | AcquiredFileSource::ObservedUrl(urls) =
                &input.source
            {
                for url in urls.as_slice() {
                    crate::engine::documents::validate_download_url(url)?;
                    bytes = bytes
                        .checked_add(url.len())
                        .context("File input size overflow")?;
                }
            }
            let source_bytes = match &input.source {
                AcquiredFileSource::TrackedLocal(path) => path.as_str().len(),
                AcquiredFileSource::ArchiveMember { member, .. } => {
                    member.as_str().len().saturating_add(64)
                }
                _ => 0,
            };
            bytes = bytes
                .checked_add(source_bytes)
                .context("File source size overflow")?;
            memory = memory
                .checked_add(
                    (bytes as u64)
                        .checked_mul(8)
                        .and_then(|n| n.checked_add(16 * 1024))
                        .context("File input size overflow")?,
                )
                .context("File input size overflow")?;
        }
        let documents = scope.reserve_storage(ResourceRequest {
            memory_bytes: memory,
            ..Default::default()
        })?;
        let requested: BTreeSet<_> = inputs.as_slice().iter().map(|input| &input.key).collect();
        let prior_files: BTreeMap<_, _> = current
            .lock()
            .dependencies
            .iter()
            .filter(|(key, _)| requested.contains(key))
            .flat_map(|(key, dependency)| {
                dependency
                    .files
                    .as_slice()
                    .iter()
                    .map(move |file| ((key, &file.slot), file))
            })
            .collect();
        drop(requested);
        let mut intent = current.intent().clone();
        intent.roots.clear();
        let mut dependencies = BTreeMap::<DependencyKey, LockedDependency>::new();
        let mut coverage = BTreeMap::new();
        let mut content = BTreeMap::new();
        for input in inputs.into_vec() {
            scope.cancellation().check()?;
            ensure!(
                input.role.member().is_some() || !intent.roots.contains_key(&input.key),
                "Repeated direct-file logical key"
            );
            let expected = match input.evidence {
                FileEvidence::Declared(value) => value,
                FileEvidence::AcceptObserved => ExpectedContent {
                    digests: None,
                    size: Some(input.file.content.lease().len()),
                    accepted_observation: Some(input.file.content.lease().id()),
                },
            };
            validate_expectation(
                &expected,
                input.file.content.lease().len(),
                policy,
                InitialObservation::RequireEvidence,
            )?;
            if let Some(digests) = &expected.digests {
                digests.check(input.file.content.observed_digests().values())?;
            }
            ensure!(
                expected
                    .size
                    .is_none_or(|size| size == input.file.content.lease().len())
                    && expected
                        .accepted_observation
                        .as_ref()
                        .is_none_or(|id| *id == input.file.content.lease().id()),
                "Acquired file differs from declared content"
            );
            let refresh_observation = (input.role.member().is_some()
                && matches!(input.source, AcquiredFileSource::Local))
                || matches!(
                    input.source,
                    AcquiredFileSource::ObservedUrl(_) | AcquiredFileSource::TrackedLocal(_)
                );
            let location = match &input.source {
                AcquiredFileSource::ArchiveMember { archive, member } => Some(format!(
                    "sha256:{}!/{}",
                    empack_core::digest::ExpectedDigest::Sha256(*archive.bytes()).hex(),
                    member.as_str()
                )),
                _ => None,
            };
            let (source, identity, acquisition, provenance) = match input.source {
                source @ (AcquiredFileSource::Local
                | AcquiredFileSource::TrackedLocal(_)
                | AcquiredFileSource::DownloadedLocal
                | AcquiredFileSource::ArchiveMember { .. }) => {
                    let provenance = match source {
                        AcquiredFileSource::DownloadedLocal => "downloaded-local-file",
                        AcquiredFileSource::ArchiveMember { .. } => "world-archive-member",
                        _ => "local-file",
                    };
                    let first = &input.placements.as_slice()[0];
                    let source = match source {
                        AcquiredFileSource::TrackedLocal(path) => path,
                        _ => ProjectLayout::path(&ManagedPath::Content {
                            layer: first.layer,
                            path: first.destination.relative().clone(),
                        })?,
                    };
                    (
                        match input.role.member() {
                            Some(slot) => SourceIntent::LocalFiles(BTreeMap::from([(
                                slot.clone(),
                                source.clone(),
                            )])),
                            None => SourceIntent::Local(source.clone()),
                        },
                        ResolvedIdentity::Local(input.key.clone()),
                        AcquisitionSpec::Local(source),
                        provenance,
                    )
                }
                AcquiredFileSource::Url(urls) | AcquiredFileSource::ObservedUrl(urls) => {
                    ensure!(
                        input.role.member().is_none(),
                        "Member groups require tracked local sources"
                    );
                    (
                        SourceIntent::Url(urls.clone()),
                        ResolvedIdentity::Url(input.key.clone()),
                        AcquisitionSpec::Url(urls),
                        "url-file",
                    )
                }
            };
            let next_root = DependencyIntent {
                source,
                kind: input.kind,
                version: if input.role.member().is_some() {
                    VersionIntent::FollowCompatible
                } else {
                    expected
                        .digests
                        .clone()
                        .map(VersionIntent::ContentPinned)
                        .unwrap_or(VersionIntent::FollowCompatible)
                },
                placement: match &input.role {
                    FileInputRole::Named(slot) | FileInputRole::Member(slot) => {
                        PlacementIntent::ByFile(BTreeMap::from([(
                            slot.clone(),
                            input.placements.clone(),
                        )]))
                    }
                    FileInputRole::Primary => PlacementIntent::Explicit(input.placements.clone()),
                },
                requirements: input.requirements,
            };
            if let Some(existing) = intent.roots.get_mut(&input.key) {
                ensure!(
                    existing.kind == next_root.kind
                        && existing.requirements == next_root.requirements
                        && existing.version == next_root.version,
                    "Member group declarations disagree"
                );
                let (SourceIntent::LocalFiles(prior), SourceIntent::LocalFiles(next)) =
                    (&mut existing.source, &next_root.source)
                else {
                    anyhow::bail!("Member groups cannot overlap single-file identities")
                };
                for (slot, path) in next {
                    ensure!(
                        prior.insert(slot.clone(), path.clone()).is_none(),
                        "Repeated member slot"
                    );
                }
                let (PlacementIntent::ByFile(prior), PlacementIntent::ByFile(next)) =
                    (&mut existing.placement, next_root.placement)
                else {
                    unreachable!()
                };
                for (slot, places) in next {
                    ensure!(
                        prior.insert(slot, places).is_none(),
                        "Repeated member placement role"
                    );
                }
            } else {
                intent.roots.insert(input.key.clone(), next_root);
            }
            let slot = input.role.slot();
            let mut file_provenance = Provenance {
                source: provenance.into(),
                location,
                declared_digests: expected.digests.clone(),
                conversions: vec![],
            };
            if refresh_observation && let Some(prior) = prior_files.get(&(&input.key, &slot)) {
                file_provenance = prior.provenance.clone();
                if prior.expected != expected {
                    file_provenance.declared_digests = expected.digests.clone();
                    file_provenance
                        .conversions
                        .push("Explicit acceptance of changed tracked bytes".into());
                }
            }
            let next = LockedDependency {
                title: input.title,
                kind: input.kind,
                identity,
                selected: None,
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: slot.clone(),
                    acquisition,
                    provenance: file_provenance,
                    expected,
                    placements: input.placements,
                }])?,
            };
            if let Some(existing) = dependencies.get_mut(&input.key) {
                ensure!(
                    existing.title == next.title && existing.identity == next.identity,
                    "Member group identity differs"
                );
                for file in next.files.into_vec() {
                    existing.files.push(file);
                }
            } else {
                dependencies.insert(input.key.clone(), next);
            }
            // A direct file does not prove that its game-level dependency set is empty.
            coverage.insert(input.key.clone(), Coverage::Unknown);
            content.insert(
                LockedFileKey {
                    dependency: input.key,
                    slot,
                },
                DependencyContent::Materialized(input.file),
            );
        }
        let source = DocumentCodec.decode_intent(
            &DocumentCodec.encode_intent(&intent)?,
            "direct file addition",
        )?;
        let project = ResolvedProject::validate(
            intent,
            ResolutionLock {
                acceptable_versions: source.intent().runtime.acceptable_versions.clone(),
                intent_revision: source.semantic_revision(),
                resolver: "empack-file-addition-v0.5".into(),
                dependencies,
                required_edges: BTreeMap::new(),
                coverage,
                runtime: current.lock().runtime.clone(),
            },
            source.semantic_revision(),
        )?;
        let group = AdditionGroup::from_resolved(&project)?;
        Ok(Self {
            project,
            group,
            content,
            _documents: documents,
        })
    }
}

#[cfg(test)]
mod tests;
