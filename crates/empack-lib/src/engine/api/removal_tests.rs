use super::tests::{engine, fixture, path, put};
use super::*;
use empack_core::{
    model::DependencyKey,
    removal::{RemovalEvidencePolicy, RemovalMode},
};
use std::{fs, path::Path};
fn request(mode: RemovalMode) -> RemoveRequest {
    RemoveRequest {
        selections: NonEmpty::new(vec![RemovalSelector::Key(
            DependencyKey::parse("assets").unwrap(),
        )])
        .unwrap(),
        mode,
        evidence: RemovalEvidencePolicy::RequireComplete,
    }
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    }
}
async fn ready(engine: &Engine, root: &Path, mode: RemovalMode) -> PreparedOperation {
    match engine
        .prepare(root.to_path_buf(), request(mode))
        .await
        .unwrap()
    {
        Preparation::Ready(value) => value,
        _ => panic!("removal cannot require a download"),
    }
}
#[tokio::test]
async fn removal_preview_is_read_only_and_requires_exact_grant_and_engine() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let (engine, governor) = engine(state.path().join("state"));
    let preview = engine
        .preview(
            root.path().to_path_buf(),
            request(RemovalMode::RemoveContent),
        )
        .await
        .unwrap();
    assert_eq!(preview.remove().unwrap().selected[0].title, "Assets");
    assert!(!preview.needs_network());
    assert!(!preview.runs_installer());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let prepared = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    assert!(governor.status().reserved.scratch_bytes > 0);
    let mut permission = grant(&prepared);
    permission.replacement = None;
    assert!(prepared.authorize(permission).is_err());
    let first = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    let second = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    assert!(first.authorize(grant(&second)).is_err());
    drop(second);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let prepared = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    let permission = grant(&prepared);
    let other = Engine::new(engine.config.clone(), governor.clone()).unwrap();
    assert!(
        other
            .start(prepared.authorize(permission).unwrap())
            .is_err()
    );
    assert!(
        engine
            .prepare(
                ProjectTarget::New(root.path().join("new")),
                request(RemovalMode::RemoveContent)
            )
            .await
            .is_err()
    );
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert!(!state.path().join("state").exists());
    other.shutdown().await;
    engine.shutdown().await;
}
#[tokio::test]
async fn remove_and_demote_have_distinct_receipts_and_subsequent_build_content() {
    for mode in [RemovalMode::RemoveContent, RemovalMode::ForgetRoots] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let (engine, governor) = engine(state.path().join("state"));
        let prepared = ready(&engine, root.path(), mode).await;
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Remove(
                receipt,
            ))) => {
                assert_eq!(receipt.mode, mode);
                assert!(receipt.project.intent().roots.is_empty());
                assert_eq!(
                    receipt.project.lock().dependencies.len(),
                    usize::from(mode == RemovalMode::ForgetRoots)
                );
                assert_eq!(receipt.selected.len(), 1);
            }
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            _ => panic!("removal failed"),
        }
        assert_eq!(governor.status().reserved, engine.config.resources.receipt);
        assert!(engine.release_completed(handle.id()));
        drop(outcome);
        drop(handle);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        assert_eq!(
            fs::read(root.path().join("pack/config/value")).unwrap(),
            b"current config"
        );
        assert_eq!(
            fs::read(root.path().join("dist/client.zip")).unwrap(),
            b"previous client"
        );
        let mut build = super::tests::request();
        build.outputs = NonEmpty::new(vec![BuildOutput {
            target: Recipe::MODRINTH,
            artifact: path("after.mrpack"),
        }])
        .unwrap();
        let prepared = match engine
            .prepare(root.path().to_path_buf(), build)
            .await
            .unwrap()
        {
            Preparation::Ready(value) => value,
            _ => panic!("retained inputs require no acquisition"),
        };
        let permission = grant(&prepared);
        let mut handle = engine
            .start(prepared.authorize(permission).unwrap())
            .unwrap();
        assert!(matches!(
            &*handle.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
        ));
        let mut zip =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist/after.mrpack")).unwrap())
                .unwrap();
        let index: serde_json::Value =
            serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap()).unwrap();
        assert_eq!(
            index["files"].as_array().unwrap().len(),
            if mode == RemovalMode::ForgetRoots {
                3
            } else {
                0
            }
        );
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut zip.by_name("overrides/config/value").unwrap(),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(bytes, b"current config");
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn removal_conflicts_fail_before_any_live_file_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (engine, _) = engine(state.path().join("state"));
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let prepared = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    put(
        root.path(),
        "pack/resourcepacks/a.zip",
        b"user changed this",
    );
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
        b"payload"
    );
    engine.shutdown().await;
}
#[tokio::test]
async fn removal_admission_failure_preserves_all_files_and_releases_reservations() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (original, _) = engine(state.path().join("state"));
    let mut config = original.config.clone();
    original.shutdown().await;
    config.resources.capture.scratch_bytes = 0;
    config.resources.assembly.scratch_bytes = 0;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 128 << 20,
        scratch_bytes: 1,
        open_files: 128,
    });
    let engine = Engine::new(config, governor.clone()).unwrap();
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                request(RemovalMode::RemoveContent)
            )
            .await
            .is_err()
    );
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
    assert!(!state.path().join("state").exists());
    engine.shutdown().await;
}

#[tokio::test]
async fn title_selection_previews_canonical_identity_and_unknown_batch_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let (engine, _) = engine(state.path().join("state"));
    let mut request = request(RemovalMode::RemoveContent);
    request.selections = NonEmpty::new(vec![RemovalSelector::Query("ASSETS".into())]).unwrap();
    let preview = engine
        .preview(root.path().to_path_buf(), request.clone())
        .await
        .unwrap();
    let selected = &preview.remove().unwrap().selected;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].key.as_str(), "assets");
    request.selections = NonEmpty::new(vec![
        RemovalSelector::Query("ASSETS".into()),
        RemovalSelector::Query("missing".into()),
    ])
    .unwrap();
    assert!(
        engine
            .prepare(root.path().to_path_buf(), request)
            .await
            .is_err()
    );
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert!(!state.path().join("state").exists());
    engine.shutdown().await;
}

#[tokio::test]
async fn unrelated_content_does_not_block_removal_or_become_a_read_precondition() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    for name in ["pack/unrelated.bin", "overrides/client/unrelated.bin"] {
        put(root.path(), name, &vec![0; 3 << 20]);
    }
    #[cfg(unix)]
    {
        put(root.path(), "pack/bad:name", b"unrelated nonportable file");
        std::os::unix::fs::symlink("/nonexistent", root.path().join("pack/unrelated-link"))
            .unwrap();
    }
    let (engine, _) = engine(state.path().join("state"));
    let prepared = ready(&engine, root.path(), RemovalMode::RemoveContent).await;
    put(root.path(), "pack/unrelated.bin", b"changed unrelated file");
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Remove(_)))
    ));
    assert_eq!(
        fs::read(root.path().join("pack/unrelated.bin")).unwrap(),
        b"changed unrelated file"
    );
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    engine.shutdown().await;
}

#[tokio::test]
async fn unselected_locked_bytes_are_not_removal_inputs() {
    use crate::engine::{documents::DocumentCodec, mrpack::tests::project};
    use empack_core::{model::*, path::InstallDestination};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let original = project(false, false);
    let mut lock = original.lock().clone();
    let other = DependencyKey::parse("retained").unwrap();
    let mut dependency = lock.dependencies.values().next().unwrap().clone();
    dependency.identity = ResolvedIdentity::Url(other.clone());
    let mut file = dependency.files.as_slice()[0].clone();
    let mut placement = file.placements.as_slice()[0].clone();
    placement.destination = InstallDestination::parse("retained/oversized.zip").unwrap();
    file.placements = NonEmpty::new(vec![placement]).unwrap();
    dependency.files = NonEmpty::new(vec![file]).unwrap();
    lock.dependencies.insert(other.clone(), dependency);
    lock.coverage.insert(other.clone(), Coverage::Unknown);
    let resolved = ResolvedProject::validate(
        original.intent().clone(),
        lock.clone(),
        lock.intent_revision,
    )
    .unwrap();
    put(
        root.path(),
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    put(
        root.path(),
        "pack/retained/oversized.zip",
        &vec![0; 3 << 20],
    );
    let (engine, _) = engine(state.path().join("state"));
    let mut removal = request(RemovalMode::RemoveContent);
    assert!(
        engine
            .prepare(root.path().to_path_buf(), removal.clone())
            .await
            .is_err()
    );
    removal.evidence = RemovalEvidencePolicy::AcknowledgeUnknown;
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_path_buf(), removal)
        .await
        .unwrap()
    else {
        panic!("unexpected input request")
    };
    assert_eq!(
        prepared.view().remove().unwrap().incomplete_evidence,
        vec![other.clone()]
    );
    assert_eq!(prepared.view().remove().unwrap().selected.len(), 1);
    let permission = grant(&prepared);
    let mut handle = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Remove(receipt))) =
        &*outcome
    else {
        panic!("removal failed")
    };
    assert_eq!(receipt.incomplete_evidence, vec![other.clone()]);
    assert_eq!(receipt.project.lock().coverage[&other], Coverage::Unknown);
    assert_eq!(
        fs::metadata(root.path().join("pack/retained/oversized.zip"))
            .unwrap()
            .len(),
        3 << 20
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn selected_portable_alias_is_not_mistaken_for_absent_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::rename(
        root.path().join("pack/resourcepacks/a.zip"),
        root.path().join("pack/resourcepacks/A.zip"),
    )
    .unwrap();
    let (engine, _) = engine(state.path().join("state"));
    let result = engine
        .prepare(
            root.path().to_path_buf(),
            request(RemovalMode::RemoveContent),
        )
        .await;
    assert!(
        result.is_err(),
        "a portable alias cannot be proof that selected content is absent"
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/A.zip")).unwrap(),
        b"payload"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn foreign_metadata_cannot_nominate_removal_even_with_uncertainty_acknowledged() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(
        root.path(),
        "pack/resourcepacks/untracked.pw.toml",
        b"filename='separate.zip'",
    );
    put(
        root.path(),
        "pack/resourcepacks/separate.zip",
        b"user bytes",
    );
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let (engine, _) = engine(state.path().join("state"));
    for mode in [RemovalMode::RemoveContent, RemovalMode::ForgetRoots] {
        for evidence in [
            RemovalEvidencePolicy::RequireComplete,
            RemovalEvidencePolicy::AcknowledgeUnknown,
        ] {
            let request = RemoveRequest {
                selections: NonEmpty::new(vec![
                    RemovalSelector::Query("assets".into()),
                    RemovalSelector::Query("untracked".into()),
                ])
                .unwrap(),
                mode,
                evidence,
            };
            assert!(
                engine
                    .prepare(root.path().to_owned(), request)
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/separate.zip")).unwrap(),
        b"user bytes"
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
    assert!(!state.path().join("state").exists());
    engine.shutdown().await;
}
