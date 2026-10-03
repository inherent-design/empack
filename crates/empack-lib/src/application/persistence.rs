//! Atomic document publication and exclusive command mutation ownership.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Holds the OS lock until the command and its cleanup finish.
pub struct ProjectLock {
    _file: Option<File>,
}

impl ProjectLock {
    pub fn in_memory() -> Self {
        Self { _file: None }
    }

    pub fn acquire(project: &Path) -> Result<Self> {
        let identity = canonical_project_path(project)?;
        let root = directories::ProjectDirs::from("design", "inherent", "empack")
            .context("Cannot determine project lock storage")?
            .data_local_dir()
            .join("locks");
        std::fs::create_dir_all(&root)?;
        let key: String = Sha256::digest(identity.as_os_str().as_encoded_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("{key}.lock")))?;
        file.try_lock().with_context(|| {
            format!("Project is busy or cannot be locked: {}", project.display())
        })?;
        // Keep the lock file: unlinking it would let waiters acquire different inodes.
        Ok(Self { _file: Some(file) })
    }
}

fn canonical_project_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().context("Invalid project path")?);
        ancestor = ancestor
            .parent()
            .context("Project has no existing ancestor")?;
    }
    let mut canonical = ancestor.canonicalize()?;
    for part in suffix.into_iter().rev() {
        canonical.push(part);
    }
    Ok(canonical)
}

/// Replace a document using a sibling temporary file. Errors after publication say so.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Refusing to replace a symlink: {}",
                path.display()
            );
            anyhow::ensure!(
                !metadata.permissions().readonly(),
                "Document is read-only: {}",
                path.display()
            );
            Some(metadata)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if let Some(metadata) = metadata {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .with_context(|| format!("Failed to publish {}", path.display()))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| {
            format!(
                "Published {}, but directory durability could not be confirmed",
                path.display()
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_write_replaces_whole_document_and_preserves_readonly_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empack.yml");
        atomic_write(&path, b"old contents").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(atomic_write(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn independent_lock_handles_exclude_each_other_and_release_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let first = ProjectLock::acquire(dir.path()).unwrap();
        assert!(ProjectLock::acquire(&dir.path().join(".")).is_err());
        drop(first);
        assert!(ProjectLock::acquire(dir.path()).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn aliases_share_a_lock_and_document_symlinks_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let alias = tempfile::tempdir().unwrap();
        let link = alias.path().join("project");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        let _guard = ProjectLock::acquire(dir.path()).unwrap();
        assert!(ProjectLock::acquire(&link).is_err());
        let original = dir.path().join("original");
        std::fs::write(&original, "original").unwrap();
        let document = dir.path().join("empack.yml");
        std::os::unix::fs::symlink(&original, &document).unwrap();
        assert!(atomic_write(&document, b"replacement").is_err());
        assert_eq!(std::fs::read_to_string(original).unwrap(), "original");
    }
}
