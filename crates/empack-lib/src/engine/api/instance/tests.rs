use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{SourceEvidencePolicy, verify_stream},
        release::{self, *},
    },
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn request(files: &[(&str, &str, &[u8], FilePolicy)]) -> InstallInstanceRequest {
    let mut supplied = BTreeMap::new();
    let files = files
        .iter()
        .map(|(key, path, bytes, policy)| {
            let file = ReleaseFile {
                key: (*key).into(),
                destination: (*path).into(),
                policy: *policy,
                client: Participation::Required,
                server: Participation::Required,
                sha256: hash(bytes),
                bytes: bytes.len() as u64,
                readonly: false,
                executable: false,
                assertions: vec![],
                source: ReleaseSource::Asset {
                    path: format!("assets/{key}"),
                },
            };
            supplied.insert(
                (*key).into(),
                verify_stream(
                    &mut std::io::Cursor::new(*bytes),
                    &file.expected().unwrap(),
                    bytes.len() as u64,
                    SourceEvidencePolicy::Compatibility,
                    crate::engine::content::InitialObservation::RequireEvidence,
                    &Cancellation::default(),
                )
                .unwrap(),
            );
            file
        })
        .collect();
    let release = DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        pack: "fixture".into(),
        version: "1".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files,
    })
    .unwrap();
    let selected = release::trust::SelectedSnapshot::select(
        release.bytes(),
        release.id(),
        &semver::Version::parse("0.6.0-beta").unwrap(),
    )
    .unwrap();
    InstallInstanceRequest {
        release: SelectedRelease::Snapshot(selected),
        side: InstanceSide::Client,
        choices: vec![],
        supplied,
        local_files: BTreeMap::new(),
        assets: None,
    }
}
async fn apply(engine: &Engine, root: &Path, request: InstallInstanceRequest) -> InstanceRecord {
    let Preparation::Ready(prepared) = engine.prepare(root.to_owned(), request).await.unwrap()
    else {
        panic!("input required")
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Instance(
            receipt,
        ))) => receipt.record.clone(),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(e)) => {
            panic!("failed: {e:#}")
        }
        OperationOutcome::Completed(ExecutionOutcome::RecoveryRequired { cause, .. }) => {
            panic!("recovery: {cause:#}")
        }
        _ => panic!("unexpected instance outcome"),
    }
}
#[tokio::test]
async fn install_update_repair_and_rollback_preserve_seeds_and_unowned_data() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || {
        request(&[
            ("mod", "mods/a.jar", b"A", FilePolicy::Managed),
            ("config", "config/a", b"initial", FilePolicy::Seed),
        ])
    };
    let first = apply(&engine, root.path(), a()).await;
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
    fs::write(root.path().join("game/config/a"), b"my settings").unwrap();
    fs::create_dir_all(root.path().join("game/world")).unwrap();
    fs::write(root.path().join("game/world/level.dat"), b"played world").unwrap();
    let second = apply(
        &engine,
        root.path(),
        request(&[
            ("new", "mods/b.jar", b"B", FilePolicy::Managed),
            ("config", "config/a", b"new default", FilePolicy::Seed),
        ]),
    )
    .await;
    assert!(!root.path().join("game/mods/a.jar").exists());
    assert_eq!(fs::read(root.path().join("game/mods/b.jar")).unwrap(), b"B");
    assert!(second.history.contains(&first.release));
    let rollback = apply(&engine, root.path(), a()).await;
    assert_eq!(rollback.release, first.release);
    assert!(!root.path().join("game/mods/b.jar").exists());
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    apply(&engine, root.path(), a()).await;
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"my settings"
    );
    assert_eq!(
        fs::read(root.path().join("game/world/level.dat")).unwrap(),
        b"played world"
    );
    assert!(
        root.path()
            .join(format!(".empack/releases/{}.json", second.release))
            .exists()
    );
}
#[tokio::test]
async fn preview_conflicts_and_late_edits_never_overwrite_current_state() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    let Preparation::Ready(preview) = engine.prepare(root.path().to_owned(), a()).await.unwrap()
    else {
        panic!()
    };
    drop(preview);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    apply(&engine, root.path(), a()).await;
    let b = || request(&[("mod", "mods/a.jar", b"B", FilePolicy::Managed)]);
    let Preparation::Ready(prepared) = engine.prepare(root.path().to_owned(), b()).await.unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    };
    fs::write(root.path().join("game/mods/a.jar"), b"user edit").unwrap();
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    let error = match engine.prepare(root.path().to_owned(), b()).await {
        Err(e) => e,
        _ => panic!("expected conflict"),
    };
    assert!(
        error.is::<crate::engine::instance::InstanceConflicts>(),
        "{error:#}"
    );
    assert_eq!(
        fs::read(root.path().join("game/mods/a.jar")).unwrap(),
        b"user edit"
    );
}
#[tokio::test]
async fn unowned_collisions_directory_targets_and_wrong_bytes_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    fs::create_dir_all(root.path().join("game/mods")).unwrap();
    fs::write(root.path().join("game/mods/a.jar"), b"A").unwrap();
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    assert!(engine.prepare(root.path().to_owned(), a()).await.is_err());
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    fs::create_dir(root.path().join("game/mods/a.jar")).unwrap();
    assert!(engine.prepare(root.path().to_owned(), a()).await.is_err());
    fs::remove_dir(root.path().join("game/mods/a.jar")).unwrap();
    let mut wrong = a();
    wrong.supplied = request(&[("mod", "mods/a.jar", b"WRONG", FilePolicy::Managed)]).supplied;
    assert!(engine.prepare(root.path().to_owned(), wrong).await.is_err());
    assert!(!root.path().join(".empack/instance.json").exists());
}
#[cfg(unix)]
#[tokio::test]
async fn selected_symlink_is_rejected_but_unrelated_symlink_is_not_observed() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    fs::create_dir(root.path().join("game")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("game/mods")).unwrap();
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    assert!(engine.prepare(root.path().to_owned(), a()).await.is_err());
    fs::remove_file(root.path().join("game/mods")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("game/unrelated")).unwrap();
    apply(&engine, root.path(), a()).await;
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

fn replace_document(
    mut input: InstallInstanceRequest,
    document: ReleaseDocument,
) -> InstallInstanceRequest {
    let release = DecodedRelease::encode(document).unwrap();
    input.release = SelectedRelease::Snapshot(
        release::trust::SelectedSnapshot::select(
            release.bytes(),
            release.id(),
            &semver::Version::parse("0.6.0-beta").unwrap(),
        )
        .unwrap(),
    );
    input
}
#[tokio::test]
async fn retained_bytes_must_satisfy_incoming_original_assertions() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    apply(&engine, root.path(), a()).await;
    let before = fs::read(root.path().join(".empack/instance.json")).unwrap();
    let mut document = a().release.release().document().clone();
    document.files[0].assertions.push(SourceDigest {
        algorithm: "md5".into(),
        value: "00".repeat(16),
    });
    assert!(
        engine
            .prepare(root.path().to_owned(), replace_document(a(), document))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join(".empack/instance.json")).unwrap(),
        before
    );
}
#[tokio::test]
async fn choice_defaults_do_not_reset_saved_selections_and_new_choices_require_input() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    let mut document = a().release.release().document().clone();
    document.choices = vec![ReleaseChoice {
        key: "extra".into(),
        alternatives: vec!["yes".into(), "no".into()],
        default: "yes".into(),
    }];
    document.files[0].client = Participation::Choice {
        key: "extra".into(),
        value: "yes".into(),
    };
    apply(
        &engine,
        root.path(),
        replace_document(a(), document.clone()),
    )
    .await;
    document.choices[0].default = "no".into();
    let after = apply(
        &engine,
        root.path(),
        replace_document(a(), document.clone()),
    )
    .await;
    assert_eq!(after.choices[0].value, "yes");
    assert!(root.path().join("game/mods/a.jar").exists());
    document.choices.push(ReleaseChoice {
        key: "new".into(),
        alternatives: vec!["yes".into(), "no".into()],
        default: "no".into(),
    });
    assert!(
        engine
            .prepare(
                root.path().to_owned(),
                replace_document(a(), document.clone())
            )
            .await
            .is_err()
    );
    let mut input = replace_document(a(), document);
    input.choices = vec![
        ChoiceSelection {
            key: "new".into(),
            value: "no".into(),
        },
        ChoiceSelection {
            key: "extra".into(),
            value: "no".into(),
        },
    ];
    apply(&engine, root.path(), input).await;
    assert!(!root.path().join("game/mods/a.jar").exists());
}
