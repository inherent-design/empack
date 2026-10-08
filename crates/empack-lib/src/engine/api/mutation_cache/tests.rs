use super::*;
use crate::application::process_runtime::Cancellation;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
    snapshot::SnapshotLimits,
    staging::MutableStage,
};
use std::io::Read;
struct Candidate {
    project: ResolvedProject,
    stage: FrozenStage,
}
impl StagedMutation for Candidate {
    fn cache_parts(&mut self) -> (&ResolvedProject, &mut FrozenStage) {
        (&self.project, &mut self.stage)
    }
}
fn candidate(bytes: &[u8]) -> Candidate {
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&tests_path(), &mut &bytes[..], 7, &Cancellation::default())
        .unwrap();
    Candidate {
        project: crate::engine::mrpack::tests::project(false, false),
        stage: stage
            .freeze(SnapshotLimits::default(), &Cancellation::default())
            .unwrap(),
    }
}
fn tests_path() -> PortableRelPath {
    PortableRelPath::parse("pack/resourcepacks/a.zip", PathSyntax::ProjectContent).unwrap()
}
#[tokio::test]
async fn optional_cache_capacity_preserves_prepared_content() {
    let normal = ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 1024,
        open_files: 16,
    };
    for headroom in [
        ResourceRequest {
            memory_bytes: 0,
            ..normal
        },
        ResourceRequest {
            memory_bytes: 8192,
            ..normal
        },
        ResourceRequest {
            open_files: 4,
            ..normal
        },
        ResourceRequest { jobs: 0, ..normal },
        ResourceRequest {
            scratch_bytes: 0,
            ..normal
        },
    ] {
        for enabled in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let reserved = ResourceRequest {
                memory_bytes: 1 << 20,
                scratch_bytes: 7,
                open_files: 1,
                ..Default::default()
            };
            let governor = ResourceGovernor::new(ResourceRequest {
                jobs: headroom.jobs,
                memory_bytes: reserved.memory_bytes + headroom.memory_bytes,
                scratch_bytes: reserved.scratch_bytes + headroom.scratch_bytes,
                open_files: reserved.open_files + headroom.open_files,
            });
            let runtime = OperationRuntime::new(governor.clone(), 1);
            let prepared = candidate(b"payload");
            let cache = enabled
                .then(|| ContentCache::new(root.path().join("cache"), Default::default()).unwrap());
            let mut handle = runtime
                .start(move |mut scope| async move {
                    let permit = scope.reserve_storage(reserved)?;
                    let prepared = RetainedOutput::from_parts(prepared, permit);
                    let result = async {
                        let prepared = publish(prepared, cache, &mut scope).await?;
                        let (mut prepared, _permit) = prepared.into_parts();
                        let mut bytes = Vec::new();
                        prepared
                            .stage
                            .reader(&tests_path())?
                            .read_to_end(&mut bytes)?;
                        ensure!(
                            bytes == b"payload",
                            "Optional cache consumed prepared bytes"
                        );
                        Ok::<_, anyhow::Error>(())
                    }
                    .await;
                    Ok(result)
                })
                .unwrap();
            let result = handle.wait().await;
            match &*result {
                OperationOutcome::Completed(result) => assert!(
                    result.is_ok(),
                    "cache={enabled}, headroom={headroom:?}: {result:?}"
                ),
                OperationOutcome::Failed(error) => panic!("{error}"),
            }
            runtime.release_completed(handle.id());
            drop((result, handle));
            runtime.shutdown().await;
            assert_eq!(governor.status().reserved, ResourceRequest::default());
            assert!(!root.path().join("cache").exists());
        }
    }
}

#[tokio::test]
async fn optional_cache_does_not_hide_bad_bytes_cancellation_or_closed_admission() {
    for failure in ["bytes", "cancel", "closed"] {
        let root = tempfile::tempdir().unwrap();
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 4 << 20,
            scratch_bytes: 4096,
            open_files: 16,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let selected = governor.clone();
        let cache = ContentCache::new(root.path().join("cache"), Default::default()).unwrap();
        let prepared = candidate(if failure == "bytes" {
            b"changed"
        } else {
            b"payload"
        });
        let mut handle = runtime
            .start(move |mut scope| async move {
                let permit = scope.reserve_storage(ResourceRequest {
                    memory_bytes: 1 << 20,
                    scratch_bytes: 7,
                    open_files: 1,
                    ..Default::default()
                })?;
                if failure == "cancel" {
                    scope.cancellation().cancel();
                }
                if failure == "closed" {
                    selected.close();
                }
                let result = publish(
                    RetainedOutput::from_parts(prepared, permit),
                    Some(cache),
                    &mut scope,
                )
                .await;
                Ok(result.err().map(|error| format!("{error:#}")))
            })
            .unwrap();
        let result = handle.wait().await;
        let OperationOutcome::Completed(Some(error)) = &*result else {
            panic!("cache must report {failure}");
        };
        assert!(
            error.contains(match failure {
                "bytes" => "digest mismatch",
                "cancel" => "cancel",
                _ => "closed",
            }),
            "{error}"
        );
        assert!(!root.path().join("cache").exists());
        runtime.release_completed(handle.id());
        drop((result, handle));
        runtime.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn optional_cache_writer_admission_skips_only_capacity() {
    for cancelled in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            open_files: 0,
            ..Default::default()
        });
        let runtime = OperationRuntime::new(governor, 1);
        let cache = ContentCache::new(root.path().join("cache"), Default::default()).unwrap();
        let content = verify_stream(
            &mut b"payload".as_slice(),
            &empack_core::model::ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            7,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )
        .unwrap();
        let mut handle = runtime
            .start(move |mut scope| async move {
                if cancelled {
                    scope.cancellation().cancel();
                }
                Ok(cache.publish(&mut scope, vec![content]).await)
            })
            .unwrap();
        let result = handle.wait().await;
        let OperationOutcome::Completed(result) = &*result else {
            panic!("cache worker failed");
        };
        assert_eq!(result.is_err(), cancelled);
        assert!(!root.path().join("cache").exists());
        runtime.release_completed(handle.id());
        runtime.shutdown().await;
    }
}
