use super::*;
use crate::engine::{
    api::tests,
    publication::{PublicationDisposition, PublicationPoint},
};
use std::{fs, path::Path};

fn fixture(root: &Path) {
    tests::put(
        root,
        "empack.yml",
        b"broken authoring is unrelated to artifact cleanup",
    );
    tests::put(root, "pack/data", b"keep pack");
    tests::put(root, "templates/start.sh", b"keep template");
    tests::put(root, "notes", b"keep notes");
    tests::put(root, "dist/archive.zip", b"binary\x00\xffartifact");
    tests::put(root, "dist/old/nested.mrpack", b"old artifact");
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", root.join("pack/unrelated-link")).unwrap();
}
async fn ready(engine: &Engine, root: &Path) -> PreparedOperation {
    match engine
        .prepare(root.to_path_buf(), CleanRequest::Artifacts)
        .await
        .unwrap()
    {
        Preparation::Ready(value) => value,
        _ => panic!("cleanup cannot require acquisition"),
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
async fn artifact_cleanup_previews_exact_files_and_preserves_every_other_namespace() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, governor) = tests::engine(host.path().join("state"));
    let expected = fs::read(root.path().join("dist/archive.zip"))
        .unwrap()
        .len() as u64
        + 12;
    let view = engine
        .preview(root.path().to_path_buf(), CleanRequest::Artifacts)
        .await
        .unwrap();
    assert_eq!(view.clean().unwrap().files.changes().len(), 2);
    assert_eq!(view.clean().unwrap().removed_bytes, expected);
    assert!(!view.needs_network());
    assert!(!view.runs_installer());
    assert!(root.path().join("dist/archive.zip").exists());
    assert!(!host.path().join("state").exists());
    let prepared = ready(&engine, root.path()).await;
    let mut wrong = grant(&prepared);
    wrong.replacement = None;
    assert!(prepared.authorize(wrong).is_err());
    let prepared = ready(&engine, root.path()).await;
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Clean(
            receipt,
        ))) => {
            assert_eq!(receipt.removed_bytes, expected);
            assert_eq!(receipt.publication.changed_files, 2);
        }
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("artifact cleanup failed"),
    }
    assert!(!root.path().join("dist/archive.zip").exists());
    assert!(!root.path().join("dist/old/nested.mrpack").exists());
    assert_eq!(
        fs::read(root.path().join("pack/data")).unwrap(),
        b"keep pack"
    );
    assert_eq!(
        fs::read(root.path().join("templates/start.sh")).unwrap(),
        b"keep template"
    );
    assert_eq!(fs::read(root.path().join("notes")).unwrap(), b"keep notes");
    assert_eq!(
        fs::read(root.path().join("empack.yml")).unwrap(),
        b"broken authoring is unrelated to artifact cleanup"
    );
    engine.release_completed(handle.id());
    drop((outcome, handle));
    let view = engine
        .preview(root.path().to_path_buf(), CleanRequest::Artifacts)
        .await
        .unwrap();
    assert!(view.clean().unwrap().files.changes().is_empty());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn artifact_cleanup_rejects_late_edits_additions_and_wrong_engine() {
    for mode in ["edit", "addition", "other"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (engine, governor) = tests::engine(host.path().join("state"));
        let prepared = ready(&engine, root.path()).await;
        let permission = grant(&prepared);
        if mode == "other" {
            let (other, _) = tests::engine(host.path().join("other"));
            assert!(
                other
                    .start(prepared.authorize(permission).unwrap())
                    .is_err()
            );
            other.shutdown().await;
        } else {
            tests::put(
                root.path(),
                if mode == "edit" {
                    "dist/archive.zip"
                } else {
                    "dist/new.zip"
                },
                b"user bytes",
            );
            let mut handle = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let result = handle.wait().await;
            assert!(matches!(
                &*result,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            engine.release_completed(handle.id());
            drop((result, handle));
        }
        assert!(root.path().join("dist/archive.zip").exists());
        assert_eq!(
            fs::read(root.path().join("dist/old/nested.mrpack")).unwrap(),
            b"old artifact"
        );
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
#[cfg(unix)]
#[tokio::test]
async fn artifact_cleanup_refuses_linked_roots_and_members_before_any_deletion() {
    for nested in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        tests::put(outside.path(), "sentinel", b"outside");
        if nested {
            fixture(root.path());
            std::os::unix::fs::symlink(outside.path(), root.path().join("dist/linked")).unwrap();
        } else {
            std::os::unix::fs::symlink(outside.path(), root.path().join("dist")).unwrap();
        }
        let (engine, governor) = tests::engine(host.path().join("state"));
        assert!(
            engine
                .prepare(root.path().to_path_buf(), CleanRequest::Artifacts)
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(outside.path().join("sentinel")).unwrap(),
            b"outside"
        );
        if nested {
            assert!(root.path().join("dist/archive.zip").exists());
        }
        assert!(!host.path().join("state").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn interrupted_artifact_cleanup_uses_shared_finish_and_restore_recovery() {
    for action in [RecoveryAction::Finish, RecoveryAction::Restore] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (engine, _) = tests::engine(host.path().join("state"));
        let prepared = ready(&engine, root.path()).await;
        let permission = grant(&prepared);
        let approved = prepared.authorize(permission).unwrap();
        let (kind, _permit) = approved.prepared.data.into_parts();
        let PreparedKind::Clean(prepared) = kind else {
            panic!("not cleanup")
        };
        let publisher = Publisher::open(&host.path().join("state")).unwrap();
        assert!(
            crate::engine::publication::tests::interrupt_publication(
                &publisher,
                &prepared.root,
                prepared.verified,
                PublicationPoint::TargetChanged,
            )
            .is_err()
        );
        assert!(
            engine
                .prepare(root.path().to_path_buf(), CleanRequest::Artifacts)
                .await
                .is_err()
        );
        let status = engine
            .inspect_recovery(root.path().to_path_buf())
            .await
            .unwrap()
            .unwrap();
        let recovery = match engine
            .prepare(
                root.path().to_path_buf(),
                RecoverRequest {
                    operation: status.operation,
                    action,
                },
            )
            .await
            .unwrap()
        {
            Preparation::Ready(value) => value,
            _ => panic!("recovery needs input"),
        };
        let permission = grant(&recovery);
        let mut handle = engine
            .start(recovery.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Recovery(
            receipt,
        ))) = &*outcome
        else {
            panic!("cleanup recovery failed")
        };
        assert_eq!(
            receipt.publication.disposition,
            if action == RecoveryAction::Finish {
                PublicationDisposition::Published
            } else {
                PublicationDisposition::Restored
            }
        );
        for path in ["dist/archive.zip", "dist/old/nested.mrpack"] {
            assert_eq!(
                root.path().join(path).exists(),
                action == RecoveryAction::Restore
            );
        }
        assert_eq!(
            fs::read(root.path().join("pack/data")).unwrap(),
            b"keep pack"
        );
        engine.release_completed(handle.id());
        drop((outcome, handle));
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn cleanup_accepts_artifacts_larger_than_source_file_limit() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = tests::engine(host.path().join("state"));
    // Scale the production 8 GiB source / 64 GiB archive mismatch down to fixture limits.
    let size = engine.config.snapshot.file_bytes + 1;
    assert!(size <= engine.config.archive.compressed_bytes);
    tests::put(root.path(), "dist/large.zip", &vec![0; size as usize]);
    let prepared = ready(&engine, root.path()).await;
    assert_eq!(prepared.view().clean().unwrap().removed_bytes, size);
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    assert!(!root.path().join("dist/large.zip").exists());
    engine.shutdown().await;
}
