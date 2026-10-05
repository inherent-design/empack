use super::*;
use crate::engine::{
    documents::DocumentCodec,
    mrpack::tests::project,
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{OperationOutcome, OperationRuntime},
    snapshot::SnapshotLimits,
};
use empack_core::path::PathSyntax;
use std::fs;

fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn write_project(root: &std::path::Path, project: &ResolvedProject) {
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(project).unwrap(),
    )
    .unwrap();
}
fn capture(root: &std::path::Path, host: &std::path::Path) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path("result.mrpack")],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}
#[test]
fn reference_evidence_and_layer_precedence_determine_missing_bytes_without_effects() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    write_project(root.path(), &project(false, false));
    let plan = |mode| {
        plan_build_acquisitions(
            &capture(root.path(), host.path()),
            &BuildAcquisitions::default(),
            mode,
            &cancel,
        )
        .unwrap()
    };
    assert!(
        plan(BuildMaterialization::ReferenceArchive)
            .needs()
            .is_empty()
    );
    assert_eq!(plan(BuildMaterialization::AllContent).needs().len(), 2);
    fs::create_dir_all(root.path().join("overrides/client/resourcepacks")).unwrap();
    fs::write(
        root.path().join("overrides/client/resourcepacks/a.zip"),
        b"replacement",
    )
    .unwrap();
    let layered = plan(BuildMaterialization::ReferenceArchive);
    assert_eq!(layered.needs().len(), 1);
    assert_eq!(
        layered.needs()[0].reason,
        AcquisitionReason::LayeredReplacement
    );
    write_project(root.path(), &project(true, false));
    assert_eq!(
        plan(BuildMaterialization::ReferenceArchive).needs().len(),
        2
    );
    // Captured materialization supplies evidence for the first slot's two placements.
    fs::create_dir_all(root.path().join("pack/resourcepacks")).unwrap();
    fs::write(root.path().join("pack/resourcepacks/copy.zip"), b"payload").unwrap();
    let remaining = plan(BuildMaterialization::ReferenceArchive);
    assert_eq!(remaining.needs().len(), 1);
    assert!(
        matches!(&remaining.needs()[0].key, AcquisitionKey::Locked(key) if key.slot.as_str()=="second")
    );
    assert!(!host.path().join("private").exists());
    assert!(!root.path().join("dist").exists());
}
#[tokio::test]
async fn download_batch_verifies_every_requested_file_before_artifact_publication() {
    for fail_second in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        write_project(root.path(), &project(true, false));
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/result.mrpack"), b"previous artifact").unwrap();
        let original_lock = fs::read(root.path().join("empack.lock")).unwrap();
        let workspace = capture(root.path(), host.path());
        let mut plan = plan_build_acquisitions(
            &workspace,
            &BuildAcquisitions::default(),
            BuildMaterialization::ReferenceArchive,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(plan.needs().len(), 2);
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/first")
            .with_body("payload")
            .create_async()
            .await;
        let second = server
            .mock("GET", "/second")
            .with_status(if fail_second { 503 } else { 200 })
            .with_body("payload")
            .create_async()
            .await;
        // The test resolver maps stable HTTPS declarations to a local transport fixture.
        for (need, name) in plan.needs.iter_mut().zip(["first", "second"]) {
            need.source = BuildContentSource::Download(
                NonEmpty::new(vec![format!("{}/{name}", server.url())]).unwrap(),
            );
        }
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            scratch_bytes: 16,
            open_files: 10,
        });
        let runtime = OperationRuntime::new(governor.clone(), 2);
        let transport = HttpAcquisition::for_loopback_tests();
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(plan
                    .acquire_http(
                        &transport,
                        &mut scope,
                        SourceEvidencePolicy::Compatibility,
                        TransferLimits {
                            file_bytes: 16,
                            transfer_bytes: 16,
                            ..TransferLimits::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        assert_eq!(
            fs::read(root.path().join("dist/result.mrpack")).unwrap(),
            b"previous artifact"
        );
        if fail_second {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        } else {
            let result = match &*outcome {
                OperationOutcome::Completed(Ok(result)) => result,
                _ => panic!("acquisition failed"),
            };
            assert!(result.pending.is_empty());
            assert_eq!(result.acquired.locked.len(), 2);
            let prepared = prepare_mrpack_build(
                workspace,
                path("result.mrpack"),
                &result.acquired,
                SourceEvidencePolicy::Compatibility,
                OptionalConversion::RejectMetadataLoss,
                &Cancellation::default(),
            )
            .unwrap();
            let publisher = Publisher::open(&host.path().join("private")).unwrap();
            prepared
                .publish(&publisher, &Cancellation::default())
                .unwrap();
            let mut archive = zip::ZipArchive::new(
                fs::File::open(root.path().join("dist/result.mrpack")).unwrap(),
            )
            .unwrap();
            let index: serde_json::Value =
                serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
            assert_eq!(index["files"].as_array().unwrap().len(), 3);
            assert_eq!(
                index["files"][0]["downloads"][0],
                "https://example.com/unrelated-name.jar"
            );
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                original_lock
            );
        }
        first.assert_async().await;
        second.assert_async().await;
    }
}
fn manual_embedded_project() -> ResolvedProject {
    let initial = project(true, false);
    let mut intent = initial.intent().clone();
    let id = empack_core::identity::ProviderProjectId::Modrinth(
        empack_core::identity::ModrinthProjectId::parse("AANobbMI").unwrap(),
    );
    let pin = ResolvedPin {
        project: id.clone(),
        selection: id.parse_pin("Version1").unwrap(),
    };
    intent.roots.values_mut().next().unwrap().source =
        empack_core::model::SourceIntent::Provider(id.clone());
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap();
    let mut lock = initial.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let dependency = lock.dependencies.values_mut().next().unwrap();
    dependency.identity = empack_core::model::ResolvedIdentity::Provider(id);
    dependency.selected = Some(pin.clone());
    let mut files = dependency.files.as_slice().to_vec();
    files[0].acquisition = AcquisitionSpec::Manual {
        pin: Some(pin),
        instructions: "select the exact file".into(),
    };
    files[1].acquisition = AcquisitionSpec::Embedded {
        archive: path("pack/sources/source.zip"),
        member: path("assets/item.zip"),
    };
    dependency.files = NonEmpty::new(files).unwrap();
    ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap()
}

#[tokio::test]
async fn unresolved_manual_and_archive_sources_remain_explicit_pending_obligations() {
    let resolved = manual_embedded_project();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &resolved);
    let plan = plan_build_acquisitions(
        &capture(root.path(), host.path()),
        &BuildAcquisitions::default(),
        BuildMaterialization::AllContent,
        &Cancellation::default(),
    )
    .unwrap();
    let governor = ResourceGovernor::new(ResourceRequest::default());
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(plan
                .acquire_http(
                    &HttpAcquisition::for_loopback_tests(),
                    &mut scope,
                    SourceEvidencePolicy::Compatibility,
                    TransferLimits::default(),
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let result = match &*outcome {
        OperationOutcome::Completed(Ok(result)) => result,
        _ => panic!("pending work must not need admission"),
    };
    assert_eq!(result.pending.len(), 2);
    assert!(result.acquired.locked.is_empty());
    assert!(matches!(
        &result.pending[0].source,
        BuildContentSource::Manual { pin: Some(_) }
    ));
    assert!(matches!(
        &result.pending[1].source,
        BuildContentSource::Embedded { .. }
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert!(!host.path().join("private").exists());
    assert!(!root.path().join("dist").exists());
}

#[test]
fn captured_embedded_members_feed_verified_publication_without_distributing_the_source_archive() {
    use std::io::{Cursor, Read, Write};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &manual_embedded_project());
    fs::create_dir_all(root.path().join("pack/resourcepacks")).unwrap();
    fs::write(root.path().join("pack/resourcepacks/a.zip"), b"payload").unwrap();
    fs::create_dir_all(root.path().join("pack/sources")).unwrap();
    let archive_path = root.path().join("pack/sources/source.zip");
    let write_source = |bytes: &[u8]| {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "assets/item.zip",
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
        fs::write(&archive_path, zip.finish().unwrap().into_inner()).unwrap();
    };
    write_source(b"payload");
    let cancel = Cancellation::default();
    let workspace = capture(root.path(), host.path());
    let plan = plan_build_acquisitions(
        &workspace,
        &BuildAcquisitions::default(),
        BuildMaterialization::ReferenceArchive,
        &cancel,
    )
    .unwrap();
    assert_eq!(plan.needs().len(), 1);
    let acquired = plan
        .begin()
        .acquire_embedded(
            &workspace,
            crate::engine::artifacts::ArchiveLimits::default(),
            SourceEvidencePolicy::Compatibility,
            &cancel,
        )
        .unwrap();
    assert!(acquired.pending.is_empty());
    assert_eq!(acquired.acquired.locked.len(), 1);
    let prepared = prepare_mrpack_build(
        workspace,
        path("result.mrpack"),
        &acquired.acquired,
        SourceEvidencePolicy::Compatibility,
        OptionalConversion::RejectMetadataLoss,
        &cancel,
    )
    .unwrap();
    assert!(!root.path().join("dist").exists());
    prepared
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    let previous = fs::read(root.path().join("dist/result.mrpack")).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&previous)).unwrap();
    assert!(zip.by_name("overrides/sources/source.zip").is_err());
    let mut entry = zip.by_name("client-overrides/resourcepacks/b.zip").unwrap();
    assert_ne!(entry.unix_mode().unwrap() & 0o100, 0);
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    write_source(b"changed");
    let workspace = capture(root.path(), host.path());
    let plan = plan_build_acquisitions(
        &workspace,
        &BuildAcquisitions::default(),
        BuildMaterialization::ReferenceArchive,
        &cancel,
    )
    .unwrap();
    assert!(
        plan.begin()
            .acquire_embedded(
                &workspace,
                crate::engine::artifacts::ArchiveLimits::default(),
                SourceEvidencePolicy::Compatibility,
                &cancel
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/result.mrpack")).unwrap(),
        previous
    );
}

#[test]
fn selected_artifact_cannot_overwrite_a_declared_local_source() {
    let initial = project(true, false);
    let mut intent = initial.intent().clone();
    let source = path("dist/result.mrpack");
    intent.roots.values_mut().next().unwrap().source =
        empack_core::model::SourceIntent::Local(source.clone());
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap();
    let mut lock = initial.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let (key, dependency) = lock.dependencies.iter_mut().next().unwrap();
    dependency.identity = empack_core::model::ResolvedIdentity::Local(key.clone());
    dependency.files = NonEmpty::new(
        dependency
            .files
            .as_slice()
            .iter()
            .cloned()
            .map(|mut file| {
                file.acquisition = AcquisitionSpec::Local(source.clone());
                file
            })
            .collect(),
    )
    .unwrap();
    let project = ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &project);
    fs::create_dir(root.path().join("dist")).unwrap();
    fs::write(root.path().join(source.as_str()), b"payload").unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(host.path().join("private")));
    for output in ["result.mrpack", "RESULT.mrpack"] {
        assert!(
            reader
                .capture_build(
                    root.path(),
                    &[path(output)],
                    SnapshotLimits::default(),
                    &Cancellation::default()
                )
                .is_err()
        );
    }
    assert_eq!(
        fs::read(root.path().join(source.as_str())).unwrap(),
        b"payload"
    );
    assert!(!host.path().join("private").exists());
}
