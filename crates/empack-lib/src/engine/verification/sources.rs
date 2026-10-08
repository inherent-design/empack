//! Placements and acquisition sources have independent lifetimes.
use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_observation},
        mrpack::AcquiredBuildFile,
    },
};
use empack_core::model::{AcquisitionSpec, ResolvedProject};

/// Retiring an old placement cannot discard a source still used by the resulting project.
pub(in crate::engine) fn retain_acquisition_sources<'a>(
    snapshot: &NativeSnapshot,
    project: &ResolvedProject,
    removals: &mut BTreeSet<ManagedPath>,
    incoming: impl IntoIterator<Item = &'a ManagedPath>,
) -> Result<()> {
    let sources: BTreeSet<_> = project
        .lock()
        .dependencies
        .values()
        .flat_map(|dependency| dependency.files.as_slice())
        .filter_map(|file| match &file.acquisition {
            AcquisitionSpec::Local(path) | AcquisitionSpec::Embedded { archive: path, .. } => {
                Some(path)
            }
            _ => None,
        })
        .collect();
    let incoming: BTreeSet<_> = incoming.into_iter().collect();
    let mut retained = BTreeSet::new();
    for target in removals.iter() {
        let path = ProjectLayout::path(target)?;
        if !sources.contains(&path) {
            continue;
        }
        ensure!(
            !incoming.contains(target),
            "A changed selection would replace a retained acquisition source: {}",
            path.as_str()
        );
        let observed = observed_mutation_for(snapshot, [target.clone()])?;
        let ObservedPath::File(_) = &observed[target] else {
            anyhow::bail!(
                "Retained acquisition source is not a regular file: {}",
                path.as_str()
            );
        };
        retained.insert(target.clone());
    }
    removals.retain(|target| !retained.contains(target));
    Ok(())
}

/// A replacement must satisfy every surviving local-source assertion, not just its own owner.
pub(super) fn verify_source_changes(
    plan: &FilePlan,
    project: &ResolvedProject,
    documents: &BTreeMap<ManagedPath, Vec<u8>>,
    payloads: &BTreeMap<ManagedPath, AcquiredBuildFile>,
    cancel: &Cancellation,
) -> Result<()> {
    let changes: BTreeMap<_, _> = plan
        .changes()
        .iter()
        .map(|change| Ok((ProjectLayout::path(change.target())?, change)))
        .collect::<Result<_>>()?;
    for file in project
        .lock()
        .dependencies
        .values()
        .flat_map(|dependency| dependency.files.as_slice())
    {
        let (source, archive) = match &file.acquisition {
            AcquisitionSpec::Local(path) => (path, false),
            AcquisitionSpec::Embedded { archive, .. } => (archive, true),
            _ => continue,
        };
        let Some(change) = changes.get(source) else {
            continue;
        };
        let FileChange::Replace {
            target,
            before,
            after,
        } = change
        else {
            anyhow::bail!(
                "Mutation deletes a retained acquisition source: {}",
                source.as_str()
            );
        };
        if archive {
            ensure!(
                matches!(before, ObservedPath::File(old) if old.content == after.content),
                "Replacing a retained archive source requires verified member resolution"
            );
            continue;
        }
        if let Some(bytes) = documents.get(target) {
            verify_observation(
                &mut bytes.as_slice(),
                &file.expected,
                bytes.len() as u64,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                cancel,
            )?;
        } else if let Some(bytes) = payloads.get(target) {
            verify_observation(
                &mut bytes.content.lease().open(),
                &file.expected,
                bytes.content.lease().len(),
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                cancel,
            )?;
        } else {
            anyhow::bail!(
                "Changed acquisition source has no verified output: {}",
                source.as_str()
            );
        }
    }
    Ok(())
}
