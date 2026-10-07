use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{AcquiredContent, verify_stream},
        project::ProjectReader,
        publication::{Publisher, RecoveryReader},
        resources::ResourceGovernor,
        runtime::{OperationOutcome, OperationRuntime},
        snapshot::SnapshotLimits,
    },
};
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    files::FilePermissions,
    path::InstallDestination,
    requirements::{ChoiceKey, OptionalChoice, Requirement},
};
use std::{fs, sync::Arc};

fn current() -> ResolvedProject {
    crate::engine::addition::tests::fixture(&[], &[], &[], true)
}
fn declared() -> ExpectedContent {
    ExpectedContent {
        digests: Some(
            DigestSet::new(vec![
                ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
            ])
            .unwrap(),
        ),
        size: Some(7),
        accepted_observation: None,
    }
}
fn input(file: AcquiredBuildFile, local: bool) -> AcquiredFileInput {
    let requirements = Requirements {
        client: Requirement::Optional(OptionalChoice {
            key: ChoiceKey::parse("assets").unwrap(),
            default_enabled: false,
            description: Some("Optional assets".into()),
        }),
        server: Requirement::Unsupported,
    };
    AcquiredFileInput {
        key: DependencyKey::parse("assets-alias").unwrap(),
        title: "Assets".into(),
        kind: ContentKind::ResourcePack,
        source: if local {
            AcquiredFileSource::Local
        } else {
            AcquiredFileSource::Url(
                NonEmpty::new(vec!["https://example.com/download?file=42".into()]).unwrap(),
            )
        },
        evidence: FileEvidence::Declared(declared()),
        requirements: requirements.clone(),
        placements: NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse("resourcepacks/chosen-name.zip").unwrap(),
            layer: ContentLayer::Client,
            requirements,
        }])
        .unwrap(),
        file,
    }
}
async fn prepare(
    mode: &'static str,
) -> (
    Arc<OperationOutcome<Result<FileAddition>>>,
    ResourceGovernor,
) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 2 << 20,
        scratch_bytes: 128,
        open_files: 10,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                let work = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        memory_bytes: 1 << 16,
                        scratch_bytes: 32,
                        open_files: 3,
                    },
                    ResourceRequest {
                        scratch_bytes: 32,
                        open_files: 3,
                        ..Default::default()
                    },
                    |cancel| {
                        verify_stream(
                            &mut &b"payload"[..],
                            &declared(),
                            32,
                            SourceEvidencePolicy::Compatibility,
                            InitialObservation::RequireEvidence,
                            &cancel,
                        )
                    },
                )?;
                let content = AcquiredContent::retain_resources(
                    scope.accept(work.wait().await?)?.transpose()?,
                )?;
                let mut value = input(
                    AcquiredBuildFile {
                        content,
                        permissions: FilePermissions {
                            readonly: true,
                            executable: false,
                        },
                    },
                    mode != "url",
                );
                let policy = if mode == "strong" {
                    SourceEvidencePolicy::StrongSourceRequired
                } else {
                    SourceEvidencePolicy::Compatibility
                };
                match mode {
                    "observed" | "strong" => value.evidence = FileEvidence::AcceptObserved,
                    "digest" => {
                        value.evidence = FileEvidence::Declared(ExpectedContent {
                            digests: Some(DigestSet::new(vec![ExpectedDigest::parse(
                                "md5",
                                "00000000000000000000000000000000",
                            )?])?),
                            ..declared()
                        })
                    }
                    "size" => {
                        value.evidence = FileEvidence::Declared(ExpectedContent {
                            size: Some(6),
                            ..declared()
                        })
                    }
                    "missing" => {
                        value.evidence = FileEvidence::Declared(ExpectedContent {
                            digests: None,
                            size: None,
                            accepted_observation: None,
                        })
                    }
                    "secret" => {
                        value.source = AcquiredFileSource::Url(NonEmpty::new(vec![
                            "https://example.com/file?api-key=fixture-secret".into(),
                        ])?)
                    }
                    _ => {}
                }
                let inputs = if mode == "duplicate" {
                    vec![value.clone(), value]
                } else {
                    vec![value]
                };
                FileAddition::from_acquired(&scope, &current(), NonEmpty::new(inputs)?, policy)
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    (outcome, governor)
}

#[tokio::test]
async fn acquired_local_and_url_files_preserve_placement_requirements_and_sync_convergence() {
    for mode in ["local", "url"] {
        let (outcome, governor) = prepare(mode).await;
        let OperationOutcome::Completed(Ok(addition)) = &*outcome else {
            panic!("normalization failed")
        };
        let key = DependencyKey::parse("assets-alias").unwrap();
        let root_intent = &addition.project().intent().roots[&key];
        let dependency = &addition.project().lock().dependencies[&key];
        assert_eq!(dependency.kind, ContentKind::ResourcePack);
        assert_eq!(dependency.selected, None);
        assert_eq!(addition.project().lock().coverage[&key], Coverage::Unknown);
        assert_eq!(dependency.files.as_slice()[0].expected, declared());
        assert!(
            matches!(&root_intent.version, VersionIntent::ContentPinned(value) if Some(value) == declared().digests.as_ref())
        );
        let target = "overrides/client/resourcepacks/chosen-name.zip";
        if mode == "local" {
            assert!(
                matches!(&root_intent.source, SourceIntent::Local(path) if path.as_str() == target)
            );
        } else {
            assert!(
                matches!(&root_intent.source, SourceIntent::Url(urls) if urls.as_slice()[0] == "https://example.com/download?file=42")
            );
        }
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("empack.yml"),
            DocumentCodec.encode_intent(current().intent()).unwrap(),
        )
        .unwrap();
        fs::write(
            root.path().join("empack.lock"),
            DocumentCodec.encode_lock(&current()).unwrap(),
        )
        .unwrap();
        let reader = ProjectReader::new(RecoveryReader::new(host.path().join("state")));
        let cancel = Cancellation::default();
        let snapshot = reader
            .capture_addition(
                root.path(),
                addition.group(),
                SnapshotLimits::default(),
                &cancel,
            )
            .unwrap();
        crate::engine::addition::plan_addition(
            snapshot,
            addition.group(),
            addition.content().clone(),
            &cancel,
        )
        .unwrap()
        .stage(&cancel)
        .unwrap()
        .publish(
            &Publisher::open(&host.path().join("state")).unwrap(),
            &cancel,
        )
        .unwrap();
        assert_eq!(fs::read(root.path().join(target)).unwrap(), b"payload");
        assert!(
            fs::metadata(root.path().join(target))
                .unwrap()
                .permissions()
                .readonly()
        );
        for _ in 0..2 {
            let snapshot = reader
                .capture_synchronization(root.path(), SnapshotLimits::default(), &cancel)
                .unwrap();
            let sync = crate::engine::synchronization::plan_synchronization_with_resolution(
                snapshot,
                addition.content().clone(),
                None,
                &cancel,
            )
            .unwrap()
            .stage(&cancel)
            .unwrap();
            assert!(sync.files().changes().is_empty());
        }
        let decoded = DocumentCodec
            .decode_intent(
                &fs::read(root.path().join("empack.yml")).unwrap(),
                "published",
            )
            .unwrap();
        assert_eq!(
            decoded.intent().roots[&key].requirements,
            root_intent.requirements
        );
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
#[tokio::test]
async fn accepted_observation_does_not_become_declared_authenticity() {
    let (outcome, governor) = prepare("observed").await;
    let OperationOutcome::Completed(Ok(addition)) = &*outcome else {
        panic!("explicit observation rejected")
    };
    let dependency = addition
        .project()
        .lock()
        .dependencies
        .values()
        .next()
        .unwrap();
    let file = &dependency.files.as_slice()[0];
    assert!(file.expected.digests.is_none());
    assert!(file.provenance.declared_digests.is_none());
    assert_eq!(
        file.expected.accepted_observation.as_ref(),
        Some(
            &addition
                .content()
                .values()
                .next()
                .unwrap()
                .materialized()
                .unwrap()
                .content
                .lease()
                .id()
        )
    );
    assert_eq!(
        addition
            .project()
            .intent()
            .roots
            .values()
            .next()
            .unwrap()
            .version,
        VersionIntent::FollowCompatible
    );
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn invalid_direct_file_intent_never_produces_an_addition_group() {
    for mode in ["strong", "digest", "size", "missing", "secret", "duplicate"] {
        let (outcome, governor) = prepare(mode).await;
        assert!(
            matches!(&*outcome, OperationOutcome::Completed(Err(_))),
            "{mode}"
        );
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
