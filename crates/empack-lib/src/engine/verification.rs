//! Independent file inventory checks. Semantic/artifact checks compose above this boundary.
use super::{
    layout::{CollisionIndex, ProjectLayout},
    snapshot::{FileObservation, NativeSnapshot, Observation, SnapshotLimits},
    staging::FrozenStage,
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ContentId,
    files::{
        FileCapabilities, FileChange, FileContent, FilePermissions, FilePlan, ManagedPath,
        ObservedPath,
    },
    path::PortableRelPath,
};
use std::collections::{BTreeMap, BTreeSet};
mod sources;
pub(super) use sources::retain_acquisition_sources;

/// Freeze and independently verify a prepared document/content mutation. Semantic ownership
/// belongs to the operation planner; no publisher is available at this staging boundary.
pub(super) fn stage_mutation(
    workspace: super::project::WorkspaceSnapshot,
    plan: FilePlan,
    documents: &BTreeMap<ManagedPath, Vec<u8>>,
    content: &BTreeMap<ManagedPath, super::mrpack::AcquiredBuildFile>,
    project: &empack_core::model::ResolvedProject,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<(super::snapshot::ProjectReadRoot, VerifiedFileChange)> {
    sources::verify_source_changes(&plan, project, documents, content, cancel)?;
    let limits = candidate_stage_limits(workspace.observations(), &plan)?;
    let mut stage = super::staging::MutableStage::empty()?;
    for (target, bytes) in documents {
        let expected = plan
            .expected()
            .get(target)
            .context("Document is outside the mutation plan")?;
        stage.write_attributed(
            &ProjectLayout::path(target)?,
            &mut bytes.as_slice(),
            bytes.len() as u64,
            expected.permissions,
            cancel,
        )?;
    }
    for (target, file) in content {
        ensure!(
            plan.expected().contains_key(target),
            "Content is outside the mutation plan"
        );
        stage.write_attributed(
            &ProjectLayout::path(target)?,
            &mut file.content.lease().open(),
            file.content.lease().len(),
            file.permissions,
            cancel,
        )?;
    }
    let stage = stage.freeze(limits, cancel)?;
    let (root, base) = workspace.into_native();
    Ok((
        root,
        VerifiedFileChange::verify_mutation(base, plan, stage)?,
    ))
}

/// Native storage capabilities are independent of archive-format permission support.
pub fn native_capabilities() -> FileCapabilities {
    FileCapabilities {
        executable_bits: cfg!(unix),
    }
}

/// Construct a pure file plan using the capabilities of the native publication target.
pub fn plan_files(
    observed: &BTreeMap<ManagedPath, ObservedPath>,
    desired: &BTreeMap<ManagedPath, FileContent>,
    removals: &BTreeSet<ManagedPath>,
) -> Result<FilePlan> {
    Ok(FilePlan::prepare_for(
        observed,
        desired,
        removals,
        native_capabilities(),
    )?)
}

/// Stage only actual writes. The complete native read set remains the publication precondition
/// and contributes unchanged file postconditions independently of this narrower write plan.
pub(super) fn plan_mutation_files(
    observed: &BTreeMap<ManagedPath, ObservedPath>,
    desired: &BTreeMap<ManagedPath, FileContent>,
    removals: &BTreeSet<ManagedPath>,
) -> Result<FilePlan> {
    let complete = plan_files(observed, desired, removals)?;
    let changed: BTreeSet<_> = complete
        .changes()
        .iter()
        .map(|change| change.target())
        .collect();
    let observed = observed
        .iter()
        .filter(|(path, _)| changed.contains(path))
        .map(|(path, value)| (path.clone(), value.clone()))
        .collect();
    let desired = desired
        .iter()
        .filter(|(path, _)| changed.contains(path))
        .map(|(path, value)| (path.clone(), value.clone()))
        .collect();
    let removals = removals
        .iter()
        .filter(|path| changed.contains(path))
        .cloned()
        .collect();
    plan_files(&observed, &desired, &removals)
}

/// Convert native observations into pure planner data, preserving each raw content revision.
pub fn observed_files(snapshot: &NativeSnapshot) -> Result<BTreeMap<ManagedPath, ObservedPath>> {
    let mut observed = BTreeMap::new();
    let mut collisions = CollisionIndex::default();
    for (path, observation) in snapshot.entries() {
        if let Observation::File(file) = observation {
            collisions.insert_file(path)?;
            observed.insert(
                ProjectLayout::classify(path)?,
                ObservedPath::File(content(file)),
            );
        } else if matches!(observation, Observation::Absent)
            && let Ok(target) = ProjectLayout::classify(path)
        {
            observed.insert(target, ObservedPath::Absent);
        }
    }
    Ok(observed)
}

/// Add requested destinations only when captured membership or absence proves their prior state.
pub fn observed_files_for(
    snapshot: &NativeSnapshot,
    targets: impl IntoIterator<Item = ManagedPath>,
) -> Result<BTreeMap<ManagedPath, ObservedPath>> {
    let mut observed = observed_files(snapshot)?;
    extend_observations(snapshot, &mut observed, targets)?;
    Ok(observed)
}

/// Project replacement owns managed content and explicitly missing template seeds. Other
/// captured user templates remain read-set evidence, never implicit mutation targets.
pub(super) fn observed_project_replacement_for(
    snapshot: &NativeSnapshot,
    targets: impl IntoIterator<Item = ManagedPath>,
) -> Result<BTreeMap<ManagedPath, ObservedPath>> {
    let targets: BTreeSet<_> = targets.into_iter().collect();
    let mut observed = observed_files_for(snapshot, targets.iter().cloned())?;
    observed.retain(|target, _| {
        !matches!(
            target,
            ManagedPath::UserTemplate(_) | ManagedPath::Scaffold(_)
        ) || targets.contains(target)
    });
    Ok(observed)
}

/// Read-only inputs outside the publication footprint remain in the native snapshot.
/// Only explicitly selected artifact destinations participate in this file plan.
pub fn observed_artifacts_for(
    snapshot: &NativeSnapshot,
    targets: impl IntoIterator<Item = ManagedPath>,
) -> Result<BTreeMap<ManagedPath, ObservedPath>> {
    let targets: Vec<_> = targets.into_iter().collect();
    ensure!(
        targets
            .iter()
            .all(|target| matches!(target, ManagedPath::Artifact(_))),
        "Artifact plan contains another managed role"
    );
    let mut observed = BTreeMap::new();
    extend_observations(snapshot, &mut observed, targets)?;
    Ok(observed)
}
/// Narrow document/content mutations keep other captured inputs outside the write footprint.
/// Callers must separately establish semantic ownership of every selected target.
pub(super) fn observed_mutation_for(
    snapshot: &NativeSnapshot,
    targets: impl IntoIterator<Item = ManagedPath>,
) -> Result<BTreeMap<ManagedPath, ObservedPath>> {
    let targets: Vec<_> = targets.into_iter().collect();
    ensure!(
        targets.iter().all(|target| matches!(
            target,
            ManagedPath::IntentDocument
                | ManagedPath::LockDocument
                | ManagedPath::BackendDocument(_)
                | ManagedPath::Content { .. }
        )),
        "Mutation cannot own templates or distributions"
    );
    let mut observed = BTreeMap::new();
    extend_observations(snapshot, &mut observed, targets)?;
    Ok(observed)
}
fn extend_observations(
    snapshot: &NativeSnapshot,
    observed: &mut BTreeMap<ManagedPath, ObservedPath>,
    targets: impl IntoIterator<Item = ManagedPath>,
) -> Result<()> {
    for target in targets {
        if observed.contains_key(&target) {
            continue;
        }
        let path = ProjectLayout::path(&target)?;
        ensure!(
            ProjectLayout::classify(&path)? == target,
            "Noncanonical managed target role"
        );
        if let Some(Observation::File(file)) = snapshot.entries().get(&path) {
            observed.insert(target, ObservedPath::File(content(file)));
            continue;
        }
        if matches!(snapshot.entries().get(&path), Some(Observation::Absent)) {
            observed.insert(target, ObservedPath::Absent);
            continue;
        }
        if matches!(
            snapshot.entries().get(&path),
            Some(Observation::Directory { .. } | Observation::Ancestor(_))
        ) {
            observed.insert(target, ObservedPath::Directory);
            continue;
        }
        let mut absent = false;
        for (index, _) in path.as_str().match_indices('/') {
            let prefix = empack_core::path::PortableRelPath::parse(
                &path.as_str()[..index],
                empack_core::path::PathSyntax::ProjectContent,
            )?;
            let next = path.as_str()[index + 1..].split('/').next().unwrap();
            match snapshot.entries().get(&prefix) {
                Some(Observation::Absent) => absent = true,
                Some(Observation::Directory { members, .. })
                    if !members.contains(next) && snapshot.membership_covers(&path)? =>
                {
                    absent = true
                }
                _ => {}
            }
        }
        ensure!(
            absent,
            "Target has no captured absence or file observation: {}",
            path.as_str()
        );
        observed.insert(target, ObservedPath::Absent);
    }
    Ok(())
}

/// A file-level proof only. A completed project still requires semantic and artifact verification.
/// Fields are private, so publication code cannot substitute unchecked candidates.
pub struct VerifiedFileChange {
    plan: FilePlan,
    stage: FrozenStage,
    base: NativeSnapshot,
}
impl VerifiedFileChange {
    pub(in crate::engine) fn stage_mut(&mut self) -> &mut FrozenStage {
        &mut self.stage
    }

    pub(super) fn into_parts(self) -> (FilePlan, FrozenStage, NativeSnapshot) {
        (self.plan, self.stage, self.base)
    }
    pub fn plan(&self) -> &FilePlan {
        &self.plan
    }
    /// Consume a candidate only when its complete selected file inventory matches the pure plan.
    #[cfg(test)]
    pub fn verify(base: NativeSnapshot, plan: FilePlan, stage: FrozenStage) -> Result<Self> {
        let observed = observed_files_for(
            &base,
            plan.expected()
                .keys()
                .cloned()
                .chain(plan.changes().iter().map(|change| change.target().clone())),
        )?;
        Self::verify_observed(base, plan, stage, observed)
    }
    /// User templates outside the explicit plan remain captured inputs. Project creation and
    /// replacement may seed missing templates, but cannot reset or delete existing user files.
    pub(super) fn verify_project_replacement(
        base: NativeSnapshot,
        plan: FilePlan,
        stage: FrozenStage,
    ) -> Result<Self> {
        for change in plan.changes() {
            if matches!(
                change.target(),
                ManagedPath::UserTemplate(_) | ManagedPath::Scaffold(_)
            ) {
                ensure!(
                    matches!(
                        change,
                        FileChange::Replace {
                            before: ObservedPath::Absent,
                            ..
                        }
                    ),
                    "Project replacement cannot overwrite or delete user templates or scaffolds"
                );
            }
        }
        let observed = observed_project_replacement_for(
            &base,
            plan.expected()
                .keys()
                .cloned()
                .chain(plan.changes().iter().map(|change| change.target().clone())),
        )?;
        Self::verify_observed(base, plan, stage, observed)
    }
    /// Verify only explicitly owned document/content changes, retaining the complete read set.
    pub(super) fn verify_mutation(
        base: NativeSnapshot,
        plan: FilePlan,
        stage: FrozenStage,
    ) -> Result<Self> {
        let observed = observed_mutation_for(
            &base,
            plan.expected()
                .keys()
                .cloned()
                .chain(plan.changes().iter().map(|change| change.target().clone())),
        )?;
        Self::verify_observed(base, plan, stage, observed)
    }
    /// Verify an artifact-only candidate without copying or gaining write authority over source inputs.
    pub fn verify_artifacts(
        base: NativeSnapshot,
        plan: FilePlan,
        stage: FrozenStage,
    ) -> Result<Self> {
        let observed = observed_artifacts_for(
            &base,
            plan.expected()
                .keys()
                .cloned()
                .chain(plan.changes().iter().map(|change| change.target().clone())),
        )?;
        Self::verify_observed(base, plan, stage, observed)
    }
    fn verify_observed(
        base: NativeSnapshot,
        plan: FilePlan,
        stage: FrozenStage,
        observed: BTreeMap<ManagedPath, ObservedPath>,
    ) -> Result<Self> {
        ensure!(
            plan.capabilities() == native_capabilities(),
            "File plan uses different filesystem capabilities"
        );
        let removals: BTreeSet<_> = plan
            .changes()
            .iter()
            .filter_map(|change| {
                if let FileChange::Remove { target, .. } = change {
                    Some(target.clone())
                } else {
                    None
                }
            })
            .collect();
        ensure!(
            plan_files(&observed, plan.expected(), &removals)? == plan,
            "File plan was prepared against different observations"
        );
        for change in plan.changes() {
            let target = ProjectLayout::path(change.target())?;
            // A different logical role may not silently select the same native target.
            ensure!(
                ProjectLayout::classify(&target)? == *change.target(),
                "Noncanonical managed target role"
            );
            let before = match change {
                FileChange::Replace { before, .. } => before.clone(),
                FileChange::Remove { before, .. } => ObservedPath::File(before.clone()),
            };
            ensure!(
                observed.get(change.target()) == Some(&before),
                "File plan does not match its native base"
            );
        }
        let mut expected = BTreeMap::new();
        let mut collisions = CollisionIndex::default();
        for (target, file) in plan.expected() {
            let path = ProjectLayout::path(target)?;
            ensure!(
                ProjectLayout::classify(&path)? == *target,
                "Noncanonical managed target role"
            );
            collisions.insert_file(&path)?;
            expected.insert(path, file.clone());
        }
        candidate_stage_limits(&base, &plan)?;
        let actual: BTreeMap<_, _> = stage
            .inventory()
            .iter()
            .filter_map(|(path, observation)| {
                if let Observation::File(file) = observation {
                    Some((path.clone(), content(file)))
                } else {
                    None
                }
            })
            .collect();
        ensure!(
            actual.len() == expected.len()
                && actual.iter().all(|(path, file)| expected
                    .get(path)
                    .is_some_and(|expected| file.equivalent(expected, native_capabilities()))),
            "Candidate inventory differs from planned files, bytes or permissions"
        );
        // A caller cannot omit observed files by constructing a plan against another observation set.
        for (target, before) in &observed {
            if matches!(before, ObservedPath::File(_)) {
                ensure!(plan.expected().contains_key(target) || plan.changes().iter().any(|change|
                    matches!(change, FileChange::Remove { target: removed, .. } if removed == target)),
                    "File plan silently drops an observed file");
            }
        }
        Ok(Self { plan, stage, base })
    }
}

/// Preflight the complete resulting read set against each captured allowance, then
/// bound private staging to precisely the planned files. This does not verify bytes.
pub(super) fn candidate_stage_limits(
    base: &NativeSnapshot,
    plan: &FilePlan,
) -> Result<SnapshotLimits> {
    let expected: BTreeMap<_, _> = plan
        .expected()
        .iter()
        .map(|(target, file)| Ok((ProjectLayout::path(target)?, file)))
        .collect::<Result<_>>()?;
    // Recovery recaptures each group with its original limits. Check the complete
    // post-publication read set now, including unchanged sources, before any write.
    let mut after: BTreeMap<_, _> = base
        .entries()
        .iter()
        .filter_map(|(path, entry)| {
            if let Observation::File(file) = entry {
                Some((path.clone(), file.bytes))
            } else {
                None
            }
        })
        .collect();
    for change in plan.changes() {
        after.remove(&ProjectLayout::path(change.target())?);
    }
    after.extend(
        expected
            .iter()
            .map(|(path, file)| (path.clone(), file.bytes)),
    );
    for group in base.groups() {
        let scopes = &group.scopes;
        let limits = group.limits;
        let selected = |path: &PortableRelPath| {
            scopes.iter().any(|scope| {
                path == scope
                    || path
                        .as_str()
                        .strip_prefix(scope.as_str())
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        };
        let mut total = 0u64;
        let mut entries = BTreeSet::new();
        for (path, bytes) in after.iter().filter(|(path, _)| selected(path)) {
            total = total
                .checked_add(*bytes)
                .ok_or_else(|| anyhow::anyhow!("Candidate byte count overflow"))?;
            ensure!(
                *bytes <= limits.file_bytes
                    && total <= limits.total_bytes
                    && path.components().count() <= limits.depth,
                "Candidate exceeds publication read budget"
            );
            entries.insert(path.as_str());
            for (index, _) in path.as_str().match_indices('/') {
                entries.insert(&path.as_str()[..index]);
            }
        }
        // Keep captured directory/absence entries too; absent removed files can remain
        // selected scopes during recovery. Conservative counting is safe for custom caps.
        entries.extend(
            base.entries()
                .keys()
                .filter(|path| selected(path))
                .map(|path| path.as_str()),
        );
        ensure!(
            entries.len() <= limits.entries,
            "Candidate exceeds snapshot entry budget"
        );
    }
    let mut limits = SnapshotLimits {
        entries: 0,
        depth: 0,
        file_bytes: 0,
        total_bytes: 0,
    };
    let mut entries = BTreeSet::new();
    for (path, file) in &expected {
        limits.file_bytes = limits.file_bytes.max(file.bytes);
        limits.total_bytes = limits
            .total_bytes
            .checked_add(file.bytes)
            .ok_or_else(|| anyhow::anyhow!("Candidate byte count overflow"))?;
        limits.depth = limits.depth.max(path.components().count());
        entries.insert(path.as_str());
        for (index, _) in path.as_str().match_indices('/') {
            entries.insert(&path.as_str()[..index]);
        }
    }
    limits.entries = entries.len();
    Ok(limits)
}

pub(super) fn content(file: &FileObservation) -> FileContent {
    FileContent {
        content: ContentId::from_sha256(file.content),
        bytes: file.bytes,
        permissions: FilePermissions {
            readonly: file.readonly,
            #[cfg(unix)]
            executable: file.mode & 0o100 != 0,
            #[cfg(not(unix))]
            executable: false,
        },
    }
}

#[cfg(test)]
mod tests;
