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
        "weak-strict",
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        source_project(
            root.path(),
            case == "archive-corruption",
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
        if case == "late-source-edit" {
            let Preparation::Ready(prepared) = engine
                .prepare(root.path().to_path_buf(), request)
                .await
                .unwrap()
            else {
                panic!("ready")
            };
            let permission = grant(&prepared);
            put(root.path(), "sources/payload", b"changed after preview");
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
