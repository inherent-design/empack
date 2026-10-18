//! World ownership is decided before observing or reconciling individual members.
use super::super::release::{validate_world_root, within_world};
use super::*;
use cap_fs_ext::DirExt;

pub(super) struct WorldInventories<'a> {
    pub old: &'a mut BTreeMap<ManagedPath, ReleaseFile>,
    pub incoming: &'a mut BTreeMap<ManagedPath, ReleaseFile>,
}
pub(super) fn protect(
    root: &ProjectReadRoot,
    layout: InstanceLayout,
    previous: Option<&InstanceRecord>,
    inventories: WorldInventories<'_>,
    mut snapshot: NativeSnapshot,
    limits: SnapshotLimits,
    cancel: &Cancellation,
) -> Result<(Vec<String>, NativeSnapshot)> {
    let WorldInventories { old, incoming } = inventories;
    let mut initialized: BTreeSet<String> = previous
        .map(|record| record.initialized_worlds.iter().cloned().collect())
        .unwrap_or_default();
    initialized.extend(old.values().filter_map(|file| file.world_root.clone()));
    let groups: BTreeSet<_> = incoming
        .values()
        .filter_map(|file| file.world_root.clone())
        .collect();
    let mut protected = initialized.clone();
    for group in &groups {
        validate_world_root(group)?;
        if protected.iter().any(|prior| overlaps(prior, group)) {
            protected.insert(group.clone());
            continue;
        }
        let destination = ProjectLayout::path(&layout.target(path(group)?))?;
        if occupied(root, &destination, cancel)? {
            protected.insert(group.clone());
        } else {
            // Captured group absence prevents a concurrent creator from receiving seed members.
            let captured = root.capture(std::slice::from_ref(&destination), limits, cancel)?;
            ensure!(
                matches!(
                    captured.entries().get(&destination),
                    Some(super::super::snapshot::Observation::Absent)
                ),
                "World destination changed during preparation"
            );
            snapshot = snapshot.merge(captured)?;
        }
    }
    initialized.extend(groups);
    let preserve = |file: &ReleaseFile| {
        protected.iter().any(|group| {
            super::super::layout::collision_key(&file.destination)
                == super::super::layout::collision_key(group)
                || within_world(&file.destination, group)
        })
    };
    old.retain(|_, file| !preserve(file));
    incoming.retain(|_, file| !preserve(file));
    Ok((initialized.into_iter().collect(), snapshot))
}
fn overlaps(a: &str, b: &str) -> bool {
    super::super::layout::collision_key(a) == super::super::layout::collision_key(b)
        || within_world(a, b)
        || within_world(b, a)
}
fn occupied(
    root: &ProjectReadRoot,
    destination: &PortableRelPath,
    cancel: &Cancellation,
) -> Result<bool> {
    let mut directory = root.directory.try_clone()?;
    let mut parts = destination.components().peekable();
    while let Some(part) = parts.next() {
        cancel.check()?;
        let metadata = match directory.symlink_metadata(part) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if parts.peek().is_none() || !metadata.is_dir() || metadata.is_symlink() {
            return Ok(true);
        }
        directory = directory.open_dir_nofollow(part)?;
    }
    Ok(true)
}
