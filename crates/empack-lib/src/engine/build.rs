//! Build preparation from one captured workspace, before any distribution write.
use super::{
    backend::BackendFile,
    content::SourceEvidencePolicy,
    layout::ProjectLayout,
    mrpack::{AcquiredBuildFile, LockedFileKey, MrpackPlan, OptionalConversion, SourceFile},
    project::WorkspaceSnapshot,
    snapshot::Observation,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::ManagedPath,
    model::{AcquisitionSpec, ContentLayer, ResolvedProject},
    requirements::{Requirement, Requirements},
};
use std::collections::{BTreeMap, BTreeSet};

/// Prepare a reference export using captured local bytes and exact locked download evidence.
/// Remote acquisition is a separate operation; missing reference evidence remains an error.
/// Unlisted backend files need adoption or an explicit observed-snapshot plan before export.
pub fn prepare_mrpack(
    workspace: &WorkspaceSnapshot,
    external: &BTreeMap<LockedFileKey, AcquiredBuildFile>,
    evidence: SourceEvidencePolicy,
    optional: OptionalConversion,
    cancel: &Cancellation,
) -> Result<MrpackPlan> {
    let project = workspace.require_resolved()?;
    let sources = workspace.source_entries(cancel)?;
    let backend = workspace.backend_files(cancel)?;
    let mut acquired = BTreeMap::new();
    let mut occupied = BTreeSet::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            cancel.check()?;
            if evidence == SourceEvidencePolicy::StrongSourceRequired {
                ensure!(
                    file.expected
                        .digests
                        .as_ref()
                        .is_some_and(|set| set.values().iter().any(|digest| matches!(
                            digest.algorithm(),
                            empack_core::digest::DigestAlgorithm::Sha256
                                | empack_core::digest::DigestAlgorithm::Sha512
                        ))),
                    "Strong-source build policy requires an independent strong declaration"
                );
            }
            let file_key = LockedFileKey {
                dependency: key.clone(),
                slot: file.slot.clone(),
            };
            let mut local = None;
            if let AcquisitionSpec::Local(path) = &file.acquisition {
                occupied.insert(path.clone());
                let (content, permissions) =
                    workspace.acquire_file(path, Some(&file.expected), evidence, cancel)?;
                local = Some(AcquiredBuildFile {
                    content,
                    permissions,
                });
            }
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                occupied.insert(path.clone());
                match workspace.observations().entries().get(&path) {
                    Some(Observation::File(_)) => {
                        let (content, permissions) = workspace.acquire_file(
                            &path,
                            Some(&file.expected),
                            evidence,
                            cancel,
                        )?;
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
                if let Some(external) = external.get(&file_key) {
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
        .iter()
        .map(|(key, value)| (key.clone(), value))
        .chain(acquired.iter().map(|(key, value)| (key.clone(), value)))
        .collect();
    check_backend(&project, &backend, &combined)?;
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
    MrpackPlan::prepare(&project, &owned, source_files, optional)
}

fn check_backend(
    project: &ResolvedProject,
    observed: &[BackendFile],
    acquired: &BTreeMap<LockedFileKey, &AcquiredBuildFile>,
) -> Result<()> {
    for observed in observed {
        let mut matches = 0;
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                let mut matched = false;
                for placement in file.placements.as_slice() {
                    if placement.destination != observed.destination {
                        continue;
                    }
                    if !observed.matches_selection_and_requirements(
                        dependency.selected.as_ref(),
                        &placement.requirements,
                    )? {
                        continue;
                    }
                    let acquired = acquired.get(&LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    });
                    let digests = acquired
                        .map(|file| file.content.observed_digests())
                        .or(file.expected.digests.as_ref())
                        .context("Backend content has no locked byte evidence")?;
                    ensure!(
                        digests.values().contains(&observed.digest),
                        "Backend digest differs from locked or acquired bytes"
                    );
                    matched = true;
                }
                matches += usize::from(matched);
            }
        }
        ensure!(
            matches == 1,
            "Backend file is unaccounted or ambiguous in exact lock: {}",
            observed.metadata_path.as_str()
        );
    }
    Ok(())
}

/// One verified mrpack output and its complete captured read set. Publication cannot rerun build work.
pub struct PreparedMrpackBuild {
    root: super::snapshot::ProjectReadRoot,
    change: super::verification::VerifiedFileChange,
    conversions: Vec<String>,
    bytes: u64,
}
impl PreparedMrpackBuild {
    pub fn conversions(&self) -> &[String] {
        &self.conversions
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn publish(
        self,
        publisher: &super::publication::Publisher,
        cancel: &Cancellation,
    ) -> Result<super::publication::PublicationReceipt> {
        publisher.publish(&self.root, self.change, cancel)
    }
}

/// Compose captured inputs, semantic mrpack planning, archive verification and file publication
/// proof. This does not publish, download or run tools. The destination must already be captured.
pub fn prepare_mrpack_build(
    workspace: WorkspaceSnapshot,
    artifact: empack_core::path::PortableRelPath,
    external: &BTreeMap<LockedFileKey, AcquiredBuildFile>,
    evidence: SourceEvidencePolicy,
    optional: OptionalConversion,
    cancel: &Cancellation,
) -> Result<PreparedMrpackBuild> {
    use super::{
        staging::MutableStage,
        verification::{VerifiedFileChange, observed_artifacts_for, plan_files},
    };
    use empack_core::files::{FileContent, FilePermissions};
    use std::io::Seek;
    ensure!(
        artifact.as_str().ends_with(".mrpack"),
        "Mrpack output requires a .mrpack filename"
    );
    let target = ManagedPath::Artifact(artifact);
    let observed = observed_artifacts_for(workspace.observations(), [target.clone()])?;
    let plan = prepare_mrpack(&workspace, external, evidence, optional, cancel)?;
    let mut archive = super::staging::PrivateFile::new()?;
    let verified = plan.write(archive.file(), cancel)?;
    let desired = BTreeMap::from([(
        target.clone(),
        FileContent {
            content: verified.content().clone(),
            bytes: verified.len(),
            permissions: FilePermissions {
                readonly: false,
                executable: false,
            },
        },
    )]);
    let file_plan = plan_files(&observed, &desired, &BTreeSet::new())?;
    let mut stage = MutableStage::empty()?;
    archive.file().rewind()?;
    stage.write(
        &ProjectLayout::path(&target)?,
        archive.file(),
        verified.len(),
        cancel,
    )?;
    let limits = super::snapshot::SnapshotLimits {
        file_bytes: super::artifacts::ArchiveLimits::default().compressed_bytes,
        ..super::snapshot::SnapshotLimits::default()
    };
    let frozen = stage.freeze(limits, cancel)?;
    let (root, snapshot) = workspace.into_native();
    let change = VerifiedFileChange::verify_artifacts(snapshot, file_plan, frozen)?;
    Ok(PreparedMrpackBuild {
        root,
        change,
        conversions: plan.conversions().to_vec(),
        bytes: verified.len(),
    })
}
