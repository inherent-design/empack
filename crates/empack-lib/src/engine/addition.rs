//! Bind canonical dependency additions to captured document revisions before native preparation.
mod batch;
mod direct;
pub use batch::ResolvedAdditionBatch;
mod file_input;
pub use direct::{
    DirectFileInput, DirectFileLimits, DirectFileSource, FileKindPolicy, validate_file_kind,
};
mod native;
use super::documents::{DecodedIntent, DecodedLock, DocumentCodec, PreparedDocument};
use anyhow::Result;
use empack_core::{
    addition::{AdditionGroup, AdditionPlan, ReplacementSelection},
    model::ResolvedProject,
};
pub use file_input::{AcquiredFileInput, AcquiredFileSource, FileAddition, FileEvidence};
pub use native::{AdditionReceipt, PreparedAddition, prepare_addition};
pub(in crate::engine) use native::{plan_addition, plan_adoption, plan_replacement, plan_update};

/// A coherent document candidate, not authority to replace installed bytes.
pub struct AdditionCandidate {
    plan: AdditionPlan,
    project: ResolvedProject,
    intent: PreparedDocument,
    lock: Vec<u8>,
}
impl AdditionCandidate {
    /// A stale lock must be reconciled before adding to its declared logical state.
    pub fn prepare(
        source: &DecodedIntent,
        lock: &DecodedLock,
        group: &AdditionGroup,
    ) -> Result<Self> {
        Self::prepare_mode(source, lock, group, false)
    }
    /// Refresh only selected installed identities, preserving current authoring intent and pins.
    pub fn prepare_update(
        source: &DecodedIntent,
        lock: &DecodedLock,
        group: &AdditionGroup,
    ) -> Result<Self> {
        Self::prepare_mode(source, lock, group, true)
    }
    /// Bind an explicitly selected foreign-identity replacement into one document candidate.
    pub fn prepare_replacement(
        source: &DecodedIntent,
        lock: &DecodedLock,
        group: &AdditionGroup,
        selection: &ReplacementSelection,
    ) -> Result<Self> {
        Self::from_plan(
            source,
            AdditionPlan::prepare_replacement(&lock.bind(source)?, group, selection)?,
        )
    }
    /// A first observed lock requires a complete coherent description of retained intent.
    pub fn prepare_adoption(
        source: &DecodedIntent,
        lock: Option<&DecodedLock>,
        group: &AdditionGroup,
    ) -> Result<Self> {
        if let Some(lock) = lock {
            return Self::from_plan(
                source,
                AdditionPlan::prepare_adoption(&lock.bind(source)?, group)?,
            );
        }
        Self::from_plan(
            source,
            AdditionPlan::prepare_initial_adoption(source.intent(), group)?,
        )
    }
    fn prepare_mode(
        source: &DecodedIntent,
        lock: &DecodedLock,
        group: &AdditionGroup,
        update: bool,
    ) -> Result<Self> {
        let current = lock.bind(source)?;
        let plan = if update {
            AdditionPlan::prepare_update(&current, group)?
        } else {
            AdditionPlan::prepare(&current, group)?
        };
        Self::from_plan(source, plan)
    }
    fn from_plan(source: &DecodedIntent, plan: AdditionPlan) -> Result<Self> {
        let intent = DocumentCodec.replace_intent(source, plan.intent())?;
        let next = DocumentCodec.decode_intent(&intent.bytes, "addition candidate")?;
        let project = plan.clone().resolve(next.semantic_revision())?;
        let lock = DocumentCodec.encode_lock(&project)?;
        Ok(Self {
            plan,
            project,
            intent,
            lock,
        })
    }
    pub fn plan(&self) -> &AdditionPlan {
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

#[cfg(test)]
pub(in crate::engine) mod tests {
    use super::*;
    use crate::engine::{documents::DocumentEdit, mrpack::tests::project};
    use empack_core::{addition::AdditionError, identity::*, model::*, path::InstallDestination};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn initial_adoption_requires_resolution_of_retained_roots_and_runtime() {
        let complete = fixture(
            &[("a", "ProjectA", "VersionA"), ("b", "ProjectB", "VersionB")],
            &["a", "b"],
            &[],
            true,
        );
        let subset = fixture(&[("a", "ProjectA", "VersionA")], &["a"], &[], true);
        let source = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(complete.intent()).unwrap(),
                "source",
            )
            .unwrap();
        assert!(
            AdditionCandidate::prepare_adoption(
                &source,
                None,
                &AdditionGroup::from_resolved(&subset).unwrap()
            )
            .is_err()
        );
        let group = AdditionGroup::from_resolved(&complete).unwrap();
        let candidate = AdditionCandidate::prepare_adoption(&source, None, &group).unwrap();
        assert_eq!(candidate.project().intent(), complete.intent());
        assert_eq!(
            candidate.project().lock().dependencies,
            complete.lock().dependencies
        );
        let mut other = complete.intent().clone();
        other.runtime.minecraft = GameVersion::parse("0.0.0").unwrap();
        let other = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(&other).unwrap(),
                "other runtime",
            )
            .unwrap();
        assert!(AdditionCandidate::prepare_adoption(&other, None, &group).is_err());
    }
    #[test]
    fn explicit_replacement_preserves_retained_records_and_requires_exact_selection() {
        use empack_core::removal::{RemovalError, RemovalEvidencePolicy};
        let current = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("keep", "Project2", "Version2"),
            ],
            &["root", "keep"],
            &[],
            true,
        );
        let replacement = fixture(&[("root", "Project3", "Version3")], &["root"], &[], true);
        let source = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(current.intent()).unwrap(),
                "source",
            )
            .unwrap();
        let prior = DocumentCodec
            .decode_prior_lock(&DocumentCodec.encode_lock(&current).unwrap(), "prior")
            .unwrap();
        let group = AdditionGroup::from_resolved(&replacement).unwrap();
        let mut selection = ReplacementSelection {
            keys: NonEmpty::new(vec![key("root")]).unwrap(),
            evidence: RemovalEvidencePolicy::RequireComplete,
        };
        assert!(AdditionCandidate::prepare(&source, &prior, &group).is_err());
        let planned =
            AdditionCandidate::prepare_replacement(&source, &prior, &group, &selection).unwrap();
        assert_eq!(
            planned.project().lock().dependencies[&key("keep")],
            current.lock().dependencies[&key("keep")]
        );
        assert_eq!(
            planned.project().lock().dependencies[&key("root")],
            replacement.lock().dependencies[&key("root")]
        );
        assert_eq!(
            planned.plan().replaced()[&key("root")],
            current.lock().dependencies[&key("root")]
        );
        for (request, keys) in [
            (replacement.clone(), vec![key("keep")]),
            (replacement.clone(), vec![key("root"), key("root")]),
            (
                fixture(&[("root", "Project2", "Version2")], &["root"], &[], true),
                vec![key("root")],
            ),
            (
                fixture(
                    &[
                        ("root", "Project3", "Version3"),
                        ("keep", "Project2", "Version4"),
                    ],
                    &["root", "keep"],
                    &[],
                    true,
                ),
                vec![key("root")],
            ),
        ] {
            selection.keys = NonEmpty::new(keys).unwrap();
            assert!(
                AdditionCandidate::prepare_replacement(
                    &source,
                    &prior,
                    &AdditionGroup::from_resolved(&request).unwrap(),
                    &selection
                )
                .is_err()
            );
        }
        selection.keys = NonEmpty::new(vec![key("root")]).unwrap();
        for known in [false, true] {
            let edges = if known {
                vec![("keep", "root")]
            } else {
                vec![]
            };
            let uncertain = fixture(
                &[
                    ("root", "Project1", "Version1"),
                    ("keep", "Project2", "Version2"),
                ],
                &["root", "keep"],
                &edges,
                false,
            );
            let prior = DocumentCodec
                .decode_prior_lock(&DocumentCodec.encode_lock(&uncertain).unwrap(), "uncertain")
                .unwrap();
            selection.evidence = RemovalEvidencePolicy::RequireComplete;
            assert!(
                AdditionCandidate::prepare_replacement(&source, &prior, &group, &selection)
                    .is_err()
            );
            selection.evidence = RemovalEvidencePolicy::AcknowledgeUnknown;
            let result =
                AdditionCandidate::prepare_replacement(&source, &prior, &group, &selection);
            if known {
                assert!(matches!(
                    result.err().unwrap().downcast_ref::<AdditionError>(),
                    Some(AdditionError::Replacement(RemovalError::RequiredBy(_)))
                ));
            } else {
                assert_eq!(result.unwrap().plan().incomplete_evidence(), &[key("keep")]);
            }
        }
    }
    #[test]
    fn first_lock_adoption_preserves_authored_identity_pin_and_placement() {
        let resolved = fixture(&[("root", "ProjectA", "VersionA")], &["root"], &[], true);
        let group = AdditionGroup::from_resolved(&resolved).unwrap();
        for mode in ["identity", "pin", "placement"] {
            let mut intent = resolved.intent().clone();
            let root = intent.roots.get_mut(&key("root")).unwrap();
            match mode {
                "identity" => {
                    root.source = SourceIntent::Provider(ProviderProjectId::Modrinth(
                        ModrinthProjectId::parse("ProjectB").unwrap(),
                    ))
                }
                "pin" => {
                    root.version = VersionIntent::Exact(
                        ProviderProjectId::Modrinth(ModrinthProjectId::parse("ProjectA").unwrap())
                            .parse_pin("VersionB")
                            .unwrap(),
                    )
                }
                "placement" => {
                    let mut placement = resolved.lock().dependencies[&key("root")].files.as_slice()
                        [0]
                    .placements
                    .as_slice()[0]
                        .clone();
                    placement.destination = InstallDestination::parse("mods/another.jar").unwrap();
                    root.placement =
                        PlacementIntent::Explicit(NonEmpty::new(vec![placement]).unwrap());
                }
                _ => unreachable!(),
            }
            let source = DocumentCodec
                .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "authored")
                .unwrap();
            assert!(
                AdditionCandidate::prepare_adoption(&source, None, &group).is_err(),
                "{mode}"
            );
        }
        let mut intent = resolved.intent().clone();
        intent.roots.get_mut(&key("root")).unwrap().version = VersionIntent::Exact(
            resolved.lock().dependencies[&key("root")]
                .selected
                .as_ref()
                .unwrap()
                .selection
                .clone(),
        );
        let source = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(&intent).unwrap(),
                "matching pin",
            )
            .unwrap();
        let candidate = AdditionCandidate::prepare_adoption(&source, None, &group).unwrap();
        assert_eq!(candidate.project().intent(), &intent);
        assert_eq!(candidate.intent_document().bytes, source.original());
    }
    fn key(value: &str) -> DependencyKey {
        DependencyKey::parse(value).unwrap()
    }
    pub(in crate::engine) fn fixture(
        entries: &[(&str, &str, &str)],
        roots: &[&str],
        edges: &[(&str, &str)],
        complete: bool,
    ) -> ResolvedProject {
        let base = project(false, false);
        let root = base.intent().roots.values().next().unwrap().clone();
        let value = base.lock().dependencies.values().next().unwrap().clone();
        let mut intent = base.intent().clone();
        intent.roots.clear();
        let mut lock = base.lock().clone();
        lock.dependencies.clear();
        lock.coverage.clear();
        lock.required_edges.clear();
        for (label, id, version) in entries {
            let key = key(label);
            let project = ProviderProjectId::Modrinth(ModrinthProjectId::parse(id).unwrap());
            let mut root = root.clone();
            root.source = SourceIntent::Provider(project.clone());
            let mut value = value.clone();
            value.title = format!("Title for {id}");
            value.identity = ResolvedIdentity::Provider(project.clone());
            value.selected = Some(ResolvedPin {
                project: project.clone(),
                selection: project.parse_pin(version).unwrap(),
            });
            let mut files = value.files.as_slice().to_vec();
            for file in &mut files {
                let mut placements = file.placements.as_slice().to_vec();
                for placement in &mut placements {
                    placement.destination = InstallDestination::parse(&format!(
                        "{id}/{}",
                        placement.destination.relative().as_str()
                    ))
                    .unwrap();
                }
                file.placements = NonEmpty::new(placements).unwrap();
            }
            value.files = NonEmpty::new(files).unwrap();
            if roots.contains(label) {
                // This fixture deliberately namespaces files by provider, rather than using
                // the content kind's automatic directory.
                root.placement = PlacementIntent::Explicit(
                    NonEmpty::new(
                        value
                            .files
                            .as_slice()
                            .iter()
                            .flat_map(|file| file.placements.as_slice().iter().cloned())
                            .collect(),
                    )
                    .unwrap(),
                );
                intent.roots.insert(key.clone(), root);
            }
            lock.dependencies.insert(key.clone(), value);
            lock.coverage.insert(
                key,
                if complete {
                    Coverage::CompleteForSelection
                } else {
                    Coverage::Partial
                },
            );
        }
        for (from, to) in edges {
            lock.required_edges
                .entry(key(from))
                .or_default()
                .insert(key(to));
        }
        let decoded = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
            .unwrap();
        lock.intent_revision = decoded.semantic_revision();
        ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap()
    }
    fn candidate(
        current: &ResolvedProject,
        incoming: &ResolvedProject,
    ) -> Result<AdditionCandidate> {
        let mut bytes = b"# original comment\n".to_vec();
        bytes.extend(DocumentCodec.encode_intent(current.intent()).unwrap());
        let source = DocumentCodec.decode_intent(&bytes, "fixture").unwrap();
        let lock = DocumentCodec
            .decode_prior_lock(&DocumentCodec.encode_lock(current).unwrap(), "fixture")
            .unwrap();
        AdditionCandidate::prepare(&source, &lock, &AdditionGroup::from_resolved(incoming)?)
    }
    #[test]
    fn readding_provider_alias_preserves_logical_key_comments_and_unlisted_content() {
        let current = fixture(
            &[
                ("renderer", "Project1", "Version1"),
                ("unlisted", "Project2", "Version2"),
            ],
            &["renderer"],
            &[],
            true,
        );
        let request = fixture(
            &[("new-alias", "Project1", "Version1")],
            &["new-alias"],
            &[],
            true,
        );
        let result = candidate(&current, &request).unwrap();
        assert_eq!(
            result.plan().bindings(),
            &BTreeMap::from([(key("new-alias"), key("renderer"))])
        );
        assert!(result.plan().changed().is_empty());
        assert_eq!(result.intent_document().edit, DocumentEdit::Unchanged);
        assert!(
            result
                .intent_document()
                .bytes
                .starts_with(b"# original comment\n")
        );
        assert_eq!(result.project().lock(), current.lock());
    }
    #[test]
    fn new_root_retains_shared_selection_and_rekeys_required_edges() {
        let current = fixture(&[("library-alias", "Project1", "Version1")], &[], &[], true);
        let request = fixture(
            &[
                ("root", "Project2", "Version2"),
                ("library", "Project1", "Version1"),
            ],
            &["root"],
            &[("root", "library")],
            true,
        );
        let result = candidate(&current, &request).unwrap();
        assert_eq!(
            result.project().lock().required_edges[&key("root")],
            BTreeSet::from([key("library-alias")])
        );
        assert_eq!(result.plan().changed(), &BTreeSet::from([key("root")]));
        assert!(
            !result
                .project()
                .intent()
                .roots
                .contains_key(&key("library-alias"))
        );
    }
    #[test]
    fn conflicting_shared_pins_and_occupied_labels_fail_the_whole_addition() {
        let current = fixture(
            &[("existing", "Project1", "Version1")],
            &["existing"],
            &[],
            true,
        );
        let request = fixture(
            &[
                ("root", "Project2", "Version2"),
                ("library", "Project1", "Version2"),
            ],
            &["root"],
            &[("root", "library")],
            true,
        );
        assert_eq!(
            candidate(&current, &request)
                .err()
                .unwrap()
                .downcast_ref::<AdditionError>(),
            Some(&AdditionError::RetainedSelectionConflict(key("existing")))
        );
        let request = fixture(
            &[("existing", "Project2", "Version2")],
            &["existing"],
            &[],
            true,
        );
        assert_eq!(
            candidate(&current, &request)
                .err()
                .unwrap()
                .downcast_ref::<AdditionError>(),
            Some(&AdditionError::OccupiedKey(key("existing")))
        );
    }
    #[test]
    fn unrequested_installations_and_runtime_changes_are_not_addition_authority() {
        let request = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("hidden", "Project2", "Version2"),
            ],
            &["root"],
            &[],
            true,
        );
        assert_eq!(
            AdditionGroup::from_resolved(&request).unwrap_err(),
            AdditionError::UnrequestedSelection(key("hidden"))
        );
        let current = fixture(&[], &[], &[], true);
        let request = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
        let mut intent = request.intent().clone();
        intent.runtime.minecraft = GameVersion::parse("1.21.1").unwrap();
        let mut lock = request.lock().clone();
        lock.runtime.minecraft = intent.runtime.minecraft.clone();
        let decoded = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
            .unwrap();
        lock.intent_revision = decoded.semantic_revision();
        let request = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
        assert_eq!(
            candidate(&current, &request)
                .err()
                .unwrap()
                .downcast_ref::<AdditionError>(),
            Some(&AdditionError::RuntimeMismatch)
        );
    }
    #[test]
    fn readding_an_exact_selection_does_not_discard_known_dependency_evidence() {
        let current = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("old", "Project2", "Version2"),
            ],
            &["root"],
            &[("root", "old")],
            false,
        );
        let request = fixture(
            &[
                ("root-alias", "Project1", "Version1"),
                ("new", "Project3", "Version3"),
            ],
            &["root-alias"],
            &[("root-alias", "new")],
            false,
        );
        let result = candidate(&current, &request).unwrap();
        assert_eq!(
            result.project().lock().required_edges[&key("root")],
            BTreeSet::from([key("old"), key("new")])
        );
        assert_eq!(
            result.project().lock().coverage[&key("root")],
            Coverage::Partial
        );
    }
    #[test]
    fn updating_a_root_cannot_invalidate_retained_required_dependents() {
        let current = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("dependent", "Project2", "Version2"),
            ],
            &["root", "dependent"],
            &[("dependent", "root")],
            true,
        );
        let request = fixture(&[("root", "Project1", "Version2")], &["root"], &[], true);
        assert_eq!(
            candidate(&current, &request)
                .err()
                .unwrap()
                .downcast_ref::<AdditionError>(),
            Some(&AdditionError::RequiredBy(vec![key("dependent")]))
        );
    }
    #[test]
    fn explicit_pin_intent_survives_canonical_alias_binding() {
        let current = fixture(
            &[("existing", "Project1", "Version1")],
            &["existing"],
            &[],
            true,
        );
        let request = fixture(&[("alias", "Project1", "Version2")], &["alias"], &[], true);
        let mut intent = request.intent().clone();
        let pin = request.lock().dependencies[&key("alias")]
            .selected
            .as_ref()
            .unwrap()
            .selection
            .clone();
        intent.roots.get_mut(&key("alias")).unwrap().version = VersionIntent::Exact(pin.clone());
        let decoded = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
            .unwrap();
        let mut lock = request.lock().clone();
        lock.intent_revision = decoded.semantic_revision();
        let request = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
        let result = candidate(&current, &request).unwrap();
        assert_eq!(
            result.project().intent().roots[&key("existing")].version,
            VersionIntent::Exact(pin)
        );
        assert!(!result.project().intent().roots.contains_key(&key("alias")));
        assert_eq!(result.plan().changed(), &BTreeSet::from([key("existing")]));
    }
    #[test]
    fn contradictory_complete_edge_evidence_is_not_silently_replaced() {
        let current = fixture(
            &[
                ("root", "Project1", "Version1"),
                ("required", "Project2", "Version2"),
            ],
            &["root"],
            &[("root", "required")],
            true,
        );
        let request = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
        assert_eq!(
            candidate(&current, &request)
                .err()
                .unwrap()
                .downcast_ref::<AdditionError>(),
            Some(&AdditionError::RetainedSelectionConflict(key("root")))
        );
        assert_eq!(
            AdditionGroup::from_resolved(&fixture(&[], &[], &[], true)).unwrap_err(),
            AdditionError::EmptyRequest
        );
    }
}

#[cfg(test)]
mod update_tests;
