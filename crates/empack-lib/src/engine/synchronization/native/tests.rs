use super::*;
use crate::engine::{
    content::verify_stream, documents::DocumentCodec, mrpack::tests::project,
    project::ProjectReader, publication::RecoveryReader, snapshot::SnapshotLimits,
};
use empack_core::files::FilePermissions;
use std::{fs, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn fixture(root: &Path) -> ResolvedProject {
    let project = project(false, false);
    let mut intent = b"# keep comments\n".to_vec();
    intent.extend(DocumentCodec.encode_intent(project.intent()).unwrap());
    put(root, "empack.yml", &intent);
    let mut lock = b" \n".to_vec();
    lock.extend(DocumentCodec.encode_lock(&project).unwrap());
    put(root, "empack.lock", &lock);
    put(root, "pack/resourcepacks/a.zip", b"edited bytes");
    put(root, "pack/unlisted.bin", b"retain");
    project
}
fn acquired(project: &ResolvedProject, wrong: bool) -> BTreeMap<LockedFileKey, AcquiredBuildFile> {
    let bytes = if wrong { b"bad".as_slice() } else { b"payload" };
    let content = verify_stream(
        &mut &bytes[..],
        &empack_core::model::ExpectedContent {
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
    project
        .lock()
        .dependencies
        .iter()
        .flat_map(|(key, dep)| {
            dep.files.as_slice().iter().map(|file| {
                (
                    LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    },
                    AcquiredBuildFile {
                        content: content.clone(),
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                )
            })
        })
        .collect()
}
fn prepare(
    root: &Path,
    state: &Path,
    project: &ResolvedProject,
    wrong: bool,
) -> Result<PreparedSynchronization> {
    let cancel = Cancellation::default();
    let snapshot = ProjectReader::new(RecoveryReader::new(state.join("state")))
        .capture_synchronization(root, SnapshotLimits::default(), &cancel)?;
    prepare_synchronization(snapshot, acquired(project, wrong), &cancel)
}
#[test]
fn restores_modified_and_missing_locked_files_then_syncs_without_changes() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let lock = fs::read(root.path().join("empack.lock")).unwrap();
    let prepared = prepare(root.path(), state.path(), &project, false).unwrap();
    assert_eq!(prepared.files().changes().len(), 3);
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"edited bytes"
    );
    let publisher = Publisher::open(&state.path().join("state")).unwrap();
    prepared
        .publish(&publisher, &Cancellation::default())
        .unwrap();
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert_eq!(
            fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
            b"payload"
        );
    }
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert_eq!(fs::read(root.path().join("empack.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(root.path().join("pack/unlisted.bin")).unwrap(),
        b"retain"
    );
    let unchanged = prepare(root.path(), state.path(), &project, false).unwrap();
    assert!(unchanged.files().changes().is_empty());
    assert!(
        unchanged.files().expected().is_empty(),
        "unchanged payloads must not require private staging"
    );
    put(
        root.path(),
        "pack/resourcepacks/a.zip",
        b"changed after no-op preview",
    );
    assert!(
        unchanged
            .publish(&publisher, &Cancellation::default())
            .is_err()
    );
}
#[test]
fn invalid_content_directory_targets_and_late_changes_cannot_partially_publish() {
    for case in ["wrong", "directory", "late"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let project = fixture(root.path());
        let before = fs::read(root.path().join("empack.yml")).unwrap();
        if case == "directory" {
            put(root.path(), "pack/resourcepacks/b.zip/sentinel", b"keep");
        }
        let prepared = prepare(root.path(), state.path(), &project, case == "wrong");
        if case == "late" {
            put(
                root.path(),
                "pack/resourcepacks/a.zip",
                b"changed after preview",
            );
            assert!(
                prepared
                    .unwrap()
                    .publish(
                        &Publisher::open(&state.path().join("state")).unwrap(),
                        &Cancellation::default()
                    )
                    .is_err()
            );
        } else {
            assert!(prepared.is_err(), "{case}");
        }
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
        assert!(!root.path().join("pack/resourcepacks/copy.zip").exists());
    }
}
#[test]
fn metadata_edits_rebind_documents_and_backend_drift_retires_only_selected_records() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let mut intent = project.intent().clone();
    intent.metadata.name = "Updated title".into();
    put(
        root.path(),
        "empack.yml",
        &DocumentCodec.encode_intent(&intent).unwrap(),
    );
    let metadata = "filename = 'a.zip'\nside = 'both'\n[download]\nurl = 'https://example.com/old'\nhash-format = 'sha256'\nhash = '0000000000000000000000000000000000000000000000000000000000000000'\n";
    put(
        root.path(),
        "pack/resourcepacks/stale.pw.toml",
        metadata.as_bytes(),
    );
    let untouched = metadata.replace("a.zip", "unrelated.zip");
    put(
        root.path(),
        "pack/resourcepacks/unrelated.pw.toml",
        untouched.as_bytes(),
    );
    put(root.path(),"pack/index.toml",b"hash-format = 'sha256'\n[[files]]\nfile = 'resourcepacks/stale.pw.toml'\nmetafile = true\n[[files]]\nfile = 'resourcepacks/unrelated.pw.toml'\nmetafile = true\n");
    put(
        root.path(),
        "pack/pack.toml",
        b"name = 'old title'\n[index]\nfile = 'index.toml'\n",
    );
    let prepared = prepare(root.path(), state.path(), &project, false).unwrap();
    assert_eq!(
        prepared.candidate().project().intent().metadata.name,
        "Updated title"
    );
    let receipt = prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(
        receipt.project.lock().dependencies,
        project.lock().dependencies
    );
    assert!(
        !root
            .path()
            .join("pack/resourcepacks/stale.pw.toml")
            .exists()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/unrelated.pw.toml")).unwrap(),
        untouched.as_bytes()
    );
    let index: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pack/index.toml")).unwrap()).unwrap();
    assert_eq!(index["files"].as_array().unwrap().len(), 1);
    let pack: toml::Value =
        toml::from_str(&fs::read_to_string(root.path().join("pack/pack.toml")).unwrap()).unwrap();
    assert_eq!(pack["name"].as_str(), Some("Updated title"));
    assert_eq!(pack["versions"]["minecraft"].as_str(), Some("1.20.1"));
    assert_eq!(pack["versions"]["fabric"].as_str(), Some("0.16.0"));

    assert!(
        prepare(root.path(), state.path(), &project, false)
            .unwrap()
            .files()
            .changes()
            .is_empty()
    );
}
#[cfg(unix)]
#[test]
fn selected_links_fail_while_unrelated_links_are_retained() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    put(outside.path(), "sentinel", b"outside");
    std::os::unix::fs::symlink(outside.path(), root.path().join("pack/unrelated-link")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("sentinel"),
        root.path().join("pack/unrelated.pw.toml"),
    )
    .unwrap();
    assert!(prepare(root.path(), state.path(), &project, false).is_ok());
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside"
    );
    std::os::unix::fs::symlink(
        outside.path().join("sentinel"),
        root.path().join("pack/resourcepacks/b.zip"),
    )
    .unwrap();
    assert!(prepare(root.path(), state.path(), &project, false).is_err());
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside"
    );
}

#[test]
fn unrelated_uninterpretable_metadata_does_not_block_recorded_restoration() {
    for directory in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let project = fixture(root.path());
        if directory {
            put(root.path(), "pack/unrelated.pw.toml/sentinel", b"keep");
        } else {
            put(root.path(), "pack/unrelated.pw.toml", b"malformed = [");
        }
        let prepared = prepare(root.path(), state.path(), &project, false).unwrap();
        prepared
            .publish(
                &Publisher::open(&state.path().join("state")).unwrap(),
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
            b"payload"
        );
        if directory {
            assert_eq!(
                fs::read(root.path().join("pack/unrelated.pw.toml/sentinel")).unwrap(),
                b"keep"
            );
        } else {
            assert_eq!(
                fs::read(root.path().join("pack/unrelated.pw.toml")).unwrap(),
                b"malformed = ["
            );
        }
    }
}

#[test]
fn selected_metadata_changes_after_preview_refuse_publication() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let path = "pack/resourcepacks/selected.pw.toml";
    let metadata = "filename = 'a.zip'\nside = 'both'\n[download]\nurl = 'https://example.com/a'\nhash-format = 'sha256'\nhash = '0000000000000000000000000000000000000000000000000000000000000000'\n";
    put(root.path(), path, metadata.as_bytes());
    let prepared = prepare(root.path(), state.path(), &project, false).unwrap();
    put(
        root.path(),
        path,
        metadata.replace("a.zip", "unrelated.zip").as_bytes(),
    );
    assert!(
        prepared
            .publish(
                &Publisher::open(&state.path().join("state")).unwrap(),
                &Cancellation::default()
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"edited bytes"
    );
    assert!(root.path().join(path).exists());
}

#[test]
fn metadata_discovery_remains_bounded_and_cancellable() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    for n in 0..12 {
        put(
            root.path(),
            &format!("pack/unrelated/{n}.pw.toml"),
            b"invalid = [",
        );
    }
    let reader = ProjectReader::new(RecoveryReader::new(state.path().join("state")));
    let error = reader
        .capture_synchronization(
            root.path(),
            SnapshotLimits {
                entries: 10,
                ..SnapshotLimits::default()
            },
            &Cancellation::default(),
        )
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("Metadata discovery exceeds entry limit"),
        "{error:#}"
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        reader
            .capture_synchronization(root.path(), SnapshotLimits::default(), &cancel)
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("pack/resourcepacks/a.zip")).unwrap(),
        b"edited bytes"
    );
}
