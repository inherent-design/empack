//! Explicit selection removal never infers permission to collect unrequested content.
use crate::model::{
    Coverage, DependencyKey, LockedDependency, ModelError, NonEmpty, ProjectIntent, ResolutionLock,
    ResolvedProject, SemanticRevision,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use core::fmt;

/// Removing authoring roots and deleting their installed content are different requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalMode {
    /// Remove only explicit authoring roots; keep exact locked files and dependency evidence.
    /// A retained dependency may still be required. This is not a content-removal receipt.
    ForgetRoots,
    /// Remove the selected exact entries after checking all retained dependency evidence.
    /// Unrequested selections, including unreachable ones, remain installed.
    RemoveContent,
}
/// Unknown dependency evidence is distinct from a known requirement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemovalEvidencePolicy {
    /// Refuse deletion while retained dependencies have incomplete edge information.
    #[default]
    RequireComplete,
    /// Explicitly accept unknown dependents for the selected content. Known required edges,
    /// content assertions, ownership and publication safeguards remain mandatory.
    AcknowledgeUnknown,
}
/// Evidence that prevents an all-requested removal plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalError {
    /// A selected logical key is absent from the exact resolution.
    UnknownSelection(DependencyKey),
    /// One logical selection appeared more than once.
    DuplicateSelection(DependencyKey),
    /// Root demotion cannot select a dependency that was not an explicit root.
    NotExplicitRoot(DependencyKey),
    /// Retained selections have known required edges to a selected dependency.
    RequiredBy(Vec<DependencyKey>),
    /// Retained selections have incomplete evidence; their missing edges prove nothing.
    IncompleteEvidence(Vec<DependencyKey>),
}
impl fmt::Display for RemovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSelection(key) => write!(f, "Unknown dependency selection: {}", key.as_str()),
            Self::DuplicateSelection(key) => write!(f, "Repeated dependency selection: {}", key.as_str()),
            Self::NotExplicitRoot(key) => write!(f, "Dependency is not an explicit root: {}", key.as_str()),
            Self::RequiredBy(_) => f.write_str("Retained dependencies require selected content; retain the content or select its dependents explicitly"),
            Self::IncompleteEvidence(_) => f.write_str("Retained dependency evidence is incomplete; resolve it before deleting content or explicitly forget roots while retaining content"),
        }
    }
}
impl core::error::Error for RemovalError {}

/// Pure next-state proposal, without native file observations or deletion authority.
#[derive(Debug, Clone)]
pub struct RemovalPlan {
    intent: ProjectIntent,
    lock: ResolutionLock,
    selected: BTreeMap<DependencyKey, LockedDependency>,
    mode: RemovalMode,
    incomplete: Vec<DependencyKey>,
}
impl RemovalPlan {
    /// Plan every selection or return no candidate. Known dependents take precedence over
    /// incomplete-evidence diagnostics. Selecting a whole cycle is valid; traversing a graph
    /// is unnecessary because every retained incoming edge must independently remain valid.
    pub fn prepare(
        project: &ResolvedProject,
        selections: &NonEmpty<DependencyKey>,
        mode: RemovalMode,
    ) -> Result<Self, RemovalError> {
        Self::prepare_with_policy(
            project,
            selections,
            mode,
            RemovalEvidencePolicy::RequireComplete,
        )
    }
    /// Plan with an explicit uncertainty policy. Acknowledgement never overrides known
    /// required edges, and the plan retains every unresolved dependent for its receipt.
    pub fn prepare_with_policy(
        project: &ResolvedProject,
        selections: &NonEmpty<DependencyKey>,
        mode: RemovalMode,
        evidence: RemovalEvidencePolicy,
    ) -> Result<Self, RemovalError> {
        let mut incomplete = Vec::new();
        let mut selected = BTreeMap::new();
        for key in selections.as_slice() {
            let value = project
                .lock()
                .dependencies
                .get(key)
                .ok_or_else(|| RemovalError::UnknownSelection(key.clone()))?;
            if selected.insert(key.clone(), value.clone()).is_some() {
                return Err(RemovalError::DuplicateSelection(key.clone()));
            }
            if mode == RemovalMode::ForgetRoots && !project.intent().roots.contains_key(key) {
                return Err(RemovalError::NotExplicitRoot(key.clone()));
            }
        }
        if mode == RemovalMode::RemoveContent {
            let dependents: Vec<_> = project
                .lock()
                .required_edges
                .iter()
                .filter(|(key, edges)| {
                    !selected.contains_key(*key) && edges.iter().any(|to| selected.contains_key(to))
                })
                .map(|(key, _)| key.clone())
                .collect();
            if !dependents.is_empty() {
                return Err(RemovalError::RequiredBy(dependents));
            }
            incomplete = project
                .lock()
                .coverage
                .iter()
                .filter(|(key, coverage)| {
                    !selected.contains_key(*key) && **coverage != Coverage::CompleteForSelection
                })
                .map(|(key, _)| key.clone())
                .collect();
            if !incomplete.is_empty() && evidence == RemovalEvidencePolicy::RequireComplete {
                return Err(RemovalError::IncompleteEvidence(incomplete));
            }
        }
        let mut intent = project.intent().clone();
        let mut lock = project.lock().clone();
        for key in selected.keys() {
            intent.roots.remove(key);
        }
        if mode == RemovalMode::RemoveContent {
            lock.dependencies
                .retain(|key, _| !selected.contains_key(key));
            lock.coverage.retain(|key, _| !selected.contains_key(key));
            lock.required_edges
                .retain(|key, _| !selected.contains_key(key));
            // Remaining edges cannot reference a removed selection: checked above.
        }
        Ok(Self {
            intent,
            lock,
            selected,
            mode,
            incomplete,
        })
    }
    /// Updated authoring meaning. The codec must compute its canonical revision before binding.
    pub fn intent(&self) -> &ProjectIntent {
        &self.intent
    }
    /// Exact selected identities, pins, roles, placements and original content assertions.
    pub fn selected(&self) -> &BTreeMap<DependencyKey, LockedDependency> {
        &self.selected
    }
    /// Explicit deletion/demotion semantics retained for the preview and receipt.
    pub fn mode(&self) -> RemovalMode {
        self.mode
    }
    /// Retained selections whose dependency edges remain uncertain after explicit acknowledgement.
    /// Empty for root demotion, which deletes no content.
    pub fn incomplete_evidence(&self) -> &[DependencyKey] {
        &self.incomplete
    }
    /// Logical keys that will leave the lock. This never includes inferred orphans.
    pub fn removed(&self) -> BTreeSet<DependencyKey> {
        if self.mode == RemovalMode::RemoveContent {
            self.selected.keys().cloned().collect()
        } else {
            BTreeSet::new()
        }
    }
    /// Bind the codec's canonical next-intent revision and recheck model invariants.
    /// This establishes semantic coherence only; native postconditions require separate proof.
    pub fn resolve(self, revision: SemanticRevision) -> Result<ResolvedProject, ModelError> {
        let mut lock = self.lock;
        lock.intent_revision = revision;
        ResolvedProject::validate(self.intent, lock, revision)
    }
}
