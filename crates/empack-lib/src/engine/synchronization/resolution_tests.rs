use super::*;
use crate::engine::addition::tests::fixture;
use empack_core::{identity::*, model::*, synchronization::SynchronizationResolution};
fn key(value: &str) -> DependencyKey {
    DependencyKey::parse(value).unwrap()
}
fn decoded(intent: &ProjectIntent) -> DecodedIntent {
    DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(intent).unwrap(), "intent")
        .unwrap()
}
fn resolve(intent: &ProjectIntent, lock: &ResolutionLock) -> ResolvedProject {
    let source = decoded(intent);
    let mut lock = lock.clone();
    lock.intent_revision = source.semantic_revision();
    ResolvedProject::validate(intent.clone(), lock, source.semantic_revision()).unwrap()
}
#[test]
fn changed_pin_resolution_retains_unrelated_exact_selections_and_authoring() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("other", "Project2", "Version1"),
        ],
        &["root"],
        &[],
        true,
    );
    let mut intent = current.intent().clone();
    intent.roots.get_mut(&key("root")).unwrap().version = VersionIntent::Exact(
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version2").unwrap()),
    );
    let changed = fixture(
        &[
            ("root", "Project1", "Version2"),
            ("other", "Project2", "Version1"),
        ],
        &["root"],
        &[],
        true,
    );
    let proposed = resolve(&intent, changed.lock());
    let scope = SynchronizationResolution::prepare(&intent, current.lock(), &proposed).unwrap();
    assert_eq!(
        scope.affected_roots(),
        &std::collections::BTreeSet::from([key("root")])
    );
    assert_eq!(scope.changed(), scope.affected_roots());
    let source = decoded(&intent);
    let prior = DocumentCodec
        .decode_prior_lock(&DocumentCodec.encode_lock(&current).unwrap(), "prior")
        .unwrap();
    assert!(SynchronizationCandidate::prepare(&source, &prior).is_err());
    let candidate = SynchronizationCandidate::prepare_resolved(&source, &prior, &proposed).unwrap();
    assert_eq!(candidate.intent_document().bytes, source.original());
    assert_eq!(
        candidate.project().lock().dependencies[&key("other")],
        current.lock().dependencies[&key("other")]
    );
    assert!(!candidate.preserves_lock_document());
    let upgraded_other = fixture(
        &[
            ("root", "Project1", "Version2"),
            ("other", "Project2", "Version2"),
        ],
        &["root"],
        &[],
        true,
    );
    assert!(
        SynchronizationResolution::prepare(
            &intent,
            current.lock(),
            &resolve(&intent, upgraded_other.lock())
        )
        .is_err()
    );
}
#[test]
fn synchronization_does_not_implicitly_upgrade_delete_or_add_unrelated_content() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("other", "Project2", "Version1"),
        ],
        &["root"],
        &[],
        true,
    );
    for next in [
        fixture(
            &[
                ("root", "Project1", "Version2"),
                ("other", "Project2", "Version1"),
            ],
            &["root"],
            &[],
            true,
        ),
        fixture(&[("root", "Project1", "Version1")], &["root"], &[], true),
        fixture(
            &[
                ("root", "Project1", "Version1"),
                ("other", "Project2", "Version1"),
                ("new", "Project3", "Version1"),
            ],
            &["root"],
            &[],
            true,
        ),
    ] {
        assert!(
            SynchronizationResolution::prepare(
                current.intent(),
                current.lock(),
                &resolve(current.intent(), next.lock())
            )
            .is_err()
        );
    }
    let mut runtime = current.lock().clone();
    runtime.runtime.loader_version = Some(LoaderVersion::parse("0.17.0").unwrap());
    let mut intent = current.intent().clone();
    intent.runtime.loader_version = None;
    assert!(
        SynchronizationResolution::prepare(&intent, current.lock(), &resolve(&intent, &runtime))
            .is_err()
    );
}
#[test]
fn changed_root_can_add_required_closure_but_cannot_rewrite_a_valid_root() {
    let current = fixture(&[("old", "Project1", "Version1")], &["old"], &[], true);
    let next = fixture(
        &[
            ("old", "Project1", "Version1"),
            ("new", "Project2", "Version1"),
            ("required", "Project3", "Version1"),
        ],
        &["old", "new"],
        &[("new", "required")],
        true,
    );
    let scope = SynchronizationResolution::prepare(next.intent(), current.lock(), &next).unwrap();
    assert!(scope.changed().contains(&key("required")));
    let conflict = fixture(
        &[
            ("old", "Project1", "Version2"),
            ("new", "Project2", "Version1"),
        ],
        &["old", "new"],
        &[("new", "old")],
        true,
    );
    assert!(
        SynchronizationResolution::prepare(conflict.intent(), current.lock(), &conflict).is_err()
    );
}
#[test]
fn changed_search_requires_resolution_and_respects_selected_provider_policy() {
    let current = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
    let mut intent = current.intent().clone();
    intent.roots.get_mut(&key("root")).unwrap().source = SourceIntent::Search {
        query: "new query".into(),
        providers: NonEmpty::new(vec![ProviderKind::Modrinth]).unwrap(),
    };
    let source = decoded(&intent);
    let prior = DocumentCodec
        .decode_prior_lock(&DocumentCodec.encode_lock(&current).unwrap(), "prior")
        .unwrap();
    assert!(SynchronizationCandidate::prepare(&source, &prior).is_err());
    assert!(
        SynchronizationCandidate::prepare_resolved(
            &source,
            &prior,
            &resolve(&intent, current.lock())
        )
        .is_ok()
    );
    intent.roots.get_mut(&key("root")).unwrap().source = SourceIntent::Search {
        query: "query".into(),
        providers: NonEmpty::new(vec![ProviderKind::CurseForge]).unwrap(),
    };
    assert!(
        intent.roots[&key("root")]
            .validate_selection(&key("root"), &current.lock().dependencies[&key("root")])
            .is_err()
    );
}

#[test]
fn new_requirements_cannot_invalidate_retained_dependents() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("shared", "Project2", "Version1"),
        ],
        &["root"],
        &[("root", "shared")],
        true,
    );
    let proposed = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("shared", "Project2", "Version2"),
            ("new", "Project3", "Version1"),
        ],
        &["root", "new"],
        &[("root", "shared"), ("new", "shared")],
        true,
    );
    assert!(
        SynchronizationResolution::prepare(proposed.intent(), current.lock(), &proposed).is_err()
    );
}

#[test]
fn renamed_provider_labels_reuse_exact_selection_and_rebind_required_edges() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("dependent", "Project2", "Version1"),
        ],
        &["root", "dependent"],
        &[("dependent", "root")],
        true,
    );
    let mut intent = current.intent().clone();
    let root = intent.roots.remove(&key("root")).unwrap();
    intent.roots.insert(key("renamed"), root);
    let source = decoded(&intent);
    let prior = DocumentCodec
        .decode_prior_lock(&DocumentCodec.encode_lock(&current).unwrap(), "prior")
        .unwrap();
    let candidate = SynchronizationCandidate::prepare(&source, &prior).unwrap();
    assert_eq!(
        candidate.project().lock().dependencies[&key("renamed")],
        current.lock().dependencies[&key("root")]
    );
    assert!(
        !candidate
            .project()
            .lock()
            .dependencies
            .contains_key(&key("root"))
    );
    assert_eq!(
        candidate.project().lock().required_edges[&key("dependent")],
        std::collections::BTreeSet::from([key("renamed")])
    );
    assert!(
        SynchronizationCandidate::prepare_resolved(&source, &prior, candidate.project()).is_ok()
    );
}
