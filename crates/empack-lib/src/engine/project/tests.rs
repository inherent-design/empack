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
    fs::create_dir(project.join("dist")).unwrap();
    fs::write(project.join("dist/old.zip"), vec![0; 8192]).unwrap();
    let captured = reader
        .capture_build(
            &project,
            &[PortableRelPath::parse("new.mrpack", PathSyntax::ArtifactName).unwrap()],
            SnapshotLimits {
                file_bytes: 4096,
                ..SnapshotLimits::default()
            },
            &cancel,
        )
        .unwrap();
    assert!(matches!(
        captured
            .observations()
            .entries()
            .get(&PortableRelPath::parse("dist/new.mrpack", PathSyntax::ProjectContent).unwrap()),
        Some(Observation::Absent)
    ));
    fs::write(project.join("dist/old.zip"), b"unrelated replacement").unwrap();
    captured
        .root()
        .revalidate(captured.observations(), &cancel)
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
    assert_eq!(fs::read_dir(&project).unwrap().count(), 4);
    assert!(
        !captured
            .observations()
            .entries()
            .keys()
            .any(|path| path.as_str() == "dist/old.zip")
    );
    let prepare = || {
        let workspace = reader
            .capture_build(
                &project,
                &[PortableRelPath::parse("new.mrpack", PathSyntax::ArtifactName).unwrap()],
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap();
        crate::engine::build::prepare_mrpack_build(
            workspace,
            PortableRelPath::parse("new.mrpack", PathSyntax::ArtifactName).unwrap(),
            &BTreeMap::new(),
            SourceEvidencePolicy::Compatibility,
            crate::engine::mrpack::OptionalConversion::RejectMetadataLoss,
            &cancel,
        )
    };
    let prepared = prepare().unwrap();
    assert!(prepared.bytes() > 0);
    assert!(prepared.conversions().is_empty());
    assert!(!project.join("dist/new.mrpack").exists());
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
    let publisher = crate::engine::publication::Publisher::open(&host).unwrap();
    assert!(prepared.publish(&publisher, &cancel).is_err());
    assert!(!project.join("dist/new.mrpack").exists());
    fs::write(project.join("input.jar"), b"input").unwrap();
    let receipt = prepare().unwrap().publish(&publisher, &cancel).unwrap();
    assert_eq!(receipt.changed_files, 1);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(project.join("dist/new.mrpack")).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut archive.by_name("overrides/mods/local.jar").unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"input");
    assert_eq!(fs::read(project.join("empack.yml")).unwrap(), document);
    assert_eq!(
        fs::read(project.join("dist/old.zip")).unwrap(),
        b"unrelated replacement"
    );
    drop(archive);
    fs::create_dir_all(project.join("pack/config")).unwrap();
    fs::write(project.join("pack/config/fresh.txt"), b"fresh source").unwrap();
    prepare().unwrap().publish(&publisher, &cancel).unwrap();
    let mut fresh =
        zip::ZipArchive::new(fs::File::open(project.join("dist/new.mrpack")).unwrap()).unwrap();
    let mut source = Vec::new();
    std::io::Read::read_to_end(
        &mut fresh.by_name("overrides/config/fresh.txt").unwrap(),
        &mut source,
    )
    .unwrap();
    assert_eq!(source, b"fresh source");
    drop(fresh);
    let artifact = fs::read(project.join("dist/new.mrpack")).unwrap();
    fs::create_dir_all(project.join("pack/mods")).unwrap();
    fs::write(
        project.join("pack/mods/unaccounted.pw.toml"),
        r#"filename = "other.jar"
[download]
url = "https://example.com/other.jar"
hash-format = "md5"
hash = "00000000000000000000000000000000"
[update.modrinth]
mod-id = "AANobbMI"
version = "Version1"
"#,
    )
    .unwrap();
    assert!(prepare().is_err());
    assert_eq!(fs::read(project.join("dist/new.mrpack")).unwrap(), artifact);
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

#[test]
fn backend_observation_reads_captured_bytes_and_rejects_destination_aliases() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("empack.yml"), DOCUMENT).unwrap();
    fs::create_dir_all(temp.path().join("pack/mods")).unwrap();
    let metadata = r#"filename = "renderer.jar"
side = "client"
[download]
url = "https://example.com/renderer.jar"
hash-format = "sha256"
hash = "0000000000000000000000000000000000000000000000000000000000000000"
[update.modrinth]
mod-id = "AANobbMI"
version = "Version1"
[option]
optional = true
default = false
description = "Renderer"
"#;
    let path = temp.path().join("pack/mods/arbitrary-label.pw.toml");
    fs::write(&path, metadata).unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("unused-host")));
    let cancel = Cancellation::default();
    let capture = || {
        reader
            .capture(
                temp.path(),
                &[PortableRelPath::parse("pack", PathSyntax::ProjectContent).unwrap()],
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap()
    };
    let snapshot = capture();
    let files = snapshot.backend_files(&cancel).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].metadata_path.as_str(),
        "mods/arbitrary-label.pw.toml"
    );
    assert_eq!(
        files[0].destination.relative().as_str(),
        "mods/renderer.jar"
    );
    assert_eq!(
        files[0].provider.as_ref().unwrap().project.to_string(),
        "AANobbMI"
    );
    assert!(!files[0].optional.as_ref().unwrap().default_enabled);
    fs::write(&path, metadata.replace("Version1", "Version2")).unwrap();
    assert!(snapshot.backend_files(&cancel).is_err());
    fs::write(
        temp.path().join("pack/mods/second.pw.toml"),
        metadata.replace("renderer.jar", "Renderer.jar"),
    )
    .unwrap();
    assert!(capture().backend_files(&cancel).is_err());
}

#[test]
fn source_enumeration_includes_new_content_preserves_sides_and_honors_captured_rules() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("empack.yml"), DOCUMENT).unwrap();
    for name in [
        "pack/config/new.toml",
        "pack/config/private.toml",
        "pack/mods/ignored.pw.toml",
        "pack/index.toml",
        "overrides/client/config/new.toml",
        "overrides/server/config/private.toml",
    ] {
        let path = temp.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"payload").unwrap();
    }
    let ignore = temp.path().join("pack/.packwizignore");
    fs::write(&ignore, b"config/private.toml\nmods/ignored.pw.toml\n").unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("unused-host")));
    let cancel = Cancellation::default();
    let scopes: Vec<_> = ["pack", "overrides/client", "overrides/server"]
        .into_iter()
        .map(|path| PortableRelPath::parse(path, PathSyntax::ProjectContent).unwrap())
        .collect();
    let snapshot = reader
        .capture(temp.path(), &scopes, SnapshotLimits::default(), &cancel)
        .unwrap();
    let entries = snapshot.source_entries(&cancel).unwrap();
    let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "overrides/client/config/new.toml",
            "overrides/server/config/private.toml",
            "pack/config/new.toml"
        ]
    );
    assert!(snapshot.backend_files(&cancel).unwrap().is_empty());
    fs::write(&ignore, b"").unwrap();
    assert!(snapshot.source_entries(&cancel).is_err());
    let documents_only = reader
        .capture(temp.path(), &[], SnapshotLimits::default(), &cancel)
        .unwrap();
    assert!(documents_only.source_entries(&cancel).is_err());
}
