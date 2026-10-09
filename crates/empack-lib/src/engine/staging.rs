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

pub(super) enum StageToolArgument {
    Text(String),
    Path(PortableRelPath),
    Root,
}

/// No live project path or writable file handle is exposed by the stage writer.
pub struct MutableStage {
    // Native handles must close before TempDir cleanup, especially without Windows delete sharing.
    root: ProjectReadRoot,
    storage: TempDir,
    #[cfg(windows)]
    private_parent: TempDir,
}
impl MutableStage {
    pub fn empty() -> Result<Self> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("empack-stage-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        #[cfg(windows)]
        let private_parent = {
            let parent = builder.tempdir()?;
            let directory = Dir::open_ambient_dir(parent.path(), cap_std::ambient_authority())?;
            super::windows_privacy::protect_empty_temporary(&directory)?;
            parent
        };
        #[cfg(windows)]
        let storage = builder.tempdir_in(private_parent.path())?;
        #[cfg(not(windows))]
        let storage = builder.tempdir()?;
        let root = ProjectReadRoot::open(storage.path())?;
        #[cfg(windows)]
        super::windows_privacy::verify(&root.directory)?;
        Ok(Self {
            storage,
            root,
            #[cfg(windows)]
            private_parent,
        })
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
                        None,
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
        self.write_with_permissions(path, source, maximum, cancel, None, None)
    }

    /// Apply explicit portable attributes to a fresh private candidate before freezing.
    pub(super) fn write_attributed(
        &mut self,
        path: &PortableRelPath,
        source: &mut dyn Read,
        maximum: u64,
        attributes: empack_core::files::FilePermissions,
        cancel: &Cancellation,
    ) -> Result<()> {
        self.write_with_permissions(path, source, maximum, cancel, None, Some(attributes))
    }

    fn write_with_permissions(
        &mut self,
        path: &PortableRelPath,
        source: &mut dyn Read,
        maximum: u64,
        cancel: &Cancellation,
        permissions: Option<std::fs::Permissions>,
        attributes: Option<empack_core::files::FilePermissions>,
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
            if let Some(attributes) = attributes {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = if attributes.readonly { 0o400 } else { 0o600 }
                        | if attributes.executable { 0o100 } else { 0 };
                    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
                }
                #[cfg(not(unix))]
                {
                    let mut permissions = file.metadata()?.permissions();
                    permissions.set_readonly(attributes.readonly);
                    file.set_permissions(permissions)?;
                }
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

    /// Delete one private-stage file. Directories and links cannot become recursive removals.
    #[cfg(test)]
    pub fn remove(&mut self, path: &PortableRelPath) -> Result<()> {
        self.root.check_binding()?;
        let (parent, leaf) = native::parent(&self.root.directory, path)?;
        let _file = native::open_file(&parent, &leaf)?;
        parent.remove_file(&leaf)?;
        Ok(())
    }

    fn directory(&self, path: &PortableRelPath) -> Result<()> {
        let mut directory = self.root.directory.try_clone()?;
        for component in path.components() {
            directory = ensure_directory(&directory, component)?;
        }
        Ok(())
    }

    /// Run a trusted tool against this private tree. The caller must own this future through
    /// retirement; the stage cannot be frozen or reused while the process tree is alive.
    /// This is process ownership and output isolation, not an OS sandbox for arbitrary code.
    pub(super) async fn run_tool(
        self,
        executable: &std::path::Path,
        arguments: &[StageToolArgument],
        deadline: std::time::Duration,
        cancel: Cancellation,
    ) -> Result<Self> {
        self.root.check_binding()?;
        let mut command = std::process::Command::new(executable);
        command.current_dir(self.storage.path());
        for argument in arguments {
            match argument {
                StageToolArgument::Text(value) => {
                    command.arg(value);
                }
                StageToolArgument::Path(relative) => {
                    command.arg(self.storage.path().join(relative.as_str()));
                }
                StageToolArgument::Root => {
                    command.arg(self.storage.path());
                }
            }
        }
        let result =
            crate::application::process_runtime::execute_async(command, deadline, cancel, None)
                .await?;
        ensure!(
            result.success,
            "Private preparation tool failed: {}",
            result.error_output()
        );
        self.root.check_binding()?;
        Ok(self)
    }

    /// Consuming the only writer retires synchronous writes after any owned tool has returned.
    /// Capture actual inventory and retain private bytes; trees use one packed backing file.
    /// Later member copying rechecks hashes and cannot read neighbouring packed ranges.
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
        // A whole pack must not require one host descriptor per frozen member.
        // Single-file leases keep their existing retained object; trees share one private backing.
        let mut packed = (snapshot
            .entries()
            .values()
            .filter(|entry| matches!(entry, Observation::File(_)))
            .count()
            > 1)
        .then(PrivateFile::new)
        .transpose()?;
        for (path, observation) in snapshot.entries() {
            if let Observation::File(expected) = observation {
                let (parent, leaf) = native::parent(&self.root.directory, path)?;
                let mut file = native::open_file(&parent, &leaf)?;
                ensure!(
                    native::identity(&file)? == expected.object,
                    "Stage object changed while freezing"
                );
                let data = if let Some(packed) = &mut packed {
                    let offset = packed.file().stream_position()?;
                    let (digest, bytes) =
                        copy_bounded(&mut file, packed.file(), expected.bytes, cancel)?;
                    ensure!(
                        digest == expected.content && bytes == expected.bytes,
                        "Stage bytes changed while packing"
                    );
                    RetainedData::Packed { offset }
                } else {
                    RetainedData::Native(file)
                };
                files.insert(
                    path.clone(),
                    RetainedFile {
                        data,
                        observation: expected.clone(),
                    },
                );
            }
        }
        self.root.revalidate(&snapshot, cancel)?;
        if let Some(packed) = &mut packed {
            packed.file().sync_all()?;
        }
        drop(self.root);
        let storage = if packed.is_some() {
            self.storage.close()?;
            None
        } else {
            Some(self.storage)
        };
        #[cfg(windows)]
        let private_parent = if packed.is_some() {
            self.private_parent.close()?;
            None
        } else {
            Some(self.private_parent)
        };
        Ok(FrozenStage {
            packed,
            _storage: storage,
            #[cfg(windows)]
            _private_parent: private_parent,
            files,
            snapshot,
        })
    }
}

enum RetainedData {
    Native(File),
    Packed { offset: u64 },
}
struct RetainedFile {
    data: RetainedData,
    observation: FileObservation,
}
/// Candidate inventory is evidence for a verifier, not authorization for publication.
pub struct FrozenStage {
    packed: Option<PrivateFile>,
    files: BTreeMap<PortableRelPath, RetainedFile>,
    snapshot: NativeSnapshot,
    _storage: Option<TempDir>,
    #[cfg(windows)]
    _private_parent: Option<TempDir>,
}
impl FrozenStage {
    fn source(&mut self, path: &PortableRelPath) -> std::io::Result<(&mut File, u64, u64)> {
        let retained = self.files.get_mut(path).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Retained content is unavailable",
            )
        })?;
        match &mut retained.data {
            RetainedData::Native(file) => {
                Ok((file, 0, retained.observation.bytes.saturating_add(1)))
            }
            RetainedData::Packed { offset } => Ok((
                self.packed.as_mut().expect("packed member backing").file(),
                *offset,
                retained.observation.bytes,
            )),
        }
    }
    /// Borrow exactly one retained member; packed neighbours cannot enter its stream.
    pub(super) fn reader(&mut self, path: &PortableRelPath) -> Result<impl Read + '_> {
        let (file, offset, limit) = self.source(path)?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(file.take(limit))
    }
    /// Retire a member after its verified private copy has been retained elsewhere.
    /// The inventory remains capture evidence; this file is no longer readable from this stage.
    pub(super) fn retire_input(&mut self, path: &PortableRelPath) -> Result<()> {
        self.files
            .remove(path)
            .context("Frozen input was already retired")?;
        Ok(())
    }

    /// Independent lease cursors serialize native seeking; no pathname or writable handle escapes.
    pub(super) fn read_at(
        &mut self,
        path: &PortableRelPath,
        offset: u64,
        buffer: &mut [u8],
    ) -> std::io::Result<usize> {
        let (file, start, limit) = self.source(path)?;
        if offset >= limit {
            return Ok(0);
        }
        let absolute = start
            .checked_add(offset)
            .ok_or_else(|| std::io::Error::other("Packed offset overflow"))?;
        file.seek(SeekFrom::Start(absolute))?;
        let length = buffer
            .len()
            .min(usize::try_from(limit - offset).unwrap_or(usize::MAX));
        file.read(&mut buffer[..length])
    }

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
        let expected = self
            .files
            .get(path)
            .context("Candidate is not a frozen regular file")?
            .observation
            .clone();
        let mut reader = self.reader(path)?;
        let (digest, bytes) = copy_bounded(&mut reader, output, expected.bytes, cancel)?;
        ensure!(
            digest == expected.content && bytes == expected.bytes,
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

/// Private seekable scratch storage for native archive adapters. The file closes before its
/// protected parent is released. This is not a stage that can be frozen or published directly.
pub(super) struct PrivateFile {
    file: File,
    _storage: MutableStage,
}
impl PrivateFile {
    pub(super) fn new() -> Result<Self> {
        let storage = MutableStage::empty()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        options.follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = storage
            .root
            .directory
            .open_with("candidate", &options)?
            .into_std();
        Ok(Self {
            file,
            _storage: storage,
        })
    }
    pub(super) fn file(&mut self) -> &mut File {
        &mut self.file
    }
}

#[cfg(test)]
mod tests;
