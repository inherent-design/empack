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

#[test]
fn packed_trees_bound_descriptors_and_keep_member_ranges_independent() {
    let cancel = Cancellation::default();
    let mut stage = MutableStage::empty().unwrap();
    for index in 0..600 {
        let bytes = format!("member-{index}");
        stage
            .write(
                &path(&format!("files/{index:04}")),
                &mut bytes.as_bytes(),
                32,
                &cancel,
            )
            .unwrap();
    }
    stage
        .write(&path("empty"), &mut b"".as_slice(), 0, &cancel)
        .unwrap();
    let readonly = stage.storage.path().join("files/0300");
    let mut permissions = fs::metadata(&readonly).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&readonly, permissions).unwrap();
    let original = stage.storage.path().to_owned();
    let mut frozen = stage.freeze(SnapshotLimits::default(), &cancel).unwrap();
    assert!(frozen.packed.is_some());
    assert!(
        !original.exists(),
        "the unpacked duplicate must retire before subsequent copies"
    );
    for index in [599, 0, 300, 1] {
        let member = path(&format!("files/{index:04}"));
        let mut bytes = Vec::new();
        frozen.copy_verified(&member, &mut bytes, &cancel).unwrap();
        assert_eq!(bytes, format!("member-{index}").as_bytes());
        let mut probe = [0; 64];
        assert_eq!(
            frozen
                .read_at(&member, bytes.len() as u64, &mut probe)
                .unwrap(),
            0
        );
        assert_eq!(frozen.read_at(&member, u64::MAX, &mut probe).unwrap(), 0);
        let count = frozen.read_at(&member, 2, &mut probe).unwrap();
        assert_eq!(&probe[..count], &bytes[2..]);
    }
    let mut empty = Vec::new();
    frozen
        .copy_verified(&path("empty"), &mut empty, &cancel)
        .unwrap();
    assert!(empty.is_empty());
    frozen.retire_input(&path("files/0000")).unwrap();
    assert!(
        frozen
            .copy_verified(&path("files/0000"), &mut Vec::new(), &cancel)
            .is_err()
    );
    // Only the test can access the backing writer; consumers receive bounded readers.
    frozen.packed.as_mut().unwrap().file().rewind().unwrap();
    frozen
        .packed
        .as_mut()
        .unwrap()
        .file()
        .write_all(b"corrupt!")
        .unwrap();
    assert!(
        frozen
            .copy_verified(&path("files/0001"), &mut Vec::new(), &cancel)
            .is_ok()
    );
    // The first nonempty member starts at zero, even though an empty member sorts before it.
    let mut affected = MutableStage::empty().unwrap();
    affected
        .write(&path("a"), &mut b"first".as_slice(), 5, &cancel)
        .unwrap();
    affected
        .write(&path("b"), &mut b"next".as_slice(), 4, &cancel)
        .unwrap();
    let mut affected = affected.freeze(SnapshotLimits::default(), &cancel).unwrap();
    affected.packed.as_mut().unwrap().file().rewind().unwrap();
    affected
        .packed
        .as_mut()
        .unwrap()
        .file()
        .write_all(b"wrong")
        .unwrap();
    assert!(
        affected
            .copy_verified(&path("a"), &mut Vec::new(), &cancel)
            .is_err()
    );
    let mut intact = Vec::new();
    affected
        .copy_verified(&path("b"), &mut intact, &cancel)
        .unwrap();
    assert_eq!(intact, b"next");
}
