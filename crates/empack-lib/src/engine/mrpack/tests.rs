use super::*;
use crate::engine::content::{InitialObservation, SourceEvidencePolicy, verify_stream};
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    model::*,
    requirements::*,
};
use std::io::{Read, Seek};

fn acquired(mut bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut bytes,
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn build_file(bytes: &[u8]) -> AcquiredBuildFile {
    AcquiredBuildFile {
        content: acquired(bytes),
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
fn project(weak: bool, optional: bool) -> ResolvedProject {
    let url = "https://example.com/unrelated-name.jar";
    let environment = if optional {
        json!({"client":{"optional":"extra","default-enabled":false,"description":"Extra content"},"server":"unsupported"})
    } else {
        json!({"client":"required","server":"unsupported"})
    };
    let intent = DocumentCodec.decode_intent(&serde_json::to_vec(&json!({"schema":2,"pack":{"name":"Test","version":"alpha"},"runtime":{"minecraft":"1.20.1","loader":{"kind":"fabric","version":"0.16.0"}},"distribution":{"targets":["mrpack"],"archive":"zip"},"dependencies":{
        "assets":{"source":{"kind":"url","downloads":[url]},"content":"resource-pack","version":{"mode":"follow-compatible"},"placement":"automatic","environment":environment}
    }})).unwrap(), "test.yml").unwrap();
    let key = DependencyKey::parse("assets").unwrap();
    let content = acquired(b"payload");
    let digests = if weak {
        DigestSet::new(vec![
            ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
        ])
        .unwrap()
    } else {
        content.observed_digests().clone()
    };
    let files = [
        (
            "first",
            vec!["resourcepacks/a.zip", "resourcepacks/copy.zip"],
        ),
        ("second", vec!["resourcepacks/b.zip"]),
    ]
    .into_iter()
    .map(|(slot, destinations)| ResolvedFile {
        slot: FileSlot::parse(slot).unwrap(),
        acquisition: AcquisitionSpec::Url(NonEmpty::new(vec![url.into()]).unwrap()),
        expected: ExpectedContent {
            digests: Some(digests.clone()),
            size: Some(7),
            accepted_observation: None,
        },
        provenance: Provenance {
            source: "fixture".into(),
            location: None,
            declared_digests: Some(digests.clone()),
            conversions: vec![],
        },
        placements: NonEmpty::new(
            destinations
                .into_iter()
                .map(|destination| Placement {
                    destination: InstallDestination::parse(destination).unwrap(),
                    layer: ContentLayer::Common,
                    requirements: intent.intent().roots[&key].requirements.clone(),
                })
                .collect(),
        )
        .unwrap(),
    })
    .collect();
    let lock = ResolutionLock {
        intent_revision: intent.semantic_revision(),
        resolver: "fixture.v1".into(),
        dependencies: BTreeMap::from([(
            key.clone(),
            LockedDependency {
                title: "Assets".into(),
                kind: ContentKind::ResourcePack,
                identity: ResolvedIdentity::Url(key.clone()),
                selected: None,
                files: NonEmpty::new(files).unwrap(),
            },
        )]),
        required_edges: BTreeMap::new(),
        coverage: BTreeMap::from([(key, Coverage::Unknown)]),
        runtime: RuntimeResolution {
            minecraft: intent.intent().runtime.minecraft.clone(),
            loader: LoaderKind::Fabric,
            loader_version: intent.intent().runtime.loader_version.clone(),
        },
    };
    ResolvedProject::validate(intent.intent().clone(), lock, intent.semantic_revision()).unwrap()
}
fn source(layer: ContentLayer, bytes: &[u8]) -> SourceFile {
    SourceFile {
        label: format!("{layer:?}"),
        destination: InstallDestination::parse("config/value.bin").unwrap(),
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
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
        content: acquired(bytes),
    }
}
#[test]
fn normalized_multi_file_multi_placement_export_preserves_references_and_side_bytes() {
    let project = project(false, false);
    let plan = MrpackPlan::prepare(
        &project,
        &BTreeMap::new(),
        vec![
            source(ContentLayer::Common, b"common"),
            source(ContentLayer::Client, b"client"),
            source(ContentLayer::Server, b"server"),
        ],
        OptionalConversion::RejectMetadataLoss,
    )
    .unwrap();
    assert_eq!(plan.inventory().entries().len(), 6);
    let mut candidate = tempfile::tempfile().unwrap();
    let result = plan
        .write(&mut candidate, &Cancellation::default())
        .unwrap();
    assert_eq!(result.members(), 4);
    candidate.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(candidate).unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 3);
    assert_eq!(
        index["files"][0]["env"],
        json!({"client":"required","server":"unsupported"})
    );
    assert_eq!(index["files"][0]["path"], "resourcepacks/a.zip");
    assert_eq!(index["dependencies"]["fabric-loader"], "0.16.0");
    for (prefix, expected) in [
        ("overrides", "common"),
        ("client-overrides", "client"),
        ("server-overrides", "server"),
    ] {
        let mut bytes = String::new();
        archive
            .by_name(&format!("{prefix}/config/value.bin"))
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
}
#[test]
fn weak_source_assertions_require_acquisition_for_export_hashes_without_upgrading_the_lock() {
    let project = project(true, false);
    assert!(
        MrpackPlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
    let mut files = BTreeMap::new();
    for slot in ["first", "second"] {
        files.insert(
            LockedFileKey {
                dependency: DependencyKey::parse("assets").unwrap(),
                slot: FileSlot::parse(slot).unwrap(),
            },
            build_file(b"payload"),
        );
    }
    let plan = MrpackPlan::prepare(
        &project,
        &files,
        vec![],
        OptionalConversion::RejectMetadataLoss,
    )
    .unwrap();
    assert_eq!(plan.inventory().entries().len(), 3);
    assert_eq!(
        project
            .lock()
            .dependencies
            .values()
            .next()
            .unwrap()
            .files
            .as_slice()[0]
            .expected
            .digests
            .as_ref()
            .unwrap()
            .strongest(),
        DigestAlgorithm::Md5
    );
    files.insert(
        LockedFileKey {
            dependency: DependencyKey::parse("assets").unwrap(),
            slot: FileSlot::parse("first").unwrap(),
        },
        build_file(b"changed"),
    );
    assert!(
        MrpackPlan::prepare(
            &project,
            &files,
            vec![],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
}
#[test]
fn optional_metadata_conversion_is_explicit_and_embedded_optionality_never_flattens_silently() {
    let project = project(false, true);
    assert!(
        MrpackPlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
    let plan = MrpackPlan::prepare(
        &project,
        &BTreeMap::new(),
        vec![],
        OptionalConversion::AcknowledgedMetadataLoss,
    )
    .unwrap();
    assert_eq!(plan.conversions().len(), 1);
    let index: Value = serde_json::from_slice(&plan.index).unwrap();
    assert_eq!(index["files"][0]["env"]["client"], "optional");
    let mut optional = source(ContentLayer::Client, b"bytes");
    optional.requirements.client = Requirement::Optional(OptionalChoice {
        key: ChoiceKey::parse("local").unwrap(),
        default_enabled: true,
        description: None,
    });
    assert!(
        MrpackPlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![optional],
            OptionalConversion::AcknowledgedMetadataLoss
        )
        .is_err()
    );
}

#[test]
fn misplaced_acquisitions_and_cross_layer_portable_aliases_are_rejected() {
    let project = project(false, false);
    let unrelated = BTreeMap::from([(
        LockedFileKey {
            dependency: DependencyKey::parse("assets").unwrap(),
            slot: FileSlot::parse("unknown").unwrap(),
        },
        build_file(b"payload"),
    )]);
    assert!(
        MrpackPlan::prepare(
            &project,
            &unrelated,
            vec![],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
    let common = source(ContentLayer::Common, b"common");
    let mut client = source(ContentLayer::Client, b"client");
    client.destination = InstallDestination::parse("Config/value.bin").unwrap();
    assert!(
        MrpackPlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![common, client],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
}

#[test]
fn acquired_executable_is_embedded_with_portable_mode_instead_of_a_lossy_reference() {
    let project = project(false, false);
    let mut executable = build_file(b"payload");
    executable.permissions.executable = true;
    let files = BTreeMap::from([(
        LockedFileKey {
            dependency: DependencyKey::parse("assets").unwrap(),
            slot: FileSlot::parse("first").unwrap(),
        },
        executable,
    )]);
    let plan = MrpackPlan::prepare(
        &project,
        &files,
        vec![],
        OptionalConversion::RejectMetadataLoss,
    )
    .unwrap();
    let mut output = tempfile::tempfile().unwrap();
    plan.write(&mut output, &Cancellation::default()).unwrap();
    output.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(output).unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 1);
    for name in ["a.zip", "copy.zip"] {
        let file = archive
            .by_name(&format!("client-overrides/resourcepacks/{name}"))
            .unwrap();
        assert_eq!(file.unix_mode().unwrap() & 0o111, 0o111);
    }
}

#[test]
fn layered_downloads_preserve_common_and_side_bytes_without_duplicate_references() {
    let original = project(false, false);
    let mut lock = original.lock().clone();
    let key = DependencyKey::parse("assets").unwrap();
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    let mut files = dependency.files.as_slice().to_vec();
    for (index, file) in files.iter_mut().enumerate() {
        let layer = if index == 0 {
            ContentLayer::Common
        } else {
            ContentLayer::Client
        };
        let bytes: &[u8] = if index == 0 { b"common" } else { b"client" };
        file.expected.digests = Some(acquired(bytes).observed_digests().clone());
        file.expected.size = Some(bytes.len() as u64);
        file.provenance.declared_digests = file.expected.digests.clone();
        file.placements = NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse("resourcepacks/layered.zip").unwrap(),
            layer,
            requirements: Requirements {
                client: Requirement::Required,
                server: if index == 0 {
                    Requirement::Required
                } else {
                    Requirement::Unsupported
                },
            },
        }])
        .unwrap();
    }
    dependency.files = NonEmpty::new(files).unwrap();
    let mut intent = original.intent().clone();
    intent.roots.get_mut(&key).unwrap().placement = PlacementIntent::Explicit(
        NonEmpty::new(
            dependency
                .files
                .as_slice()
                .iter()
                .flat_map(|file| file.placements.as_slice().iter().cloned())
                .collect(),
        )
        .unwrap(),
    );
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap();
    lock.intent_revision = decoded.semantic_revision();
    let project = ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap();
    assert!(
        MrpackPlan::prepare(
            &project,
            &BTreeMap::new(),
            vec![],
            OptionalConversion::RejectMetadataLoss
        )
        .is_err()
    );
    let files = BTreeMap::from([
        (
            LockedFileKey {
                dependency: key.clone(),
                slot: FileSlot::parse("first").unwrap(),
            },
            build_file(b"common"),
        ),
        (
            LockedFileKey {
                dependency: key,
                slot: FileSlot::parse("second").unwrap(),
            },
            build_file(b"client"),
        ),
    ]);
    let plan = MrpackPlan::prepare(
        &project,
        &files,
        vec![],
        OptionalConversion::RejectMetadataLoss,
    )
    .unwrap();
    let mut output = tempfile::tempfile().unwrap();
    plan.write(&mut output, &Cancellation::default()).unwrap();
    output.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(output).unwrap();
    let index: Value =
        serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
    assert!(index["files"].as_array().unwrap().is_empty());
    for (prefix, expected) in [
        ("server-overrides", "common"),
        ("client-overrides", "client"),
    ] {
        let mut bytes = String::new();
        archive
            .by_name(&format!("{prefix}/resourcepacks/layered.zip"))
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
}

#[test]
fn optional_layered_fallback_requires_a_representable_conversion() {
    let original = project(false, true);
    let mut lock = original.lock().clone();
    let key = DependencyKey::parse("assets").unwrap();
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    let mut files = dependency.files.as_slice().to_vec();
    let mut placements = files[0].placements.as_slice().to_vec();
    for placement in &mut placements {
        placement.layer = ContentLayer::Client;
    }
    files[0].placements = NonEmpty::new(placements).unwrap();
    dependency.files = NonEmpty::new(files).unwrap();
    let project = ResolvedProject::validate(
        original.intent().clone(),
        lock.clone(),
        lock.intent_revision,
    )
    .unwrap();
    let mut common = source(ContentLayer::Common, b"fallback");
    common.destination = InstallDestination::parse("resourcepacks/a.zip").unwrap();
    let files = BTreeMap::from([(
        LockedFileKey {
            dependency: key,
            slot: FileSlot::parse("first").unwrap(),
        },
        build_file(b"payload"),
    )]);
    let error = MrpackPlan::prepare(
        &project,
        &files,
        vec![common],
        OptionalConversion::AcknowledgedMetadataLoss,
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("Optional side override needs a selection"),
        "{error}"
    );
}
