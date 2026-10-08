use super::tests::{engine, fixture, put};
use super::*;
use crate::engine::{documents::DocumentCodec, mrpack::tests::project};
use empack_core::model::*;
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};

fn recorded() -> SyncRequest {
    SyncRequest::Recorded {
        resolution: None,
        evidence: SourceEvidencePolicy::Compatibility,
    }
}
fn source_project(root: &Path, embedded: bool, weak: bool) -> ResolvedProject {
    fixture(root);
    let base = project(weak, false);
    let mut intent = base.intent().clone();
    let key = DependencyKey::parse("assets").unwrap();
    let source = super::tests::path("sources/payload");
    if embedded {
        intent.roots.clear();
    } else {
        intent.roots.get_mut(&key).unwrap().source = SourceIntent::Local(source.clone());
    }
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "intent")
        .unwrap();
    let mut lock = base.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    dependency.identity = ResolvedIdentity::Local(key.clone());
    dependency.files = NonEmpty::new(
        dependency
            .files
            .as_slice()
            .iter()
            .cloned()
            .map(|mut file| {
                file.acquisition = if embedded {
                    AcquisitionSpec::Embedded {
                        archive: super::tests::path("sources/archive.zip"),
                        member: super::tests::path("inside/payload"),
                    }
                } else {
                    AcquisitionSpec::Local(source.clone())
                };
                file
            })
            .collect(),
    )
    .unwrap();
    let resolved = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
    put(
        root,
        "empack.yml",
        &DocumentCodec.encode_intent(resolved.intent()).unwrap(),
    );
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    put(root, "sources/payload", b"payload");
    if embedded {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "inside/payload",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"payload").unwrap();
        put(
            root,
            "sources/archive.zip",
            &writer.finish().unwrap().into_inner(),
        );
    }
    resolved
}
fn grant(prepared: &PreparedOperation) -> ExecutionGrant {
    ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        replacement: prepared.view().replacement(),
    }
}
fn files(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, found: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                visit(root, &entry.path(), found);
            } else if kind.is_file() {
                found.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut found = std::collections::BTreeMap::new();
    visit(root, root, &mut found);
    found
}

#[tokio::test]
async fn recorded_sync_restores_local_and_archive_members_then_preserves_raw_noop_documents() {
    for embedded in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        source_project(root.path(), embedded, false);
        fs::remove_file(root.path().join("pack/resourcepacks/a.zip")).unwrap();
        put(root.path(), "pack/resourcepacks/copy.zip", b"old bytes");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "/nonexistent-unrelated",
            root.path().join("pack/unrelated-link"),
        )
        .unwrap();
        let before = files(root.path());
        let (engine, governor) = engine(state.path().join("state"));
        let view = engine
            .preview(root.path().to_path_buf(), recorded())
            .await
            .unwrap();
        assert!(!view.sync().unwrap().files.changes().is_empty());
        assert_eq!(files(root.path()), before);
        assert!(!state.path().join("state").exists());
        drop(view);
        for repeat in [false, true] {
            let Preparation::Ready(prepared) = engine
                .prepare(root.path().to_path_buf(), recorded())
                .await
                .unwrap()
            else {
                panic!("ready")
            };
            assert_eq!(
                prepared.view().sync().unwrap().files.changes().is_empty(),
                repeat
            );
            let permission = grant(&prepared);
            let mut operation = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = operation.wait().await;
            assert!(
                matches!(
                    &*outcome,
                    OperationOutcome::Completed(ExecutionOutcome::Completed(
                        ExecutionReceipt::Sync(_)
                    ))
                ),
                "recorded synchronization did not complete"
            );
            engine.release_completed(operation.id());
            drop(outcome);
            drop(operation);
            assert_eq!(governor.status().reserved, ResourceRequest::default());
            for name in ["a.zip", "copy.zip", "b.zip"] {
                assert_eq!(
                    fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
                    b"payload"
                );
            }
            assert_eq!(
                fs::read(root.path().join("empack.yml")).unwrap(),
                before["empack.yml"]
            );
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                before["empack.lock"]
            );
        }
        engine.shutdown().await;
    }
}
#[tokio::test]
async fn recorded_sync_failures_and_stale_sources_never_publish_a_subset() {
    for case in [
        "missing",
        "directory",
        "changed",
        "archive-corruption",
        "late-source-edit",
        "late-archive-edit",
        "weak-strict",
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        source_project(
            root.path(),
            matches!(case, "archive-corruption" | "late-archive-edit"),
            case == "weak-strict",
        );
        fs::remove_file(root.path().join("pack/resourcepacks/a.zip")).unwrap();
        match case {
            "missing" => fs::remove_file(root.path().join("sources/payload")).unwrap(),
            "directory" => {
                fs::remove_file(root.path().join("sources/payload")).unwrap();
                fs::create_dir(root.path().join("sources/payload")).unwrap();
            }
            "changed" => put(root.path(), "sources/payload", b"changed"),
            "archive-corruption" => {
                let path = root.path().join("sources/archive.zip");
                let mut bytes = fs::read(&path).unwrap();
                let offset = bytes
                    .windows(7)
                    .position(|bytes| bytes == b"payload")
                    .unwrap();
                // The first payload occurrence is in the filename; corrupt the actual stored body.
                let offset = bytes[offset + 7..]
                    .windows(7)
                    .position(|bytes| bytes == b"payload")
                    .unwrap()
                    + offset
                    + 7;
                bytes[offset] ^= 1;
                fs::write(path, bytes).unwrap();
            }
            _ => {}
        }
        let (engine, governor) = engine(state.path().join("state"));
        let request = if case == "weak-strict" {
            SyncRequest::Recorded {
                resolution: None,
                evidence: SourceEvidencePolicy::StrongSourceRequired,
            }
        } else {
            recorded()
        };
        if matches!(case, "late-source-edit" | "late-archive-edit") {
            let Preparation::Ready(prepared) = engine
                .prepare(root.path().to_path_buf(), request)
                .await
                .unwrap()
            else {
                panic!("ready")
            };
            let permission = grant(&prepared);
            put(
                root.path(),
                if case == "late-archive-edit" {
                    "sources/archive.zip"
                } else {
                    "sources/payload"
                },
                b"changed after preview",
            );
            let before = files(root.path());
            let mut operation = engine
                .start(prepared.authorize(permission).unwrap())
                .unwrap();
            let outcome = operation.wait().await;
            assert!(matches!(
                &*outcome,
                OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(_))
            ));
            assert_eq!(files(root.path()), before);
            engine.release_completed(operation.id());
            drop(outcome);
            drop(operation);
        } else {
            let before = files(root.path());
            let error = engine
                .prepare(root.path().to_path_buf(), request)
                .await
                .err()
                .expect(case);
            assert!(!format!("{error:#}").is_empty());
            assert_eq!(files(root.path()), before, "{case}");
        }
        engine.shutdown().await;
        assert_eq!(
            governor.status().reserved,
            ResourceRequest::default(),
            "{case}"
        );
    }
}
#[tokio::test]
async fn recorded_sync_reuses_verified_members_when_archive_is_absent() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source_project(root.path(), true, false);
    fs::remove_file(root.path().join("sources/archive.zip")).unwrap();
    let before = files(root.path());
    let (engine, governor) = engine(state.path().join("state"));
    let view = engine
        .preview(root.path().to_path_buf(), recorded())
        .await
        .unwrap();
    assert!(view.sync().unwrap().files.changes().is_empty());
    drop(view);
    assert_eq!(files(root.path()), before);
    for name in ["a.zip", "copy.zip", "b.zip"] {
        fs::remove_file(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap();
    }
    assert!(
        engine
            .prepare(root.path().to_path_buf(), recorded())
            .await
            .is_err()
    );
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[cfg(unix)]
#[tokio::test]
async fn recorded_sync_rejects_linked_source_ancestors() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source_project(root.path(), false, false);
    fs::rename(
        root.path().join("sources"),
        root.path().join("original-sources"),
    )
    .unwrap();
    put(outside.path(), "payload", b"payload");
    std::os::unix::fs::symlink(outside.path(), root.path().join("sources")).unwrap();
    let before = files(root.path());
    let (engine, _) = engine(state.path().join("state"));
    assert!(
        engine
            .prepare(root.path().to_path_buf(), recorded())
            .await
            .is_err()
    );
    engine.shutdown().await;
    assert_eq!(files(root.path()), before);
    assert_eq!(
        fs::read(outside.path().join("payload")).unwrap(),
        b"payload"
    );
}

#[tokio::test]
async fn recorded_sync_uses_a_later_verified_placement() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source_project(root.path(), true, false);
    fs::remove_file(root.path().join("sources/archive.zip")).unwrap();
    put(root.path(), "pack/resourcepacks/a.zip", b"damaged");
    let before = files(root.path());
    let (engine, _) = engine(state.path().join("state"));
    let Preparation::Ready(prepared) = engine
        .prepare(root.path().to_path_buf(), recorded())
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(files(root.path()), before);
    let permission = grant(&prepared);
    let mut operation = engine
        .start(prepared.authorize(permission).unwrap())
        .unwrap();
    let outcome = operation.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Sync(_)))
    ));
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"payload"
    );
    engine.release_completed(operation.id());
    drop(outcome);
    drop(operation);
    engine.shutdown().await;
}

#[tokio::test]
async fn recorded_sync_does_not_reserve_copies_of_unselected_archive_bytes() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let resolved = source_project(root.path(), true, false);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("inside/payload", b"payload".to_vec()),
        ("unselected", vec![1; 512 << 10]),
    ] {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    put(
        root.path(),
        "sources/archive.zip",
        &writer.finish().unwrap().into_inner(),
    );
    fs::copy(
        root.path().join("sources/archive.zip"),
        root.path().join("sources/second.zip"),
    )
    .unwrap();
    let mut lock = resolved.lock().clone();
    let dependency = lock.dependencies.values_mut().next().unwrap();
    dependency.files = NonEmpty::new(
        dependency
            .files
            .as_slice()
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, mut file)| {
                if index == 1 {
                    file.acquisition = AcquisitionSpec::Embedded {
                        archive: super::tests::path("sources/second.zip"),
                        member: super::tests::path("inside/payload"),
                    };
                }
                file.expected.size = None; // Estimate selected member sizes, not the maximum file allowance.
                file
            })
            .collect(),
    )
    .unwrap();
    let resolved = ResolvedProject::validate(
        resolved.intent().clone(),
        lock,
        resolved.lock().intent_revision,
    )
    .unwrap();
    put(
        root.path(),
        "empack.lock",
        &DocumentCodec.encode_lock(&resolved).unwrap(),
    );
    let (original, _) = engine(state.path().join("state"));
    let mut config = original.config.clone();
    original.shutdown().await;
    config.resources.capture.scratch_bytes = 0;
    config.resources.local_acquisition.scratch_bytes = 0;
    config.resources.assembly.scratch_bytes = 0;
    config.resources.acquired.scratch_bytes = 0;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 128 << 20,
        scratch_bytes: 128 << 10,
        open_files: 128,
    });
    let engine = Engine::new(config, governor.clone()).unwrap();
    let before = files(root.path());
    let preview = engine
        .preview(root.path().to_path_buf(), recorded())
        .await
        .unwrap();
    assert!(preview.sync().unwrap().files.changes().is_empty());
    assert_eq!(files(root.path()), before);
    drop(preview);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn recorded_local_sync_does_not_reserve_an_unused_archive_parser() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    source_project(root.path(), false, false);
    let (original, _) = engine(state.path().join("state"));
    let mut config = original.config.clone();
    original.shutdown().await;
    config.archive.entries = 100_000;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: 128 << 20,
        scratch_bytes: 64 << 20,
        open_files: 128,
    });
    let engine = Engine::new(config, governor.clone()).unwrap();
    let preview = engine
        .preview(root.path().to_path_buf(), recorded())
        .await
        .unwrap();
    assert!(preview.sync().unwrap().files.changes().is_empty());
    drop(preview);
    engine.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn acquired_references_cannot_override_captured_local_sources_or_unknown_slots() {
    use crate::engine::{
        content::{InitialObservation, verify_stream},
        mrpack::{AcquiredBuildFile, LockedFileKey},
    };
    for unknown in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let project = source_project(root.path(), false, false);
        let (key, dependency) = project.lock().dependencies.first_key_value().unwrap();
        let file = &dependency.files.as_slice()[0];
        let content = verify_stream(
            &mut b"payload".as_slice(),
            &file.expected,
            100,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &crate::application::process_runtime::Cancellation::default(),
        )
        .unwrap();
        let supplied = BTreeMap::from([(
            LockedFileKey {
                dependency: if unknown {
                    DependencyKey::parse("unknown").unwrap()
                } else {
                    key.clone()
                },
                slot: file.slot.clone(),
            },
            AcquiredBuildFile {
                content,
                permissions: empack_core::files::FilePermissions {
                    readonly: false,
                    executable: false,
                },
            },
        )]);
        let before = files(root.path());
        let (engine, governor) = engine(state.path().join("state"));
        let error = engine
            .preview(
                root.path().to_path_buf(),
                SyncRequest::AcquiredReferences {
                    resolution: None,
                    evidence: SourceEvidencePolicy::Compatibility,
                    content: supplied,
                },
            )
            .await
            .err()
            .expect("invalid reference accepted");
        assert!(
            format!("{error:#}").contains("recorded remote reference"),
            "{error:#}"
        );
        assert_eq!(files(root.path()), before);
        engine.shutdown().await;
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}
