//! Restricted content exercises native saved recipes, not simulated packwiz output.
use crate::e2e::TestProject;
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
    let project = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap());
    with_provider(
        name,
        ResolvedPin {
            selection: project.parse_pin("456").unwrap(),
            project,
        },
    )
}
pub fn with_provider(name: &str, pin: ResolvedPin) -> TestProject {
    let project = TestProject::workflow_fixture(name, "fabric", "1.21.1");
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
    let mut lock = prior.lock().clone();
    let key = DependencyKey::parse("manual-assets").unwrap();
    let identity = pin.project.clone();
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Required,
    };
    let placements = NonEmpty::new(vec![Placement {
        destination: InstallDestination::parse("resourcepacks/manual.zip").unwrap(),
        layer: ContentLayer::Common,
        requirements: requirements.clone(),
    }])
    .unwrap();
    let digests = DigestSet::new(vec![
        ExpectedDigest::parse(
            "sha256",
            "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        )
        .unwrap(),
    ])
    .unwrap();
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            source: SourceIntent::Provider(identity.clone()),
            kind: ContentKind::ResourcePack,
            version: VersionIntent::Exact(pin.selection.clone()),
            placement: PlacementIntent::Explicit(placements.clone()),
            requirements,
        },
    );
    lock.dependencies.insert(
        key.clone(),
        LockedDependency {
            title: "Manual assets".into(),
            kind: ContentKind::ResourcePack,
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
                    size: Some(7),
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
    lock.coverage.insert(key, Coverage::CompleteForSelection);
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
