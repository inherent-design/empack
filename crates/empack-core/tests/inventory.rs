use empack_core::{
    digest::{ContentId, DigestSet},
    files::FilePermissions,
    inventory::*,
    model::{ContentLayer, ExpectedContent, NonEmpty},
    path::InstallDestination,
    projection::BuildTarget,
    requirements::*,
};
use std::collections::BTreeMap;
fn input(layer: ContentLayer, name: &str, byte: u8) -> InventoryInput {
    InventoryInput {
        owner: ContentOwner::Source(format!("{layer:?}/{name}")),
        destination: InstallDestination::parse(name).unwrap(),
        layer,
        requirements: Requirements {
            client: if layer == ContentLayer::Server {
                Requirement::Unsupported
            } else {
                Requirement::Required
            },
            server: if layer == ContentLayer::Client {
                Requirement::Unsupported
            } else {
                Requirement::Required
            },
        },
        representation: Representation::Embedded {
            content: ContentId::from_sha256([byte; 32]),
            bytes: 1,
            permissions: FilePermissions {
                readonly: false,
                executable: true,
            },
        },
    }
}
fn choice() -> OptionalChoice {
    OptionalChoice {
        key: ChoiceKey::parse("extra").unwrap(),
        default_enabled: false,
        description: Some("Extra resources".into()),
    }
}
#[test]
fn sides_preserve_distinct_bytes_and_mrpack_keeps_all_layers() {
    let entries = vec![
        input(ContentLayer::Server, "config/a", 3),
        input(ContentLayer::Common, "config/a", 1),
        input(ContentLayer::Client, "config/a", 2),
    ];
    for (target, byte) in [(BuildTarget::ClientFull, 2), (BuildTarget::ServerFull, 3)] {
        let inventory =
            BuildInventory::project(&entries, target, &OptionalPolicy::Preserve).unwrap();
        assert_eq!(inventory.entries().len(), 1);
        assert!(
            matches!(&inventory.entries()[0].representation,Representation::Embedded{content,permissions,..}if content.bytes()==&[byte;32] && permissions.executable)
        );
        assert_eq!(inventory.precedence().len(), 1);
    }
    let inventory =
        BuildInventory::project(&entries, BuildTarget::Mrpack, &OptionalPolicy::Preserve).unwrap();
    assert_eq!(inventory.entries().len(), 3);
    let mut reversed = entries.clone();
    reversed.reverse();
    assert_eq!(
        inventory,
        BuildInventory::project(&reversed, BuildTarget::Mrpack, &OptionalPolicy::Preserve).unwrap()
    );
}
#[test]
fn optionality_needs_explicit_full_policy_and_preserves_bootstrap_choices() {
    let mut optional = input(ContentLayer::Client, "resourcepacks/extra.zip", 1);
    optional.requirements.client = Requirement::Optional(choice());
    let inputs = [optional];
    assert!(matches!(
        BuildInventory::project(&inputs, BuildTarget::ClientFull, &OptionalPolicy::Preserve),
        Err(InventoryError::ChoiceRequired(_))
    ));
    let bootstrap =
        BuildInventory::project(&inputs, BuildTarget::Client, &OptionalPolicy::Preserve).unwrap();
    assert!(matches!(
        bootstrap.entries()[0].requirements.client,
        Requirement::Optional(_)
    ));
    let policy = OptionalPolicy::Resolve {
        choices: BTreeMap::new(),
        use_defaults: true,
    };
    let full = BuildInventory::project(&inputs, BuildTarget::ClientFull, &policy).unwrap();
    assert!(full.entries().is_empty());
    assert!(!full.choices()[0].enabled);
    assert!(full.choices()[0].used_default);
    let selected = OptionalPolicy::Resolve {
        choices: BTreeMap::from([("extra".into(), true)]),
        use_defaults: false,
    };
    assert_eq!(
        BuildInventory::project(&inputs, BuildTarget::ClientFull, &selected)
            .unwrap()
            .entries()
            .len(),
        1
    );
    let typo = OptionalPolicy::Resolve {
        choices: BTreeMap::from([("exrta".into(), true)]),
        use_defaults: true,
    };
    assert!(matches!(
        BuildInventory::project(&inputs, BuildTarget::ClientFull, &typo),
        Err(InventoryError::UnknownChoice(_))
    ));
}
#[test]
fn reference_presence_never_satisfies_full_bytes_and_replaced_content_is_recorded() {
    let mut reference = input(ContentLayer::Common, "mods/a.jar", 1);
    reference.representation = Representation::Download {
        expected: ExpectedContent {
            digests: Some(DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap()),
            size: Some(7),
            accepted_observation: None,
        },
        allowed: DownloadOrigins::Urls(
            NonEmpty::new(vec!["https://example.com/a.jar".into()]).unwrap(),
        ),
    };
    assert!(matches!(
        BuildInventory::project(
            &[reference.clone()],
            BuildTarget::ClientFull,
            &OptionalPolicy::Preserve
        ),
        Err(InventoryError::MaterializationRequired(_))
    ));
    assert_eq!(
        BuildInventory::project(
            &[reference.clone()],
            BuildTarget::Client,
            &OptionalPolicy::Preserve
        )
        .unwrap()
        .entries()
        .len(),
        1
    );
    let replacement = input(ContentLayer::Client, "mods/a.jar", 2);
    let full = BuildInventory::project(
        &[reference, replacement],
        BuildTarget::ClientFull,
        &OptionalPolicy::Preserve,
    )
    .unwrap();
    assert_eq!(full.entries().len(), 1);
    assert_eq!(full.precedence().len(), 1);
}
#[test]
fn duplicate_destinations_and_conflicting_choice_definitions_fail() {
    let mut a = input(ContentLayer::Common, "a", 1);
    let mut b = input(ContentLayer::Common, "b", 1);
    assert!(matches!(
        BuildInventory::project(
            &[a.clone(), a.clone()],
            BuildTarget::Mrpack,
            &OptionalPolicy::Preserve
        ),
        Err(InventoryError::DuplicateDestination(_))
    ));
    a.requirements.client = Requirement::Optional(choice());
    let mut different = choice();
    different.default_enabled = true;
    b.requirements.client = Requirement::Optional(different);
    assert!(matches!(
        BuildInventory::project(&[a, b], BuildTarget::Mrpack, &OptionalPolicy::Preserve),
        Err(InventoryError::ConflictingChoice(_))
    ));
}

#[test]
fn optional_overlay_keeps_fallback_or_requires_a_materialized_choice() {
    let common = input(ContentLayer::Common, "config/a", 1);
    let mut side = input(ContentLayer::Client, "config/a", 2);
    side.requirements.client = Requirement::Optional(choice());
    assert!(matches!(
        BuildInventory::project(
            &[common.clone(), side.clone()],
            BuildTarget::Client,
            &OptionalPolicy::Preserve
        ),
        Err(InventoryError::OptionalOverlayNeedsSelection(_))
    ));
    let disabled = OptionalPolicy::Resolve {
        choices: BTreeMap::new(),
        use_defaults: true,
    };
    let result = BuildInventory::project(
        &[common.clone(), side.clone()],
        BuildTarget::ClientFull,
        &disabled,
    )
    .unwrap();
    assert!(
        matches!(&result.entries()[0].representation,Representation::Embedded{content,..}if content.bytes()==&[1;32])
    );
    assert!(result.precedence().is_empty());
    let mut common_optional = common;
    common_optional.requirements.client = Requirement::Optional(choice());
    side.requirements.client = Requirement::Required;
    let result = BuildInventory::project(
        &[common_optional, side],
        BuildTarget::ClientFull,
        &OptionalPolicy::Preserve,
    )
    .unwrap();
    assert!(
        result.choices().is_empty(),
        "a replaced optional input does not require a meaningless decision"
    );
    assert_eq!(result.precedence().len(), 1);
}

#[test]
fn selection_can_defer_bytes_but_completed_inventory_cannot() {
    let mut common = input(ContentLayer::Common, "config/a", 1);
    common.representation = Representation::Unacquired {
        expected: empack_core::model::ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
    };
    let inputs = vec![common.clone()];
    for target in [
        BuildTarget::Mrpack,
        BuildTarget::Client,
        BuildTarget::ClientFull,
        BuildTarget::ServerFull,
    ] {
        let selected = BuildSelection::select(&inputs, target, &OptionalPolicy::Preserve).unwrap();
        assert_eq!(selected.entries().len(), 1);
        assert!(selected.finish().is_err());
        assert!(BuildInventory::project(&inputs, target, &OptionalPolicy::Preserve).is_err());
    }
    let side = input(ContentLayer::Client, "config/a", 2);
    let complete = BuildSelection::select(
        &[common, side],
        BuildTarget::ClientFull,
        &OptionalPolicy::Preserve,
    )
    .unwrap()
    .finish()
    .unwrap();
    assert_eq!(complete.entries().len(), 1);
    assert_eq!(complete.precedence().len(), 1);
}

#[test]
fn provider_references_preserve_unknown_size_and_require_real_byte_evidence() {
    use empack_core::{
        identity::*,
        model::{FileSlot, ResolvedPin},
    };
    let mut reference = input(ContentLayer::Common, "mods/a.jar", 1);
    let pin = ResolvedPin {
        project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
        selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
    };
    reference.representation = Representation::Download {
        expected: ExpectedContent {
            digests: Some(DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap()),
            size: None,
            accepted_observation: None,
        },
        allowed: DownloadOrigins::Provider {
            pin,
            slot: FileSlot::parse("primary").unwrap(),
        },
    };
    let planned = BuildInventory::project(
        &[reference.clone()],
        BuildTarget::Server,
        &OptionalPolicy::Preserve,
    )
    .unwrap();
    assert_eq!(
        planned.entries()[0].representation,
        reference.representation
    );
    assert!(matches!(
        BuildInventory::project(
            &[reference.clone()],
            BuildTarget::ServerFull,
            &OptionalPolicy::Preserve
        ),
        Err(InventoryError::MaterializationRequired(_))
    ));
    if let Representation::Download { expected, .. } = &mut reference.representation {
        expected.digests = None;
    }
    assert!(matches!(
        BuildInventory::project(
            &[reference.clone()],
            BuildTarget::Server,
            &OptionalPolicy::Preserve
        ),
        Err(InventoryError::InvalidReference(_))
    ));
    if let Representation::Download {
        expected,
        allowed: DownloadOrigins::Provider { pin, .. },
    } = &mut reference.representation
    {
        expected.accepted_observation = Some(ContentId::from_sha256([1; 32]));
        pin.selection = PinSelector::ModrinthVersion(ModrinthVersionId::parse("abcdefgh").unwrap());
    }
    assert!(matches!(
        BuildInventory::project(&[reference], BuildTarget::Server, &OptionalPolicy::Preserve),
        Err(InventoryError::InvalidReference(_))
    ));
}

#[test]
fn common_overrides_have_explicit_precedence_between_base_and_side() {
    let entries = vec![
        input(ContentLayer::Common, "config/a", 1),
        input(ContentLayer::CommonOverride, "config/a", 2),
        input(ContentLayer::Client, "config/a", 3),
    ];
    for (target, byte, replaced) in [
        (BuildTarget::ClientFull, 3, 2),
        (BuildTarget::ServerFull, 2, 1),
    ] {
        let inventory =
            BuildInventory::project(&entries, target, &OptionalPolicy::Preserve).unwrap();
        assert_eq!(inventory.entries().len(), 1);
        assert!(
            matches!(&inventory.entries()[0].representation, Representation::Embedded { content, .. } if content.bytes() == &[byte;32])
        );
        assert_eq!(inventory.precedence().len(), replaced);
        let mut reverse = entries.clone();
        reverse.reverse();
        assert_eq!(
            inventory,
            BuildInventory::project(&reverse, target, &OptionalPolicy::Preserve).unwrap()
        );
    }
    assert_eq!(
        BuildInventory::project(&entries, BuildTarget::Mrpack, &OptionalPolicy::Preserve)
            .unwrap()
            .entries()
            .len(),
        3
    );
}
#[test]
fn optional_common_override_requires_a_choice_and_preserves_disabled_fallback() {
    let base = input(ContentLayer::Common, "config/a", 1);
    let mut overlay = input(ContentLayer::CommonOverride, "config/a", 2);
    overlay.requirements.client = Requirement::Optional(choice());
    overlay.requirements.server = Requirement::Unsupported;
    let inputs = [base, overlay];
    assert!(matches!(
        BuildInventory::project(&inputs, BuildTarget::Client, &OptionalPolicy::Preserve),
        Err(InventoryError::OptionalOverlayNeedsSelection(_))
    ));
    for (enabled, byte) in [(false, 1), (true, 2)] {
        let inventory = BuildInventory::project(
            &inputs,
            BuildTarget::ClientFull,
            &OptionalPolicy::Resolve {
                choices: BTreeMap::from([("extra".into(), enabled)]),
                use_defaults: false,
            },
        )
        .unwrap();
        assert!(
            matches!(&inventory.entries()[0].representation, Representation::Embedded { content, .. } if content.bytes() == &[byte;32])
        );
        assert_eq!(inventory.choices()[0].enabled, enabled);
    }
    let inventory =
        BuildInventory::project(&inputs, BuildTarget::ServerFull, &OptionalPolicy::Preserve)
            .unwrap();
    assert!(
        matches!(&inventory.entries()[0].representation, Representation::Embedded { content, .. } if content.bytes() == &[1;32])
    );
}
