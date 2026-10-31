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
    intent.roots.get_mut(&key("root")).unwrap().selection = DependencySelection::ProviderPin(
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

fn with_incompatibility(
    project: &ResolvedProject,
    from: &str,
    target: &str,
    selection: Option<&str>,
) -> ResolvedProject {
    let mut lock = project.lock().clone();
    lock.dependencies
        .get_mut(&key(from))
        .unwrap()
        .incompatibilities
        .push(ProviderIncompatibility {
            project: Some(ProviderProjectId::Modrinth(
                ModrinthProjectId::parse(target).unwrap(),
            )),
            selection: selection.map(|version| {
                PinSelector::ModrinthVersion(ModrinthVersionId::parse(version).unwrap())
            }),
        });
    ResolvedProject::validate(
        project.intent().clone(),
        lock,
        project.lock().intent_revision,
    )
    .unwrap()
}

#[test]
fn update_checks_new_negative_evidence_against_unselected_retained_inventory() {
    // B is installed but neither an authored root nor a newly requested dependency.
    let current = fixture(
        &[("a", "ProjectA", "VersionA"), ("b", "ProjectB", "VersionB")],
        &["a"],
        &[],
        true,
    );
    let selected = fixture(&[("a", "ProjectA", "Version2")], &["a"], &[], true);
    for (version, conflicts) in [
        (None, true),
        (Some("VersionB"), true),
        (Some("Version3"), false),
    ] {
        let proposed = with_incompatibility(&selected, "a", "ProjectB", version);
        let result = update(&current, &proposed);
        if conflicts {
            let error = result.err().unwrap();
            assert!(
                format!("{error:#}").contains("a declares incompatibility with b"),
                "{error:#}"
            );
        } else {
            let updated = result.unwrap();
            assert_eq!(updated.project().intent(), current.intent());
            assert_eq!(updated.project().lock().dependencies.len(), 2);
            assert_eq!(
                updated.project().lock().dependencies[&key("b")],
                current.lock().dependencies[&key("b")]
            );
            assert_eq!(
                updated.project().lock().dependencies[&key("a")].incompatibilities,
                proposed.lock().dependencies[&key("a")].incompatibilities
            );
        }
    }
}

#[test]
fn update_retains_old_negative_evidence_until_its_selection_changes() {
    let base = fixture(
        &[("a", "ProjectA", "VersionA"), ("b", "ProjectB", "VersionB")],
        &["a", "b"],
        &[],
        true,
    );
    // Old B permits current A, but explicitly excludes the proposed new A.
    let current = with_incompatibility(&base, "b", "ProjectA", Some("Version2"));
    let a = fixture(&[("a", "ProjectA", "Version2")], &["a"], &[], true);
    let error = update(&current, &a).err().unwrap();
    assert!(
        format!("{error:#}").contains("b declares incompatibility with a"),
        "{error:#}"
    );

    let refreshed_b = fixture(&[("b", "ProjectB", "VersionB")], &["b"], &[], true);
    let refreshed = update(&current, &refreshed_b).unwrap();
    assert_eq!(
        refreshed.project().lock().dependencies[&key("b")].incompatibilities,
        current.lock().dependencies[&key("b")].incompatibilities
    );
    // Requesting B again without its old assertion cannot launder a conflicting A.
    for (b_version, accepted) in [("VersionB", false), ("Version3", true)] {
        let proposed = fixture(
            &[("a", "ProjectA", "Version2"), ("b", "ProjectB", b_version)],
            &["a", "b"],
            &[],
            true,
        );
        let result = update(&current, &proposed);
        if !accepted {
            let error = result.err().unwrap();
            assert!(
                format!("{error:#}").contains("b declares incompatibility with a"),
                "{error:#}"
            );
            continue;
        }
        let updated = result.unwrap();
        assert_eq!(updated.project().intent(), current.intent());
        assert_eq!(updated.project().lock().dependencies.len(), 2);
        for name in ["a", "b"] {
            assert_eq!(
                updated.project().lock().dependencies[&key(name)].selected,
                proposed.lock().dependencies[&key(name)].selected
            );
        }
        assert!(
            updated.project().lock().dependencies[&key("b")]
                .incompatibilities
                .is_empty()
        );
    }
}
