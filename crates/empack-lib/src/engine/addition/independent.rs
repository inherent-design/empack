//! Group requests whose dependencies or native content footprints cannot retire independently.
use crate::engine::layout::{ProjectLayout, collision_key};
use anyhow::Result;
use empack_core::{addition::AdditionGroup, files::ManagedPath, model::*};
use std::collections::BTreeSet;

/// Partition resolved requests before execution. Each returned component contains indices
/// into `groups`, in input order. A failed member blocks its whole component. Connections
/// include shared identities, labels, required closure in the current project, old and new
/// destination overlaps, and tracked source reads overlapping another request's writes.
/// This does not prove native ownership or permit publishing a component without verification.
pub fn independent_components(
    current: &ResolvedProject,
    groups: &[AdditionGroup],
) -> Result<Vec<Vec<usize>>> {
    let footprints: Vec<_> = groups
        .iter()
        .map(|group| Footprint::new(current, group))
        .collect::<Result<_>>()?;
    let mut components = Vec::new();
    let mut assigned = vec![false; groups.len()];
    for first in 0..groups.len() {
        if assigned[first] {
            continue;
        }
        assigned[first] = true;
        let mut component = vec![first];
        let mut cursor = 0;
        while cursor < component.len() {
            let from = component[cursor];
            for next in 0..groups.len() {
                if !assigned[next] && footprints[from].overlaps(&footprints[next]) {
                    assigned[next] = true;
                    component.push(next);
                }
            }
            cursor += 1;
        }
        component.sort_unstable();
        components.push(component);
    }
    Ok(components)
}
struct Footprint<'a> {
    labels: BTreeSet<&'a DependencyKey>,
    identities: Vec<&'a ResolvedIdentity>,
    required: BTreeSet<&'a DependencyKey>,
    writes: Vec<String>,
    reads: Vec<String>,
}
impl<'a> Footprint<'a> {
    fn new(current: &'a ResolvedProject, group: &'a AdditionGroup) -> Result<Self> {
        let mut result = Self {
            labels: BTreeSet::new(),
            identities: Vec::new(),
            required: BTreeSet::new(),
            writes: Vec::new(),
            reads: Vec::new(),
        };
        for (key, dependency) in group.dependencies() {
            result.labels.insert(key);
            result.identities.push(&dependency.identity);
            result.capture_files(dependency)?;
            for (old_key, old) in current
                .lock()
                .dependencies
                .iter()
                .filter(|(old_key, old)| *old_key == key || old.identity == dependency.identity)
            {
                result.labels.insert(old_key);
                result.capture_files(old)?;
            }
        }
        let mut pending: Vec<_> = result.labels.iter().copied().collect();
        while let Some(key) = pending.pop() {
            if result.required.insert(key) {
                pending.extend(current.lock().required_edges.get(key).into_iter().flatten());
            }
        }
        Ok(result)
    }
    fn capture_files(&mut self, dependency: &LockedDependency) -> Result<()> {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                self.writes.push(collision_key(path.as_str()));
            }
            match &file.acquisition {
                AcquisitionSpec::Local(path) | AcquisitionSpec::Embedded { archive: path, .. } => {
                    self.reads.push(collision_key(path.as_str()));
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn overlaps(&self, other: &Self) -> bool {
        self.labels.iter().any(|key| other.required.contains(key))
            || other.labels.iter().any(|key| self.required.contains(key))
            || self
                .identities
                .iter()
                .any(|identity| other.identities.contains(identity))
            || self.writes.iter().any(|a| {
                other
                    .writes
                    .iter()
                    .chain(&other.reads)
                    .any(|b| paths_overlap(a, b))
            })
            || other
                .writes
                .iter()
                .any(|a| self.reads.iter().any(|b| paths_overlap(a, b)))
    }
}
fn paths_overlap(a: &str, b: &str) -> bool {
    a == b
        || a.strip_prefix(b).is_some_and(|tail| tail.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|tail| tail.starts_with('/'))
}

#[cfg(test)]
mod tests;
