//! Host-private records select bytes and facts, never project paths or execution approval.
use super::*;
use crate::engine::{
    native,
    publication::{open_private_directory, root_key},
    snapshot::{Observation, ProjectReadRoot, SnapshotLimits},
    staging::create_temporary,
};
use cap_std::fs::{Dir, OpenOptions};
use empack_core::path::{PathSyntax, PortableRelPath};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

pub(super) const MAX_RECORD: u64 = 4 << 20;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    parent: String,
    project: Option<String>,
    documents: [Option<[u8; 32]>; 2],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub schema: u32,
    pub binding: Binding,
    pub archive: [u8; 32],
    pub archive_bytes: u64,
    pub archive_digests: Vec<(String, String)>,
    pub revision: [u8; 32],
    pub strong: bool,
    pub files: BTreeMap<String, [u8; 32]>,
}
fn location(target: &Path) -> Result<(ProjectReadRoot, String, String)> {
    ensure!(target.is_absolute(), "Import target must be absolute");
    let parent = ProjectReadRoot::open(target.parent().context("Import target needs a parent")?)?;
    let leaf = target
        .file_name()
        .and_then(|name| name.to_str())
        .context("Import target needs a portable name")?;
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
pub(super) fn bind(target: &Path, cancel: &Cancellation) -> Result<(String, Binding)> {
    let (parent, leaf, name) = location(target)?;
    let parent_key = root_key(&parent)?;
    let project = match parent.directory.symlink_metadata(&leaf) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Import target is not a regular directory"
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
                _ => anyhow::bail!("Import target document is not a regular file"),
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
pub(super) fn observe(
    state: &Path,
    target: &Path,
    cancel: &Cancellation,
) -> Result<Option<PendingImportCleanup>> {
    let (_, _, name) = location(target)?;
    let directory = match open_private_directory(&state.join("pending-imports"), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock_shared()
        .context("Pending import state is busy")?;
    let mut file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let (content, _) =
        crate::engine::io::copy_bounded(&mut file, &mut io::sink(), MAX_RECORD, cancel)?;
    Ok(Some(PendingImportCleanup {
        state: state.into(),
        name,
        content,
    }))
}
pub(super) fn read(
    state: &Path,
    target: &Path,
    cancel: &Cancellation,
) -> Result<Option<(SavedImportRecord, Record)>> {
    let (name, binding) = bind(target, cancel)?;
    let directory = match open_private_directory(&state.join("pending-imports"), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock_shared()
        .context("Pending import state is busy")?;
    let mut file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    let (content, _) = crate::engine::io::copy_bounded(&mut file, &mut bytes, MAX_RECORD, cancel)?;
    let record: Record = serde_json::from_slice(&bytes).context("Invalid pending import record")?;
    ensure!(record.schema == 1, "Unsupported pending import schema");
    ensure!(
        record.binding == binding,
        "Saved import is stale: target identity or documents changed"
    );
    ensure!(
        bind(target, cancel)?.1 == binding,
        "Import target changed during inspection"
    );
    Ok(Some((
        SavedImportRecord {
            state: state.into(),
            name,
            content,
            binding,
        },
        record,
    )))
}
pub(super) fn save(
    state: &Path,
    target: &Path,
    record: &Record,
    prior: Option<&SavedImportRecord>,
    cancel: &Cancellation,
) -> Result<SavedImportRecord> {
    let (name, binding) = bind(target, cancel)?;
    ensure!(
        record.binding == binding,
        "Import target changed before suspension"
    );
    let directory = open_private_directory(&state.join("pending-imports"), true)?;
    let _lock = lock(&directory)?;
    match native::open_file(&directory, &name) {
        Ok(mut existing) => {
            let prior = prior
                .context("An import is already pending; continue it or explicitly discard it")?;
            ensure!(
                prior.state == state && prior.name == name,
                "Saved import belongs to another target"
            );
            let (content, _) = crate::engine::io::copy_bounded(
                &mut existing,
                &mut io::sink(),
                MAX_RECORD,
                cancel,
            )?;
            ensure!(
                content == prior.content,
                "Saved import changed before extension"
            );
        }
        Err(error) if missing(&error) => {
            ensure!(prior.is_none(), "Saved import disappeared before extension")
        }
        Err(error) => return Err(error),
    }
    let (temporary, mut file) = create_temporary(&directory)?;
    let result = (|| {
        let bytes = serde_json::to_vec(record)?;
        ensure!(
            bytes.len() as u64 <= MAX_RECORD,
            "Pending import record exceeds byte limit"
        );
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        cancel.check()?;
        ensure!(
            bind(target, cancel)?.1 == binding,
            "Import target changed before suspension"
        );
        directory.rename(&temporary, &directory, &name)?;
        sync(&directory)?;
        Ok(SavedImportRecord {
            state: state.into(),
            name,
            content: Sha256::digest(&bytes).into(),
            binding,
        })
    })();
    if result.is_err() {
        let _ = directory.remove_file(temporary);
    }
    result
}
pub(super) fn discard(saved: &PendingImportCleanup, cancel: &Cancellation) -> Result<bool> {
    cancel.check()?;
    let directory = open_private_directory(&saved.state.join("pending-imports"), false)?;
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock().context("Pending import state is busy")?;
    let mut file = match native::open_file(&directory, &saved.name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let (content, _) =
        crate::engine::io::copy_bounded(&mut file, &mut io::sink(), MAX_RECORD, cancel)?;
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
    file.try_lock().context("Pending import state is busy")?;
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

#[cfg(test)]
mod tests;
