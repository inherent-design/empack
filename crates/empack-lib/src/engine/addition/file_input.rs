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
use std::collections::BTreeMap;

#[derive(Clone)]
pub enum AcquiredFileSource {
    /// The first selected placement becomes the tracked local source. The original host path
    /// does not become a project read/write capability or an absolute path in the manifest.
    Local,
    /// Explicit download-as-local conversion. No transient locator enters durable state.
    DownloadedLocal,
    Url(NonEmpty<String>),
}
#[derive(Clone)]
pub enum FileEvidence {
    Declared(ExpectedContent),
    /// Explicit acceptance of these initial bytes, never manufactured source authenticity.
    AcceptObserved,
}
#[derive(Clone)]
pub struct AcquiredFileInput {
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
            if let AcquiredFileSource::Url(urls) = &input.source {
                for url in urls.as_slice() {
                    crate::engine::documents::validate_download_url(url)?;
                    bytes = bytes
                        .checked_add(url.len())
                        .context("File input size overflow")?;
                }
            }
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
        let mut intent = current.intent().clone();
        intent.roots.clear();
        let mut dependencies = BTreeMap::new();
        let mut coverage = BTreeMap::new();
        let mut content = BTreeMap::new();
        for input in inputs.into_vec() {
            scope.cancellation().check()?;
            ensure!(
                !intent.roots.contains_key(&input.key),
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
            let (source, identity, acquisition, provenance) = match input.source {
                source @ (AcquiredFileSource::Local | AcquiredFileSource::DownloadedLocal) => {
                    let provenance = match source {
                        AcquiredFileSource::DownloadedLocal => "downloaded-local-file",
                        _ => "local-file",
                    };
                    let first = &input.placements.as_slice()[0];
                    let source = ProjectLayout::path(&ManagedPath::Content {
                        layer: first.layer,
                        path: first.destination.relative().clone(),
                    })?;
                    (
                        SourceIntent::Local(source.clone()),
                        ResolvedIdentity::Local(input.key.clone()),
                        AcquisitionSpec::Local(source),
                        provenance,
                    )
                }
                AcquiredFileSource::Url(urls) => (
                    SourceIntent::Url(urls.clone()),
                    ResolvedIdentity::Url(input.key.clone()),
                    AcquisitionSpec::Url(urls),
                    "url-file",
                ),
            };
            intent.roots.insert(
                input.key.clone(),
                DependencyIntent {
                    source,
                    kind: input.kind,
                    version: expected
                        .digests
                        .clone()
                        .map(VersionIntent::ContentPinned)
                        .unwrap_or(VersionIntent::FollowCompatible),
                    placement: PlacementIntent::Explicit(input.placements.clone()),
                    requirements: input.requirements,
                },
            );
            let slot = FileSlot::parse("primary")?;
            dependencies.insert(
                input.key.clone(),
                LockedDependency {
                    title: input.title,
                    kind: input.kind,
                    identity,
                    selected: None,
                    files: NonEmpty::new(vec![ResolvedFile {
                        slot: slot.clone(),
                        acquisition,
                        provenance: Provenance {
                            source: provenance.into(),
                            location: None,
                            declared_digests: expected.digests.clone(),
                            conversions: vec![],
                        },
                        expected,
                        placements: input.placements,
                    }])?,
                },
            );
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
