use super::*;
use crate::engine::publication::{PublicationDisposition, tests::interrupted_fixture};
use std::fs;

async fn ready(
    engine: &Engine,
    path: &std::path::Path,
    request: RecoverRequest,
) -> PreparedOperation {
    match engine.prepare(path.to_path_buf(), request).await.unwrap() {
        Preparation::Ready(prepared) => prepared,
        _ => panic!("recovery unexpectedly needs acquisition"),
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    }
}
#[tokio::test]
async fn engine_recovery_is_read_only_until_exact_approval_and_does_not_require_valid_intent() {
    for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
        let project = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let host = state.path().join("state");
        let (engine, governor) = tests::engine(host.clone());
        assert!(
            engine
                .inspect_recovery(project.path().to_path_buf())
                .await
                .unwrap()
                .is_none()
        );
        assert!(!host.exists());
        let status = interrupted_fixture(project.path(), &host);
        assert_eq!(
            engine
                .inspect_recovery(project.path().to_path_buf())
                .await
                .unwrap(),
            Some(status.clone())
        );
        let before = fs::read(project.path().join("empack.yml")).unwrap();
        let request = || RecoverRequest {
            operation: status.operation.clone(),
            action,
        };
        let preview = engine
            .preview(project.path().to_path_buf(), request())
            .await
            .unwrap();
        assert_eq!(preview.recovery().unwrap().status, status);
        assert!(!preview.needs_network());
        assert!(!preview.runs_installer());
        assert_eq!(fs::read(project.path().join("empack.yml")).unwrap(), before);
        let prepared = ready(&engine, project.path(), request()).await;
        let mut wrong = grant(&prepared);
        wrong.replacement = None;
        assert!(prepared.authorize(wrong).is_err());
        let prepared = ready(&engine, project.path(), request()).await;
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Recovery(
            receipt,
        ))) = &*outcome
        else {
            panic!("recovery did not complete");
        };
        assert_eq!(
            receipt.publication.disposition,
            if action == RecoveryAction::Finish {
                PublicationDisposition::Published
            } else {
                PublicationDisposition::Restored
            }
        );
        engine.release_completed(handle.id());
        drop((outcome, handle));
        assert!(
            engine
                .inspect_recovery(project.path().to_path_buf())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            if action == RecoveryAction::Finish {
                b"new intent"
            } else {
                b"old intent"
            }
        );
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn recovery_rejects_wrong_operation_cross_engine_and_late_user_edits() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let host = state.path().join("state");
    let status = interrupted_fixture(project.path(), &host);
    let (engine, governor) = tests::engine(host.clone());
    assert!(
        engine
            .prepare(
                project.path().to_path_buf(),
                RecoverRequest {
                    operation: "different".into(),
                    action: RecoveryAction::Finish
                }
            )
            .await
            .is_err()
    );
    let request = || RecoverRequest {
        operation: status.operation.clone(),
        action: RecoveryAction::Finish,
    };
    let prepared = ready(&engine, project.path(), request()).await;
    let permission = grant(&prepared);
    let (other, _) = tests::engine(host);
    assert!(
        other
            .start(prepared.authorize(permission).unwrap())
            .is_err()
    );
    other.shutdown().await;
    let prepared = ready(&engine, project.path(), request()).await;
    let permission = grant(&prepared);
    fs::write(project.path().join("empack.yml"), b"user edit!").unwrap();
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    engine.release_completed(handle.id());
    drop((outcome, handle));
    assert_eq!(
        fs::read(project.path().join("empack.yml")).unwrap(),
        b"user edit!"
    );
    assert!(
        engine
            .inspect_recovery(project.path().to_path_buf())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn new_project_recovery_finishes_absent_visible_and_moved_roots_through_engine() {
    use crate::engine::publication::{PublicationPoint, interrupted_creation_fixture};
    for mode in ["absent", "visible", "moved"] {
        let parent = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut selected = parent.path().join("pack");
        let host = state.path().join("state");
        let (engine, governor) = tests::engine(host.clone());
        assert!(
            engine
                .inspect_recovery(ProjectTarget::New(selected.clone()))
                .await
                .unwrap()
                .is_none()
        );
        assert!(!host.exists());
        assert!(!selected.exists());
        let status = interrupted_creation_fixture(
            &selected,
            &host,
            if mode == "absent" {
                PublicationPoint::IntentDurable
            } else {
                PublicationPoint::TargetChanged
            },
        );
        if mode == "moved" {
            let moved = parent.path().join("renamed");
            fs::rename(&selected, &moved).unwrap();
            selected = moved;
        }
        let target = || {
            if mode == "absent" {
                ProjectTarget::New(selected.clone())
            } else {
                ProjectTarget::Existing(selected.clone())
            }
        };
        assert_eq!(
            engine.inspect_recovery(target()).await.unwrap(),
            Some(status.clone())
        );
        assert_eq!(status.kind, RecoveryKind::Creation);
        assert!(
            engine
                .prepare(
                    target(),
                    RecoverRequest {
                        operation: status.operation.clone(),
                        action: RecoveryAction::Restore
                    }
                )
                .await
                .is_err()
        );
        let request = || RecoverRequest {
            operation: status.operation.clone(),
            action: RecoveryAction::Finish,
        };
        let preview = engine.preview(target(), request()).await.unwrap();
        assert_eq!(
            preview.recovery().unwrap().files.changes().is_empty(),
            mode != "absent"
        );
        assert_eq!(selected.exists(), mode != "absent");
        let prepared = match engine.prepare(target(), request()).await.unwrap() {
            Preparation::Ready(value) => value,
            _ => panic!("creation recovery needs input"),
        };
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(
                ExecutionReceipt::Recovery(receipt),
            )) => assert_eq!(
                receipt.publication.disposition,
                PublicationDisposition::Published
            ),
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{mode}: {error:#}")
            }
            _ => panic!("{mode}: creation recovery failed"),
        }
        engine.release_completed(handle.id());
        drop((outcome, handle));
        assert_eq!(
            fs::read(selected.join("pack/config/value")).unwrap(),
            b"payload"
        );
        assert!(engine.inspect_recovery(target()).await.unwrap().is_none());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn creation_recovery_approval_cannot_overwrite_a_new_occupant_or_replay_progress() {
    use crate::engine::publication::{PublicationPoint, Publisher, interrupted_creation_fixture};
    for progressed in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let selected = parent.path().join("pack");
        let host = state.path().join("state");
        let status =
            interrupted_creation_fixture(&selected, &host, PublicationPoint::IntentDurable);
        let (engine, governor) = tests::engine(host.clone());
        let prepared = match engine
            .prepare(
                ProjectTarget::New(selected.clone()),
                RecoverRequest {
                    operation: status.operation,
                    action: RecoveryAction::Finish,
                },
            )
            .await
            .unwrap()
        {
            Preparation::Ready(value) => value,
            _ => panic!("creation recovery needs input"),
        };
        let permission = grant(&prepared);
        if progressed {
            Publisher::open_existing(&host)
                .unwrap()
                .unwrap()
                .recover_new(&selected)
                .unwrap();
        } else {
            fs::create_dir(&selected).unwrap();
            fs::write(selected.join("sentinel"), b"keep").unwrap();
        }
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        assert!(matches!(
            &*outcome,
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
        ));
        engine.release_completed(handle.id());
        drop((outcome, handle));
        if progressed {
            assert_eq!(
                fs::read(selected.join("pack/config/value")).unwrap(),
                b"payload"
            );
        } else {
            assert_eq!(fs::read(selected.join("sentinel")).unwrap(), b"keep");
            assert!(!selected.join("empack.yml").exists());
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
