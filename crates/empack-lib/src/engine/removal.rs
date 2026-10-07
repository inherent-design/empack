//! Resolve removal intent without obtaining filesystem or backend mutation authority.
use super::documents::{DecodedIntent, DecodedLock, DocumentCodec, PreparedDocument};
use anyhow::Result;
use empack_core::{
    model::{DependencyKey, NonEmpty, ResolvedProject},
    removal::{RemovalEvidencePolicy, RemovalMode, RemovalPlan},
};

mod native;
mod selection;
pub(super) use native::plan_selected_removal;
pub use native::{
    ObservedRemovalSelection, PreparedRemoval, RemovalReceipt, prepare_removal,
    prepare_removal_with_policy,
};
pub use selection::{RemovalSelector, SelectionError};
pub(super) use selection::{ResolvedSelections, resolve as resolve_selections};

/// Coherent next documents and explicit exact selections. This is not permission to delete files.
/// Native preparation must still bind placements and metadata to captured regular files and
/// verify original content assertions before constructing a publication candidate.
pub struct RemovalCandidate {
    plan: RemovalPlan,
    project: ResolvedProject,
    intent: PreparedDocument,
    lock: Vec<u8>,
}
impl RemovalCandidate {
    /// Current intent/lock agreement is required. A stale lock cannot establish safe removability.
    /// The source's raw document revision remains a publication precondition even for comment edits.
    pub fn prepare(
        source: &DecodedIntent,
        lock: &DecodedLock,
        selections: &NonEmpty<DependencyKey>,
        mode: RemovalMode,
    ) -> Result<Self> {
        Self::prepare_with_policy(
            source,
            lock,
            selections,
            mode,
            RemovalEvidencePolicy::RequireComplete,
        )
    }
    /// Explicit acknowledgement changes only the incomplete-dependency check.
    pub fn prepare_with_policy(
        source: &DecodedIntent,
        lock: &DecodedLock,
        selections: &NonEmpty<DependencyKey>,
        mode: RemovalMode,
        evidence: RemovalEvidencePolicy,
    ) -> Result<Self> {
        let current = lock.bind(source)?;
        let plan = RemovalPlan::prepare_with_policy(&current, selections, mode, evidence)?;
        Self::from_plan(source, plan)
    }
    pub(super) fn prepare_selected(
        source: &DecodedIntent,
        lock: &DecodedLock,
        selections: &ResolvedSelections,
        mode: RemovalMode,
        evidence: RemovalEvidencePolicy,
    ) -> Result<Self> {
        let current = lock.bind(source)?;
        let plan = logical_plan(&current, selections, mode, evidence)?;
        Self::from_plan(source, plan)
    }
    fn from_plan(source: &DecodedIntent, plan: RemovalPlan) -> Result<Self> {
        let intent = DocumentCodec.replace_intent(source, plan.intent())?;
        let next = DocumentCodec.decode_intent(&intent.bytes, "removal candidate")?;
        let project = plan.clone().resolve(next.semantic_revision())?;
        let lock = DocumentCodec.encode_lock(&project)?;
        Ok(Self {
            plan,
            project,
            intent,
            lock,
        })
    }
    pub fn plan(&self) -> &RemovalPlan {
        &self.plan
    }
    pub fn project(&self) -> &ResolvedProject {
        &self.project
    }
    pub fn intent_document(&self) -> &PreparedDocument {
        &self.intent
    }
    pub fn lock_document(&self) -> &[u8] {
        &self.lock
    }
}

/// Installed metadata lacks the complete graph facts retained in the exact lock.
#[derive(Debug, thiserror::Error)]
#[error(
    "Untracked installed metadata has incomplete dependency evidence; explicitly acknowledge unknown dependents before removing content"
)]
pub struct UntrackedDependencyEvidence(pub Vec<empack_core::path::PortableRelPath>);

pub(super) fn untracked_evidence(
    project: &ResolvedProject,
    records: &[super::backend::BackendFile],
    mode: RemovalMode,
    evidence: RemovalEvidencePolicy,
) -> Result<Vec<empack_core::path::PortableRelPath>> {
    if mode == RemovalMode::ForgetRoots {
        return Ok(vec![]);
    }
    let unknown: Vec<_> = records
        .iter()
        .filter(|record| {
            // An unrelated stale claim is unknown evidence, not ownership of another file.
            // Selected destinations are checked strictly in native preparation.
            record.locked_owner(project).ok().flatten().is_none()
        })
        .map(|record| record.metadata_path.clone())
        .collect();
    anyhow::ensure!(
        unknown.is_empty() || evidence == RemovalEvidencePolicy::AcknowledgeUnknown,
        UntrackedDependencyEvidence(unknown.clone())
    );
    Ok(unknown)
}

/// Observation cardinality and demotion semantics are checked before native preparation.
pub(super) fn logical_plan(
    project: &ResolvedProject,
    selections: &ResolvedSelections,
    mode: RemovalMode,
    evidence: RemovalEvidencePolicy,
) -> Result<RemovalPlan> {
    anyhow::ensure!(
        !selections.locked.is_empty() || !selections.observed.is_empty(),
        "Removal requires at least one selection"
    );
    anyhow::ensure!(
        selections.observed.is_empty() || mode == RemovalMode::RemoveContent,
        "An untracked installation is not an explicit root to forget"
    );
    if selections.locked.is_empty() {
        Ok(RemovalPlan::prepare_observed(project, evidence)?)
    } else {
        Ok(RemovalPlan::prepare_with_policy(
            project,
            &NonEmpty::new(selections.locked.clone())?,
            mode,
            evidence,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::mrpack::tests::project;
    use empack_core::{model::*, path::InstallDestination, removal::RemovalError};
    use std::collections::{BTreeMap, BTreeSet};

    fn key(value: &str) -> DependencyKey {
        DependencyKey::parse(value).unwrap()
    }
    fn keys(values: &[&str]) -> NonEmpty<DependencyKey> {
        NonEmpty::new(values.iter().map(|name| key(name)).collect()).unwrap()
    }
    fn fixture(edges: &[(&str, &str)], incomplete: &[&str]) -> (DecodedIntent, DecodedLock) {
        let original = project(false, false);
        let root = original.intent().roots.values().next().unwrap();
        let selected = original.lock().dependencies.values().next().unwrap();
        let mut intent = original.intent().clone();
        intent.roots.clear();
        let mut lock = original.lock().clone();
        lock.dependencies.clear();
        lock.coverage.clear();
        for name in ["alias-a", "alias-b", "unrequested"] {
            let k = key(name);
            intent.roots.insert(k.clone(), root.clone());
            let mut value = selected.clone();
            value.identity = ResolvedIdentity::Url(k.clone());
            value.title = format!("Title for {name}");
            let mut files = value.files.as_slice().to_vec();
            for file in &mut files {
                let mut placements = file.placements.as_slice().to_vec();
                for (index, placement) in placements.iter_mut().enumerate() {
                    placement.destination = InstallDestination::parse(&format!(
                        "resourcepacks/{name}-{}-{index}.zip",
                        file.slot.as_str()
                    ))
                    .unwrap();
                }
                file.placements = NonEmpty::new(placements).unwrap();
            }
            value.files = NonEmpty::new(files).unwrap();
            lock.dependencies.insert(k.clone(), value);
            lock.coverage.insert(
                k,
                if incomplete.contains(&name) {
                    Coverage::Partial
                } else {
                    Coverage::CompleteForSelection
                },
            );
        }
        lock.required_edges = BTreeMap::new();
        for (from, to) in edges {
            lock.required_edges
                .entry(key(from))
                .or_default()
                .insert(key(to));
        }
        let bytes = DocumentCodec.encode_intent(&intent).unwrap();
        let mut commented = b"# preserve raw source revision\n".to_vec();
        commented.extend(bytes);
        let source = DocumentCodec.decode_intent(&commented, "fixture").unwrap();
        lock.intent_revision = source.semantic_revision();
        let resolved = ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap();
        let lock = DocumentCodec
            .decode_prior_lock(&DocumentCodec.encode_lock(&resolved).unwrap(), "fixture")
            .unwrap();
        (source, lock)
    }
    #[test]
    fn explicit_removal_preserves_unrequested_selections_and_raw_revision_binding() {
        let (source, lock) = fixture(&[], &[]);
        let candidate = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::RemoveContent,
        )
        .unwrap();
        assert_eq!(candidate.plan().removed(), BTreeSet::from([key("alias-a")]));
        assert!(
            !candidate
                .project()
                .intent()
                .roots
                .contains_key(&key("alias-a"))
        );
        assert!(
            !candidate
                .project()
                .lock()
                .dependencies
                .contains_key(&key("alias-a"))
        );
        for name in ["alias-b", "unrequested"] {
            assert_eq!(
                candidate.project().lock().dependencies[&key(name)],
                lock.lock().dependencies[&key(name)]
            );
        }
        assert_eq!(candidate.intent_document().expected, source.raw_revision());
        let next = DocumentCodec
            .decode_intent(&candidate.intent_document().bytes, "candidate")
            .unwrap();
        DocumentCodec
            .decode_lock(candidate.lock_document(), &next, "candidate")
            .unwrap();
        assert!(source.original().starts_with(b"# preserve"));
    }
    #[test]
    fn required_cycles_need_explicit_group_selection_and_never_collect_other_nodes() {
        let (source, lock) = fixture(&[("alias-a", "alias-b"), ("alias-b", "alias-a")], &[]);
        let error = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::RemoveContent,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<RemovalError>(),
            Some(&RemovalError::RequiredBy(vec![key("alias-b")]))
        );
        let candidate = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a", "alias-b"]),
            RemovalMode::RemoveContent,
        )
        .unwrap();
        assert_eq!(
            candidate
                .project()
                .lock()
                .dependencies
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![key("unrequested")]
        );
        assert!(candidate.project().lock().required_edges.is_empty());
    }
    #[test]
    fn incomplete_retained_evidence_blocks_deletion_but_explicit_demotion_preserves_content() {
        let (source, lock) = fixture(&[], &["alias-b"]);
        let error = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::RemoveContent,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<RemovalError>(),
            Some(&RemovalError::IncompleteEvidence(vec![key("alias-b")]))
        );
        let candidate = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::ForgetRoots,
        )
        .unwrap();
        assert!(candidate.plan().removed().is_empty());
        assert_eq!(
            candidate.project().lock().dependencies,
            lock.lock().dependencies
        );
        assert_eq!(candidate.project().lock().coverage, lock.lock().coverage);
        assert!(
            !candidate
                .project()
                .intent()
                .roots
                .contains_key(&key("alias-a"))
        );
    }
    #[test]
    fn demotion_keeps_required_edges_and_cannot_demote_an_already_transitive_selection() {
        let (source, lock) = fixture(&[("alias-b", "alias-a")], &[]);
        let candidate = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::ForgetRoots,
        )
        .unwrap();
        assert_eq!(
            candidate.project().lock().required_edges,
            lock.lock().required_edges
        );
        let source = DocumentCodec
            .decode_intent(&candidate.intent_document().bytes, "next")
            .unwrap();
        let lock = DocumentCodec
            .decode_prior_lock(candidate.lock_document(), "next")
            .unwrap();
        let error = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::ForgetRoots,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<RemovalError>(),
            Some(&RemovalError::NotExplicitRoot(key("alias-a")))
        );
    }
    #[test]
    fn all_requested_selection_errors_return_no_successful_subset() {
        let (source, lock) = fixture(&[], &[]);
        for (selected, expected) in [
            (
                keys(&["alias-a", "missing"]),
                RemovalError::UnknownSelection(key("missing")),
            ),
            (
                keys(&["alias-a", "alias-a"]),
                RemovalError::DuplicateSelection(key("alias-a")),
            ),
            (
                keys(&["Title for alias-a"]),
                RemovalError::UnknownSelection(key("Title for alias-a")),
            ),
        ] {
            let error =
                RemovalCandidate::prepare(&source, &lock, &selected, RemovalMode::RemoveContent)
                    .err()
                    .unwrap();
            assert_eq!(error.downcast_ref::<RemovalError>(), Some(&expected));
            assert_eq!(source.intent().roots.len(), 3);
            lock.bind(&source).unwrap();
        }
    }
    #[test]
    fn incomplete_selected_nodes_do_not_prevent_an_explicit_complete_group_removal() {
        let (source, lock) = fixture(&[], &["alias-a", "alias-b", "unrequested"]);
        let candidate = RemovalCandidate::prepare(
            &source,
            &lock,
            &keys(&["alias-a", "alias-b", "unrequested"]),
            RemovalMode::RemoveContent,
        )
        .unwrap();
        assert!(candidate.project().lock().dependencies.is_empty());
        assert!(candidate.project().lock().coverage.is_empty());
    }
    #[test]
    fn stale_resolution_cannot_authorize_removal() {
        let (source, lock) = fixture(&[], &[]);
        let mut changed = source.intent().clone();
        changed.metadata.version = "edited".into();
        let source = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&changed).unwrap(), "edited")
            .unwrap();
        assert!(
            RemovalCandidate::prepare(
                &source,
                &lock,
                &keys(&["alias-a"]),
                RemovalMode::RemoveContent
            )
            .is_err()
        );
    }
    #[test]
    fn acknowledging_unknown_dependents_retains_uncertainty_but_never_bypasses_known_edges() {
        let (source, lock) = fixture(&[], &["alias-b"]);
        let candidate = RemovalCandidate::prepare_with_policy(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::RemoveContent,
            RemovalEvidencePolicy::AcknowledgeUnknown,
        )
        .unwrap();
        assert_eq!(candidate.plan().incomplete_evidence(), &[key("alias-b")]);
        assert_eq!(
            candidate.project().lock().coverage[&key("alias-b")],
            Coverage::Partial
        );
        assert!(
            !candidate
                .project()
                .lock()
                .dependencies
                .contains_key(&key("alias-a"))
        );
        let (source, lock) = fixture(&[("alias-b", "alias-a")], &["alias-b"]);
        let error = RemovalCandidate::prepare_with_policy(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::RemoveContent,
            RemovalEvidencePolicy::AcknowledgeUnknown,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<RemovalError>(),
            Some(&RemovalError::RequiredBy(vec![key("alias-b")]))
        );
        let candidate = RemovalCandidate::prepare_with_policy(
            &source,
            &lock,
            &keys(&["alias-a"]),
            RemovalMode::ForgetRoots,
            RemovalEvidencePolicy::AcknowledgeUnknown,
        )
        .unwrap();
        assert!(candidate.plan().incomplete_evidence().is_empty());
        assert_eq!(
            candidate.project().lock().required_edges,
            lock.lock().required_edges
        );
    }
}
