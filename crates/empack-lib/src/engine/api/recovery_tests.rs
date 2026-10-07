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
