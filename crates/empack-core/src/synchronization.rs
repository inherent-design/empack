//! Reconcile changed intent without treating synchronization as an implicit update or cleanup.
use crate::model::*;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

/// Rebind an explicit provider label to its unique existing identity without changing its
/// selection or losing graph edges. A still-explicit old label or occupied new key is not renamed.
pub fn rebind_prior_aliases(
    intent: &ProjectIntent,
    prior: &ResolutionLock,
) -> Result<ResolutionLock, ModelError> {
    prior.validate_structure()?;
    let mut aliases = BTreeMap::new();
    for (requested, root) in &intent.roots {
        if prior.dependencies.contains_key(requested) {
            continue;
        }
        let SourceIntent::Provider(project) = &root.source else {
            continue;
        };
        if let Some((existing, _)) = prior.dependencies.iter().find(|(key, dependency)| {
            !intent.roots.contains_key(*key) && matches!(&dependency.identity, ResolvedIdentity::Provider(value) if value == project)
        }) && aliases.insert(existing.clone(), requested.clone()).is_some() {
            return Err(ModelError("Multiple requested labels claim one prior identity".into()));
        }
    }
    let bound = |key: &DependencyKey| aliases.get(key).unwrap_or(key).clone();
    let mut lock = prior.clone();
    lock.dependencies = prior
        .dependencies
        .iter()
        .map(|(key, value)| (bound(key), value.clone()))
        .collect();
    lock.coverage = prior
        .coverage
        .iter()
        .map(|(key, value)| (bound(key), *value))
        .collect();
    lock.required_edges = prior
        .required_edges
        .iter()
        .map(|(key, edges)| (bound(key), edges.iter().map(bound).collect()))
        .collect();
    lock.validate_structure()?;
    Ok(lock)
}

/// Checked scope of fresh resolution. Native observations and publication remain separate.
#[derive(Debug, Clone)]
pub struct SynchronizationResolution {
    affected_roots: BTreeSet<DependencyKey>,
    changed: BTreeSet<DependencyKey>,
}
impl SynchronizationResolution {
    /// Admit only resolutions justified by changed or missing root intent and required closure.
    /// Previously valid explicit roots and unrelated installations retain their exact evidence.
    pub fn prepare(
        intent: &ProjectIntent,
        prior: &ResolutionLock,
        proposed: &ResolvedProject,
    ) -> Result<Self, ModelError> {
        let normalized_prior = rebind_prior_aliases(intent, prior)?;
        let prior = &normalized_prior;
        if proposed.intent() != intent {
            return Err(ModelError(
                "Synchronization resolution changes authoring intent".into(),
            ));
        }
        let runtime_changed = prior.runtime.minecraft != intent.runtime.minecraft
            || prior.runtime.loader != intent.runtime.loader
            || (intent.runtime.loader_version.is_some()
                && prior.runtime.loader_version != intent.runtime.loader_version);
        if !runtime_changed && proposed.lock().runtime != prior.runtime {
            return Err(ModelError(
                "Synchronization cannot upgrade a valid locked runtime".into(),
            ));
        }
        let affected_roots: BTreeSet<_> = intent
            .roots
            .iter()
            .filter_map(|(key, root)| {
                // Search is unresolved authoring intent. Without a per-query lock declaration, a
                // changed document cannot establish that its previous search selection still fits.
                let search_changed = matches!(root.source, SourceIntent::Search { .. })
                    && prior.intent_revision != proposed.lock().intent_revision;
                (runtime_changed
                    || search_changed
                    || prior
                        .dependencies
                        .get(key)
                        .is_none_or(|selected| root.validate_selection(key, selected).is_err()))
                .then_some(key.clone())
            })
            .collect();
        let mut allowed = BTreeSet::new();
        let mut pending: Vec<_> = affected_roots.iter().cloned().collect();
        while let Some(key) = pending.pop() {
            if allowed.insert(key.clone()) {
                pending.extend(
                    proposed
                        .lock()
                        .required_edges
                        .get(&key)
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
            }
        }
        let mut changed = BTreeSet::new();
        for (key, previous) in &prior.dependencies {
            let next = proposed.lock().dependencies.get(key).ok_or_else(|| {
                ModelError("Synchronization cannot discard a prior installation".into())
            })?;
            let retained_root = intent.roots.contains_key(key) && !affected_roots.contains(key);
            if !allowed.contains(key) || retained_root {
                if previous != next
                    || prior.coverage.get(key) != proposed.lock().coverage.get(key)
                    || prior.required_edges.get(key) != proposed.lock().required_edges.get(key)
                {
                    return Err(ModelError("Synchronization changes a valid retained selection; request an explicit update".into()));
                }
            } else if previous != next {
                changed.insert(key.clone());
                if previous.selected != next.selected
                    && prior.required_edges.iter().any(|(dependent, edges)| {
                        !allowed.contains(dependent) && edges.contains(key)
                    })
                {
                    return Err(ModelError(
                        "Changed selection has retained dependents requiring explicit resolution"
                            .into(),
                    ));
                }
            }
        }
        for key in proposed.lock().dependencies.keys() {
            if !prior.dependencies.contains_key(key) {
                if !allowed.contains(key) {
                    return Err(ModelError(
                        "Synchronization includes an unjustified new installation".into(),
                    ));
                }
                changed.insert(key.clone());
            }
        }
        Ok(Self {
            affected_roots,
            changed,
        })
    }
    /// Explicit roots whose previous lock no longer satisfies current intent.
    pub fn affected_roots(&self) -> &BTreeSet<DependencyKey> {
        &self.affected_roots
    }
    /// Logical records whose exact installed description is new or changed.
    pub fn changed(&self) -> &BTreeSet<DependencyKey> {
        &self.changed
    }
}
