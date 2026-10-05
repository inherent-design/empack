//! Private copies separate backend mutations from project bytes and cache hardlinks.
use super::io::copy_bounded;
use super::{
    native,
    snapshot::{
        FileObservation, NativeSnapshot, Observation, ProjectReadRoot, SnapshotLimits, observe_file,
    },
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use empack_core::path::{PathSyntax, PortableRelPath};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// No live project path or writable file handle is exposed by the stage writer.
pub struct MutableStage {
    storage: TempDir,
    root: ProjectReadRoot,
}
impl MutableStage {
    pub fn empty() -> Result<Self> {
        let storage = tempfile::Builder::new().prefix("empack-stage-").tempdir()?;
        let root = ProjectReadRoot::open(storage.path())?;
        Ok(Self { storage, root })
    }

    /// Copy retained observations, never hardlink them into a writable backend tree.
    pub fn from_snapshot(
        source: &ProjectReadRoot,
        snapshot: &NativeSnapshot,
        cancel: &Cancellation,
    ) -> Result<Self> {
        source.revalidate(snapshot, cancel)?;
        let mut stage = Self::empty()?;
        for (path, observation) in snapshot.entries() {
            cancel.check()?;
            match observation {
                Observation::Directory { .. } | Observation::Ancestor(_) => {
                    stage.directory(path)?
                }
                Observation::File(expected) => {
                    let (parent, leaf) = native::parent(&source.directory, path)?;
                    let mut file = native::open_file(&parent, &leaf)?;
                    ensure!(
                        native::identity(&file)? == expected.object,
                        "Source object changed before staging"
                    );
                    let permission = file.metadata()?.permissions();
                    stage.write_with_permissions(
                        path,
                        &mut file,
                        expected.bytes,
                        cancel,
                        Some(permission),
                    )?;
                    let (parent, leaf) = native::parent(&stage.root.directory, path)?;
                    let mut copied = native::open_file(&parent, &leaf)?;
                    let actual = observe_file(&mut copied, expected.bytes, cancel)?;
                    ensure!(
                        actual.content == expected.content && actual.bytes == expected.bytes,
                        "Source bytes changed while staging {}",
                        path.as_str()
                    );
                }
                Observation::Absent => {}
            }
        }
        source.revalidate(snapshot, cancel)?;
        Ok(stage)
    }

    /// Write a fresh private object and replace a stage file only after bounded copying.
    pub fn write(
        &mut self,
        path: &PortableRelPath,
        source: &mut dyn Read,
        maximum: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        self.write_with_permissions(path, source, maximum, cancel, None)
    }

    fn write_with_permissions(
        &mut self,
        path: &PortableRelPath,
        source: &mut dyn Read,
        maximum: u64,
        cancel: &Cancellation,
        permissions: Option<std::fs::Permissions>,
    ) -> Result<()> {
        cancel.check()?;
        self.root.check_binding()?;
        let (parent, leaf) = create_parent(&self.root.directory, path)?;
        match parent.symlink_metadata(&leaf) {
            Ok(metadata) => ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Stage target is not a regular file"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let (temporary, mut file) = create_temporary(&parent)?;
        let result = (|| {
            copy_bounded(source, &mut file, maximum, cancel)?;
            if let Some(permissions) = permissions {
                file.set_permissions(permissions)?;
            }
            file.sync_all()?;
            drop(file);
            parent.rename(&temporary, &parent, &leaf)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = parent.remove_file(&temporary);
        }
        result
    }

    fn directory(&self, path: &PortableRelPath) -> Result<()> {
        let mut directory = self.root.directory.try_clone()?;
        for component in path.components() {
            directory = ensure_directory(&directory, component)?;
        }
        Ok(())
    }

    /// Consuming the only writer retires synchronous writes. Tool execution is not admitted by this API.
    /// Capture actual inventory and retain opened handles; later copying rechecks their bytes.
    pub fn freeze(self, limits: SnapshotLimits, cancel: &Cancellation) -> Result<FrozenStage> {
        let mut scopes = Vec::new();
        for entry in self.root.directory.entries()? {
            cancel.check()?;
            ensure!(scopes.len() < limits.entries, "Stage exceeds entry limit");
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Nonportable stage filename"))?;
            scopes.push(PortableRelPath::parse(&name, PathSyntax::ArtifactName)?);
        }
        let snapshot = self.root.capture(&scopes, limits, cancel)?;
        let mut files = BTreeMap::new();
        for (path, observation) in snapshot.entries() {
            if let Observation::File(expected) = observation {
                let (parent, leaf) = native::parent(&self.root.directory, path)?;
                let file = native::open_file(&parent, &leaf)?;
                ensure!(
                    native::identity(&file)? == expected.object,
                    "Stage object changed while freezing"
                );
                files.insert(
                    path.clone(),
                    RetainedFile {
                        file,
                        observation: expected.clone(),
                    },
                );
            }
        }
        self.root.revalidate(&snapshot, cancel)?;
        Ok(FrozenStage {
            _storage: self.storage,
            files,
            snapshot,
        })
    }
}

struct RetainedFile {
    file: File,
    observation: FileObservation,
}
/// Candidate inventory is evidence for a verifier, not authorization for publication.
pub struct FrozenStage {
    files: BTreeMap<PortableRelPath, RetainedFile>,
    snapshot: NativeSnapshot,
    _storage: TempDir,
}
impl FrozenStage {
    pub fn inventory(&self) -> &BTreeMap<PortableRelPath, Observation> {
        self.snapshot.entries()
    }

    /// Copy the retained object, not a reopened pathname. Recheck bytes during copying.
    /// On failure the recipient must discard its incomplete private candidate.
    pub fn copy_verified(
        &mut self,
        path: &PortableRelPath,
        output: &mut dyn Write,
        cancel: &Cancellation,
    ) -> Result<()> {
        let retained = self
            .files
            .get_mut(path)
            .context("Candidate is not a frozen regular file")?;
        retained.file.seek(SeekFrom::Start(0))?;
        let (digest, bytes) = copy_bounded(
            &mut retained.file,
            output,
            retained.observation.bytes,
            cancel,
        )?;
        ensure!(
            digest == retained.observation.content && bytes == retained.observation.bytes,
            "Frozen candidate bytes changed: {}",
            path.as_str()
        );
        Ok(())
    }
}

fn ensure_directory(parent: &Dir, name: &str) -> Result<Dir> {
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let directory = parent.open_dir_nofollow(name)?;
    native::reject_reparse(&directory.try_clone()?.into_std_file())?;
    Ok(directory)
}
fn create_parent(root: &Dir, path: &PortableRelPath) -> Result<(Dir, String)> {
    let mut directory = root.try_clone()?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            return Ok((directory, component.to_owned()));
        }
        directory = ensure_directory(&directory, component)?;
    }
    unreachable!("portable paths are nonempty")
}

pub(super) fn create_temporary(parent: &Dir) -> Result<(String, File)> {
    for _ in 0..128 {
        let name = format!(
            ".empack-candidate-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match parent.open_with(&name, &options) {
            Ok(file) => return Ok((name, file.into_std())),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("Cannot allocate private candidate file")
}

#[cfg(test)]
mod tests;
