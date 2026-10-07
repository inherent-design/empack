//! Merge a resolved dependency request without renaming existing identities or collecting content.
use crate::{
    model::*,
    removal::{RemovalError, RemovalEvidencePolicy, RemovalMode, RemovalPlan},
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::fmt;

/// A validated, root-reachable request group. Global pack settings are not part of an addition.
#[derive(Debug, Clone)]
pub struct AdditionGroup {
    roots: BTreeMap<DependencyKey, DependencyIntent>,
    lock: ResolutionLock,
}
impl AdditionGroup {
    /// Extract dependency intent and exact evidence from a coherent resolved request.
    /// Every proposed installation needs an explicit root or a known required path from one.
    pub fn from_resolved(project: &ResolvedProject) -> Result<Self, AdditionError> {
        if project.intent().roots.is_empty() {
            return Err(AdditionError::EmptyRequest);
        }
        let mut reachable = BTreeSet::new();
        let mut pending: Vec<_> = project.intent().roots.keys().cloned().collect();
        while let Some(key) = pending.pop() {
            if reachable.insert(key.clone()) {
                pending.extend(
                    project
                        .lock()
                        .required_edges
                        .get(&key)
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
            }
        }
        if let Some(key) = project
            .lock()
            .dependencies
            .keys()
            .find(|key| !reachable.contains(*key))
        {
            return Err(AdditionError::UnrequestedSelection(key.clone()));
        }
        Ok(Self {
            roots: project.intent().roots.clone(),
            lock: project.lock().clone(),
        })
    }
}

/// Exact logical records authorized for replacement by explicitly requested roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementSelection {
    /// Every key must be installed and appear as an explicit replacement root.
    pub keys: NonEmpty<DependencyKey>,
    /// Missing dependency evidence requires an explicit policy choice.
    pub evidence: RemovalEvidencePolicy,
}

/// A resolved addition cannot silently displace another logical record or retained requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdditionError {
    /// No explicit root requested an installation.
    EmptyRequest,
    /// The old selection cannot safely leave the resulting project.
    Replacement(RemovalError),
    /// Replacement must explicitly supply the selected logical record.
    MissingReplacementRoot(DependencyKey),
    /// An update selected an identity that is not installed.
    UpdateMissing(DependencyKey),
    /// The proposed closure includes an unjustified selection.
    UnrequestedSelection(DependencyKey),
    /// Dependency resolution used another game or loader runtime.
    RuntimeMismatch,
    /// A proposed label belongs to another identity, or two requests map to one label.
    OccupiedKey(DependencyKey),
    /// Non-provider identity does not agree with its logical key.
    InvalidLogicalIdentity(DependencyKey),
    /// A shared, unrequested installation differs from the new dependency requirement.
    RetainedSelectionConflict(DependencyKey),
    /// Updating a selection could invalidate known retained dependents.
    RequiredBy(Vec<DependencyKey>),
    /// The complete next project violates a semantic invariant.
    InvalidProject(ModelError),
}
impl fmt::Display for AdditionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UpdateMissing(key) => write!(f, "Cannot update an uninstalled identity: {}", key.as_str()),
            Self::Replacement(error) => error.fmt(f),
            Self::MissingReplacementRoot(key) => write!(f, "Replacement requires an explicit root for {}", key.as_str()),
            Self::EmptyRequest => f.write_str("Addition requires an explicit dependency root"),
            Self::UnrequestedSelection(key) => write!(f, "Unrequested installation in addition: {}", key.as_str()),
            Self::RuntimeMismatch => f.write_str("Addition was resolved for a different runtime"),
            Self::OccupiedKey(key) => write!(f, "Dependency label already names another selection: {}", key.as_str()),
            Self::InvalidLogicalIdentity(key) => write!(f, "File identity differs from logical key: {}", key.as_str()),
            Self::RetainedSelectionConflict(key) => write!(f, "Required selection conflicts with retained content: {}; resolve its update explicitly", key.as_str()),
            Self::RequiredBy(_) => f.write_str("Selection update has retained required dependents; resolve those dependents in the same request"),
            Self::InvalidProject(error) => error.fmt(f),
        }
    }
}
impl core::error::Error for AdditionError {}

/// Pure desired-state merge. Source capture and verified byte publication remain separate.
#[derive(Debug, Clone)]
pub struct AdditionPlan {
    intent: ProjectIntent,
    lock: ResolutionLock,
    bindings: BTreeMap<DependencyKey, DependencyKey>,
    changed: BTreeSet<DependencyKey>,
    existing_roots: BTreeSet<DependencyKey>,
    replaced: BTreeMap<DependencyKey, LockedDependency>,
    incomplete: Vec<DependencyKey>,
}
impl AdditionPlan {
    /// Describe selected observed installations when no prior lock exists. Existing authoring
    /// roots remain; the complete resulting intent must be resolved by the proposed group.
    /// Native adoption still has to verify every selected placement before publication.
    pub fn prepare_initial_adoption(
        source: &ProjectIntent,
        group: &AdditionGroup,
    ) -> Result<Self, AdditionError> {
        let mut intent = source.clone();
        intent.roots.extend(group.roots.clone());
        let lock = group.lock.clone();
        ResolvedProject::validate(intent.clone(), lock.clone(), lock.intent_revision)
            .map_err(AdditionError::InvalidProject)?;
        Ok(Self {
            intent,
            bindings: lock
                .dependencies
                .keys()
                .map(|key| (key.clone(), key.clone()))
                .collect(),
            changed: lock.dependencies.keys().cloned().collect(),
            existing_roots: BTreeSet::new(),
            replaced: BTreeMap::new(),
            incomplete: Vec::new(),
            lock,
        })
    }
    /// Resolve canonical identities before any label or file mutation. Existing aliases win.
    /// Unlisted selections remain; differing shared dependencies require explicit resolution.
    pub fn prepare(
        current: &ResolvedProject,
        group: &AdditionGroup,
    ) -> Result<Self, AdditionError> {
        if current.lock().runtime != group.lock.runtime {
            return Err(AdditionError::RuntimeMismatch);
        }
        let mut bindings = BTreeMap::new();
        let mut chosen = BTreeSet::new();
        for (key, dependency) in &group.lock.dependencies {
            let destination = match &dependency.identity {
                ResolvedIdentity::Provider(identity) => current.lock().dependencies.iter()
                    .find(|(_, existing)| matches!(&existing.identity, ResolvedIdentity::Provider(value) if value == identity))
                    .map(|(key, _)| key).unwrap_or(key),
                ResolvedIdentity::Url(identity) | ResolvedIdentity::Local(identity) => {
                    if identity != key { return Err(AdditionError::InvalidLogicalIdentity(key.clone())); }
                    key
                }
            };
            if !chosen.insert(destination.clone())
                || current
                    .lock()
                    .dependencies
                    .get(destination)
                    .is_some_and(|existing| existing.identity != dependency.identity)
            {
                return Err(AdditionError::OccupiedKey(destination.clone()));
            }
            bindings.insert(key.clone(), destination.clone());
        }
        let roots: BTreeSet<_> = group
            .roots
            .keys()
            .map(|key| bindings[key].clone())
            .collect();
        let mut intent = current.intent().clone();
        let mut lock = current.lock().clone();
        let mut changed = BTreeSet::new();
        for (key, dependency) in &group.lock.dependencies {
            let destination = &bindings[key];
            let proposed: BTreeSet<_> = group
                .lock
                .required_edges
                .get(key)
                .into_iter()
                .flatten()
                .map(|edge| bindings[edge].clone())
                .collect();
            let preserve_evidence = lock.dependencies.get(destination).is_some_and(|existing| {
                same_installed_selection(existing, dependency)
                    || (existing.selected.is_some() && existing.selected == dependency.selected)
            });
            if let Some(existing) = lock.dependencies.get(destination) {
                if !roots.contains(destination) {
                    if !same_installed_selection(existing, dependency) {
                        return Err(AdditionError::RetainedSelectionConflict(
                            destination.clone(),
                        ));
                    }
                    // Keep original byte/provenance facts, but do not discard newly established
                    // required edges. Conflicting complete edge sets need explicit resolution.
                    merge_evidence(&mut lock, destination, &proposed, group.lock.coverage[key])?;
                    continue;
                }
                if existing.selected != dependency.selected {
                    let dependents: Vec<_> = current
                        .lock()
                        .required_edges
                        .iter()
                        .filter(|(from, edges)| {
                            !roots.contains(*from) && edges.contains(destination)
                        })
                        .map(|(key, _)| key.clone())
                        .collect();
                    if !dependents.is_empty() {
                        return Err(AdditionError::RequiredBy(dependents));
                    }
                }
            }
            if lock.dependencies.get(destination) != Some(dependency) {
                changed.insert(destination.clone());
            }
            lock.dependencies
                .insert(destination.clone(), dependency.clone());
            if preserve_evidence {
                merge_evidence(&mut lock, destination, &proposed, group.lock.coverage[key])?;
            } else {
                lock.coverage
                    .insert(destination.clone(), group.lock.coverage[key]);
                if proposed.is_empty() {
                    lock.required_edges.remove(destination);
                } else {
                    lock.required_edges.insert(destination.clone(), proposed);
                }
            }
        }
        for (key, root) in &group.roots {
            intent.roots.insert(bindings[key].clone(), root.clone());
        }
        // A codec will bind the actual next semantic revision before publication. The temporary
        // matching revision here checks every structural and intent/selection invariant now.
        ResolvedProject::validate(intent.clone(), lock.clone(), lock.intent_revision)
            .map_err(AdditionError::InvalidProject)?;
        Ok(Self {
            intent,
            lock,
            bindings,
            changed,
            replaced: BTreeMap::new(),
            incomplete: Vec::new(),
            existing_roots: roots
                .into_iter()
                .filter(|key| current.lock().dependencies.contains_key(key))
                .collect(),
        })
    }
    /// Compose removal safety and addition as one semantic candidate, never a published
    /// intermediate deletion. Canonical alias matching cannot redirect a selected replacement.
    pub fn prepare_replacement(
        current: &ResolvedProject,
        group: &AdditionGroup,
        selection: &ReplacementSelection,
    ) -> Result<Self, AdditionError> {
        for key in selection.keys.as_slice() {
            if !group.roots.contains_key(key) {
                return Err(AdditionError::MissingReplacementRoot(key.clone()));
            }
        }
        let removal = RemovalPlan::prepare_with_policy(
            current,
            &selection.keys,
            RemovalMode::RemoveContent,
            selection.evidence,
        )
        .map_err(AdditionError::Replacement)?;
        let replaced = removal.selected().clone();
        let incomplete = removal.incomplete_evidence().to_vec();
        let retained = removal
            .resolve(current.lock().intent_revision)
            .map_err(AdditionError::InvalidProject)?;
        let mut plan = Self::prepare(&retained, group)?;
        for key in &plan.existing_roots {
            if plan.changed.contains(key)
                || plan.intent.roots.get(key) != current.intent().roots.get(key)
            {
                return Err(AdditionError::OccupiedKey(key.clone()));
            }
        }
        for key in selection.keys.as_slice() {
            if plan.bindings.get(key) != Some(key) {
                return Err(AdditionError::OccupiedKey(key.clone()));
            }
            plan.existing_roots.insert(key.clone());
            plan.changed.insert(key.clone());
        }
        plan.replaced = replaced;
        plan.incomplete = incomplete;
        Ok(plan)
    }
    /// Prior exact identities and placements explicitly selected for replacement.
    pub fn replaced(&self) -> &BTreeMap<DependencyKey, LockedDependency> {
        &self.replaced
    }
    /// Retained records whose unknown dependency evidence was explicitly acknowledged.
    pub fn incomplete_evidence(&self) -> &[DependencyKey] {
        &self.incomplete
    }
    /// Refresh selected installed identities without rewriting authoring intent. Temporary roots
    /// in the resolved group identify the requested selections, including transitive records;
    /// they do not promote those records or relax exact pins in the published project.
    pub fn prepare_update(
        current: &ResolvedProject,
        group: &AdditionGroup,
    ) -> Result<Self, AdditionError> {
        let mut plan = Self::prepare(current, group)?;
        for key in group.roots.keys() {
            let bound = &plan.bindings[key];
            if !current.lock().dependencies.contains_key(bound) {
                return Err(AdditionError::UpdateMissing(bound.clone()));
            }
        }
        plan.intent = current.intent().clone();
        ResolvedProject::validate(
            plan.intent.clone(),
            plan.lock.clone(),
            plan.lock.intent_revision,
        )
        .map_err(AdditionError::InvalidProject)?;
        Ok(plan)
    }
    /// Explicitly requested roots which already have a locked installation.
    pub fn existing_roots(&self) -> &BTreeSet<DependencyKey> {
        &self.existing_roots
    }
    /// Next authoring intent; unrelated settings and roots retain their meaning.
    pub fn intent(&self) -> &ProjectIntent {
        &self.intent
    }
    /// Requested labels mapped to their canonical existing or newly selected logical keys.
    pub fn bindings(&self) -> &BTreeMap<DependencyKey, DependencyKey> {
        &self.bindings
    }
    /// Entries whose exact installed description changes; not a native write footprint.
    pub fn changed(&self) -> &BTreeSet<DependencyKey> {
        &self.changed
    }
    /// Bind the codec's canonical revision and recheck the complete next project.
    pub fn resolve(self, revision: SemanticRevision) -> Result<ResolvedProject, ModelError> {
        let mut lock = self.lock;
        lock.intent_revision = revision;
        ResolvedProject::validate(self.intent, lock, revision)
    }
}

fn same_installed_selection(left: &LockedDependency, right: &LockedDependency) -> bool {
    left.identity == right.identity
        && left.kind == right.kind
        && left.selected == right.selected
        && left.files.as_slice().len() == right.files.as_slice().len()
        && right.files.as_slice().iter().all(|requested| {
            left.files.as_slice().iter().any(|existing| {
                existing.slot == requested.slot
                    && existing.expected == requested.expected
                    && existing.placements == requested.placements
            })
        })
}

fn merge_evidence(
    lock: &mut ResolutionLock,
    key: &DependencyKey,
    proposed: &BTreeSet<DependencyKey>,
    proposed_coverage: Coverage,
) -> Result<(), AdditionError> {
    let previous = lock.required_edges.get(key).cloned().unwrap_or_default();
    let previous_coverage = lock.coverage[key];
    if (previous_coverage == Coverage::CompleteForSelection && !proposed.is_subset(&previous))
        || (proposed_coverage == Coverage::CompleteForSelection && !previous.is_subset(proposed))
    {
        return Err(AdditionError::RetainedSelectionConflict(key.clone()));
    }
    let merged: BTreeSet<_> = previous.union(proposed).cloned().collect();
    if !merged.is_empty() {
        lock.required_edges.insert(key.clone(), merged);
    }
    lock.coverage.insert(
        key.clone(),
        match (previous_coverage, proposed_coverage) {
            (Coverage::CompleteForSelection, _) | (_, Coverage::CompleteForSelection) => {
                Coverage::CompleteForSelection
            }
            (Coverage::Partial, _) | (_, Coverage::Partial) => Coverage::Partial,
            _ => Coverage::Unknown,
        },
    );
    Ok(())
}
