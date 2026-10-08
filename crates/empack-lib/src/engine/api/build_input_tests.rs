use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, verify_stream},
        documents::DocumentCodec,
        mrpack::{AcquiredBuildFile, LockedFileKey},
    },
};
use empack_core::{
    files::FilePermissions,
    identity::{ModrinthProjectId, ProviderProjectId},
    model::*,
};
use sha2::Digest;
use std::{collections::BTreeMap, fs, io::Read, path::Path};

pub(super) fn fixture(root: &Path, weak: bool) {
    let base = crate::engine::mrpack::tests::project(weak, false);
    let identity = ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap());
    let pin = ResolvedPin {
        project: identity.clone(),
        selection: identity.parse_pin("Version1").unwrap(),
    };
    let mut intent = base.intent().clone();
    intent.roots.values_mut().next().unwrap().source = SourceIntent::Provider(identity.clone());
    let raw = DocumentCodec.encode_intent(&intent).unwrap();
    let revision = DocumentCodec
        .decode_intent(&raw, "fixture")
        .unwrap()
        .semantic_revision();
    let mut lock = base.lock().clone();
    lock.intent_revision = revision;
    let dependency = lock.dependencies.values_mut().next().unwrap();
    dependency.identity = ResolvedIdentity::Provider(identity);
    dependency.selected = Some(pin.clone());
    dependency.files = NonEmpty::new(
        dependency
            .files
            .as_slice()
            .iter()
            .cloned()
            .map(|mut file| {
                file.acquisition = AcquisitionSpec::Manual {
                    pin: Some(pin.clone()),
                    instructions: "Supply exact bytes".into(),
                };
                file
            })
            .collect(),
    )
    .unwrap();
    let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
    tests::put(root, "empack.yml", &raw);
    tests::put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
}
pub(super) fn supplied(slot: &str, bytes: &[u8]) -> BuildAcquisitions {
    let content = verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        100,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap();
    BuildAcquisitions {
        locked: BTreeMap::from([(
            LockedFileKey {
                dependency: DependencyKey::parse("assets").unwrap(),
                slot: FileSlot::parse(slot).unwrap(),
            },
            AcquiredBuildFile {
                content,
                permissions: FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        )]),
        observed: BTreeMap::new(),
    }
}
async fn pending(engine: &Engine, root: &Path) -> Box<PreparationContinuation> {
    match engine
        .prepare(root.to_path_buf(), tests::request())
        .await
        .unwrap()
    {
        Preparation::NeedsInput(value) => value,
        _ => panic!("manual inputs must remain pending"),
    }
}
#[tokio::test]
async fn supplied_manual_files_survive_repeated_input_decisions_and_publish_verified_outputs() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let intent = fs::read(root.path().join("empack.yml")).unwrap();
    let lock = fs::read(root.path().join("empack.lock")).unwrap();
    let (engine, governor) = tests::engine(host.path().join("state"));
    let initial = pending(&engine, root.path()).await;
    let first_plan = initial.plan();
    assert_eq!(initial.build().unwrap().unresolved.len(), 2);
    let next = match engine
        .resume(*initial, supplied("first", b"payload"))
        .await
        .unwrap()
    {
        Preparation::NeedsInput(value) => value,
        _ => panic!("second file still needs explicit input"),
    };
    assert_eq!(next.build().unwrap().unresolved.len(), 1);
    assert_ne!(next.plan(), first_plan);
    assert!(!root.path().join("dist").exists());
    assert!(!host.path().join("state").exists());
    let prepared = match engine
        .resume(*next, supplied("second", b"payload"))
        .await
        .unwrap()
    {
        Preparation::Ready(value) => value,
        _ => panic!("complete verified input was not accepted"),
    };
    assert!(!prepared.view().needs_network());
    assert!(prepared.view().build().unwrap().content.is_empty());
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
    };
    let mut handle = engine.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(
            receipt,
        ))) => assert_eq!(receipt.artifacts.len(), 2),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("supplied build did not publish"),
    }
    for (archive, prefix) in [
        ("client.zip", ".minecraft/"),
        ("result.mrpack", "client-overrides/"),
    ] {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist").join(archive)).unwrap())
                .unwrap();
        for file in ["a.zip", "copy.zip", "b.zip"] {
            let mut bytes = vec![];
            archive
                .by_name(&format!("{prefix}resourcepacks/{file}"))
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"payload");
        }
    }
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), intent);
    assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), lock);
    engine.release_completed(handle.id());
    drop((outcome, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
#[tokio::test]
async fn supplied_build_refuses_wrong_bytes_extra_keys_and_strong_policy_downgrades() {
    for mode in ["wrong", "extra", "strong"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), true);
        let (engine, governor) = tests::engine(host.path().join("state"));
        let mut request = tests::request();
        if mode == "strong" {
            request.evidence = SourceEvidencePolicy::StrongSourceRequired;
        }
        let input = supplied(
            if mode == "extra" { "missing" } else { "first" },
            if mode == "wrong" {
                b"changed"
            } else {
                b"payload"
            },
        );
        assert!(
            engine
                .prepare(root.path().to_path_buf(), request.with_content(input))
                .await
                .is_err(),
            "{mode}"
        );
        assert!(!root.path().join("dist").exists());
        assert!(!host.path().join("state").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn continuation_rejects_changed_inputs_other_engines_and_duplicate_supplied_keys() {
    for mode in ["edited", "other", "duplicate"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), false);
        let (engine, governor) = tests::engine(host.path().join("state"));
        let initial = pending(&engine, root.path()).await;
        let result = match mode {
            "edited" => {
                let path = root.path().join("empack.yml");
                let mut bytes = fs::read(&path).unwrap();
                bytes.extend_from_slice(b"\n# user edit\n");
                fs::write(path, bytes).unwrap();
                engine.resume(*initial, supplied("first", b"payload")).await
            }
            "other" => {
                let (other, _) = tests::engine(host.path().join("other"));
                let result = other.resume(*initial, supplied("first", b"payload")).await;
                other.shutdown().await;
                result
            }
            "duplicate" => {
                let next = match engine
                    .resume(*initial, supplied("first", b"payload"))
                    .await
                    .unwrap()
                {
                    Preparation::NeedsInput(value) => value,
                    _ => panic!("second file still pending"),
                };
                engine.resume(*next, supplied("first", b"payload")).await
            }
            _ => unreachable!(),
        };
        assert!(result.is_err(), "{mode}");
        assert!(!root.path().join("dist").exists());
        assert!(!host.path().join("state").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        engine.shutdown().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn continuation_cannot_follow_a_retargeted_project_selection() {
    let host = tempfile::tempdir().unwrap();
    let first = host.path().join("first");
    let second = host.path().join("second");
    fixture(&first, false);
    fixture(&second, false);
    let alias = host.path().join("selected");
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let (engine, governor) = tests::engine(host.path().join("state"));
    let initial = pending(&engine, &alias).await;
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    assert!(
        engine
            .resume(*initial, supplied("first", b"payload"))
            .await
            .is_err(),
        "continuation switched native project roots"
    );
    assert!(!first.join("dist").exists());
    assert!(!second.join("dist").exists());
    assert!(!host.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn resume_reuses_the_prior_preparation_reservation() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let (template, _) = tests::engine(host.path().join("state"));
    let config = template.config.clone();
    template.shutdown().await;
    let governor = ResourceGovernor::new(config.resources.capture);
    let engine = Engine::new(config, governor.clone()).unwrap();
    let initial = pending(&engine, root.path()).await;
    let next = match engine.resume(*initial, supplied("first", b"payload")).await {
        Ok(Preparation::NeedsInput(value)) => value,
        Err(error) => panic!("resume double-reserved preparation: {error:#}"),
        _ => panic!("second file remains required"),
    };
    let prepared = engine
        .resume(*next, supplied("second", b"payload"))
        .await
        .unwrap();
    assert!(matches!(prepared, Preparation::Ready(_)));
    drop(prepared);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert!(!root.path().join("dist").exists());
    engine.shutdown().await;
}

fn local_selection(slot: &str, source: &Path) -> BTreeMap<AcquisitionKey, PathBuf> {
    BTreeMap::from([(
        AcquisitionKey::Locked(LockedFileKey {
            dependency: DependencyKey::parse("assets").unwrap(),
            slot: FileSlot::parse(slot).unwrap(),
        }),
        source.to_path_buf(),
    )])
}
#[tokio::test]
async fn resumed_local_build_inputs_share_the_retained_byte_allowance() {
    for first_is_local in [false, true] {
        for second_is_local in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let host = tempfile::tempdir().unwrap();
            fixture(root.path(), false);
            let authored = fs::read(root.path().join("empack.yml")).unwrap();
            let locked = fs::read(root.path().join("empack.lock")).unwrap();
            let source = host.path().join("selected.bin");
            fs::write(&source, b"payload").unwrap();
            let (mut engine, governor) = tests::engine(host.path().join("state"));
            engine.config.transfer.transfer_bytes = 10;
            let request = if first_is_local {
                tests::request()
                    .with_content(BuildAcquisitions::default())
                    .with_local_files(local_selection("first", &source))
            } else {
                tests::request().with_content(supplied("first", b"payload"))
            };
            let pending = match engine
                .prepare(root.path().to_path_buf(), request)
                .await
                .unwrap()
            {
                Preparation::NeedsInput(pending) => pending,
                _ => panic!("second slot must remain unresolved"),
            };
            let result = if second_is_local {
                engine
                    .resume_with_local_files(*pending, local_selection("second", &source))
                    .await
            } else {
                engine
                    .resume(*pending, supplied("second", b"payload"))
                    .await
            };
            assert!(
                result.is_err(),
                "resume accepted fourteen bytes with a ten-byte allowance"
            );
            drop(result);
            assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), authored);
            assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), locked);
            assert!(!root.path().join("dist").exists());
            assert!(!host.path().join("state").exists());
            assert_eq!(governor.status().reserved, ResourceRequest::default());
            engine.shutdown().await;
        }
    }
}
#[tokio::test]
async fn local_build_inputs_resume_with_verified_private_bytes_and_publish_offline() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    tests::put(root.path(), "pack/mods/extra.pw.toml", b"filename='extra.jar'\nside='client'\n[download]\nurl='https://example.invalid/extra.jar'\nhash-format='md5'\nhash='321c3cf486ed509164edec1e1981fec8'\n");
    let authored = fs::read(root.path().join("empack.yml")).unwrap();
    let locked = fs::read(root.path().join("empack.lock")).unwrap();
    let source = host.path().join("selected.bin");
    fs::write(&source, b"payload").unwrap();
    let (engine, governor) = tests::engine(host.path().join("state"));
    let mut files = local_selection("first", &source);
    files.insert(
        AcquisitionKey::Observed(tests::path("mods/extra.pw.toml")),
        source.clone(),
    );
    let request = tests::request()
        .with_content(BuildAcquisitions::default())
        .with_local_files(files);
    let pending = match engine
        .prepare(root.path().to_path_buf(), request)
        .await
        .unwrap()
    {
        Preparation::NeedsInput(pending) => pending,
        _ => panic!("second slot must remain unresolved"),
    };
    assert_eq!(pending.build().unwrap().unresolved.len(), 1);
    // The first selection is retained privately; subsequent host-file edits cannot alter it.
    fs::write(&source, b"changed").unwrap();
    let second = host.path().join("second.bin");
    fs::write(&second, b"payload").unwrap();
    let ready = match engine
        .resume_with_local_files(*pending, local_selection("second", &second))
        .await
        .unwrap()
    {
        Preparation::Ready(ready) => ready,
        _ => panic!("all verified bytes must complete preparation"),
    };
    fs::remove_file(&second).unwrap();
    assert!(ready.view().build().unwrap().content.is_empty());
    assert!(!ready.view().needs_network());
    assert!(!root.path().join("dist").exists());
    assert!(!host.path().join("state").exists());
    let grant = ExecutionGrant {
        plan: ready.view().plan(),
        replacement: ready.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
    };
    let mut handle = engine.start(ready.authorize(grant).unwrap()).unwrap();
    let outcome = handle.wait().await;
    match &*outcome {
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_))) => (),
        OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
            panic!("{error:#}")
        }
        _ => panic!("local input build failed"),
    }
    for (archive, prefix) in [
        ("client.zip", ".minecraft/"),
        ("result.mrpack", "client-overrides/"),
    ] {
        let mut zip =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist").join(archive)).unwrap())
                .unwrap();
        for file in ["a.zip", "b.zip", "copy.zip"] {
            let mut bytes = Vec::new();
            zip.by_name(&format!("{prefix}resourcepacks/{file}"))
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"payload");
        }
        if archive == "client.zip" {
            let mut bytes = Vec::new();
            zip.by_name(".minecraft/mods/extra.jar")
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes, b"payload");
        } else {
            // Verified observed bytes provide the missing export hashes; the durable origin
            // remains a download reference with its original environment participation.
            let index: serde_json::Value =
                serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap()).unwrap();
            let file = index["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|file| file["path"] == "mods/extra.jar")
                .unwrap();
            assert_eq!(
                file["downloads"],
                serde_json::json!(["https://example.invalid/extra.jar"])
            );
            assert_eq!(
                file["env"],
                serde_json::json!({"client":"required", "server":"unsupported"})
            );
            assert_eq!(file["fileSize"], 7);
            assert_eq!(
                file["hashes"]["sha512"],
                empack_core::digest::ExpectedDigest::Sha512(
                    sha2::Sha512::digest(b"payload").into()
                )
                .hex()
            );
        }
    }
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), authored);
    assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), locked);
    engine.release_completed(handle.id());
    drop((outcome, handle));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[tokio::test]
async fn local_build_inputs_reject_wrong_missing_unrequested_unsafe_and_over_budget_files() {
    for mode in [
        "wrong",
        "missing",
        "extra",
        "relative",
        "directory",
        "strong",
        "budget",
        "batch-budget",
        "deadline",
        "duplicate",
    ] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), mode == "strong");
        tests::put(root.path(), "dist/result.mrpack", b"old mrpack");
        tests::put(root.path(), "dist/client.zip", b"old client");
        let authored = fs::read(root.path().join("empack.yml")).unwrap();
        let locked = fs::read(root.path().join("empack.lock")).unwrap();
        let source = host.path().join("selected.bin");
        match mode {
            "missing" => (),
            "directory" => fs::create_dir(&source).unwrap(),
            "wrong" => fs::write(&source, b"changed").unwrap(),
            _ => fs::write(&source, b"payload").unwrap(),
        }
        let (mut engine, governor) = tests::engine(host.path().join("state"));
        if mode == "budget" {
            engine.config.transfer.transfer_bytes = 6;
        }
        if mode == "batch-budget" {
            engine.config.transfer.transfer_bytes = 10;
        }
        if mode == "deadline" {
            engine.config.transfer.deadline = std::time::Duration::ZERO;
        }
        let mut request = tests::request();
        if mode == "strong" {
            request.evidence = SourceEvidencePolicy::StrongSourceRequired;
        }
        let previous = if mode == "duplicate" {
            supplied("first", b"payload")
        } else {
            BuildAcquisitions::default()
        };
        let mut files = local_selection(
            if mode == "extra" { "unknown" } else { "first" },
            if mode == "relative" {
                Path::new("selected.bin")
            } else {
                &source
            },
        );
        if mode == "batch-budget" {
            files.extend(local_selection("second", &source));
        }
        assert!(
            engine
                .prepare(
                    root.path().to_path_buf(),
                    request.with_content(previous).with_local_files(files)
                )
                .await
                .is_err(),
            "{mode}"
        );
        assert_eq!(
            fs::read(root.path().join("dist/result.mrpack")).unwrap(),
            b"old mrpack",
            "{mode}"
        );
        assert_eq!(
            fs::read(root.path().join("dist/client.zip")).unwrap(),
            b"old client",
            "{mode}"
        );
        assert_eq!(
            fs::read(root.path().join("empack.yml")).unwrap(),
            authored,
            "{mode}"
        );
        assert_eq!(
            fs::read(root.path().join("empack.lock")).unwrap(),
            locked,
            "{mode}"
        );
        assert!(!host.path().join("state").exists(), "{mode}");
        assert_eq!(
            governor.status().reserved,
            ResourceRequest::default(),
            "{mode}"
        );
        engine.shutdown().await;
    }
}

#[tokio::test]
async fn local_build_inputs_reject_stale_resume_without_reading_a_new_association() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let (engine, governor) = tests::engine(host.path().join("state"));
    let prior = pending(&engine, root.path()).await;
    let path = root.path().join("empack.yml");
    let mut authored = fs::read(&path).unwrap();
    authored.extend_from_slice(b"\n# later authoring edit\n");
    fs::write(&path, &authored).unwrap();
    let result = engine
        .resume_with_local_files(
            *prior,
            local_selection("first", &host.path().join("missing")),
        )
        .await;
    assert!(result.is_err());
    let error = result.err().unwrap();
    assert!(!format!("{error:#}").contains("Local source"), "{error:#}");
    assert_eq!(fs::read(path).unwrap(), authored);
    assert!(!root.path().join("dist").exists());
    assert!(!host.path().join("state").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn local_build_inputs_refuse_a_selected_symlink() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let source = host.path().join("source");
    fs::write(&source, b"payload").unwrap();
    let link = host.path().join("link");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let (engine, governor) = tests::engine(host.path().join("state"));
    assert!(
        engine
            .prepare(
                root.path().to_path_buf(),
                tests::request()
                    .with_content(BuildAcquisitions::default())
                    .with_local_files(local_selection("first", &link))
            )
            .await
            .is_err()
    );
    assert_eq!(fs::read(&source).unwrap(), b"payload");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    engine.shutdown().await;
}
