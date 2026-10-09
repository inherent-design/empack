use super::tests::fixture;
use super::*;
use empack_core::{identity::*, model::*};

fn key(value: &str) -> DependencyKey {
    DependencyKey::parse(value).unwrap()
}
fn update(current: &ResolvedProject, request: &ResolvedProject) -> Result<AdditionCandidate> {
    let mut bytes = b"# preserve author intent\n".to_vec();
    bytes.extend(DocumentCodec.encode_intent(current.intent())?);
    AdditionCandidate::prepare_update(
        &DocumentCodec.decode_intent(&bytes, "intent")?,
        &DocumentCodec.decode_prior_lock(&DocumentCodec.encode_lock(current)?, "lock")?,
        &AdditionGroup::from_resolved(request)?,
    )
}
#[test]
fn update_binds_aliases_preserves_raw_intent_and_retains_unselected_installations() {
    let current = fixture(
        &[
            ("existing", "Project1", "Version1"),
            ("other", "Project2", "Version1"),
        ],
        &["existing"],
        &[],
        true,
    );
    let requested = fixture(&[("alias", "Project1", "Version2")], &["alias"], &[], true);
    let updated = update(&current, &requested).unwrap();
    assert_eq!(updated.project().intent(), current.intent());
    assert_eq!(
        updated.intent_document().edit,
        crate::engine::documents::DocumentEdit::Unchanged
    );
    assert!(
        updated
            .intent_document()
            .bytes
            .starts_with(b"# preserve author intent\n")
    );
    assert_eq!(updated.plan().bindings()[&key("alias")], key("existing"));
    assert_eq!(
        updated.project().lock().dependencies[&key("existing")].selected,
        requested.lock().dependencies[&key("alias")].selected
    );
    assert_eq!(
        updated.project().lock().dependencies[&key("other")],
        current.lock().dependencies[&key("other")]
    );
}
#[test]
fn transitive_update_requires_dependents_and_does_not_promote_temporary_roots() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("dependency", "Project2", "Version1"),
        ],
        &["root"],
        &[("root", "dependency")],
        true,
    );
    let dependency_only = fixture(
        &[("dependency", "Project2", "Version2")],
        &["dependency"],
        &[],
        true,
    );
    assert!(update(&current, &dependency_only).is_err());
    let requested = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("dependency", "Project2", "Version2"),
        ],
        &["root", "dependency"],
        &[("root", "dependency")],
        true,
    );
    let updated = update(&current, &requested).unwrap();
    assert_eq!(updated.project().intent(), current.intent());
    assert!(
        !updated
            .project()
            .intent()
            .roots
            .contains_key(&key("dependency"))
    );
    assert_eq!(
        updated.project().lock().dependencies[&key("dependency")].selected,
        requested.lock().dependencies[&key("dependency")].selected
    );
}
#[test]
fn update_cannot_relax_explicit_pins_or_add_uninstalled_requested_identities() {
    let current = fixture(&[("root", "Project1", "Version1")], &["root"], &[], true);
    let mut intent = current.intent().clone();
    intent.roots.get_mut(&key("root")).unwrap().version = VersionIntent::Exact(
        PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version1").unwrap()),
    );
    let source = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "intent")
        .unwrap();
    let mut lock = current.lock().clone();
    lock.intent_revision = source.semantic_revision();
    let pinned = ResolvedProject::validate(intent, lock, source.semantic_revision()).unwrap();
    let refreshed = fixture(&[("root", "Project1", "Version2")], &["root"], &[], true);
    assert!(update(&pinned, &refreshed).is_err());
    let uninstalled = fixture(&[("new", "Project3", "Version1")], &["new"], &[], true);
    assert!(matches!(
        update(&current, &uninstalled)
            .err()
            .unwrap()
            .downcast_ref::<empack_core::addition::AdditionError>(),
        Some(empack_core::addition::AdditionError::UpdateMissing(_))
    ));
}

#[test]
fn adoption_preserves_existing_root_roles_and_adds_only_new_roots() {
    let current = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("dependency", "Project2", "Version1"),
        ],
        &["root"],
        &[("root", "dependency")],
        true,
    );
    let requested = fixture(
        &[
            ("root", "Project1", "Version1"),
            ("dependency", "Project2", "Version2"),
            ("new", "Project3", "Version1"),
        ],
        &["root", "dependency", "new"],
        &[("root", "dependency")],
        true,
    );
    let source = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(current.intent()).unwrap(),
            "intent",
        )
        .unwrap();
    let lock = DocumentCodec
        .decode_prior_lock(&DocumentCodec.encode_lock(&current).unwrap(), "lock")
        .unwrap();
    let adopted = AdditionCandidate::prepare_adoption(
        &source,
        Some(&lock),
        &AdditionGroup::from_resolved(&requested).unwrap(),
    )
    .unwrap();
    assert_eq!(adopted.project().intent().roots.len(), 2);
    assert_eq!(
        adopted.project().intent().roots[&key("root")],
        current.intent().roots[&key("root")]
    );
    assert!(
        !adopted
            .project()
            .intent()
            .roots
            .contains_key(&key("dependency"))
    );
    assert_eq!(
        adopted.project().lock().dependencies[&key("dependency")].selected,
        requested.lock().dependencies[&key("dependency")].selected
    );
    assert!(adopted.project().intent().roots.contains_key(&key("new")));
}
