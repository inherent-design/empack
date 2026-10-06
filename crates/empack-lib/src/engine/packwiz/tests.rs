use super::*;
use empack_core::{
    digest::DigestSet,
    identity::{CurseForgeFileId, CurseForgeProjectId},
    inventory::{InventoryInput, OptionalPolicy},
    model::{ContentLayer, FileSlot, GameVersion, NonEmpty, ResolvedPin},
    path::InstallDestination,
    requirements::{ChoiceKey, OptionalChoice, Requirement, Requirements},
};
use std::io::Read;
fn input(name: &str, optional: bool) -> InventoryInput {
    InventoryInput {
        owner: ContentOwner::Source(name.into()),
        destination: InstallDestination::parse(name).unwrap(),
        layer: ContentLayer::Common,
        requirements: Requirements {
            client: if optional {
                Requirement::Optional(OptionalChoice {
                    key: ChoiceKey::parse("extra").unwrap(),
                    default_enabled: false,
                    description: Some("Extra texture".into()),
                })
            } else {
                Requirement::Required
            },
            server: Requirement::Unsupported,
        },
        representation: Representation::Download {
            expected: ExpectedContent {
                digests: Some(
                    DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap(),
                ),
                size: None,
                accepted_observation: None,
            },
            allowed: DownloadOrigins::Urls(
                NonEmpty::new(vec!["https://example.com/different-name.zip".into()]).unwrap(),
            ),
        },
    }
}
fn metadata() -> PackMetadata {
    PackMetadata {
        name: "Test\n\"quoted\"".into(),
        version: "alpha".into(),
        author: None,
        description: None,
    }
}
fn runtime() -> RuntimeResolution {
    RuntimeResolution {
        minecraft: GameVersion::parse("1.20.1").unwrap(),
        loader: LoaderKind::Vanilla,
        loader_version: None,
    }
}
fn text(file: &AcquiredBuildFile) -> String {
    let mut text = String::new();
    file.content
        .lease()
        .open()
        .read_to_string(&mut text)
        .unwrap();
    text
}
#[test]
fn reference_tree_preserves_destinations_optional_metadata_and_curseforge_identity() {
    let url = input("resourcepacks/renamed.zip", true);
    let mut provider = input("mods/provider.jar", false);
    if let Representation::Download { allowed, .. } = &mut provider.representation {
        *allowed = DownloadOrigins::Provider {
            pin: ResolvedPin {
                project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
                selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
            },
            slot: FileSlot::parse("primary").unwrap(),
        };
    }
    let inventory = BuildInventory::project(
        &[url, provider],
        BuildTarget::Client,
        &OptionalPolicy::Preserve,
    )
    .unwrap();
    let plan = PackwizPlan::prepare(
        inventory,
        &metadata(),
        &runtime(),
        &BTreeMap::new(),
        InstallerInteraction::Interactive,
        &Cancellation::default(),
    )
    .unwrap();
    let pack: toml::Value =
        toml::from_str(&text(&plan.files()[&path("pack.toml").unwrap()])).unwrap();
    assert_eq!(pack["name"].as_str().unwrap(), metadata().name);
    let index_file = &plan.files()[&path("index.toml").unwrap()];
    assert_eq!(
        pack["index"]["hash"].as_str().unwrap(),
        ExpectedDigest::Sha256(*index_file.content.lease().id().bytes()).hex()
    );
    let index: toml::Value = toml::from_str(&text(index_file)).unwrap();
    let entries = index["files"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert_eq!(entry["metafile"].as_bool(), Some(true));
        let file_path = path(entry["file"].as_str().unwrap()).unwrap();
        let file = &plan.files()[&file_path];
        assert_eq!(
            entry["hash"].as_str().unwrap(),
            ExpectedDigest::Sha256(*file.content.lease().id().bytes()).hex()
        );
        let parsed = BackendFile::parse(file_path, text(file).as_bytes()).unwrap();
        assert_eq!(parsed.digest.algorithm(), DigestAlgorithm::Md5);
        if parsed
            .destination
            .relative()
            .as_str()
            .starts_with("resourcepacks/")
        {
            assert_eq!(
                parsed.destination.relative().as_str(),
                "resourcepacks/renamed.zip"
            );
            assert!(!parsed.optional.unwrap().default_enabled);
            assert!(
                matches!(parsed.download, BackendDownload::Url(url) if url.ends_with("different-name.zip"))
            );
        } else {
            assert!(matches!(
                parsed.download,
                BackendDownload::CurseForgeMetadata
            ));
            assert_eq!(
                parsed.provider.unwrap().selection.unwrap(),
                PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap())
            );
        }
    }
}
#[test]
fn unrepresentable_choices_and_reserved_paths_fail_before_a_tree_is_returned() {
    for inputs in [
        vec![input("pack.toml", false)],
        vec![input("mods/a.jar", true), input("mods/b.jar", true)],
    ] {
        let inventory =
            BuildInventory::project(&inputs, BuildTarget::Client, &OptionalPolicy::Preserve)
                .unwrap();
        assert!(
            PackwizPlan::prepare(
                inventory,
                &metadata(),
                &runtime(),
                &BTreeMap::new(),
                InstallerInteraction::Interactive,
                &Cancellation::default()
            )
            .is_err()
        );
    }
    let mut local = input("config/example.txt", true);
    let acquired = generated(b"local", &Cancellation::default()).unwrap();
    local.representation = Representation::Embedded {
        content: acquired.content.lease().id(),
        bytes: 5,
        permissions: acquired.permissions,
    };
    let files = BTreeMap::from([(local.owner.clone(), acquired)]);
    for (policy, success) in [
        (OptionalPolicy::Preserve, false),
        (
            OptionalPolicy::Resolve {
                choices: BTreeMap::from([("extra".into(), true)]),
                use_defaults: false,
            },
            true,
        ),
    ] {
        let inventory =
            BuildInventory::project(&[local.clone()], BuildTarget::Client, &policy).unwrap();
        let result = PackwizPlan::prepare(
            inventory,
            &metadata(),
            &runtime(),
            &files,
            InstallerInteraction::Interactive,
            &Cancellation::default(),
        );
        assert_eq!(result.is_ok(), success);
        if let Ok(plan) = result {
            assert_eq!(
                text(&plan.files()[&path("config/example.txt").unwrap()]),
                "local"
            );
        }
    }
}

#[test]
fn headless_bootstrap_requires_resolved_optional_choices() {
    let optional = input("resourcepacks/extra.zip", true);
    for (policy, success, members) in [
        (OptionalPolicy::Preserve, false, 0),
        (
            OptionalPolicy::Resolve {
                choices: BTreeMap::from([("extra".into(), false)]),
                use_defaults: false,
            },
            true,
            2,
        ),
        (
            OptionalPolicy::Resolve {
                choices: BTreeMap::from([("extra".into(), true)]),
                use_defaults: false,
            },
            true,
            3,
        ),
    ] {
        let inventory = BuildInventory::project(
            std::slice::from_ref(&optional),
            BuildTarget::Client,
            &policy,
        )
        .unwrap();
        let result = PackwizPlan::prepare(
            inventory,
            &metadata(),
            &runtime(),
            &BTreeMap::new(),
            InstallerInteraction::Headless,
            &Cancellation::default(),
        );
        assert_eq!(result.is_ok(), success);
        if let Ok(plan) = result {
            assert_eq!(plan.files().len(), members);
        }
    }
}
