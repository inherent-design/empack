use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        project::ProjectReader,
        publication::{Publisher, RecoveryReader},
        resources::{ResourceGovernor, ResourceRequest},
        runtime::{OperationOutcome, OperationRuntime},
        snapshot::SnapshotLimits,
    },
};
use empack_core::{
    identity::CurseForgeFileId,
    requirements::{Requirement, Requirements},
};
use serde_json::json;
use sha2::Digest;
use std::fs;
use std::io::{Cursor, Write};

#[tokio::test]
async fn provider_world_interpretation_publishes_members_and_preserves_identity_on_sync() {
    let mut source = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for member in ["Downloaded/level.dat", "Downloaded/region/r.0.0.mca"] {
        source
            .start_file(member, zip::write::SimpleFileOptions::default())
            .unwrap();
        source.write_all(b"payload").unwrap();
    }
    let source = source.finish().unwrap().into_inner();
    let mut server = mockito::Server::new_async().await;
    server.mock("GET","/mods/123").with_body(json!({"data":{"id":123,"gameId":432,"classId":17,"slug":"world-title","name":"World"}}).to_string()).create_async().await;
    server.mock("GET","/mods/123/files/456").with_body(json!({"data":{"id":456,"gameId":432,"modId":123,"fileName":"download.zip","fileLength":source.len(),"downloadUrl":format!("{}/world.zip",server.url()),"hashes":[{"algo":2,"value":md5::Md5::digest(&source).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}],"gameVersions":["1.20.1"],"dependencies":[],"isAvailable":true,"releaseType":1,"fileDate":"2026-01-01T00:00:00Z"}}).to_string()).create_async().await;
    let download = server
        .mock("GET", "/world.zip")
        .with_body(source)
        .expect(1)
        .create_async()
        .await;
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into()));
    let base = super::super::tests::current();
    let original = base.clone();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 32 << 20,
        scratch_bytes: 1 << 20,
        open_files: 32,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let input = ProviderAddInput {
                    selector: ProjectSelector::parse(ProviderKind::CurseForge, "123")?,
                    key: Some(DependencyKey::parse("my-world")?),
                    kind: Some(ContentKind::World),
                    pin: Some(PinSelector::CurseForgeFile(CurseForgeFileId::parse("456")?)),
                    requirements: Requirements {
                        client: Requirement::Required,
                        server: Requirement::Unsupported,
                    },
                    folder: None,
                    files: ProviderFiles::PrimaryPlaced(NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse("saves/ChosenName")?,
                        layer: ContentLayer::Common,
                        requirements: Requirements {
                            client: Requirement::Required,
                            server: Requirement::Unsupported,
                        },
                    }])?),
                };
                let ProviderAdditionOutcome::Archives(draft) = catalog
                    .resolve_addition(
                        &mut scope,
                        &base,
                        NonEmpty::new(vec![input])?,
                        ReleasePolicy::PreferStable,
                        super::super::tests::limits(),
                    )
                    .await?
                else {
                    anyhow::bail!("World needs member interpretation before publication")
                };
                draft
                    .acquire(
                        &mut scope,
                        &HttpAcquisition::for_loopback_tests(),
                        SourceEvidencePolicy::Compatibility,
                        DirectFileLimits {
                            archive: crate::engine::artifacts::ArchiveLimits {
                                entries: 16,
                                total_bytes: 1024,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    )
                    .await
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    let addition = match &*outcome {
        OperationOutcome::Completed(Ok(addition)) => addition,
        OperationOutcome::Completed(Err(error)) => panic!("{error:#}"),
        _ => panic!("World operation failed"),
    };
    let key = DependencyKey::parse("my-world").unwrap();
    let dependency = &addition.project().lock().dependencies[&key];
    assert_eq!(dependency.files.as_slice().len(), 2);
    assert!(matches!(
        dependency.identity,
        ResolvedIdentity::Provider(ProviderProjectId::CurseForge(_))
    ));
    assert!(matches!(
        addition.project().intent().roots[&key].placement,
        PlacementIntent::ArchiveRoot(_)
    ));
    assert_eq!(addition.materialized().len(), 2);
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(original.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&original).unwrap(),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(state.path().join("operations")));
    let snapshot = reader
        .capture_addition(
            root.path(),
            addition.group(),
            SnapshotLimits::default(),
            &cancel,
        )
        .unwrap();
    let prepared = crate::engine::addition::plan_addition(
        snapshot,
        addition.group(),
        addition.materialized().clone(),
        &cancel,
    )
    .unwrap()
    .stage(&cancel)
    .unwrap();
    prepared
        .publish(
            &Publisher::open(&state.path().join("operations")).unwrap(),
            &cancel,
        )
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("pack/saves/ChosenName/level.dat")).unwrap(),
        b"payload"
    );
    assert!(
        !root
            .path()
            .join("pack/saves/ChosenName/download.zip")
            .exists()
    );
    for _ in 0..2 {
        let snapshot = reader
            .capture_recorded_synchronization(root.path(), None, SnapshotLimits::default(), &cancel)
            .unwrap();
        let inputs = crate::engine::synchronization::recorded::RecordedInputs::new(
            snapshot,
            None,
            crate::engine::artifacts::ArchiveLimits::default(),
            &cancel,
        )
        .unwrap();
        let prepared = inputs
            .prepare_with_references(
                SourceEvidencePolicy::Compatibility,
                BTreeMap::new(),
                &cancel,
            )
            .unwrap()
            .stage(&cancel)
            .unwrap();
        assert!(prepared.files().changes().is_empty());
    }
    download.assert_async().await;
    drop(outcome);
    drop(handle);
    drop(runtime);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
