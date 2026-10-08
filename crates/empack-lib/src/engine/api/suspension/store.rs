//! Host-selected continuation records, independently coordinated from project publication.
use super::*;
use crate::engine::{
    native,
    publication::{open_private_directory, root_key},
    snapshot::ProjectReadRoot,
    staging::create_temporary,
};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

pub(super) struct Loaded {
    pub name: String,
    pub content: [u8; 32],
    pub record: record::Record,
    pub documents_match: bool,
}
pub(super) fn documents(
    snapshot: &crate::engine::snapshot::NativeSnapshot,
) -> Result<[Option<[u8; 32]>; 2]> {
    let read = |name| -> Result<_> {
        Ok(
            match snapshot
                .entries()
                .get(&PortableRelPath::parse(name, PathSyntax::ProjectContent)?)
            {
                Some(Observation::File(file)) => Some(file.content),
                Some(Observation::Absent) => None,
                _ => anyhow::bail!("Saved build document is not a regular file"),
            },
        )
    };
    Ok([read("empack.yml")?, read("empack.lock")?])
}
pub(super) struct Selected {
    pub bytes: u64,
    root: ProjectReadRoot,
    name: String,
    file: File,
    _directory: Dir,
    _lock: File,
}
pub(super) fn probe(
    state: &Path,
    project: &Path,
    cancel: &Cancellation,
) -> Result<Option<Selected>> {
    cancel.check()?;
    let selected = ProjectReadRoot::open(project)?;
    let name = format!("{}.json", root_key(&selected)?);
    let directory = match open_private_directory(&state.join("pending-builds"), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock = native::open_file(&directory, "state.lock")?;
    lock.try_lock_shared()
        .context("Pending build state is busy")?;
    let file = match native::open_file(&directory, &name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let bytes = file.metadata()?.len();
    ensure!(
        bytes <= record::MAX_RECORD,
        "Pending build record exceeds byte limit"
    );
    Ok(Some(Selected {
        bytes,
        root: selected,
        name,
        file,
        _directory: directory,
        _lock: lock,
    }))
}
/// Inspect an opaque record for explicit deletion. Parsing is deliberately unnecessary:
/// malformed or stale recipes remain removable without becoming executable authority.
pub(super) fn observe(
    state: &Path,
    project: &Path,
    cancel: &Cancellation,
) -> Result<Option<(String, [u8; 32])>> {
    let Some(mut selected) = probe(state, project, cancel)? else {
        return Ok(None);
    };
    let (content, bytes) = crate::engine::io::copy_bounded(
        &mut selected.file,
        &mut io::sink(),
        selected.bytes,
        cancel,
    )?;
    ensure!(
        bytes == selected.bytes,
        "Pending build record changed during inspection"
    );
    selected.root.check_binding()?;
    Ok(Some((selected.name, content)))
}
pub(super) fn read(selected: Selected, cancel: &Cancellation) -> Result<Option<Loaded>> {
    let Selected {
        bytes: maximum,
        root: selected,
        name,
        mut file,
        _directory,
        _lock,
    } = selected;
    let mut bytes = Vec::new();
    let (content, _) = crate::engine::io::copy_bounded(&mut file, &mut bytes, maximum, cancel)?;
    ensure!(
        bytes.len() as u64 == maximum,
        "Pending build record changed during inspection"
    );
    selected.check_binding()?;
    let record: record::Record =
        serde_json::from_slice(&bytes).context("Invalid pending build record")?;
    ensure!(record.schema == 1, "Unsupported pending build schema");
    let snapshot = selected.capture(
        &[
            PortableRelPath::parse("empack.yml", PathSyntax::ProjectContent)?,
            PortableRelPath::parse("empack.lock", PathSyntax::ProjectContent)?,
        ],
        SnapshotLimits {
            entries: 16,
            depth: 2,
            file_bytes: crate::engine::documents::MAX_DOCUMENT_BYTES as u64,
            total_bytes: 2 * crate::engine::documents::MAX_DOCUMENT_BYTES as u64,
        },
        cancel,
    )?;
    let documents_match = documents(&snapshot)? == record.documents;
    Ok(Some(Loaded {
        name,
        content,
        record,
        documents_match,
    }))
}
pub(super) fn save(
    state: &Path,
    workspace: &WorkspaceSnapshot,
    value: &record::Record,
    cancel: &Cancellation,
) -> Result<bool> {
    cancel.check()?;
    workspace
        .root()
        .revalidate(workspace.observations(), cancel)?;
    let name = format!("{}.json", root_key(workspace.root())?);
    let directory = open_private_directory(&state.join("pending-builds"), true)?;
    let _guard = lock(&directory)?;
    let replaced = match native::open_file(&directory, &name) {
        Ok(_) => true,
        Err(error) if missing(&error) => false,
        Err(error) => return Err(error),
    };
    let (temporary, mut file) = create_temporary(&directory)?;
    let result = (|| {
        serde_json::to_writer(
            BoundedWriter {
                file: &mut file,
                remaining: record::MAX_RECORD,
            },
            value,
        )?;
        file.sync_all()?;
        drop(file);
        cancel.check()?;
        workspace
            .root()
            .revalidate(workspace.observations(), cancel)?;
        directory.rename(&temporary, &directory, &name)?;
        sync(&directory)?;
        Ok(replaced)
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
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
    file.try_lock().context("Pending build state is busy")?;
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
struct BoundedWriter<'a> {
    file: &'a mut File,
    remaining: u64,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("Pending build record exceeds byte limit"));
        }
        let count = self.file.write(bytes)?;
        self.remaining -= count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// The name is retained native inspection data, never a field decoded from the record.
pub(super) fn discard(
    state: &Path,
    name: &str,
    expected: [u8; 32],
    cancel: &Cancellation,
) -> Result<bool> {
    cancel.check()?;
    let directory = match open_private_directory(&state.join("pending-builds"), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let guard = native::open_file(&directory, "state.lock")?;
    guard.try_lock().context("Pending build state is busy")?;
    let mut file = match native::open_file(&directory, name) {
        Ok(file) => file,
        Err(error) if missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let (actual, _) =
        crate::engine::io::copy_bounded(&mut file, &mut io::sink(), record::MAX_RECORD, cancel)?;
    if actual != expected {
        return Ok(false);
    }
    drop(file);
    cancel.check()?;
    directory.remove_file(name)?;
    sync(&directory)?;
    Ok(true)
}
