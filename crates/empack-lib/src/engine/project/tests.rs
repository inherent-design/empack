use super::*;
use std::fs;
const DOCUMENT: &str = r#"schema: 2
pack: {name: Example, version: alpha}
runtime: {minecraft: '1.20.1', loader: {kind: vanilla}}
distribution: {targets: [mrpack], archive: zip}
dependencies: {}
layout: {}
extensions: {}
"#;

#[test]
fn build_capture_binds_local_sources_outside_managed_namespaces_and_acquires_read_only() {
    use empack_core::{
        digest::{DigestSet, ExpectedDigest},
        model::*,
        path::InstallDestination,
        requirements::*,
    };
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("input.jar"), b"input").unwrap();
    let source = PortableRelPath::parse("input.jar", PathSyntax::ProjectContent).unwrap();
    let key = DependencyKey::parse("local").unwrap();
    let requirements = Requirements {
        client: Requirement::Required,
        server: Requirement::Required,
    };
    let mut intent = DocumentCodec
        .decode_intent(DOCUMENT.as_bytes(), "fixture")
        .unwrap()
        .intent()
        .clone();
    intent.roots.insert(
        key.clone(),
        DependencyIntent {
            source: SourceIntent::Local(source.clone()),
            kind: ContentKind::Mod,
            version: VersionIntent::FollowCompatible,
            placement: PlacementIntent::Automatic,
            requirements: requirements.clone(),
        },
    );
    let document = DocumentCodec.encode_intent(&intent).unwrap();
    let decoded = DocumentCodec.decode_intent(&document, "fixture").unwrap();
    let expected = ExpectedContent {
        digests: Some(
            DigestSet::new(vec![ExpectedDigest::Sha256(
                Sha256::digest(b"input").into(),
            )])
            .unwrap(),
        ),
        size: Some(5),
        accepted_observation: None,
    };
    let file = ResolvedFile {
        slot: FileSlot::parse("main").unwrap(),
        acquisition: AcquisitionSpec::Local(source.clone()),
        expected: expected.clone(),
        provenance: Provenance {
            source: "local".into(),
            location: None,
            declared_digests: None,
            conversions: vec![],
        },
        placements: NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse("mods/local.jar").unwrap(),
            layer: ContentLayer::Common,
            requirements,
        }])
        .unwrap(),
    };
    let lock = ResolutionLock {
        intent_revision: decoded.semantic_revision(),
        resolver: "fixture.v1".into(),
        dependencies: BTreeMap::from([(
            key.clone(),
            LockedDependency {
                title: "Local".into(),
                kind: ContentKind::Mod,
                identity: ResolvedIdentity::Local(key.clone()),
                selected: None,
                files: NonEmpty::new(vec![file]).unwrap(),
            },
        )]),
        required_edges: BTreeMap::new(),
        coverage: BTreeMap::from([(key, Coverage::Unknown)]),
        runtime: RuntimeResolution {
            minecraft: intent.runtime.minecraft.clone(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
        },
    };
    let resolved = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
    fs::write(project.join("empack.yml"), &document).unwrap();
    fs::write(
        project.join("empack.lock"),
        DocumentCodec.encode_lock(&resolved).unwrap(),
    )
    .unwrap();
    let host = temp.path().join("uncreated-state");
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let cancel = Cancellation::default();
    let captured = reader
        .capture_build(&project, SnapshotLimits::default(), &cancel)
        .unwrap();
    let (content, _) = captured
        .acquire_file(
            &source,
            Some(&expected),
            SourceEvidencePolicy::Compatibility,
            &cancel,
        )
        .unwrap();
    assert_eq!(content.lease().len(), 5);
    assert!(!host.exists());
    assert_eq!(fs::read_dir(&project).unwrap().count(), 3);
    fs::write(project.join("input.jar"), b"other").unwrap();
    assert!(
        captured
            .acquire_file(
                &source,
                Some(&expected),
                SourceEvidencePolicy::Compatibility,
                &cancel
            )
            .is_err()
    );
    assert!(
        captured
            .acquire_file(
                &PortableRelPath::parse("uncaptured.jar", PathSyntax::ProjectContent).unwrap(),
                None,
                SourceEvidencePolicy::Compatibility,
                &cancel
            )
            .is_err()
    );
}
#[test]
fn capture_is_read_only_and_missing_lock_is_distinct_from_current_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("empack.yml"), DOCUMENT).unwrap();
    let host = temp.path().join("uncreated-host-state");
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let snapshot = reader
        .capture(
            &project,
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(snapshot.prior_lock().is_none());
    assert!(snapshot.require_resolved().is_err());
    assert!(!host.exists());
    assert_eq!(fs::read_dir(&project).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(project.join("empack.yml")).unwrap(),
        DOCUMENT
    );
}
#[test]
fn malformed_lock_is_an_error_and_comment_changes_invalidate_preparation() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("empack.yml"), DOCUMENT).unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("private-state")));
    let snapshot = reader
        .capture(
            temp.path(),
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    fs::write(
        temp.path().join("empack.yml"),
        format!("# new comment\n{DOCUMENT}"),
    )
    .unwrap();
    assert!(
        snapshot
            .root()
            .revalidate(snapshot.observations(), &Cancellation::default())
            .is_err()
    );
    fs::write(temp.path().join("empack.lock"), "schema: 999\n").unwrap();
    assert!(
        reader
            .capture(
                temp.path(),
                &[],
                SnapshotLimits::default(),
                &Cancellation::default()
            )
            .is_err()
    );
    assert!(!temp.path().join("private-state").exists());
}
