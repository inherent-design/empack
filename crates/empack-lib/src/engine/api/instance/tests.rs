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
                layer: ReleaseLayer::Common,
                policy: *policy,
                client: Participation::Required,
                server: Participation::Required,
                sha256: hash(bytes),
                bytes: bytes.len() as u64,
                readonly: false,
                executable: false,
                assertions: vec![],
                asset: None,
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
        require_subscription: false,
        conflicts: Vec::new(),
        action: crate::engine::instance::InstanceAction::Apply,
        release: SelectedRelease::Snapshot(selected),
        side: InstanceSide::Client,
        layout: None,
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
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    let record = match &*outcome {
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
    };
    drop(outcome);
    engine.release_completed(operation.id());
    drop(operation);
    record
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
    let mut rollback_request = a();
    rollback_request.action = InstanceAction::Rollback;
    let rollback = apply(&engine, root.path(), rollback_request).await;
    assert_eq!(rollback.release, first.release);
    assert!(!root.path().join("game/mods/b.jar").exists());
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    let mut repair = a();
    repair.action = InstanceAction::Repair;
    apply(&engine, root.path(), repair).await;
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
        run_runtime: false,
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
        description: None,
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
        description: None,
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

#[tokio::test]
async fn native_layers_preserve_optional_fallback_and_side_specific_bytes() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let input = || {
        request(&[
            ("common", "config/a", b"common", FilePolicy::Managed),
            ("client", "config/a", b"client", FilePolicy::Managed),
            ("server", "config/a", b"server", FilePolicy::Managed),
        ])
    };
    let mut doc = input().release.release().document().clone();
    doc.choices = vec![ReleaseChoice {
        key: "variant".into(),
        alternatives: vec!["yes".into(), "no".into()],
        default: "no".into(),
        description: None,
    }];
    for file in &mut doc.files {
        match file.key.as_str() {
            "client" => {
                file.layer = ReleaseLayer::Client;
                file.client = Participation::Choice {
                    key: "variant".into(),
                    value: "yes".into(),
                };
                file.server = Participation::Unsupported;
            }
            "server" => {
                file.layer = ReleaseLayer::Server;
                file.client = Participation::Unsupported;
            }
            _ => {}
        }
    }
    let make = || replace_document(input(), doc.clone());
    apply(&engine, root.path(), make()).await;
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"common"
    );
    let mut enabled = make();
    enabled.choices = vec![ChoiceSelection {
        key: "variant".into(),
        value: "yes".into(),
    }];
    apply(&engine, root.path(), enabled).await;
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"client"
    );
    let server = tempfile::tempdir().unwrap();
    let mut selected = make();
    selected.side = InstanceSide::Server;
    apply(&engine, server.path(), selected).await;
    assert_eq!(
        fs::read(server.path().join("game/config/a")).unwrap(),
        b"server"
    );
}
#[tokio::test]
async fn same_layer_and_portable_alias_collisions_fail_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    for path in ["mods/a.jar", "mods/A.jar", "MODS/a.jar"] {
        let input = request(&[
            ("one", "mods/a.jar", b"one", FilePolicy::Managed),
            ("two", path, b"two", FilePolicy::Managed),
        ]);
        assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn maintenance_rejects_unretained_releases_and_changed_repair_intent() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    let b = || request(&[("mod", "mods/a.jar", b"B", FilePolicy::Managed)]);
    for action in [
        InstanceAction::Repair,
        InstanceAction::ChangeChoices,
        InstanceAction::Rollback,
    ] {
        let mut missing = a();
        missing.action = action;
        assert!(
            engine
                .prepare(root.path().to_owned(), missing)
                .await
                .is_err()
        );
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    apply(&engine, root.path(), a()).await;
    let before = fs::read(root.path().join(".empack/instance.json")).unwrap();
    for action in [
        InstanceAction::Repair,
        InstanceAction::ChangeChoices,
        InstanceAction::Rollback,
    ] {
        let mut other = b();
        other.action = action;
        assert!(engine.prepare(root.path().to_owned(), other).await.is_err());
    }
    let mut altered = a();
    altered.action = InstanceAction::Repair;
    altered.choices.push(ChoiceSelection {
        key: "new".into(),
        value: "on".into(),
    });
    assert!(
        engine
            .prepare(root.path().to_owned(), altered)
            .await
            .is_err()
    );
    assert_eq!(
        before,
        fs::read(root.path().join(".empack/instance.json")).unwrap()
    );
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
}

#[tokio::test]
async fn referenced_instance_content_requires_approval_and_whole_batch_verification() {
    for bad_second in [false, true] {
        let mut server = mockito::Server::new_async().await;
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (mut engine, governor) = super::super::tests::engine(state.path().join("state"));
        engine.transport = HttpAcquisition::for_loopback_tests()
            .with_test_origin("https://release.test", &server.url());
        let mut input = request(&[
            ("a", "mods/a.jar", b"A", FilePolicy::Managed),
            ("b", "mods/b.jar", b"B", FilePolicy::Managed),
        ]);
        let mut document = input.release.release().document().clone();
        for file in &mut document.files {
            file.source = ReleaseSource::Url {
                alternatives: vec![format!("https://release.test/{}", file.key)],
            };
        }
        input = replace_document(input, document);
        input.supplied.clear();
        let Preparation::Ready(prepared) =
            engine.prepare(root.path().to_owned(), input).await.unwrap()
        else {
            panic!()
        };
        assert!(prepared.view().needs_network());
        let denied = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Offline,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        assert!(prepared.authorize(denied).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        // Reprepare because a rejected grant consumes its plan. Neither preparation performs HTTP.
        let mut input = request(&[
            ("a", "mods/a.jar", b"A", FilePolicy::Managed),
            ("b", "mods/b.jar", b"B", FilePolicy::Managed),
        ]);
        let mut document = input.release.release().document().clone();
        for file in &mut document.files {
            file.source = ReleaseSource::Url {
                alternatives: vec![format!("https://release.test/{}", file.key)],
            };
        }
        input = replace_document(input, document);
        input.supplied.clear();
        let Preparation::Ready(prepared) =
            engine.prepare(root.path().to_owned(), input).await.unwrap()
        else {
            panic!()
        };
        let first = server
            .mock("GET", "/a")
            .with_body("A")
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("GET", "/b")
            .with_body(if bad_second { "X" } else { "B" })
            .expect(1)
            .create_async()
            .await;
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Allow,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        let outcome = operation.wait().await;
        first.assert_async().await;
        second.assert_async().await;
        if bad_second {
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        } else {
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::Completed(
                    ExecutionReceipt::Instance(_)
                ))
            ));
            assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
            assert_eq!(fs::read(root.path().join("game/mods/b.jar")).unwrap(), b"B");
        }
        drop(outcome);
        engine.release_completed(operation.id());
        drop(operation);
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

async fn execute_network(engine: &Engine, root: &Path, input: InstallInstanceRequest) -> bool {
    let Preparation::Ready(prepared) = engine.prepare(root.to_owned(), input).await.unwrap() else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Allow,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let result = matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Instance(_)))
    );
    engine.release_completed(operation.id());
    result
}
#[tokio::test]
async fn provider_refresh_preserves_exact_selection_and_assertions() {
    use serde_json::json;
    for wrong_owner in [false, true] {
        let mut server = mockito::Server::new_async().await;
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (engine, governor) = super::super::tests::engine(state.path().join("state"));
        let mut engine = engine.with_provider_catalog(
            ProviderCatalog::for_loopback_tests(&server.url(), None),
            CatalogLimits {
                response_bytes: 4096,
                transfer_bytes: 16384,
                deadline: std::time::Duration::from_secs(3),
            },
        );
        engine.transport = HttpAcquisition::for_loopback_tests()
            .with_test_origin("https://release.test", &server.url());
        let project = server.mock("GET", "/project/AANobbMI").with_body(json!({"id":"AANobbMI","slug":"sodium","title":"Sodium","project_type":"mod","client_side":"required","server_side":"unsupported","loaders":["fabric"]}).to_string()).create_async().await;
        let version = server.mock("GET", "/version/abcdefgh").with_body(json!({"id":"abcdefgh","project_id":if wrong_owner {"ZZZZZZZZ"} else {"AANobbMI"},"game_versions":["1.21.1"],"loaders":["fabric"],"files":[{"filename":"mod.jar","primary":true,"size":1,"hashes":{"sha512":sha2::Sha512::digest(b"A").iter().map(|v|format!("{v:02x}")).collect::<String>()},"url":"https://release.test/fresh"}],"dependencies":[]}).to_string()).create_async().await;
        let payload = server
            .mock("GET", "/fresh")
            .with_body("A")
            .expect(if wrong_owner { 0 } else { 1 })
            .create_async()
            .await;
        let mut input = request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
        let mut document = input.release.release().document().clone();
        document.files[0].source = ReleaseSource::Provider {
            provider: ReleaseProvider::Modrinth,
            project: "AANobbMI".into(),
            selection: "abcdefgh".into(),
            slot: "primary".into(),
            alternatives: vec![],
        };
        input = replace_document(input, document);
        input.supplied.clear();
        assert_eq!(
            execute_network(&engine, root.path(), input).await,
            !wrong_owner
        );
        project.assert_async().await;
        version.assert_async().await;
        payload.assert_async().await;
        if wrong_owner {
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        } else {
            assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
        }
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
#[tokio::test]
async fn provider_world_archive_downloads_once_and_verifies_each_member() {
    use std::io::{Cursor, Write};
    for wrong_member in [false, true] {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [("world/level.dat", b"A"), ("world/region/r.0.0.mca", b"B")] {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        let archive = archive.finish().unwrap().into_inner();
        let mut server = mockito::Server::new_async().await;
        let payload = server
            .mock("GET", "/world.zip")
            .with_body(archive.clone())
            .expect(1)
            .create_async()
            .await;
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (mut engine, governor) = super::super::tests::engine(state.path().join("state"));
        engine.transport = HttpAcquisition::for_loopback_tests()
            .with_test_origin("https://release.test", &server.url());
        let mut input = request(&[
            ("level", "world/level.dat", b"A", FilePolicy::Seed),
            (
                "region",
                "world/region/r.0.0.mca",
                if wrong_member { b"X" } else { b"B" },
                FilePolicy::Seed,
            ),
        ]);
        let mut document = input.release.release().document().clone();
        for file in &mut document.files {
            file.source = ReleaseSource::ProviderArchiveMember {
                archive: ReleaseArchiveSource {
                    selection: ReleaseSelection {
                        provider: ReleaseProvider::CurseForge,
                        project: "123".into(),
                        selection: "456".into(),
                        slot: "primary".into(),
                    },
                    alternatives: vec!["https://release.test/world.zip".into()],
                    assertions: vec![SourceDigest {
                        algorithm: "sha256".into(),
                        value: hash(&archive),
                    }],
                    bytes: Some(archive.len() as u64),
                    sha256: Some(hash(&archive)),
                },
                member: file.destination.clone(),
            };
        }
        input = replace_document(input, document);
        input.supplied.clear();
        assert_eq!(
            execute_network(&engine, root.path(), input).await,
            !wrong_member
        );
        payload.assert_async().await;
        if wrong_member {
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        } else {
            assert_eq!(
                fs::read(root.path().join("game/world/level.dat")).unwrap(),
                b"A"
            );
            assert_eq!(
                fs::read(root.path().join("game/world/region/r.0.0.mca")).unwrap(),
                b"B"
            );
        }
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn prism_layout_persists_through_update_repair_and_rollback() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let a = || request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    let mut first = a();
    first.layout = Some(InstanceLayout::Prism);
    let initial = apply(&engine, root.path(), first).await;
    assert_eq!(initial.layout, InstanceLayout::Prism);
    assert_eq!(
        fs::read(root.path().join(".minecraft/mods/a.jar")).unwrap(),
        b"A"
    );
    fs::create_dir_all(root.path().join(".minecraft/saves/world")).unwrap();
    fs::write(
        root.path().join(".minecraft/saves/world/level.dat"),
        b"played",
    )
    .unwrap();
    let b = || request(&[("new", "mods/b.jar", b"B", FilePolicy::Managed)]);
    let current = apply(&engine, root.path(), b()).await;
    assert_eq!(current.layout, InstanceLayout::Prism);
    assert!(!root.path().join(".minecraft/mods/a.jar").exists());
    fs::remove_file(root.path().join(".minecraft/mods/b.jar")).unwrap();
    let mut repair = b();
    repair.action = InstanceAction::Repair;
    apply(&engine, root.path(), repair).await;
    assert_eq!(
        fs::read(root.path().join(".minecraft/mods/b.jar")).unwrap(),
        b"B"
    );
    let mut rollback = a();
    rollback.action = InstanceAction::Rollback;
    let restored = apply(&engine, root.path(), rollback).await;
    assert_eq!(restored.release, initial.release);
    assert_eq!(restored.layout, InstanceLayout::Prism);
    assert_eq!(
        fs::read(root.path().join(".minecraft/mods/a.jar")).unwrap(),
        b"A"
    );
    assert!(!root.path().join(".minecraft/mods/b.jar").exists());
    assert_eq!(
        fs::read(root.path().join(".minecraft/saves/world/level.dat")).unwrap(),
        b"played"
    );
    assert!(!root.path().join("game").exists());
    let mut move_layout = a();
    move_layout.layout = Some(InstanceLayout::Game);
    assert!(
        engine
            .prepare(root.path().to_path_buf(), move_layout)
            .await
            .is_err()
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn prism_layout_binds_launcher_directory_and_handles_empty_releases() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let prism = || {
        let mut input = request(&[]);
        input.layout = Some(InstanceLayout::Prism);
        input
    };
    let mut server = prism();
    server.side = InstanceSide::Server;
    assert!(
        engine
            .prepare(root.path().to_path_buf(), server)
            .await
            .is_err()
    );
    fs::create_dir(root.path().join("minecraft")).unwrap();
    fs::write(root.path().join("minecraft/sentinel"), b"unowned").unwrap();
    assert!(
        engine
            .prepare(root.path().to_path_buf(), prism())
            .await
            .is_err()
    );
    assert!(!root.path().join(".empack").exists());
    fs::remove_file(root.path().join("minecraft/sentinel")).unwrap();
    fs::remove_dir(root.path().join("minecraft")).unwrap();
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_path_buf(), prism())
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert!(!root.path().join(".minecraft").exists());
    fs::create_dir(root.path().join("minecraft")).unwrap();
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let approved = prepared.authorize(grant).unwrap();
    let mut handle = engine.start(approved).unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert!(!root.path().join(".empack").exists());
    assert!(!root.path().join(".minecraft").exists());
    fs::remove_dir(root.path().join("minecraft")).unwrap();
    apply(&engine, root.path(), prism()).await;
    assert_eq!(
        fs::read(root.path().join(".minecraft/.empack-layout")).unwrap(),
        b"empack-prism-layout-v1\n"
    );
    let mut collision = request(&[(
        "collision",
        ".EMPACK-layout/other",
        b"bad",
        FilePolicy::Managed,
    )]);
    collision.layout = Some(InstanceLayout::Prism);
    assert!(
        engine
            .prepare(root.path().to_path_buf(), collision)
            .await
            .is_err()
    );
    fs::write(root.path().join(".minecraft/.empack-layout"), b"tampered").unwrap();
    assert!(
        engine
            .prepare(root.path().to_path_buf(), prism())
            .await
            .is_err()
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn consumer_preparation_installs_once_and_never_reverts_active_updates() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    let original = || {
        let mut input = request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
        input.action = InstanceAction::Prepare;
        input
    };
    let a = apply(&engine, root.path(), original()).await;
    let next = request(&[("mod", "mods/a.jar", b"B", FilePolicy::Managed)]);
    let b = apply(&engine, root.path(), next).await;
    assert_ne!(a.release, b.release);
    let old_consumer = || {
        let mut input = original();
        input.supplied.clear();
        input
    };
    let kept = apply(&engine, root.path(), old_consumer()).await;
    assert_eq!(kept, b);
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"B");
    let before = fs::read(root.path().join(".empack/instance.json")).unwrap();
    let mut wrong = old_consumer();
    wrong.choices.push(ChoiceSelection {
        key: "new".into(),
        value: "yes".into(),
    });
    assert!(engine.prepare(root.path().to_owned(), wrong).await.is_err());
    for field in ["runtime", "pack"] {
        let input = old_consumer();
        let mut document = input.release.release().document().clone();
        if field == "runtime" {
            document.runtime.minecraft = "1.21.2".into();
        } else {
            document.pack = "other".into();
        }
        assert!(
            engine
                .prepare(root.path().to_owned(), replace_document(input, document))
                .await
                .is_err()
        );
    }
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    // Missing active content must never be replaced by the obsolete initial asset.
    assert!(
        engine
            .prepare(root.path().to_owned(), original())
            .await
            .is_err()
    );
    let mut repair = old_consumer();
    let current_bytes = request(&[("mod", "mods/a.jar", b"B", FilePolicy::Managed)]);
    repair.supplied = current_bytes.supplied;
    assert_eq!(apply(&engine, root.path(), repair).await, b);
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"B");
    assert_eq!(
        fs::read(root.path().join(".empack/instance.json")).unwrap(),
        before
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn repair_reuses_verified_cache_after_restart_without_network_or_original_assets() {
    use crate::engine::content::cache::ContentCache;
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let cache_path = host.path().join("cache");
    let mut server = mockito::Server::new_async().await;
    let download = server
        .mock("GET", "/payload")
        .with_body("A")
        .expect(1)
        .create_async()
        .await;
    let input = || {
        let mut input = request(&[("a", "mods/a.jar", b"A", FilePolicy::Managed)]);
        let mut doc = input.release.release().document().clone();
        doc.files[0].source = ReleaseSource::Url {
            alternatives: vec!["https://release.test/payload".into()],
        };
        doc.files[0].asset = Some("assets/a".into());
        input = replace_document(input, doc);
        input.supplied.clear();
        input.assets = Some(assets.path().to_owned());
        input
    };
    let (engine, governor) = super::super::tests::engine(host.path().join("state"));
    let mut engine = engine
        .with_content_cache(ContentCache::new(cache_path.clone(), Default::default()).unwrap());
    engine.transport = HttpAcquisition::for_loopback_tests()
        .with_test_origin("https://release.test", &server.url());
    let Preparation::Ready(preview) = engine
        .prepare(root.path().to_owned(), input())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(preview.view().needs_network());
    drop(preview);
    assert!(!cache_path.exists(), "preview cannot create a cache");
    assert!(execute_network(&engine, root.path(), input()).await);
    download.assert_async().await;
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    drop(engine);

    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    let (engine, governor) = super::super::tests::engine(host.path().join("state"));
    let engine = engine
        .with_content_cache(ContentCache::new(cache_path.clone(), Default::default()).unwrap());
    let repair = || {
        let mut value = input();
        value.action = InstanceAction::Repair;
        value
    };
    // Explicit associations must still fail rather than silently using another source.
    let mut missing = repair();
    missing
        .local_files
        .insert("a".into(), assets.path().join("missing"));
    assert!(
        engine
            .prepare(root.path().to_owned(), missing)
            .await
            .is_err()
    );
    fs::create_dir_all(assets.path().join("assets")).unwrap();
    fs::write(assets.path().join("assets/a"), b"X").unwrap();
    assert!(
        engine
            .prepare(root.path().to_owned(), repair())
            .await
            .is_err()
    );
    fs::remove_file(assets.path().join("assets/a")).unwrap();
    let Preparation::Ready(preview) = engine
        .prepare(root.path().to_owned(), repair())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(!preview.view().needs_network());
    drop(preview);
    apply(&engine, root.path(), repair()).await;
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
    download.assert_async().await;
    // Corrupt cache bytes are discarded as evidence, leaving the network obligation.
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    let blob = cache_path.join(format!("{}.blob", hash(b"A")));
    fs::write(&blob, b"X").unwrap();
    let Preparation::Ready(preview) = engine
        .prepare(root.path().to_owned(), repair())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(preview.view().needs_network());
    drop(preview);
    assert!(!root.path().join("game/mods/a.jar").exists());
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn approved_local_instance_content_is_reusable_but_cache_failure_is_nonfatal() {
    use crate::engine::content::cache::ContentCache;
    for unavailable in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let cache_path = host.path().join("cache");
        if unavailable {
            fs::write(&cache_path, b"unavailable").unwrap();
        }
        let (engine, governor) = super::super::tests::engine(host.path().join("state"));
        let engine =
            engine.with_content_cache(ContentCache::new(cache_path, Default::default()).unwrap());
        let input = || request(&[("a", "mods/a.jar", b"A", FilePolicy::Managed)]);
        apply(&engine, root.path(), input()).await;
        assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
        if !unavailable {
            fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
            let mut repair = input();
            repair.action = InstanceAction::Repair;
            repair.supplied.clear();
            apply(&engine, root.path(), repair).await;
            assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
        }
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn conflict_preservation_is_durable_local_evidence_and_replace_restores_release() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let a = || {
        request(&[(
            "cfg",
            "config/example.txt",
            b"publisher-a",
            FilePolicy::Managed,
        )])
    };
    let b = || {
        request(&[(
            "cfg",
            "config/example.txt",
            b"publisher-b",
            FilePolicy::Managed,
        )])
    };
    apply(&engine, root.path(), a()).await;
    fs::write(root.path().join("game/config/example.txt"), b"my settings").unwrap();
    assert!(engine.prepare(root.path().to_owned(), b()).await.is_err());
    let mut preserve = b();
    preserve.conflicts.push(ConflictResolution {
        destination: "config/example.txt".into(),
        choice: ConflictChoice::Preserve,
    });
    let record = apply(&engine, root.path(), preserve).await;
    assert_eq!(record.local_overrides.len(), 1);
    assert_eq!(
        record.local_overrides[0].accepted.sha256,
        hash(b"my settings")
    );
    assert_eq!(
        record.local_overrides[0].original.sha256,
        hash(b"publisher-b")
    );
    for _ in 0..2 {
        let mut repair = b();
        repair.action = InstanceAction::Repair;
        let record = apply(&engine, root.path(), repair).await;
        assert_eq!(record.local_overrides.len(), 1);
        assert_eq!(
            fs::read(root.path().join("game/config/example.txt")).unwrap(),
            b"my settings"
        );
    }
    // A different incoming publisher baseline requires a new local decision.
    assert!(engine.prepare(root.path().to_owned(), a()).await.is_err());
    let mut replace = b();
    replace.action = InstanceAction::Repair;
    replace.conflicts.push(ConflictResolution {
        destination: "config/example.txt".into(),
        choice: ConflictChoice::Replace,
    });
    assert!(
        apply(&engine, root.path(), replace)
            .await
            .local_overrides
            .is_empty()
    );
    assert_eq!(
        fs::read(root.path().join("game/config/example.txt")).unwrap(),
        b"publisher-b"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn conflict_decisions_bind_exact_observations_and_never_authorize_directories() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let a = || request(&[("cfg", "config/a", b"original", FilePolicy::Managed)]);
    apply(&engine, root.path(), a()).await;
    let target = root.path().join("game/config/a");
    fs::write(&target, b"edited").unwrap();
    for choice in [ConflictChoice::Preserve, ConflictChoice::Replace] {
        let mut input = a();
        input.conflicts.push(ConflictResolution {
            destination: "config/a".into(),
            choice: choice.clone(),
        });
        let Preparation::Ready(prepared) =
            engine.prepare(root.path().to_owned(), input).await.unwrap()
        else {
            panic!()
        };
        fs::write(&target, b"changed after approval").unwrap();
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            replacement: prepared.view().replacement(),
            network: NetworkPermission::Offline,
            run_installer: false,
            run_runtime: false,
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        assert!(matches!(
            &*operation.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
        ));
        engine.release_completed(operation.id());
        assert_eq!(fs::read(&target).unwrap(), b"changed after approval");
        fs::write(&target, b"edited").unwrap();
    }
    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), b"keep").unwrap();
    for choice in [ConflictChoice::Preserve, ConflictChoice::Replace] {
        let mut input = a();
        input.conflicts.push(ConflictResolution {
            destination: "config/a".into(),
            choice: choice.clone(),
        });
        assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
        assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"keep");
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn retired_local_override_preserves_user_content_and_unknown_decisions_fail() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    fs::create_dir_all(root.path().join("game/config")).unwrap();
    fs::write(root.path().join("game/config/a"), b"preexisting").unwrap();
    let mut input = request(&[("cfg", "config/a", b"publisher", FilePolicy::Managed)]);
    input.conflicts.push(ConflictResolution {
        destination: "config/a".into(),
        choice: ConflictChoice::Preserve,
    });
    assert_eq!(
        apply(&engine, root.path(), input)
            .await
            .local_overrides
            .len(),
        1
    );
    let mut unknown = request(&[]);
    unknown.conflicts.push(ConflictResolution {
        destination: "elsewhere".into(),
        choice: ConflictChoice::Replace,
    });
    assert!(
        engine
            .prepare(root.path().to_owned(), unknown)
            .await
            .is_err()
    );
    assert!(
        apply(&engine, root.path(), request(&[]))
            .await
            .local_overrides
            .is_empty()
    );
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"preexisting"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn explicit_merge_freezes_local_result_without_rewriting_publisher_evidence() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let merge = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let a = || request(&[("cfg", "config/a", b"publisher", FilePolicy::Managed)]);
    apply(&engine, root.path(), a()).await;
    fs::write(root.path().join("game/config/a"), b"edited").unwrap();
    let file = merge.path().join("result.txt");
    fs::write(&file, b"merged settings").unwrap();
    let mut input = a();
    let release_id = input.release.release().id().to_owned();
    let release_bytes = input.release.release().bytes().to_vec();
    input.supplied.clear();
    input.conflicts.push(ConflictResolution {
        destination: "config/a".into(),
        choice: ConflictChoice::Merge { file: file.clone() },
    });
    let Preparation::Ready(prepared) = engine.prepare(root.path().to_owned(), input).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"edited"
    );
    // Preparation has independently verified and frozen the selected merge bytes.
    fs::write(&file, b"later source edit").unwrap();
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Instance(
        receipt,
    ))) = &*outcome
    else {
        panic!("merge publication failed")
    };
    assert_eq!(
        receipt.record.local_overrides[0].accepted.sha256,
        hash(b"merged settings")
    );
    assert_eq!(
        receipt.record.local_overrides[0].original.sha256,
        hash(b"publisher")
    );
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"merged settings"
    );
    assert_eq!(
        fs::read(
            root.path()
                .join(format!(".empack/releases/{release_id}.json"))
        )
        .unwrap(),
        release_bytes
    );
    drop(outcome);
    engine.release_completed(operation.id());
    let mut repair = a();
    repair.action = InstanceAction::Repair;
    repair.supplied.clear();
    apply(&engine, root.path(), repair).await;
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"merged settings"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn merge_requires_a_regular_source_and_rejects_competing_associations() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let merge = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let a = || request(&[("cfg", "config/a", b"publisher", FilePolicy::Managed)]);
    apply(&engine, root.path(), a()).await;
    fs::write(root.path().join("game/config/a"), b"edited").unwrap();
    for source in [merge.path().join("missing"), merge.path().to_owned()] {
        let mut input = a();
        input.supplied.clear();
        input.conflicts.push(ConflictResolution {
            destination: "config/a".into(),
            choice: ConflictChoice::Merge { file: source },
        });
        assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
    }
    let file = merge.path().join("result");
    fs::write(&file, b"merged").unwrap();
    let mut input = a(); // Existing supplied publisher bytes cannot silently replace the merge result.
    input.conflicts.push(ConflictResolution {
        destination: "config/a".into(),
        choice: ConflictChoice::Merge { file },
    });
    assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"edited"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn byte_identical_conflict_resolution_still_checks_source_assertions() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let mut input = request(&[("cfg", "config/a", b"same", FilePolicy::Managed)]);
    fs::create_dir_all(root.path().join("game/config")).unwrap();
    fs::write(root.path().join("game/config/a"), b"same").unwrap();
    let mut document = input.release.release().document().clone();
    document.files[0].assertions.push(SourceDigest {
        algorithm: "sha512".into(),
        value: "0".repeat(128),
    });
    let release = DecodedRelease::encode(document).unwrap();
    input.release = SelectedRelease::Snapshot(
        release::trust::SelectedSnapshot::select(
            release.bytes(),
            release.id(),
            &semver::Version::parse("0.6.0-beta").unwrap(),
        )
        .unwrap(),
    );
    input.conflicts.push(ConflictResolution {
        destination: "config/a".into(),
        choice: ConflictChoice::Replace,
    });
    assert!(engine.prepare(root.path().to_owned(), input).await.is_err());
    assert!(!root.path().join(".empack/instance.json").exists());
    assert_eq!(
        fs::read(root.path().join("game/config/a")).unwrap(),
        b"same"
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn manual_instance_continuation_retains_verified_inputs_and_requires_fresh_approval() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let mut input = request(&[
        ("a", "mods/a.jar", b"first", FilePolicy::Managed),
        ("b", "mods/b.jar", b"second", FilePolicy::Managed),
    ]);
    input.supplied.remove("b");
    let Preparation::NeedsInput(pending) =
        engine.prepare(root.path().to_owned(), input).await.unwrap()
    else {
        panic!("manual content must be an owned continuation")
    };
    let OperationPreview::Instance(view) = pending.view() else {
        panic!()
    };
    assert_eq!(view.manual.len(), 1);
    assert_eq!(view.manual[0].key, "b");
    assert_eq!(view.manual[0].sha256, hash(b"second"));
    let old_plan = view.plan;
    assert!(!root.path().join(".empack").exists());
    let file = source.path().join("manual.jar");
    fs::write(&file, b"second").unwrap();
    let Preparation::Ready(prepared) = engine
        .resume_instance_files(*pending, BTreeMap::from([("b".into(), file)]))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(prepared.view().plan(), old_plan);
    assert!(!root.path().join("game").exists());
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*operation.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Instance(_)))
    ));
    assert_eq!(
        fs::read(root.path().join("game/mods/a.jar")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(root.path().join("game/mods/b.jar")).unwrap(),
        b"second"
    );
    engine.release_completed(operation.id());
    engine.shutdown().await;
}

#[tokio::test]
async fn manual_instance_continuation_rejects_wrong_bytes_unknown_keys_and_changed_base() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let a = || request(&[("a", "mods/a.jar", b"first", FilePolicy::Managed)]);
    apply(&engine, root.path(), a()).await;
    let pending = || {
        let mut input = request(&[
            ("a", "mods/a.jar", b"first", FilePolicy::Managed),
            ("b", "mods/b.jar", b"second", FilePolicy::Managed),
        ]);
        input.supplied.clear();
        input
    };
    let file = source.path().join("manual.jar");
    for (key, bytes, change_base) in [
        ("b", b"bad".as_slice(), false),
        ("unknown", b"second".as_slice(), false),
        ("b", b"second".as_slice(), true),
    ] {
        let Preparation::NeedsInput(continuation) = engine
            .prepare(root.path().to_owned(), pending())
            .await
            .unwrap()
        else {
            panic!()
        };
        fs::write(&file, bytes).unwrap();
        if change_base {
            fs::write(root.path().join("game/mods/a.jar"), b"changed").unwrap();
        }
        assert!(
            engine
                .resume_instance_files(*continuation, BTreeMap::from([(key.into(), file.clone())]))
                .await
                .is_err()
        );
        assert!(!root.path().join("game/mods/b.jar").exists());
    }
    engine.shutdown().await;
}

#[tokio::test]
async fn restricted_provider_input_returns_owned_continuation_without_publication() {
    let mut server = mockito::Server::new_async().await;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    let engine = engine.with_provider_catalog(
        ProviderCatalog::for_loopback_tests(&server.url(), Some("fixture-key".into())),
        CatalogLimits::default(),
    );
    let project = server.mock("GET", "/mods/123").with_body(
        serde_json::json!({"data":{"id":123,"gameId":432,"slug":"fixture","name":"Fixture","classId":6}}).to_string()
    ).expect(1).create_async().await;
    let digest = sha1::Sha1::digest(b"A")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let metadata = server.mock("GET", "/mods/123/files/456").with_body(
        serde_json::json!({"data":{"id":456,"gameId":432,"modId":123,"fileName":"a.jar","fileLength":1,"downloadUrl":null,"hashes":[{"algo":1,"value":digest}],"gameVersions":["1.21.1"],"dependencies":[]}}).to_string()
    ).expect(1).create_async().await;
    let mut input = request(&[("mod", "mods/a.jar", b"A", FilePolicy::Managed)]);
    let mut document = input.release.release().document().clone();
    document.files[0].source = ReleaseSource::Provider {
        provider: ReleaseProvider::CurseForge,
        project: "123".into(),
        selection: "456".into(),
        slot: "primary".into(),
        alternatives: vec![],
    };
    document.files[0].assertions = vec![SourceDigest {
        algorithm: "sha1".into(),
        value: digest,
    }];
    input = replace_document(input, document);
    input.supplied.clear();
    let Preparation::Ready(prepared) = engine.prepare(root.path().to_owned(), input).await.unwrap()
    else {
        panic!("provider lookup is available")
    };
    let approval = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Allow,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(approval).unwrap()).unwrap();
    let outcome = operation.wait().await;
    let pending = match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::NeedsInput(input)) => {
            assert_eq!(input.instance_requirements()[0].key, "mod");
            input.take_continuation().unwrap()
        }
        _ => panic!("restricted file must remain resumable"),
    };
    let Preparation::NeedsInput(pending) = engine
        .resume_instance_files(pending, BTreeMap::new())
        .await
        .unwrap()
    else {
        panic!("empty resume must retain missing input")
    };
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    let file = state.path().join("a.jar");
    fs::write(&file, b"A").unwrap();
    let Preparation::Ready(prepared) = engine
        .resume_instance_files(*pending, BTreeMap::from([("mod".into(), file)]))
        .await
        .unwrap()
    else {
        panic!("exact input is ready")
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut resumed = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    assert!(matches!(
        &*resumed.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
    project.assert_async().await;
    metadata.assert_async().await;
    drop(outcome);
    engine.release_completed(operation.id());
    engine.release_completed(resumed.id());
    drop(operation);
    drop(resumed);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn durable_instance_inputs_survive_engine_restart_without_source_files() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    let mut input = request(&[
        ("a", "mods/a.jar", b"A", FilePolicy::Managed),
        ("b", "mods/b.jar", b"B", FilePolicy::Managed),
    ]);
    input.supplied.remove("b");
    let Preparation::NeedsInput(pending) =
        engine.prepare(root.path().to_owned(), input).await.unwrap()
    else {
        panic!("missing b")
    };
    let saved = engine.suspend_instance(*pending, None).await.unwrap();
    assert_eq!(saved.retained_files, 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    drop(saved);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let (engine, governor) = super::super::tests::engine(state.path().join("state"));
    let input = state.path().join("b.jar");
    fs::write(&input, b"B").unwrap();
    let resumed = engine
        .resume_saved_instance(
            root.path().to_owned(),
            BTreeMap::from([("b".into(), input)]),
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let Preparation::Ready(prepared) = resumed.preparation else {
        panic!("exact retained a and supplied b")
    };
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = operation.wait().await;
    if let OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(e)) = &*outcome {
        panic!("{e:#}");
    }
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(_))
    ));
    assert_eq!(fs::read(root.path().join("game/mods/a.jar")).unwrap(), b"A");
    assert_eq!(fs::read(root.path().join("game/mods/b.jar")).unwrap(), b"B");
    assert!(engine.discard_saved_instance(resumed.saved).await.unwrap());
    assert!(
        engine
            .resume_saved_instance(root.path().to_owned(), BTreeMap::new(), None)
            .await
            .unwrap()
            .is_none()
    );
    drop(outcome);
    engine.release_completed(operation.id());
    drop(operation);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn saved_instance_rejects_changed_base_without_discarding_pending_input() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(state.path().join("state"));
    apply(
        &engine,
        root.path(),
        request(&[("a", "mods/a.jar", b"A", FilePolicy::Managed)]),
    )
    .await;
    let mut input = request(&[
        ("a", "mods/a.jar", b"A", FilePolicy::Managed),
        ("b", "mods/b.jar", b"B", FilePolicy::Managed),
    ]);
    input.supplied.clear();
    let Preparation::NeedsInput(pending) =
        engine.prepare(root.path().to_owned(), input).await.unwrap()
    else {
        panic!("missing b")
    };
    let saved = engine.suspend_instance(*pending, None).await.unwrap();
    let entries = || {
        fs::read_dir(state.path().join("state/pending-instances"))
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), fs::read(e.path()).unwrap())
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = entries();
    fs::remove_file(root.path().join("game/mods/a.jar")).unwrap();
    assert!(
        engine
            .resume_saved_instance(root.path().to_owned(), BTreeMap::new(), None)
            .await
            .is_err()
    );
    assert_eq!(entries(), before);
    assert!(!root.path().join("game/mods/b.jar").exists());
    assert!(engine.discard_saved_instance(saved.saved).await.unwrap());
    engine.shutdown().await;
}

#[tokio::test]
async fn durable_instance_merge_retains_local_evidence_and_cleanup_is_conditional() {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    for merged in [b"edited".as_slice(), b"merged".as_slice()] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (engine, _) = super::super::tests::engine(state.path().join("state"));
        apply(
            &engine,
            root.path(),
            request(&[("cfg", "config/a", b"original", FilePolicy::Managed)]),
        )
        .await;
        fs::write(root.path().join("game/config/a"), b"edited").unwrap();
        let merge = state.path().join("merge");
        fs::write(&merge, merged).unwrap();
        let mut input = request(&[
            ("cfg", "config/a", b"publisher", FilePolicy::Managed),
            ("mod", "mods/a.jar", b"A", FilePolicy::Managed),
        ]);
        input.supplied.clear();
        input.conflicts.push(ConflictResolution {
            destination: "config/a".into(),
            choice: ConflictChoice::Merge {
                file: merge.clone(),
            },
        });
        let Preparation::NeedsInput(pending) =
            engine.prepare(root.path().to_owned(), input).await.unwrap()
        else {
            panic!("missing mod")
        };
        engine.suspend_instance(*pending, None).await.unwrap();
        engine.shutdown().await;
        fs::remove_file(merge).unwrap();
        let (engine, _) = super::super::tests::engine(state.path().join("state"));
        let observed = engine
            .observe_pending_instance(root.path().to_owned())
            .await
            .unwrap()
            .unwrap();
        let resumed = engine
            .resume_saved_instance(root.path().to_owned(), BTreeMap::new(), None)
            .await
            .unwrap()
            .unwrap();
        let Preparation::NeedsInput(pending) = resumed.preparation else {
            panic!("still missing mod")
        };
        let file = state.path().join("a.jar");
        fs::write(&file, b"A").unwrap();
        let Preparation::Ready(prepared) = engine
            .resume_instance_files(*pending, BTreeMap::from([("mod".into(), file)]))
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            network: NetworkPermission::Offline,
            run_installer: false,
            run_runtime: false,
            replacement: prepared.view().replacement(),
        };
        let mut operation = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        assert!(matches!(
            &*operation.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::Completed(_))
        ));
        assert_eq!(fs::read(root.path().join("game/config/a")).unwrap(), merged);
        let record =
            InstanceRecord::decode(&fs::read(root.path().join(".empack/instance.json")).unwrap())
                .unwrap();
        assert_eq!(
            record.local_overrides[0].original.sha256,
            hash(b"publisher")
        );
        assert_eq!(record.local_overrides[0].accepted.sha256, hash(merged));
        let directory = state.path().join("state/pending-instances");
        let path = fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "json"))
            .unwrap();
        fs::write(&path, b"changed invalid record").unwrap();
        assert!(!engine.discard_pending_instance(observed).await.unwrap());
        let observed = engine
            .observe_pending_instance(root.path().to_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(engine.discard_pending_instance(observed).await.unwrap());
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn required_subscription_is_captured_and_late_revocation_prevents_installation() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let (engine, _) = super::super::tests::engine(host.path().join("state"));
    let input = || {
        let mut input = request(&[]);
        input.require_subscription = true;
        input
    };
    assert!(
        engine
            .prepare(root.path().to_owned(), input())
            .await
            .is_err()
    );
    let key = ed25519_dalek::SigningKey::from_bytes(&[57; 32]);
    let enroll = SubscriptionRequest::Enroll {
        pack: "fixture".into(),
        channel: "stable".into(),
        url: "https://publisher.test/stable.json".into(),
        keys: vec![key.verifying_key()],
    };
    async fn change(engine: &Engine, root: &Path, request: SubscriptionRequest) {
        let Preparation::Ready(prepared) = engine.prepare(root.to_owned(), request).await.unwrap()
        else {
            panic!()
        };
        let grant = ExecutionGrant {
            plan: prepared.view().plan(),
            replacement: prepared.view().replacement(),
            network: NetworkPermission::Offline,
            run_installer: false,
            run_runtime: false,
        };
        let mut op = engine.start(prepared.authorize(grant).unwrap()).unwrap();
        assert!(matches!(
            &*op.wait().await,
            OperationOutcome::Completed(ExecutionOutcome::Completed(_))
        ));
        engine.release_completed(op.id());
    }
    change(&engine, root.path(), enroll).await;
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_owned(), input())
        .await
        .unwrap()
    else {
        panic!()
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    };
    let approved = prepared.authorize(grant).unwrap();
    change(
        &engine,
        root.path(),
        SubscriptionRequest::ReplaceKeys { keys: vec![] },
    )
    .await;
    let mut op = engine.start(approved).unwrap();
    assert!(matches!(
        &*op.wait().await,
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
    ));
    assert!(!root.path().join(".empack/instance.json").exists());
    assert!(
        engine
            .prepare(root.path().to_owned(), input())
            .await
            .is_err()
    );
    engine.shutdown().await;
}
