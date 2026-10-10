use super::*;
use crate::engine::{
    documents::DocumentCodec, mrpack::tests::project, project::ProjectReader,
    publication::RecoveryReader, snapshot::SnapshotLimits,
};
use std::{fs, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let destination = root.join(name);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, bytes).unwrap();
}
fn fixture(root: &Path) {
    let project = project(false, false);
    put(
        root,
        "empack.yml",
        &DocumentCodec.encode_intent(project.intent()).unwrap(),
    );
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    for name in ["a.zip", "b.zip", "copy.zip"] {
        put(root, &format!("pack/resourcepacks/{name}"), b"payload");
    }
    put(root, "pack/resourcepacks/unrequested.zip", b"keep");
    put(root, "templates/client/custom", b"user template");
    put(root, "dist/previous.zip", b"published distribution");
    put(root, "original-download.zip", b"source");
}
fn selected() -> NonEmpty<DependencyKey> {
    NonEmpty::new(vec![DependencyKey::parse("assets").unwrap()]).unwrap()
}
fn prepare(root: &Path, state: &Path, mode: RemovalMode) -> Result<PreparedRemoval> {
    let cancel = Cancellation::default();
    let snapshot = ProjectReader::new(RecoveryReader::new(state.join("state"))).capture_mutation(
        root,
        SnapshotLimits::default(),
        &cancel,
    )?;
    prepare_removal_with_policy(
        snapshot,
        &selected(),
        mode,
        RemovalEvidencePolicy::AcknowledgeUnknown,
        &cancel,
    )
}
fn kept(root: &Path) {
    for (name, bytes) in [
        ("pack/resourcepacks/unrequested.zip", b"keep".as_slice()),
        ("templates/client/custom", b"user template"),
        ("dist/previous.zip", b"published distribution"),
        ("original-download.zip", b"source"),
    ] {
        assert_eq!(fs::read(root.join(name)).unwrap(), bytes);
    }
}
#[test]
fn exact_removal_publishes_documents_and_files_without_collecting_unrequested_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let before = fs::read(root.path().join("empack.yml")).unwrap();
    let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
    assert_eq!(prepared.files().changes().len(), 5);
    assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
    assert!(!state.path().join("state").exists());
    let receipt = prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(receipt.project.lock().dependencies.is_empty());
    assert!(receipt.project.intent().roots.is_empty());
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert!(
            !root
                .path()
                .join(format!("pack/resourcepacks/{name}"))
                .exists()
        );
    }
    kept(root.path());
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.path().join("empack.yml")).unwrap(), "result")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.path().join("empack.lock")).unwrap(),
            &intent,
            "result",
        )
        .unwrap();
}
#[test]
fn changed_bytes_and_directory_targets_reject_the_entire_removal() {
    for directory in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let before = fs::read(root.path().join("empack.yml")).unwrap();
        let target = root.path().join("pack/resourcepacks/a.zip");
        fs::remove_file(&target).unwrap();
        if directory {
            put(&target, "sentinel", b"keep directory");
        } else {
            fs::write(&target, b"edited").unwrap();
        }
        let error = prepare(root.path(), state.path(), RemovalMode::RemoveContent)
            .err()
            .unwrap();
        let diagnostic = format!("{error:#}");
        assert!(
            if directory {
                diagnostic.contains("directory")
            } else {
                diagnostic.contains("differs")
            },
            "{diagnostic}"
        );
        assert_eq!(fs::read(root.path().join("empack.yml")).unwrap(), before);
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
            b"payload"
        );
        if directory {
            assert_eq!(
                fs::read(target.join("sentinel")).unwrap(),
                b"keep directory"
            );
        }
        kept(root.path());
    }
}
#[test]
fn document_and_payload_edits_after_preparation_prevent_all_publication() {
    for name in ["empack.yml", "pack/resourcepacks/a.zip"] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fixture(root.path());
        let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
        let mut edited = fs::read(root.path().join(name)).unwrap();
        edited.extend(b"\n# user edit");
        put(root.path(), name, &edited);
        assert!(
            prepared
                .publish(
                    &Publisher::open(&state.path().join("state")).unwrap(),
                    &Cancellation::default()
                )
                .is_err()
        );
        assert_eq!(fs::read(root.path().join(name)).unwrap(), edited);
        assert_eq!(
            fs::read(root.path().join("pack/resourcepacks/b.zip")).unwrap(),
            b"payload"
        );
        kept(root.path());
    }
}
#[test]
fn explicit_root_demotion_publishes_only_documents_and_retains_exact_files() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let prepared = prepare(root.path(), state.path(), RemovalMode::ForgetRoots).unwrap();
    assert!(prepared.files().changes().iter().all(|change| matches!(
        change.target(),
        ManagedPath::IntentDocument | ManagedPath::LockDocument
    )));
    let receipt = prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(receipt.mode, RemovalMode::ForgetRoots);
    assert!(receipt.project.intent().roots.is_empty());
    assert_eq!(receipt.project.lock().dependencies.len(), 1);
    for name in ["a.zip", "b.zip", "copy.zip"] {
        assert_eq!(
            fs::read(root.path().join(format!("pack/resourcepacks/{name}"))).unwrap(),
            b"payload"
        );
    }
    kept(root.path());
}
#[test]
fn ignored_locked_placements_remain_observed_removal_targets() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(root.path(), "pack/.packwizignore", b"**\n");
    let prepared = prepare(root.path(), state.path(), RemovalMode::RemoveContent).unwrap();
    assert_eq!(prepared.files().changes().len(), 5);
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    kept(root.path());
    assert_eq!(
        fs::read(root.path().join("pack/.packwizignore")).unwrap(),
        b"**\n"
    );
}
#[cfg(unix)]
#[test]
fn removal_refuses_linked_ancestors_without_deleting_outside_content() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::rename(
        root.path().join("pack/resourcepacks"),
        outside.path().join("content"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("content"),
        root.path().join("pack/resourcepacks"),
    )
    .unwrap();
    let error = prepare(root.path(), state.path(), RemovalMode::RemoveContent)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("link"), "{error:#}");
    assert_eq!(
        fs::read(outside.path().join("content/a.zip")).unwrap(),
        b"payload"
    );
}
#[test]
fn foreign_metadata_neither_authorizes_nor_blocks_native_removal() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fixture(root.path());
    let names = [
        "pack/resourcepacks/assets.pw.toml",
        "pack/pack.toml",
        "pack/index.toml",
        "pack/.packwizignore",
    ];
    for name in names {
        put(root.path(), name, b"invalid foreign data [");
    }
    let cancel = Cancellation::default();
    let selectors = NonEmpty::new(vec![
        RemovalSelector::Query("Assets".into()),
        RemovalSelector::Query("assets".into()),
    ])
    .unwrap();
    let snapshot = ProjectReader::new(RecoveryReader::new(state.path().join("state")))
        .capture_removal(
            root.path(),
            &selectors,
            RemovalMode::RemoveContent,
            RemovalEvidencePolicy::AcknowledgeUnknown,
            SnapshotLimits::default(),
            &cancel,
        )
        .unwrap();
    let prepared = plan_selected_removal(
        snapshot,
        &selectors,
        RemovalMode::RemoveContent,
        RemovalEvidencePolicy::AcknowledgeUnknown,
        &cancel,
    )
    .unwrap()
    .stage(&cancel)
    .unwrap();
    assert_eq!(prepared.candidate().plan().selected().len(), 1);
    prepared
        .publish(
            &Publisher::open(&state.path().join("state")).unwrap(),
            &cancel,
        )
        .unwrap();
    for name in names {
        assert_eq!(
            fs::read(root.path().join(name)).unwrap(),
            b"invalid foreign data ["
        );
    }
    assert!(!root.path().join("pack/resourcepacks/a.zip").exists());
    kept(root.path());
}
