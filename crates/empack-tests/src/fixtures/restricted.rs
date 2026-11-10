//! Restricted content exercises native saved recipes, not simulated packwiz output.
use crate::harness::process::TestProject;
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    identity::{CurseForgeProjectId, ProviderProjectId},
    model::*,
    path::InstallDestination,
    requirements::{Requirement, Requirements},
};
use empack_lib::engine::documents::DocumentCodec;
use std::fs;

pub fn fixture(name: &str) -> TestProject {
    fixture_on(TestProject::new(), name)
}

/// Use the same restricted inputs on a caller-owned temporary filesystem.
pub fn fixture_on(root: TestProject, name: &str) -> TestProject {
    let project = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
    with_provider_on(
        root,
        name,
        ResolvedPin {
            selection: project.parse_pin("456").unwrap(),
            project,
        },
    )
}
pub fn with_provider(name: &str, pin: ResolvedPin) -> TestProject {
    with_provider_on(TestProject::new(), name, pin)
}

fn with_provider_on(root: TestProject, name: &str, pin: ResolvedPin) -> TestProject {
    with_assets(
        root,
        name,
        vec![ManualAsset {
            key: "manual-assets",
            pin,
            kind: ContentKind::ResourcePack,
            destination: "resourcepacks/manual.zip",
            bytes: 7,
            digest: (
                "sha256",
                "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
            ),
        }],
    )
}

/// Two exact provider inputs for partial synchronization and retention journeys.
/// These are component fixtures, not provider-resolution or runnable-JAR evidence.
pub fn pair(name: &str) -> TestProject {
    pair_on(TestProject::new(), name)
}

/// Preserve the component fixture while choosing its temporary filesystem.
pub fn pair_on(root: TestProject, name: &str) -> TestProject {
    let assets = [
        ("one", "123", "456", "mods/one.jar", 5,
         "7fdd80dbdded156323d36c459e5fd133a4d888c227320cfb7042be9feb35d7f07201e535697af914e69d6f46b2a88655c86c2371288052ccd4fa92058b01d3fd"),
        ("two", "124", "457", "mods/two.jar", 6,
         "9381e9a67aa361751cea90178c094ad6133742163cbd14f146be5c3ee6606d4e8ab4bdd839e7c672baa6eb87e06f59b2d3a68ad0533f2a13ef6c0c5d8769216a"),
    ].into_iter().map(|(key, id, version, destination, bytes, digest)| {
        let project = ProviderProjectId::CurseForge(CurseForgeProjectId::parse(id).unwrap());
        ManualAsset {
            key,
            pin: ResolvedPin { selection: project.parse_pin(version).unwrap(), project },
            kind: ContentKind::Mod,
            destination,
            bytes,
            digest: ("sha512", digest),
        }
    }).collect();
    with_assets(root, name, assets)
}

struct ManualAsset {
    key: &'static str,
    pin: ResolvedPin,
    kind: ContentKind,
    destination: &'static str,
    bytes: u64,
    digest: (&'static str, &'static str),
}

fn with_assets(root: TestProject, name: &str, assets: Vec<ManualAsset>) -> TestProject {
    let project = root.with_workflow_fixture(name, "fabric", "1.21.1");
    let source = DocumentCodec
        .decode_intent(&fs::read(project.dir().join("empack.yml")).unwrap(), "test")
        .unwrap();
    let prior = DocumentCodec
        .decode_lock(
            &fs::read(project.dir().join("empack.lock")).unwrap(),
            &source,
            "test",
        )
        .unwrap();
    let mut intent = prior.intent().clone();
    intent.metadata.id = "restricted.fixture".into();
    intent.runtime.java_major = 21;
    intent.installation_policies = Default::default();
    let mut lock = prior.lock().clone();
    for asset in assets {
        let pin = asset.pin;
        let key = DependencyKey::parse(asset.key).unwrap();
        let identity = pin.project.clone();
        let requirements = Requirements {
            client: Requirement::Required,
            server: Requirement::Required,
        };
        let placements = NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(asset.destination).unwrap(),
            layer: ContentLayer::Common,
            requirements: requirements.clone(),
        }])
        .unwrap();
        let digests = DigestSet::new(vec![
            ExpectedDigest::parse(asset.digest.0, asset.digest.1).unwrap(),
        ])
        .unwrap();
        intent.roots.insert(
            key.clone(),
            DependencyIntent {
                releases: None,
                source: SourceIntent::Provider(identity.clone()),
                kind: asset.kind,
                selection: DependencySelection::ProviderPin(pin.selection.clone()),
                placement: PlacementIntent::Explicit(placements.clone()),
                requirements,
            },
        );
        lock.dependencies.insert(
            key.clone(),
            LockedDependency {
                incompatibilities: Vec::new(),
                source_selector: None,
                title: "Manual assets".into(),
                kind: asset.kind,
                identity: ResolvedIdentity::Provider(identity),
                selected: Some(pin.clone()),
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: FileSlot::parse("primary").unwrap(),
                    acquisition: AcquisitionSpec::Manual {
                        pin: Some(pin),
                        instructions: "Supply the verified provider file".into(),
                    },
                    expected: ExpectedContent {
                        digests: Some(digests.clone()),
                        size: Some(asset.bytes),
                        accepted_observation: None,
                    },
                    provenance: Provenance {
                        source: "test fixture".into(),
                        location: None,
                        declared_digests: Some(digests),
                        conversions: vec![],
                    },
                    placements,
                }])
                .unwrap(),
            },
        );
        lock.coverage
            .insert(key.clone(), Coverage::CompleteForSelection);
        lock.required_edges.insert(key, Default::default());
    }
    let bytes = DocumentCodec.encode_intent(&intent).unwrap();
    let revision = DocumentCodec
        .decode_intent(&bytes, "test")
        .unwrap()
        .semantic_revision();
    lock.intent_revision = revision;
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    fs::write(project.dir().join("empack.yml"), bytes).unwrap();
    fs::write(
        project.dir().join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    project
}

/// Independent expected inventory for the single-payload restricted fixture.
/// Provider-page evidence does not make the fixture payload a real provider download.
pub fn assert_mrpack(path: &std::path::Path, name: &str) {
    let members = crate::harness::archive::zip_members(path);
    assert_eq!(
        members.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["modrinth.index.json", "overrides/resourcepacks/manual.zip"]
    );
    assert_eq!(members["overrides/resourcepacks/manual.zip"], b"payload");
    let index: serde_json::Value = serde_json::from_slice(&members["modrinth.index.json"]).unwrap();
    assert_eq!(
        index,
        serde_json::json!({
            "formatVersion": 1, "game": "minecraft", "versionId": "1.0.0", "name": name,
            "files": [], "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.15.0"}
        })
    );
}
