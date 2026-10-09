//! Conservative dependency and path components for one combined batch candidate.
use super::*;

impl AdditionGroup {
    /// Authored request roots, excluding transitive selections.
    pub fn roots(&self) -> &BTreeMap<DependencyKey, DependencyIntent> {
        &self.roots
    }

    /// One root and its complete declared required closure per group. Shared dependencies remain
    /// present in every relevant group so independence analysis can reconnect their owners.
    pub fn root_groups(&self) -> Vec<Self> {
        self.roots
            .iter()
            .map(|(key, root)| {
                let mut reachable = BTreeSet::new();
                let mut pending = alloc::vec![key.clone()];
                while let Some(next) = pending.pop() {
                    if reachable.insert(next.clone()) {
                        pending.extend(
                            self.lock
                                .required_edges
                                .get(&next)
                                .into_iter()
                                .flatten()
                                .cloned(),
                        );
                    }
                }
                let mut lock = self.lock.clone();
                lock.dependencies.retain(|key, _| reachable.contains(key));
                lock.coverage.retain(|key, _| reachable.contains(key));
                lock.required_edges.retain(|key, _| reachable.contains(key));
                Self {
                    roots: [(key.clone(), root.clone())].into(),
                    lock,
                }
            })
            .collect()
    }

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
