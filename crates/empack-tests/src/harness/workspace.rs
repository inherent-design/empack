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
    fn visit(root: &Path, at: &Path, result: &mut Snapshot) {
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
        let relative = at.strip_prefix(root).unwrap().to_owned();
        result
            .entries
            .insert(relative.clone(), (metadata.is_dir(), mode));
        if metadata.is_file() {
            result.files.insert(relative, fs::read(at).unwrap());
        } else {
            for entry in fs::read_dir(at).unwrap() {
                visit(root, &entry.unwrap().path(), result);
            }
        }
    }
    let mut result = Snapshot {
        files: BTreeMap::new(),
        entries: BTreeMap::new(),
    };
    visit(root, root, &mut result);
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
        fs::create_dir(root.path().join("empty")).unwrap();
        let before = snapshot(root.path());
        assert!(before.entries.contains_key(Path::new("")));
        assert!(before.entries.contains_key(Path::new("empty")));
        assert_eq!(before.files.len(), 1);
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
}
