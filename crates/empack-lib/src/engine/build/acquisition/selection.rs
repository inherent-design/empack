//! Select exact obligations without acquiring bytes or running a backend.
use super::*;
use empack_core::{
    inventory::{BuildSelection, ContentOwner, InventoryInput, OptionalPolicy, Representation},
    projection::BuildTarget,
};

/// Plan one target with its actual environment and optional choices. In a build batch, callers
/// union needs by acquisition key; selecting a full target does not materialize unrelated sides.
pub fn plan_target_build_acquisitions(
    workspace: &WorkspaceSnapshot,
    external: &BuildAcquisitions,
    target: BuildTarget,
    optional: &OptionalPolicy,
    evidence: SourceEvidencePolicy,
    cancel: &Cancellation,
) -> Result<BuildAcquisitionPlan> {
    if target == BuildTarget::Mrpack {
        ensure!(
            matches!(optional, OptionalPolicy::Preserve),
            "Mrpack preserves optional choices"
        );
        return plan_acquisitions(
            workspace,
            external,
            BuildMaterialization::ReferenceArchive,
            None,
            evidence,
            cancel,
        );
    }
    let references = matches!(target, BuildTarget::Server | BuildTarget::CurseForge);
    let (selected, keys) = select_game_inputs(workspace, target, optional, cancel)?;
    let mut needed = BTreeSet::new();
    let project = workspace.require_resolved()?;
    for entry in selected.entries() {
        if let Some(key) = keys.get(&entry.owner) {
            let expected = match &entry.representation {
                Representation::Unacquired { expected }
                | Representation::Download { expected, .. } => expected,
                Representation::Embedded { .. } => continue,
            };
            let expected = {
                let AcquisitionKey::Locked(key) = key;
                let file = project.lock().dependencies[&key.dependency]
                    .files
                    .as_slice()
                    .iter()
                    .find(|file| file.slot == key.slot)
                    .context("Selected build file is missing")?;
                match &file.acquisition {
                    AcquisitionSpec::ProviderArchiveMember { archive, .. } => &archive.expected,
                    _ => expected,
                }
            };
            if evidence == SourceEvidencePolicy::StrongSourceRequired {
                ensure!(
                    expected
                        .digests
                        .as_ref()
                        .is_some_and(|set| set.values().iter().any(|digest| matches!(
                            digest.algorithm(),
                            DigestAlgorithm::Sha256 | DigestAlgorithm::Sha512
                        ))),
                    "Selected build content has only weaker source evidence"
                );
            }
            if matches!(entry.representation, Representation::Unacquired { .. })
                || (target == BuildTarget::Client
                    && matches!(entry.representation, Representation::Download { .. }))
            {
                needed.insert(key.clone());
            }
        }
    }
    let mut result = plan_acquisitions(
        workspace,
        external,
        BuildMaterialization::AllContent,
        Some(&needed),
        evidence,
        cancel,
    )?;
    if references {
        for need in &mut result.needs {
            if need.reason == AcquisitionReason::MaterializedTarget {
                need.reason = AcquisitionReason::ReferenceEvidence;
            }
        }
    }
    Ok(result)
}

pub(in crate::engine::build) fn select_game_inputs(
    workspace: &WorkspaceSnapshot,
    target: BuildTarget,
    optional: &OptionalPolicy,
    cancel: &Cancellation,
) -> Result<(BuildSelection, BTreeMap<ContentOwner, AcquisitionKey>)> {
    let project = workspace.require_resolved()?;
    let references = matches!(
        target,
        BuildTarget::Client | BuildTarget::Server | BuildTarget::CurseForge
    );
    let mut inputs = Vec::new();
    let mut occupied = BTreeSet::new();
    let mut keys = BTreeMap::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            cancel.check()?;
            let owner = ContentOwner::Dependency {
                key: key.clone(),
                slot: file.slot.clone(),
            };
            keys.insert(
                owner.clone(),
                AcquisitionKey::Locked(LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                }),
            );
            match &file.acquisition {
                AcquisitionSpec::Local(path) | AcquisitionSpec::Embedded { archive: path, .. } => {
                    occupied.insert(path.clone());
                }
                _ => {}
            }
            let representation = if references {
                super::super::materialized::reference_for_target(file, None, target)?
            } else {
                None
            }
            .unwrap_or_else(|| Representation::Unacquired {
                expected: file.expected.clone(),
            });
            for placement in file.placements.as_slice() {
                occupied.insert(ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?);
                inputs.push(InventoryInput {
                    owner: owner.clone(),
                    destination: placement.destination.clone(),
                    layer: placement.layer,
                    requirements: placement.requirements.clone(),
                    representation: representation.clone(),
                });
            }
        }
    }
    for source in workspace.source_entries(cancel)? {
        if occupied.contains(&source.path) {
            continue;
        }
        let Some(Observation::File(observed)) =
            workspace.observations().entries().get(&source.path)
        else {
            anyhow::bail!("Source has no captured file observation")
        };
        inputs.push(InventoryInput {
            owner: ContentOwner::Source(source.path.as_str().into()),
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
            representation: Representation::Unacquired {
                expected: ExpectedContent {
                    digests: None,
                    size: Some(observed.bytes),
                    accepted_observation: Some(empack_core::digest::ContentId::from_sha256(
                        observed.content,
                    )),
                },
            },
        });
    }
    let selected = BuildSelection::select(&inputs, target, optional)?;
    Ok((selected, keys))
}
