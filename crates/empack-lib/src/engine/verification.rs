//! Independent file inventory checks. Semantic/artifact checks compose above this boundary.
use super::{
    layout::{CollisionIndex, ProjectLayout},
    snapshot::{FileObservation, NativeSnapshot, Observation},
    staging::FrozenStage,
};
use anyhow::{Result, ensure};
use empack_core::{
    digest::ContentId,
    files::{FileChange, FileContent, FilePermissions, FilePlan, ManagedPath, ObservedPath},
};
use std::collections::{BTreeMap, BTreeSet};

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
    for target in targets {
        if observed.contains_key(&target) {
            continue;
        }
        let path = ProjectLayout::path(&target)?;
        ensure!(
            ProjectLayout::classify(&path)? == target,
            "Noncanonical managed target role"
        );
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
                Some(Observation::Directory { members, .. }) if !members.contains(next) => {
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
    Ok(observed)
}

/// A file-level proof only. A completed project still requires semantic and artifact verification.
/// Fields are private, so publication code cannot substitute unchecked candidates.
pub struct VerifiedFileChange {
    plan: FilePlan,
    stage: FrozenStage,
    base: NativeSnapshot,
}
impl VerifiedFileChange {
    pub(super) fn into_parts(self) -> (FilePlan, FrozenStage, NativeSnapshot) {
        (self.plan, self.stage, self.base)
    }
    pub fn plan(&self) -> &FilePlan {
        &self.plan
    }
    pub fn base(&self) -> &NativeSnapshot {
        &self.base
    }
    /// Consume a candidate only when its complete selected file inventory matches the pure plan.
    pub fn verify(base: NativeSnapshot, plan: FilePlan, stage: FrozenStage) -> Result<Self> {
        let observed = observed_files_for(
            &base,
            plan.expected()
                .keys()
                .cloned()
                .chain(plan.changes().iter().map(|change| change.target().clone())),
        )?;
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
            FilePlan::prepare(&observed, plan.expected(), &removals)? == plan,
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
        let limits = base.limits();
        ensure!(
            stage.inventory().len() <= limits.entries,
            "Candidate exceeds snapshot entry budget"
        );
        let mut total = 0u64;
        for (path, file) in &expected {
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| anyhow::anyhow!("Candidate byte count overflow"))?;
            ensure!(
                file.bytes <= limits.file_bytes
                    && total <= limits.total_bytes
                    && path.components().count() <= limits.depth,
                "Candidate exceeds publication read budget"
            );
        }
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
            actual == expected,
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
