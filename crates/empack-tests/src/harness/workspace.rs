//! Observe owned fixture bytes and attributes, including empty directories.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    /// Directory flag and Unix rwx mode bits, or the non-Unix read-only attribute.
    pub entries: BTreeMap<PathBuf, (bool, u32)>,
}

/// Fixture trees contain ordinary files/directories, not links or special nodes.
/// Access/modification times are excluded: reads and directory traversal can
/// update timestamps without changing authored content or portable attributes.
pub fn snapshot(root: &Path) -> Snapshot {
    snapshot_excluding(root, &[])
}

/// Skip explicitly named host paths before opening them. Exclusions are relative
/// paths compared by whole components; similarly named author files remain visible.
pub fn snapshot_excluding(root: &Path, excluded: &[&str]) -> Snapshot {
    for path in excluded {
        let path = Path::new(path);
        assert!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "snapshot exclusions must name relative entries"
        );
    }
    fn visit(root: &Path, at: &Path, excluded: &[&str], result: &mut Snapshot) {
        let relative = at.strip_prefix(root).unwrap();
        if excluded.iter().any(|name| relative.starts_with(name)) {
            return;
        }
        let metadata = fs::symlink_metadata(at).unwrap();
        assert!(
            metadata.is_file() || metadata.is_dir(),
            "unsupported fixture entry: {}",
            at.display()
        );
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        let relative = relative.to_owned();
        result
            .entries
            .insert(relative.clone(), (metadata.is_dir(), mode));
        if metadata.is_file() {
            // Windows byte locks can deny even an EOF read of an empty lock
            // file. Its observed length already establishes its empty content.
            // Nonempty files still require an actual successful byte read.
            let bytes = if metadata.len() == 0 {
                Vec::new()
            } else {
                fs::read(at).unwrap_or_else(|error| {
                    panic!("cannot snapshot fixture file {}: {error}", at.display())
                })
            };
            result.files.insert(relative, bytes);
        } else {
            for entry in fs::read_dir(at).unwrap() {
                visit(root, &entry.unwrap().path(), excluded, result);
            }
        }
    }
    let mut result = Snapshot {
        files: BTreeMap::new(),
        entries: BTreeMap::new(),
    };
    visit(root, root, excluded, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_detects_bytes_empty_directories_and_permission_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("source.txt");
        fs::write(&file, b"a").unwrap();
        fs::write(root.path().join("empty-file"), b"").unwrap();
        fs::create_dir(root.path().join("empty")).unwrap();
        let before = snapshot(root.path());
        assert!(before.entries.contains_key(Path::new("")));
        assert!(before.entries.contains_key(Path::new("empty")));
        assert_eq!(before.files.len(), 2);
        assert_eq!(before.files[Path::new("empty-file")], b"");
        fs::write(&file, b"b").unwrap();
        assert_ne!(snapshot(root.path()), before);
        fs::write(&file, b"a").unwrap();
        fs::remove_dir(root.path().join("empty")).unwrap();
        assert_ne!(snapshot(root.path()), before);
        fs::create_dir(root.path().join("empty")).unwrap();
        assert_eq!(snapshot(root.path()), before);
        let original = fs::metadata(&file).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&file, readonly).unwrap();
        let changed = snapshot(root.path());
        fs::set_permissions(&file, original).unwrap();
        assert_ne!(changed, before);
        assert_eq!(snapshot(root.path()), before);
    }

    #[test]
    fn snapshots_exclude_only_selected_relative_components() {
        let root = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            (".host-state/private.txt", b"host bytes".as_slice()),
            (".host-state-copy/keep.txt", b"author bytes".as_slice()),
            ("pack/content.txt", b"content".as_slice()),
        ] {
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fs::create_dir(root.path().join("empty")).unwrap();
        let observed = snapshot_excluding(root.path(), &[".host-state"]);
        assert_eq!(
            observed.files,
            BTreeMap::from([
                (
                    PathBuf::from(".host-state-copy/keep.txt"),
                    b"author bytes".to_vec()
                ),
                (PathBuf::from("pack/content.txt"), b"content".to_vec()),
            ])
        );
        assert!(observed.entries.contains_key(Path::new("empty")));
        assert!(!observed.entries.contains_key(Path::new(".host-state")));
        fs::write(
            root.path().join(".host-state/private.txt"),
            b"new host bytes",
        )
        .unwrap();
        fs::create_dir(root.path().join(".host-state/active")).unwrap();
        assert_eq!(snapshot_excluding(root.path(), &[".host-state"]), observed);
        fs::write(root.path().join("pack/content.txt"), b"changed").unwrap();
        assert_ne!(snapshot_excluding(root.path(), &[".host-state"]), observed);
        assert_eq!(snapshot_excluding(root.path(), &[]), snapshot(root.path()));
        for invalid in ["", ".", "/", "../outside", "pack/../outside"] {
            assert!(
                std::panic::catch_unwind(|| snapshot_excluding(root.path(), &[invalid])).is_err()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn snapshot_observes_locked_empty_files_but_refuses_unreadable_content() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("held.lock");
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.try_lock().unwrap();
        assert_eq!(fs::read(&path).unwrap_err().raw_os_error(), Some(33));
        assert_eq!(snapshot(root.path()).files[Path::new("held.lock")], b"");
        file.write_all(b"private content").unwrap();
        file.sync_all().unwrap();
        assert_eq!(fs::read(&path).unwrap_err().raw_os_error(), Some(33));
        assert!(std::panic::catch_unwind(|| snapshot(root.path())).is_err());
        assert!(
            snapshot_excluding(root.path(), &["held.lock"])
                .files
                .is_empty()
        );
        file.unlock().unwrap();
        assert_eq!(
            snapshot(root.path()).files[Path::new("held.lock")],
            b"private content"
        );
    }
}
