//! Host-private records select bytes and facts, never project paths or execution approval.
use crate::application::process_runtime::Cancellation;
use crate::engine::{
    native,
    publication::{open_private_directory, root_key},
    snapshot::{Observation, ProjectReadRoot, SnapshotLimits},
    staging::create_temporary,
};
use anyhow::{Context, Result, ensure};
use cap_std::fs::{Dir, OpenOptions};
use empack_core::digest::ExpectedDigest;
use empack_core::path::{PathSyntax, PortableRelPath};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

pub(in crate::engine) const MAX_RECORD: u64 = 4 << 20;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::engine) struct Binding {
    pub parent: String,
    pub project: Option<String>,
    pub documents: [Option<[u8; 32]>; 2],
}
/// Record validation remains with each workflow; saved DTOs carry no approval.
pub(in crate::engine) trait BoundRecord: Serialize + DeserializeOwned {
    fn binding(&self) -> &Binding;
    fn schema(&self) -> u32;
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::engine) enum Kind {
    Import,
    Synchronization,
}
impl Kind {
    fn directory(self) -> &'static str {
        match self {
            Self::Import => "pending-imports",
            Self::Synchronization => "pending-sync",
        }
    }
    pub(in crate::engine) fn maximum(self) -> u64 {
        match self {
            Self::Import => MAX_RECORD,
            Self::Synchronization => 4 * crate::engine::documents::MAX_DOCUMENT_BYTES as u64,
        }
    }
}
pub(in crate::engine) struct SavedRecord {
    pub state: std::path::PathBuf,
    pub name: String,
    pub content: [u8; 32],
    pub binding: Binding,
    kind: Kind,
}
pub(in crate::engine) struct Cleanup {
    state: std::path::PathBuf,
    name: String,
    content: [u8; 32],
    kind: Kind,
}
impl SavedRecord {
    pub fn into_cleanup(self) -> Cleanup {
        Cleanup {
            state: self.state,
            name: self.name,
            content: self.content,
            kind: self.kind,
        }
    }
}
fn location(target: &Path) -> Result<(ProjectReadRoot, String, String)> {
    ensure!(target.is_absolute(), "Operation target must be absolute");
    let parent =
        ProjectReadRoot::open(target.parent().context("Operation target needs a parent")?)?;
    let leaf = target
        .file_name()
        .and_then(|name| name.to_str())
        .context("Operation target needs a portable name")?;
    PortableRelPath::parse(leaf, PathSyntax::ArtifactName)?;

    let parent_key = root_key(&parent)?;
    let mut digest = Sha256::new();
    digest.update(parent_key.as_bytes());
    digest.update([0]);
    digest.update(leaf.as_bytes());
    let name = format!(
        "{}.json",
        ExpectedDigest::Sha256(digest.finalize().into()).hex()
    );
    Ok((parent, leaf.to_owned(), name))
}
pub(in crate::engine) fn bind(target: &Path, cancel: &Cancellation) -> Result<(String, Binding)> {
    let (parent, leaf, name) = location(target)?;
    let parent_key = root_key(&parent)?;
    let project = match parent.directory.symlink_metadata(&leaf) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Operation target is not a regular directory"
            );
            Some(ProjectReadRoot::open_child(&parent, &leaf)?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut documents = [None, None];
    if let Some(project) = &project {
        let paths = ["empack.yml", "empack.lock"]
            .map(|name| PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap());
        let snapshot = project.capture(
            &paths,
            SnapshotLimits {
                entries: 4,
                depth: 1,
                file_bytes: crate::engine::documents::MAX_DOCUMENT_BYTES as u64,
                total_bytes: 2 * crate::engine::documents::MAX_DOCUMENT_BYTES as u64,
            },
            cancel,
        )?;
        for (index, path) in paths.iter().enumerate() {
            match snapshot.entries().get(path) {
                Some(Observation::File(file)) => documents[index] = Some(file.content),
                Some(Observation::Absent) => {}
                _ => anyhow::bail!("Operation target document is not a regular file"),
            }
        }
        project.revalidate(&snapshot, cancel)?;
    }
    parent.check_binding()?;
    Ok((
        name,
        Binding {
            parent: parent_key,
            project: project.as_ref().map(root_key).transpose()?,
            documents,
        },
    ))
}
/// Observe opaque bytes for explicit cleanup, including invalid or stale records.
pub(in crate::engine) fn observe(
    state: &Path,
    kind: Kind,
    target: &Path,
    cancel: &Cancellation,
) -> Result<Option<Cleanup>> {
    let (_, _, name) = location(target)?;
    let directory = match open_private_directory(&state.join(kind.directory()), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock_shared()
        .context("Pending operation state is busy")?;
    let mut file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let (content, _) =
        crate::engine::io::copy_bounded(&mut file, &mut io::sink(), kind.maximum(), cancel)?;
    Ok(Some(Cleanup {
        state: state.into(),
        name,
        content,
        kind,
    }))
}
/// Admit decoded storage from the native file length before allocating record bytes.
pub(in crate::engine) fn record_bytes(
    state: &Path,
    kind: Kind,
    target: &Path,
) -> Result<Option<u64>> {
    let (_, _, name) = location(target)?;
    let directory = match open_private_directory(&state.join(kind.directory()), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let bytes = file.metadata()?.len();
    ensure!(
        bytes <= kind.maximum(),
        "Pending operation exceeds record limit"
    );
    Ok(Some(bytes))
}
pub(in crate::engine) fn read<R: BoundRecord>(
    state: &Path,
    kind: Kind,
    target: &Path,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<Option<(SavedRecord, R)>> {
    let (name, binding) = bind(target, cancel)?;
    let directory = match open_private_directory(&state.join(kind.directory()), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock_shared()
        .context("Pending operation state is busy")?;
    let mut file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    let (content, _) = crate::engine::io::copy_bounded(
        &mut file,
        &mut bytes,
        kind.maximum().min(maximum),
        cancel,
    )?;
    let record: R = serde_json::from_slice(&bytes).context("Invalid pending operation record")?;
    ensure!(record.schema() == 1, "Unsupported pending operation schema");
    ensure!(
        record.binding() == &binding,
        "Saved operation is stale: target identity or documents changed"
    );
    ensure!(
        bind(target, cancel)?.1 == binding,
        "Operation target changed during inspection"
    );
    Ok(Some((
        SavedRecord {
            state: state.into(),
            name,
            content,
            binding,
            kind,
        },
        record,
    )))
}
pub(in crate::engine) fn save<R: BoundRecord>(
    state: &Path,
    kind: Kind,
    target: &Path,
    record: &R,
    prior: Option<&SavedRecord>,
    cancel: &Cancellation,
) -> Result<SavedRecord> {
    let (name, binding) = bind(target, cancel)?;
    ensure!(
        record.binding() == &binding,
        "Operation target changed before suspension"
    );
    let directory = open_private_directory(&state.join(kind.directory()), true)?;
    let _lock = lock(&directory)?;
    match native::open_file(&directory, &name) {
        Ok(mut existing) => {
            let prior = prior
                .context("An operation is already pending; continue it or explicitly discard it")?;
            ensure!(
                prior.state == state && prior.name == name && prior.kind == kind,
                "Saved operation belongs to another target"
            );
            let (content, _) = crate::engine::io::copy_bounded(
                &mut existing,
                &mut io::sink(),
                kind.maximum(),
                cancel,
            )?;
            ensure!(
                content == prior.content,
                "Saved operation changed before extension"
            );
        }
        Err(error) if missing(&error) => {
            ensure!(
                prior.is_none(),
                "Saved operation disappeared before extension"
            )
        }
        Err(error) => return Err(error),
    }
    let (temporary, mut file) = create_temporary(&directory)?;
    let result = (|| {
        let bytes = serde_json::to_vec(record)?;
        ensure!(
            bytes.len() as u64 <= kind.maximum(),
            "Pending operation record exceeds byte limit"
        );
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        cancel.check()?;
        ensure!(
            bind(target, cancel)?.1 == binding,
            "Operation target changed before suspension"
        );
        directory.rename(&temporary, &directory, &name)?;
        sync(&directory)?;
        Ok(SavedRecord {
            state: state.into(),
            name,
            content: Sha256::digest(&bytes).into(),
            binding,
            kind,
        })
    })();
    if result.is_err() {
        let _ = directory.remove_file(temporary);
    }
    result
}
pub(in crate::engine) fn discard(saved: &Cleanup, cancel: &Cancellation) -> Result<bool> {
    cancel.check()?;
    let directory = open_private_directory(&saved.state.join(saved.kind.directory()), false)?;
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock().context("Pending operation state is busy")?;
    let mut file = match native::open_file(&directory, &saved.name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let (content, _) =
        crate::engine::io::copy_bounded(&mut file, &mut io::sink(), saved.kind.maximum(), cancel)?;
    if content != saved.content {
        return Ok(false);
    }
    drop(file);
    cancel.check()?;
    directory.remove_file(&saved.name)?;
    sync(&directory)?;
    Ok(true)
}
fn lock(directory: &Dir) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match directory.open_with("state.lock", &options) {
        Ok(file) => {
            file.sync_all()?;
            sync(directory)?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let file = native::open_file(directory, "state.lock")?;
    file.try_lock().context("Pending operation state is busy")?;
    Ok(file)
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
}
fn sync(directory: &Dir) -> Result<()> {
    #[cfg(unix)]
    native::readable_directory(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}
