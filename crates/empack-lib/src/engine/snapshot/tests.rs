use super::*;
use std::fs;
use tempfile::tempdir;

fn path(input: &str) -> PortableRelPath {
    PortableRelPath::parse(input, PathSyntax::ProjectContent).unwrap()
}
fn capture(root: &ProjectReadRoot, paths: &[&str]) -> NativeSnapshot {
    root.capture(
        &paths.iter().map(|p| path(p)).collect::<Vec<_>>(),
        SnapshotLimits::default(),
        &Cancellation::default(),
    )
    .unwrap()
}

#[test]
fn persistent_fingerprint_keeps_object_evidence_without_boot_local_devices() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("pack/config")).unwrap();
    fs::create_dir(temp.path().join("directory")).unwrap();
    fs::write(temp.path().join("pack/config/file"), b"same bytes").unwrap();
    fs::write(temp.path().join("other"), b"same bytes").unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    let scopes = ["pack/config/file", "other", "absent", "directory"];
    let mut snapshot = capture(&root, &scopes);
    let persistent = snapshot.fingerprint().unwrap();
    snapshot.root.volume ^= 1;
    for observed in snapshot.entries.values_mut() {
        match observed {
            Observation::File(file) => file.object.volume ^= 1,
            Observation::Ancestor(binding) | Observation::Directory { binding, .. } => {
                binding.0.volume ^= 1
            }
            Observation::Absent => (),
        }
    }
    assert_eq!(snapshot.fingerprint().unwrap(), persistent);
    // Durable comparison never grants permission to reuse a stale live observation.
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
    snapshot.root.volume ^= 1;
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
    let mut snapshot = capture(&root, &scopes);
    let Observation::File(other) = &snapshot.entries[&path("other")] else {
        unreachable!()
    };
    let other_identity = other.object.persistent;
    let Observation::File(selected) = snapshot.entries.get_mut(&path("pack/config/file")).unwrap()
    else {
        unreachable!()
    };
    assert_ne!(selected.object.persistent, other_identity);
    selected.object.persistent = other_identity;
    assert_ne!(snapshot.fingerprint().unwrap(), persistent);
    // Unsupported identity must not quietly fall back to the device-based fingerprint.
    snapshot.root.persistent = None;
    assert!(
        snapshot
            .fingerprint()
            .unwrap_err()
            .to_string()
            .contains("Filesystem lacks persistent object identity")
    );
}

#[test]
fn raw_edits_absence_and_membership_invalidate_snapshots() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("pack")).unwrap();
    fs::write(temp.path().join("empack.yml"), "# author comment\n").unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    let snapshot = capture(&root, &["empack.yml", "pack", "empack.lock"]);
    root.revalidate(&snapshot, &Cancellation::default())
        .unwrap();
    fs::write(temp.path().join("empack.yml"), "# edited comment\n").unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
    let snapshot = capture(&root, &["pack", "empack.lock"]);
    fs::write(temp.path().join("empack.lock"), "occupied").unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
    let snapshot = capture(&root, &["pack"]);
    fs::write(temp.path().join("pack/new.txt"), "new member").unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn replaced_roots_cannot_reuse_original_observations() {
    let temp = tempdir().unwrap();
    let selected = temp.path().join("project");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("empack.yml"), "original").unwrap();
    let root = ProjectReadRoot::open(&selected).unwrap();
    let snapshot = capture(&root, &["empack.yml"]);
    fs::rename(&selected, temp.path().join("moved")).unwrap();
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("empack.yml"), "original").unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .unwrap_err()
            .to_string()
            .contains("root was replaced")
    );
}

#[test]
fn capture_is_scoped_and_byte_entry_depth_limited() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("pack")).unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    fs::write(temp.path().join("pack/input"), b"12345").unwrap();
    fs::write(temp.path().join(".git/ignored"), vec![0; 1000]).unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    let scopes = [path("pack")];
    let limits = SnapshotLimits {
        file_bytes: 5,
        total_bytes: 5,
        entries: 2,
        depth: 2,
    };
    assert_eq!(
        root.capture(&scopes, limits, &Cancellation::default())
            .unwrap()
            .entries()
            .len(),
        2
    );
    for limits in [
        SnapshotLimits {
            file_bytes: 4,
            ..limits
        },
        SnapshotLimits {
            total_bytes: 4,
            ..limits
        },
        SnapshotLimits {
            entries: 1,
            ..limits
        },
        SnapshotLimits { depth: 1, ..limits },
    ] {
        assert!(
            root.capture(&scopes, limits, &Cancellation::default())
                .is_err()
        );
    }
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(root.capture(&scopes, limits, &cancelled).is_err());
    assert!(
        root.capture(
            &[path("pack"), path("pack/input")],
            limits,
            &Cancellation::default()
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn links_and_special_files_are_rejected_even_when_leaf_is_absent() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("sentinel"), "unchanged").unwrap();
    fs::create_dir(temp.path().join("pack")).unwrap();
    symlink(outside.path(), temp.path().join("pack/linked")).unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    for selected in [
        "pack/linked/sentinel",
        "pack/linked/missing",
        "pack/linked",
        "pack",
    ] {
        assert!(
            root.capture(
                &[path(selected)],
                SnapshotLimits::default(),
                &Cancellation::default()
            )
            .is_err(),
            "{selected}"
        );
    }
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
    assert!(!outside.path().join("missing").exists());
    let socket = std::os::unix::net::UnixListener::bind(temp.path().join("socket")).unwrap();
    assert!(
        root.capture(
            &[path("socket")],
            SnapshotLimits::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    drop(socket);
}

#[test]
fn identity_changes_are_detected_even_when_bytes_are_identical() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("input"), b"same").unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    let snapshot = capture(&root, &["input"]);
    fs::write(temp.path().join("replacement"), b"same").unwrap();
    // Keep the original inode alive to prevent immediate native identifier reuse.
    fs::rename(temp.path().join("input"), temp.path().join("original")).unwrap();
    fs::rename(temp.path().join("replacement"), temp.path().join("input")).unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
}

#[test]
fn absence_binds_missing_ancestors_without_reading_unrelated_siblings() {
    let temp = tempdir().unwrap();
    let root = ProjectReadRoot::open(temp.path()).unwrap();
    let snapshot = capture(&root, &["pack/config/missing"]);
    fs::create_dir(temp.path().join("pack")).unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
    let snapshot = capture(&root, &["pack/config/missing"]);
    fs::write(temp.path().join("pack/unrelated"), b"user data").unwrap();
    root.revalidate(&snapshot, &Cancellation::default())
        .unwrap();
    fs::rename(temp.path().join("pack"), temp.path().join("old-pack")).unwrap();
    fs::create_dir(temp.path().join("pack")).unwrap();
    assert!(
        root.revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn retained_windows_roots_prevent_replacement_and_reopened_roots_reject_old_snapshots() {
    let temp = tempdir().unwrap();
    let selected = temp.path().join("selected");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("empack.yml"), "original").unwrap();
    let root = ProjectReadRoot::open(&selected).unwrap();
    let snapshot = capture(&root, &["empack.yml"]);
    let moved = temp.path().join("moved");
    // cap-std omits FILE_SHARE_DELETE for directory capabilities on Windows.
    let error = fs::rename(&selected, &moved).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(32));
    root.revalidate(&snapshot, &Cancellation::default())
        .unwrap();
    drop(root);
    fs::rename(&selected, &moved).unwrap();
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("empack.yml"), "original").unwrap();
    let replacement = ProjectReadRoot::open(&selected).unwrap();
    assert!(
        replacement
            .revalidate(&snapshot, &Cancellation::default())
            .is_err()
    );
}

#[test]
fn filtered_capture_preserves_explicit_inputs_inside_ignored_directories() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("pack/ignored")).unwrap();
    std::fs::write(root.path().join("pack/ignored/required.bin"), b"kept").unwrap();
    std::fs::write(root.path().join("pack/ignored/large.bin"), vec![0; 1024]).unwrap();
    let path = |value| PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap();
    let required = path("pack/ignored/required.bin");
    let filter = super::super::source::CaptureFilter::author(
        &["ignored/".into()],
        std::slice::from_ref(&required),
    )
    .unwrap();
    let reader = ProjectReadRoot::open(root.path()).unwrap();
    let cancel = Cancellation::default();
    let snapshot = reader
        .capture_filtered(
            &[path("pack")],
            SnapshotLimits {
                file_bytes: 8,
                total_bytes: 8,
                ..SnapshotLimits::default()
            },
            Some(&filter),
            &cancel,
        )
        .unwrap();
    assert!(matches!(
        snapshot.entries().get(&required),
        Some(Observation::File(_))
    ));
    assert!(
        !snapshot
            .entries()
            .contains_key(&path("pack/ignored/large.bin"))
    );
    std::fs::write(root.path().join("pack/ignored/large.bin"), vec![1; 2048]).unwrap();
    reader.revalidate(&snapshot, &cancel).unwrap();
    std::fs::write(root.path().join("pack/ignored/required.bin"), b"edit").unwrap();
    assert!(reader.revalidate(&snapshot, &cancel).is_err());
}
