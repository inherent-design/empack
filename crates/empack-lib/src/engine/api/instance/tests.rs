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
    for action in [InstanceAction::Repair, InstanceAction::Rollback] {
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
    for action in [InstanceAction::Repair, InstanceAction::Rollback] {
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
