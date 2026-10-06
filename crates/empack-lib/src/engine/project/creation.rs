//! A missing project is bound to an existing parent, never created during preparation.
use super::*;
use std::path::PathBuf;

/// Read-only evidence for one absent child of a retained native parent.
/// This is not permission to create the destination or to overwrite a racing entry.
pub struct NewProjectSnapshot {
    pub(in crate::engine) parent: ProjectReadRoot,
    child: PortableRelPath,
    pub(in crate::engine) absence: NativeSnapshot,
    recovery: RecoveryReader,
    selected: PathBuf,
}
impl NewProjectSnapshot {
    pub fn selected(&self) -> &Path {
        &self.selected
    }
    pub fn child(&self) -> &PortableRelPath {
        &self.child
    }
    /// A publisher must additionally hold exclusive coordination and use no-replace creation.
    pub fn revalidate(&self, cancel: &Cancellation) -> Result<()> {
        let _creation = self.recovery.enter_new(&self.parent, self.child.as_str())?;
        let _guard = self.recovery.enter(&self.parent)?;
        self.parent.revalidate(&self.absence, cancel)
    }
}
impl ProjectReader {
    /// The selected parent must already exist. No directory, lock file, registration or journal
    /// is created; unrelated siblings are neither opened nor included in the read set.
    pub fn capture_new(
        &self,
        selected: &Path,
        cancel: &Cancellation,
    ) -> Result<NewProjectSnapshot> {
        cancel.check()?;
        ensure!(
            selected.is_absolute(),
            "New project selection must be absolute"
        );
        let child = selected
            .file_name()
            .and_then(|name| name.to_str())
            .context("New project needs a portable child directory name")?;
        let child = PortableRelPath::parse(child, PathSyntax::ArtifactName)?;
        let parent = ProjectReadRoot::open(
            selected
                .parent()
                .context("New project needs an existing parent")?,
        )?;
        let _guard = self.recovery.enter(&parent)?;
        match parent.directory.symlink_metadata(child.as_str()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!("New project destination already exists"),
        }
        let absence = parent.capture(
            std::slice::from_ref(&child),
            SnapshotLimits {
                entries: 1,
                depth: 1,
                file_bytes: 0,
                total_bytes: 0,
            },
            cancel,
        )?;
        ensure!(
            matches!(absence.entries().get(&child), Some(Observation::Absent)),
            "New project destination appeared during capture"
        );
        let snapshot = NewProjectSnapshot {
            parent,
            child,
            absence,
            recovery: self.recovery.clone(),
            selected: selected.to_path_buf(),
        };
        snapshot.revalidate(cancel)?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn absent_child_capture_has_no_persistent_effects_and_ignores_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("unrelated"), b"before").unwrap();
        let state = temp.path().join("host-state");
        let reader = ProjectReader::new(RecoveryReader::new(state.clone()));
        let selected = parent.join("new project [1]");
        let snapshot = reader
            .capture_new(&selected, &Cancellation::default())
            .unwrap();
        assert_eq!(snapshot.selected(), selected);
        assert_eq!(snapshot.child().as_str(), "new project [1]");
        assert!(!selected.exists() && !state.exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        fs::write(parent.join("unrelated"), b"after").unwrap();
        snapshot.revalidate(&Cancellation::default()).unwrap();
        fs::create_dir(&selected).unwrap();
        assert!(snapshot.revalidate(&Cancellation::default()).is_err());
        assert!(
            reader
                .capture_new(&selected, &Cancellation::default())
                .is_err()
        );
        assert!(!state.exists());
    }

    #[test]
    fn occupied_or_invalid_targets_and_replaced_parents_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("state")));
        let cancel = Cancellation::default();
        for selected in [
            PathBuf::from("relative"),
            parent.join("bad?name"),
            parent.join("missing/new"),
        ] {
            assert!(reader.capture_new(&selected, &cancel).is_err());
        }
        fs::write(parent.join("file"), b"untouched").unwrap();
        assert!(reader.capture_new(&parent.join("file"), &cancel).is_err());
        let snapshot = reader.capture_new(&parent.join("new"), &cancel).unwrap();
        fs::rename(&parent, temp.path().join("old-parent")).unwrap();
        fs::create_dir(&parent).unwrap();
        assert!(snapshot.revalidate(&cancel).is_err());
        assert_eq!(
            fs::read(temp.path().join("old-parent/file")).unwrap(),
            b"untouched"
        );
        let cancelled = Cancellation::default();
        cancelled.cancel();
        assert!(
            reader
                .capture_new(&parent.join("other"), &cancelled)
                .is_err()
        );
        assert!(!parent.join("other").exists());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_links_cannot_claim_absence() {
        let temp = tempfile::tempdir().unwrap();
        let selected = temp.path().join("new");
        let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("state")));
        let snapshot = reader
            .capture_new(&selected, &Cancellation::default())
            .unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing"), &selected).unwrap();
        assert!(snapshot.revalidate(&Cancellation::default()).is_err());
        assert!(
            reader
                .capture_new(&selected, &Cancellation::default())
                .is_err()
        );
        assert!(
            fs::symlink_metadata(selected)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
