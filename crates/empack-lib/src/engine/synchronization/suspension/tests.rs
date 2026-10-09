use super::*;
use crate::engine::{
    mrpack::tests::project,
    project::ProjectReader,
    publication::RecoveryReader,
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
    snapshot::SnapshotLimits,
};

async fn small_record_roundtrip(preseed: bool) {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    let state = root.path().join("state");
    std::fs::create_dir(&target).unwrap();
    let project = project(false, false);
    let intent = DocumentCodec.encode_intent(project.intent()).unwrap();
    let lock = DocumentCodec.encode_lock(&project).unwrap();
    std::fs::write(target.join("empack.yml"), &intent).unwrap();
    std::fs::write(target.join("empack.lock"), &lock).unwrap();
    let workspace = ProjectReader::new(RecoveryReader::new(state.clone()))
        .capture(
            &target,
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    let context =
        SyncInputContext::capture(target.clone(), &workspace, &Cancellation::default()).unwrap();
    if preseed {
        FileContentStore::open(
            &state.join("pending-sync-content"),
            ContentStoreLimits::default(),
        )
        .unwrap();
        store::save(
            &state,
            Kind::Synchronization,
            &target,
            &Record {
                schema: 1,
                binding: context.binding.clone(),
                intent: String::from_utf8(intent.clone()).unwrap(),
                lock: String::from_utf8(lock.clone()).unwrap(),
                strong: false,
                files: vec![],
            },
            None,
            &Cancellation::default(),
        )
        .unwrap();
    }
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 8 << 20,
        open_files: 32,
        scratch_bytes: 4096,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let work_target = target.clone();
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let limits = crate::engine::acquisition::TransferLimits {
                    file_bytes: 1024,
                    transfer_bytes: 4096,
                    ..Default::default()
                };
                if !preseed {
                    save_pending_sync(
                        &mut scope,
                        SyncRetentionRequest {
                            state: state.clone(),
                            context: context.clone(),
                            project: project.clone(),
                            acquired: BTreeMap::new(),
                            policy: SourceEvidencePolicy::Compatibility,
                            prior: None,
                            limits,
                        },
                    )
                    .await?;
                }
                let resumed = load_pending_sync(
                    &mut scope,
                    state,
                    context,
                    SourceEvidencePolicy::Compatibility,
                    limits,
                )
                .await?
                .context("missing record")?;
                assert_eq!(resumed.project.intent(), project.intent());
                assert_eq!(resumed.project.lock(), project.lock());
                assert!(resumed.acquired.is_empty());
                assert_eq!(std::fs::read(work_target.join("empack.yml"))?, intent);
                assert_eq!(std::fs::read(work_target.join("empack.lock"))?, lock);
                Ok::<_, anyhow::Error>(())
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(result) => {
            result.as_ref().unwrap();
        }
        OperationOutcome::Failed(error) => panic!("{error}"),
    }
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn small_pending_sync_saves_and_resumes_under_eight_mib() {
    small_record_roundtrip(false).await;
}
#[tokio::test]
async fn existing_small_sync_resumes_under_eight_mib() {
    small_record_roundtrip(true).await;
}
