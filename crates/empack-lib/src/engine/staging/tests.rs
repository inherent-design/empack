use super::*;
use std::fs;
fn path(input: &str) -> PortableRelPath {
    PortableRelPath::parse(input, PathSyntax::ProjectContent).unwrap()
}

#[test]
fn copied_hardlinks_cannot_mutate_the_original_project_or_outside_alias() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("original"), b"source").unwrap();
    fs::create_dir(project.path().join("pack")).unwrap();
    fs::hard_link(
        outside.path().join("original"),
        project.path().join("pack/file"),
    )
    .unwrap();
    fs::write(project.path().join("unrelated"), b"not staged").unwrap();
    let source = ProjectReadRoot::open(project.path()).unwrap();
    let cancel = Cancellation::default();
    let snapshot = source
        .capture(&[path("pack")], SnapshotLimits::default(), &cancel)
        .unwrap();
    let mut stage = MutableStage::from_snapshot(&source, &snapshot, &cancel).unwrap();
    let (parent, leaf) = native::parent(&stage.root.directory, &path("pack/file")).unwrap();
    let stage_file = native::open_file(&parent, &leaf).unwrap();
    assert_ne!(
        native::identity(&stage_file).unwrap(),
        native::identity(&File::open(outside.path().join("original")).unwrap()).unwrap()
    );
    assert!(!stage.storage.path().join("unrelated").exists());
    stage
        .write(&path("pack/file"), &mut &b"changed"[..], 7, &cancel)
        .unwrap();
    assert_eq!(
        fs::read(outside.path().join("original")).unwrap(),
        b"source"
    );
    assert_eq!(
        fs::read(project.path().join("pack/file")).unwrap(),
        b"source"
    );
    let mut frozen = stage.freeze(SnapshotLimits::default(), &cancel).unwrap();
    let mut bytes = Vec::new();
    frozen
        .copy_verified(&path("pack/file"), &mut bytes, &cancel)
        .unwrap();
    assert_eq!(bytes, b"changed");
}

#[test]
fn failed_write_keeps_previous_candidate_and_cleans_partial_file() {
    let cancel = Cancellation::default();
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&path("file"), &mut &b"before"[..], 6, &cancel)
        .unwrap();
    assert!(
        stage
            .write(&path("file"), &mut &b"too long"[..], 2, &cancel)
            .is_err()
    );
    let mut frozen = stage.freeze(SnapshotLimits::default(), &cancel).unwrap();
    assert_eq!(frozen.inventory().len(), 1);
    let mut bytes = Vec::new();
    frozen
        .copy_verified(&path("file"), &mut bytes, &cancel)
        .unwrap();
    assert_eq!(bytes, b"before");
}

#[test]
fn freeze_retains_original_object_and_rechecks_content() {
    let cancel = Cancellation::default();
    let mut stage = MutableStage::empty().unwrap();
    stage
        .write(&path("file"), &mut &b"before"[..], 6, &cancel)
        .unwrap();
    let selected = stage.storage.path().join("file");
    let mut frozen = stage.freeze(SnapshotLimits::default(), &cancel).unwrap();
    // External pathname replacement cannot substitute unverified bytes after freeze.
    fs::rename(&selected, selected.with_file_name("old")).unwrap();
    fs::write(&selected, b"attacker").unwrap();
    let mut bytes = Vec::new();
    frozen
        .copy_verified(&path("file"), &mut bytes, &cancel)
        .unwrap();
    assert_eq!(bytes, b"before");
    // An external in-place mutation of the retained object must fail content verification.
    fs::write(selected.with_file_name("old"), b"edited").unwrap();
    assert!(
        frozen
            .copy_verified(&path("file"), &mut Vec::new(), &cancel)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn stage_writer_rejects_links_without_touching_outside_bytes() {
    use std::os::unix::fs::symlink;
    let mut stage = MutableStage::empty().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("file"), b"safe").unwrap();
    symlink(outside.path(), stage.storage.path().join("link")).unwrap();
    assert!(
        stage
            .write(
                &path("link/file"),
                &mut &b"bad"[..],
                3,
                &Cancellation::default()
            )
            .is_err()
    );
    assert!(
        stage
            .freeze(SnapshotLimits::default(), &Cancellation::default())
            .is_err()
    );
    assert_eq!(fs::read(outside.path().join("file")).unwrap(), b"safe");
}

#[cfg(unix)]
#[test]
fn stage_directory_is_private_even_with_a_permissive_process_umask() {
    use std::os::unix::fs::PermissionsExt;
    let stage = MutableStage::empty().unwrap();
    assert_eq!(
        fs::metadata(stage.storage.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn abandoned_writers_and_seekable_candidates_release_private_storage() {
    let stage = MutableStage::empty().unwrap();
    let location = stage.storage.path().to_owned();
    drop(stage);
    assert!(!location.exists());
    let mut candidate = PrivateFile::new().unwrap();
    let location = candidate._storage.storage.path().to_owned();
    candidate.file().write_all(b"private bytes").unwrap();
    candidate.file().rewind().unwrap();
    let mut bytes = Vec::new();
    candidate.file().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"private bytes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            candidate.file().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    crate::engine::windows_privacy::verify(&candidate._storage.root.directory).unwrap();
    drop(candidate);
    assert!(!location.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn owned_tool_returns_only_a_retired_private_stage() {
    use super::StageToolArgument;
    use std::{path::Path, time::Duration};
    let stage = MutableStage::empty()
        .unwrap()
        .run_tool(
            Path::new("/bin/sh"),
            &[
                StageToolArgument::Text("-c".into()),
                StageToolArgument::Text("printf retained > output".into()),
            ],
            Duration::from_secs(5),
            Cancellation::default(),
        )
        .await
        .unwrap();
    let mut frozen = stage
        .freeze(SnapshotLimits::default(), &Cancellation::default())
        .unwrap();
    let path = PortableRelPath::parse("output", PathSyntax::ProjectContent).unwrap();
    let mut bytes = Vec::new();
    frozen
        .copy_verified(&path, &mut bytes, &Cancellation::default())
        .unwrap();
    assert_eq!(bytes, b"retained");
    let result = MutableStage::empty()
        .unwrap()
        .run_tool(
            Path::new("/bin/sh"),
            &[
                StageToolArgument::Text("-c".into()),
                StageToolArgument::Text("printf partial > output; sleep 30".into()),
            ],
            Duration::from_millis(100),
            Cancellation::default(),
        )
        .await;
    assert!(result.is_err());
}
