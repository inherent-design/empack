//! Build preparation from one captured workspace, before any distribution write.
use super::{
    backend::{BackendDigestComparison, BackendFile, DigestComparisonBasis},
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

pub mod acquisition;

/// Exact logical requests and retained metadata records occupy distinct acquisition namespaces.
#[derive(Default)]
pub struct BuildAcquisitions {
    pub locked: BTreeMap<LockedFileKey, AcquiredBuildFile>,
    pub observed: BTreeMap<empack_core::path::PortableRelPath, AcquiredBuildFile>,
}

/// Prepare a reference export using captured local bytes and exact locked download evidence.
/// Remote acquisition is a separate operation; missing reference evidence remains an error.
/// Unlisted backend content remains an observed obligation; it never becomes invented intent.
pub fn prepare_mrpack(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
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
    let backend_check = check_backend(&project, &backend, &combined)?;
    let unlisted = backend_check.unlisted;
    let mut choices = BTreeSet::new();
    for dependency in project.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                for requirement in [
                    &placement.requirements.client,
                    &placement.requirements.server,
                ] {
                    if let Requirement::Optional(choice) = requirement {
                        choices.insert(choice.key.as_str().to_owned());
                    }
                }
            }
        }
    }
    let mut observed_files = Vec::new();
    let mut used_observed = BTreeSet::new();
    for record in backend {
        if !unlisted.contains(&record.metadata_path) {
            continue;
        }
        let expected = empack_core::model::ExpectedContent {
            digests: Some(empack_core::digest::DigestSet::new(vec![
                record.digest.clone(),
            ])?),
            size: None,
            accepted_observation: None,
        };
        let path = ProjectLayout::path(&ManagedPath::Content {
            layer: ContentLayer::Common,
            path: record.destination.relative().clone(),
        })?;
        occupied.insert(path.clone());
        ensure!(
            !matches!(
                workspace.observations().entries().get(&path),
                Some(Observation::Directory { .. } | Observation::Ancestor(_))
            ),
            "Observed backend payload destination is a directory"
        );
        let mut file = external.observed.get(&record.metadata_path).cloned();
        if file.is_some() {
            used_observed.insert(record.metadata_path.clone());
        }
        if matches!(
            workspace.observations().entries().get(&path),
            Some(Observation::File(_))
        ) {
            let (content, permissions) =
                workspace.acquire_file(&path, Some(&expected), evidence, cancel)?;
            if let Some(previous) = &file {
                ensure!(
                    previous.content.lease().id() == content.lease().id()
                        && previous.permissions == permissions,
                    "Acquired observed file differs from captured content"
                );
            }
            file = Some(AcquiredBuildFile {
                content,
                permissions,
            });
        }
        let file = file.with_context(|| {
            format!(
                "Acquire retained backend file before export: {}",
                record.metadata_path.as_str()
            )
        })?;
        let base = format!("observed:{}", record.metadata_path.as_str());
        let mut choice = base.clone();
        let mut sequence = 0u64;
        while !choices.insert(choice.clone()) {
            sequence = sequence
                .checked_add(1)
                .context("Observed choice identifier exhausted")?;
            choice = format!("{base}#{sequence}");
        }
        observed_files.push(super::mrpack::ObservedFile::verify(
            record,
            file,
            empack_core::requirements::ChoiceKey::parse(&choice)?,
            evidence,
        )?);
    }
    ensure!(
        used_observed.len() == external.observed.len(),
        "Acquisition contains an unrelated observed file"
    );
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
    let mut plan = MrpackPlan::prepare_with_observed(
        &project,
        &owned,
        source_files,
        observed_files,
        optional,
    )?;
    plan.backend_comparisons = backend_check.comparisons;
    Ok(plan)
}

struct BackendCheck {
    unlisted: BTreeSet<empack_core::path::PortableRelPath>,
    comparisons: Vec<BackendDigestComparison>,
}

fn check_backend(
    project: &ResolvedProject,
    observed: &[BackendFile],
    acquired: &BTreeMap<LockedFileKey, &AcquiredBuildFile>,
) -> Result<BackendCheck> {
    let mut unlisted = BTreeSet::new();
    let mut comparisons = Vec::new();
    for observed in observed {
        let mut matches = 0;
        let mut claimed = false;
        for (key, dependency) in &project.lock().dependencies {
            if observed.provider.as_ref().is_some_and(|provider| matches!(&dependency.identity,
                empack_core::model::ResolvedIdentity::Provider(project) if project == &provider.project)) {
                claimed = true;
            }
            for file in dependency.files.as_slice() {
                let mut matched = false;
                for placement in file.placements.as_slice() {
                    if placement.destination != observed.destination {
                        continue;
                    }
                    claimed = true;
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
                    let basis = if let Some(acquired) = acquired {
                        ensure!(
                            acquired
                                .content
                                .observed_digests()
                                .values()
                                .contains(&observed.digest),
                            "Backend digest differs from acquired bytes"
                        );
                        DigestComparisonBasis::AcquiredBytes
                    } else if let Some(comparable) =
                        file.expected.digests.as_ref().and_then(|set| {
                            set.values()
                                .iter()
                                .find(|digest| digest.algorithm() == observed.digest.algorithm())
                        })
                    {
                        ensure!(
                            *comparable == observed.digest,
                            "Backend digest differs from locked declaration"
                        );
                        DigestComparisonBasis::SameAlgorithmDeclaration
                    } else {
                        // The format planner still requires complete independent reference evidence.
                        // Never use the unmatched backend URL/digest to manufacture a proof.
                        DigestComparisonBasis::IndependentLockedReference
                    };
                    if !matched {
                        comparisons.push(BackendDigestComparison {
                            metadata_path: observed.metadata_path.clone(),
                            declared: observed.digest.clone(),
                            basis,
                        });
                    }
                    matched = true;
                }
                matches += usize::from(matched);
            }
        }
        if matches == 0 && !claimed {
            unlisted.insert(observed.metadata_path.clone());
            continue;
        }
        ensure!(
            matches == 1,
            "Backend file is unaccounted or ambiguous in exact lock: {}",
            observed.metadata_path.as_str()
        );
    }
    Ok(BackendCheck {
        unlisted,
        comparisons,
    })
}

/// One verified mrpack output and its complete captured read set. Publication cannot rerun build work.
pub struct PreparedMrpackBuild {
    root: super::snapshot::ProjectReadRoot,
    change: super::verification::VerifiedFileChange,
    conversions: Vec<String>,
    backend_comparisons: Vec<BackendDigestComparison>,
    observed: Vec<super::mrpack::ObservedFileEvidence>,
    bytes: u64,
}
impl PreparedMrpackBuild {
    pub fn backend_comparisons(&self) -> &[BackendDigestComparison] {
        &self.backend_comparisons
    }
    pub fn observed(&self) -> &[super::mrpack::ObservedFileEvidence] {
        &self.observed
    }
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
    external: &BuildAcquisitions,
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
        backend_comparisons: plan.backend_comparisons().to_vec(),
        observed: plan.observed().to_vec(),
        bytes: verified.len(),
    })
}
