//! Conservative dependency and path components for one combined batch candidate.
use super::*;

impl AdditionGroup {
    /// Combine successful components before planning one document replacement. Identical shared
    /// records may coalesce; differing roots, selections or dependency evidence are conflicts.
    /// No failed request is silently selected by iteration order.
    pub fn combine(current: &ResolvedProject, groups: &[&Self]) -> Result<Self, AdditionError> {
        let mut intent = current.intent().clone();
        intent.roots.clear();
        let mut lock = current.lock().clone();
        lock.dependencies.clear();
        lock.required_edges.clear();
        lock.coverage.clear();
        for group in groups {
            if group.lock.runtime != lock.runtime
                || group.lock.acceptable_versions != lock.acceptable_versions
            {
                return Err(AdditionError::RuntimeMismatch);
            }
            for (key, root) in &group.roots {
                if intent.roots.get(key).is_some_and(|old| old != root) {
                    return Err(AdditionError::BatchConflict(key.clone()));
                }
                intent.roots.insert(key.clone(), root.clone());
            }
            for (key, dependency) in &group.lock.dependencies {
                if let Some(old) = lock.dependencies.get(key) {
                    if old != dependency
                        || lock.coverage.get(key) != group.lock.coverage.get(key)
                        || lock.required_edges.get(key).cloned().unwrap_or_default()
                            != group
                                .lock
                                .required_edges
                                .get(key)
                                .cloned()
                                .unwrap_or_default()
                    {
                        return Err(AdditionError::BatchConflict(key.clone()));
                    }
                } else {
                    lock.dependencies.insert(key.clone(), dependency.clone());
                    lock.coverage.insert(key.clone(), group.lock.coverage[key]);
                    if let Some(edges) = group.lock.required_edges.get(key) {
                        lock.required_edges.insert(key.clone(), edges.clone());
                    }
                }
            }
        }
        let revision = lock.intent_revision;
        let combined = ResolvedProject::validate(intent, lock, revision)
            .map_err(AdditionError::InvalidProject)?;
        Self::from_resolved(&combined)
    }
}
