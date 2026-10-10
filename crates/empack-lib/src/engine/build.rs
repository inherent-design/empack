//! Build preparation from one captured workspace, before any distribution write.
use super::{
    content::{ContentPool, SourceEvidencePolicy},
    layout::ProjectLayout,
    mrpack::{AcquiredBuildFile, LockedFileKey, MrpackPlan, OptionalConversion, SourceFile},
    project::WorkspaceSnapshot,
    snapshot::Observation,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::ManagedPath,
    inventory::ContentOwner,
    model::{AcquisitionSpec, ContentLayer, ResolvedProject},
    requirements::{Requirement, Requirements},
};
use std::collections::{BTreeMap, BTreeSet};

pub mod acquisition;
pub mod batch;
pub mod client;
pub mod curseforge;
pub mod materialized;
pub(crate) mod native;
pub mod server;

/// Acquisition is keyed by exact native dependency and file role.
#[derive(Default)]
pub struct BuildAcquisitions {
    pub locked: BTreeMap<LockedFileKey, AcquiredBuildFile>,
}

impl BuildAcquisitions {
    /// Count logical slots, including content shared by distinct locked records.
    pub(crate) fn retained_bytes(&self) -> Result<u64> {
        self.locked.values().try_fold(0u64, |total, file| {
            total
                .checked_add(file.content.lease().len())
                .context("Retained build content size overflow")
        })
    }
}

/// Prepare a reference export using captured local bytes and exact locked download evidence.
/// Remote acquisition is a separate operation; missing reference evidence remains an error.
/// Untracked source bytes are included by authored source policy, without foreign identity.
pub fn prepare_mrpack(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    evidence: SourceEvidencePolicy,
    optional: OptionalConversion,
    cancel: &Cancellation,
) -> Result<MrpackPlan> {
    let content = capture_build_content(workspace, external, evidence, None, cancel)?;
    MrpackPlan::prepare(
        &content.project,
        &content.acquired,
        content.sources,
        optional,
    )
}

/// Shared captured obligations for reference and materialized projections.
struct CapturedBuildContent {
    project: ResolvedProject,
    acquired: BTreeMap<super::mrpack::LockedFileKey, AcquiredBuildFile>,
    sources: Vec<super::mrpack::SourceFile>,
}
fn capture_build_content(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    evidence: SourceEvidencePolicy,
    selected: Option<&BTreeSet<ContentOwner>>,
    cancel: &Cancellation,
) -> Result<CapturedBuildContent> {
    let project = workspace.require_resolved()?;
    let sources = workspace.source_entries(cancel)?;
    let maximum = workspace
        .observations()
        .entries()
        .values()
        .try_fold(0u64, |sum, entry| {
            sum.checked_add(match entry {
                Observation::File(file) => file.bytes,
                _ => 0,
            })
            .context("Captured content size overflow")
        })?;
    let mut pool = ContentPool::new(maximum)?;
    let mut acquired = BTreeMap::new();
    let mut occupied = BTreeSet::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            cancel.check()?;
            let included = selected.is_none_or(|owners| {
                owners.contains(&ContentOwner::Dependency {
                    key: key.clone(),
                    slot: file.slot.clone(),
                })
            });
            let file_key = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let member_policy =
                if included && evidence == SourceEvidencePolicy::StrongSourceRequired {
                    if let AcquisitionSpec::ProviderArchiveMember { archive, member } =
                        &file.acquisition
                    {
                        external
                            .locked
                            .get(&file_key)
                            .context("Strong world build requires its verified source archive")?
                            .content
                            .provider_member_policy(archive, member, evidence)?
                    } else {
                        crate::engine::content::validate_expectation(
                            &file.expected,
                            u64::MAX,
                            evidence,
                            crate::engine::content::InitialObservation::RequireEvidence,
                        )?;
                        evidence
                    }
                } else {
                    evidence
                };
            if let AcquisitionSpec::Embedded { archive, .. } = &file.acquisition {
                occupied.insert(archive.clone());
            }
            let mut local = None;
            if let AcquisitionSpec::Local(path) = &file.acquisition {
                occupied.insert(path.clone());
                if included {
                    let (content, permissions) =
                        workspace.acquire_file(path, Some(&file.expected), evidence, cancel)?;
                    let content = pool.insert(content, cancel)?;
                    local = Some(AcquiredBuildFile {
                        content,
                        permissions,
                    });
                }
            }
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                occupied.insert(path.clone());
                if !included {
                    continue;
                }
                match workspace.observations().entries().get(&path) {
                    Some(Observation::File(_)) => {
                        let (content, permissions) = workspace.acquire_file(
                            &path,
                            Some(&file.expected),
                            member_policy,
                            cancel,
                        )?;
                        let content = pool.insert(content, cancel)?;
                        if let Some(previous) = &local {
                            ensure!(
                                previous.content.lease().id() == content.lease().id(),
                                "Placements of one locked file differ"
                            );
                            ensure!(
                                previous.permissions == permissions,
                                "Placements of one locked file have conflicting permissions"
                            );
                        } else {
                            local = Some(AcquiredBuildFile {
                                content,
                                permissions,
                            });
                        }
                    }
                    Some(Observation::Directory { .. } | Observation::Ancestor(_)) => {
                        anyhow::bail!("Locked file destination is a directory")
                    }
                    Some(Observation::Absent) | None => {}
                }
            }
            if let Some(local) = local {
                if let Some(external) = external.locked.get(&file_key) {
                    ensure!(
                        local.content.lease().id() == external.content.lease().id(),
                        "Acquired content differs from captured installation"
                    );
                    ensure!(
                        local.permissions == external.permissions,
                        "Acquired permissions differ from captured installation"
                    );
                }
                acquired.insert(file_key, local);
            }
        }
    }
    // Borrow retained acquisitions without copying bytes or accepting duplicate ownership.
    let combined: BTreeMap<_, _> = external
        .locked
        .iter()
        .map(|(key, value)| (key.clone(), value))
        .chain(acquired.iter().map(|(key, value)| (key.clone(), value)))
        .collect();
    let mut source_files = Vec::new();
    for source in sources {
        if occupied.contains(&source.path) {
            continue;
        }
        let (content, permissions) = workspace.acquire_file(
            &source.path,
            None,
            SourceEvidencePolicy::Compatibility,
            cancel,
        )?;
        let content = pool.insert(content, cancel)?;
        source_files.push(SourceFile {
            label: source.path.as_str().into(),
            destination: source.destination,
            layer: source.layer,
            requirements: Requirements {
                client: if source.layer == ContentLayer::Server {
                    Requirement::Unsupported
                } else {
                    Requirement::Required
                },
                server: if source.layer == ContentLayer::Client {
                    Requirement::Unsupported
                } else {
                    Requirement::Required
                },
            },
            content,
            permissions,
        });
    }
    // Leases are shared immutable ownership. Their bytes are rechecked during candidate writing.
    let owned = combined
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                AcquiredBuildFile {
                    content: value.content.clone(),
                    permissions: value.permissions,
                },
            )
        })
        .collect();
    Ok(CapturedBuildContent {
        project,
        acquired: owned,
        sources: source_files,
    })
}

/// One verified mrpack output and its complete captured read set. Publication cannot rerun build work.
pub struct PreparedMrpackBuild {
    publication: PreparedArtifact,
    conversions: Vec<String>,
}
impl PreparedMrpackBuild {
    pub fn conversions(&self) -> &[String] {
        &self.conversions
    }
    pub fn bytes(&self) -> u64 {
        self.publication.bytes
    }
    #[cfg(test)]
    pub(in crate::engine) fn publish(
        self,
        publisher: &super::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<super::publication::PublicationReceipt> {
        self.publication.publish(publisher, cancel)
    }
}

/// Compose captured inputs, semantic mrpack planning, archive verification and file publication
/// proof. This does not publish, download or run tools. The destination must already be captured.
pub fn prepare_mrpack_build(
    workspace: WorkspaceSnapshot,
    artifact: empack_core::path::PortableRelPath,
    external: &BuildAcquisitions,
    evidence: SourceEvidencePolicy,
    optional: OptionalConversion,
    cancel: &Cancellation,
) -> Result<PreparedMrpackBuild> {
    ensure!(
        artifact.as_str().ends_with(".mrpack"),
        "Mrpack output requires a .mrpack filename"
    );
    let plan = prepare_mrpack(&workspace, external, evidence, optional, cancel)?;
    let mut archive = super::staging::PrivateFile::new()?;
    let verified = plan.write(archive.file(), cancel)?;
    let publication = prepare_archive_publication(workspace, artifact, archive, &verified, cancel)?;
    Ok(PreparedMrpackBuild {
        publication,
        conversions: plan.conversions().to_vec(),
    })
}

/// Internal file publication proof, reached only after the caller's semantic archive checks.
pub(in crate::engine) struct PreparedArtifact {
    root: super::snapshot::ProjectReadRoot,
    change: super::verification::VerifiedFileChange,
    bytes: u64,
}
impl PreparedArtifact {
    pub(in crate::engine) fn publish(
        self,
        publisher: &super::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<super::publication::PublicationReceipt> {
        publisher.publish(&self.root, self.change, cancel)
    }
}
pub(in crate::engine) fn prepare_archive_publication(
    workspace: WorkspaceSnapshot,
    artifact: empack_core::path::PortableRelPath,
    archive: super::staging::PrivateFile,
    verified: &super::artifacts::VerifiedArchive,
    cancel: &Cancellation,
) -> Result<PreparedArtifact> {
    prepare_archives_publication(
        workspace,
        vec![ArchiveCandidate {
            artifact,
            archive,
            verified: verified.clone(),
        }],
        &BTreeSet::new(),
        cancel,
    )
}
struct ArchiveCandidate {
    artifact: empack_core::path::PortableRelPath,
    archive: super::staging::PrivateFile,
    verified: super::artifacts::VerifiedArchive,
}
fn prepare_archives_publication(
    workspace: WorkspaceSnapshot,
    candidates: Vec<ArchiveCandidate>,
    removals: &BTreeSet<ManagedPath>,
    cancel: &Cancellation,
) -> Result<PreparedArtifact> {
    use super::{
        staging::MutableStage,
        verification::{
            VerifiedFileChange, candidate_stage_limits, observed_artifacts_for, plan_files,
        },
    };
    use empack_core::files::{FileContent, FilePermissions};
    use std::io::Seek;
    ensure!(!candidates.is_empty(), "Build publication has no artifacts");
    let mut collisions = super::layout::CollisionIndex::default();
    let mut desired = BTreeMap::new();
    let mut bytes = 0u64;
    for candidate in &candidates {
        cancel.check()?;
        collisions.insert_file(&candidate.artifact)?;
        bytes = bytes
            .checked_add(candidate.verified.len())
            .context("Artifact size total overflow")?;
        desired.insert(
            ManagedPath::Artifact(candidate.artifact.clone()),
            FileContent {
                content: candidate.verified.content().clone(),
                bytes: candidate.verified.len(),
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        );
    }
    let observed = observed_artifacts_for(
        workspace.observations(),
        desired.keys().chain(removals).cloned(),
    )?;
    let file_plan = plan_files(&observed, &desired, removals)?;
    let limits = candidate_stage_limits(workspace.observations(), &file_plan)?;
    let mut stage = MutableStage::empty()?;
    // Transfer ownership one candidate at a time. The original file retires at the end of
    // each iteration, before freeze creates a packed copy of the publication tree.
    for mut candidate in candidates {
        candidate.archive.file().rewind()?;
        stage.write(
            &ProjectLayout::path(&ManagedPath::Artifact(candidate.artifact.clone()))?,
            candidate.archive.file(),
            candidate.verified.len(),
            cancel,
        )?;
    }
    let frozen = stage.freeze(limits, cancel)?;
    let (root, snapshot) = workspace.into_native();
    let change = VerifiedFileChange::verify_artifacts(snapshot, file_plan, frozen)?;
    Ok(PreparedArtifact {
        root,
        change,
        bytes,
    })
}
