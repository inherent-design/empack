//! File publication with durable host-private recovery data. No installer is replayed.
use super::{
    io::copy_bounded,
    layout::{CollisionIndex, ProjectLayout},
    native::{self, ObjectIdentity},
    snapshot::{FileObservation, Observation, ProjectReadRoot, SnapshotLimits, observe_file},
    staging::create_temporary,
    verification::{VerifiedFileChange, content},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, OpenOptions};
use empack_core::{
    files::{FileChange, FileContent},
    path::{PathSyntax, PortableRelPath},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const JOURNAL_SCHEMA: u32 = 4;
mod creation;
mod recovery;
pub use creation::PreparedRootCreation;
pub use recovery::{PreparedRecovery, RecoveryAction, RecoveryStatus};
const JOURNAL_LIMIT: u64 = 16 * 1024 * 1024;
static NEXT_OPERATION: AtomicU64 = AtomicU64::new(0);

/// Private host state is separate from project data and evictable download caches.
pub struct Publisher {
    host: Dir,
}
/// Read-only journal access for snapshot preparation. It cannot create or recover state.
#[derive(Clone)]
pub struct RecoveryReader {
    host_state: std::path::PathBuf,
}
/// A shared OS lock when an existing publication coordination file is present.
pub(super) struct ProjectReadGuard {
    _lock: Option<File>,
}
impl RecoveryReader {
    pub fn new(host_state: std::path::PathBuf) -> Self {
        Self { host_state }
    }
    pub(super) fn enter(&self, root: &ProjectReadRoot) -> Result<ProjectReadGuard> {
        let Some(publisher) = Publisher::open_existing(&self.host_state)? else {
            return Ok(ProjectReadGuard { _lock: None });
        };
        root.check_binding()?;
        let state = match publisher.host.open_dir_nofollow(root_key(root)?) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProjectReadGuard { _lock: None });
            }
            Err(error) => return Err(error.into()),
        };
        let file = match native::open_file(&state, "operation.lock") {
            Ok(value) => Some(value),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(file) = &file {
            file.try_lock_shared()
                .context("Project publication is busy")?;
        }
        // Inspect after acquiring coordination; corrupt and hot journals both prevent ordinary reads.
        ensure!(
            !publisher.recovery_required(root)?,
            "Project requires publication recovery before ordinary reads"
        );
        Ok(ProjectReadGuard { _lock: file })
    }
}
/// Structured error context: durable publication may have started for this operation.
#[derive(Debug, thiserror::Error)]
#[error("Publication recovery required for {operation}")]
pub struct RecoveryRequired {
    pub operation: String,
}

/// Whether the receipt describes publication or restoration of an interrupted operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationDisposition {
    Published,
    Restored,
}

/// Durable state reached by a publication attempt. Errors after intent persistence require recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReceipt {
    pub disposition: PublicationDisposition,
    pub operation: String,
    pub changed_files: usize,
    /// Unix directory synchronization was performed. Windows reports file synchronization only.
    pub directory_synced: bool,
    /// Native POSIX executable bits were observable and checked on this platform.
    pub executable_bits_verified: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationPoint {
    RecoveryDataDurable,
    CreationIndexDurable,
    IntentDurable,
    SiblingWritten,
    SiblingSynced,
    TargetChanged,
    DirectorySynced,
    ProgressDurable,
    Verified,
    Committed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    volume: u64,
    object: [u8; 16],
    created: Option<(u64, u32)>,
}
impl From<ObjectIdentity> for Binding {
    fn from(value: ObjectIdentity) -> Self {
        Self {
            volume: value.volume,
            object: value.object.to_le_bytes(),
            created: value.created,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    sha256: [u8; 32],
    bytes: u64,
    readonly: bool,
    executable: bool,
}
impl From<&FileContent> for Fingerprint {
    fn from(value: &FileContent) -> Self {
        Self {
            sha256: *value.content.bytes(),
            bytes: value.bytes,
            readonly: value.permissions.readonly,
            executable: value.permissions.executable,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    target: String,
    before: Option<Fingerprint>,
    before_object: Option<Binding>,
    after: Option<Fingerprint>,
    sibling: Option<String>,
    unix_mode: Option<u32>,
    applied: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    root: Binding,
    operation: String,
    changes: Vec<Change>,
    groups: Vec<JournalCapture>,
    expected: BTreeMap<String, Fingerprint>,
    restoring: bool,
    retained_files: BTreeSet<String>,
    committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalCapture {
    scopes: Vec<String>,
    limits: SnapshotLimits,
    filter: Option<super::source::CaptureFilter>,
}

impl Publisher {
    /// The composition root supplies trusted host state, never an imported project directory.
    pub fn open(host_state: &Path) -> Result<Self> {
        Self::open_impl(host_state, true)
    }

    fn open_impl(host_state: &Path, create: bool) -> Result<Self> {
        if create && !host_state.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(host_state)?;
            }
            #[cfg(windows)]
            super::windows_privacy::create(host_state)?;
        }
        let metadata = std::fs::symlink_metadata(host_state)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Journal state is not a private directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid has no preconditions or borrowed storage.
            ensure!(
                metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
                "Journal state must be owned by this user and private (0700)"
            );
        }
        let host = Dir::open_ambient_dir(host_state, cap_std::ambient_authority())?;
        native::reject_reparse(&host.try_clone()?.into_std_file())?;
        #[cfg(windows)]
        super::windows_privacy::verify(&host)?;
        Ok(Self { host })
    }

    /// Open already-created private state without creating directories during preview.
    pub fn open_existing(host_state: &Path) -> Result<Option<Self>> {
        match Self::open_impl(host_state, false) {
            Ok(publisher) => Ok(Some(publisher)),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Report unfinished publication before allowing ordinary project reads or planning.
    pub fn recovery_required(&self, root: &ProjectReadRoot) -> Result<bool> {
        root.check_binding()?;
        let key = root_key(root)?;
        let directory = match self.host.open_dir_nofollow(key) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        native::reject_reparse(&directory.try_clone()?.into_std_file())?;
        let journal = load_journal(&directory)?;
        if let Some(journal) = &journal {
            validate_journal(journal, root)?;
        }
        let creation_pending = creation::requires_root_recovery(&directory, root)?;
        Ok(creation_pending || journal.is_some_and(|journal| !journal.committed))
    }

    pub fn publish(
        &self,
        root: &ProjectReadRoot,
        verified: VerifiedFileChange,
        cancel: &Cancellation,
    ) -> Result<PublicationReceipt> {
        self.publish_with_hook(root, verified, cancel, &mut |_| Ok(()))
    }

    fn publish_with_hook(
        &self,
        root: &ProjectReadRoot,
        verified: VerifiedFileChange,
        cancel: &Cancellation,
        hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
    ) -> Result<PublicationReceipt> {
        let (plan, mut stage, base) = verified.into_parts();
        let state = self.project_state(root)?;
        let _lock = lock(&state)?;
        ensure!(
            !creation::requires_root_recovery(&state, root)?,
            "Project requires creation recovery"
        );
        if let Some(journal) = load_journal(&state)? {
            validate_journal(&journal, root)?;
            ensure!(journal.committed, "Project requires publication recovery");
        }
        root.revalidate(&base, cancel)?;
        let operation = format!(
            "op-{:x}-{:x}-{:x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            std::process::id(),
            NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)
        );
        state.create_dir(&operation)?;
        let retained = state.open_dir_nofollow(&operation)?;
        private_directory(&retained)?;
        sync_directory(&state)?;
        let mut changes = Vec::new();
        let mut retained_files = BTreeSet::new();
        for (index, change) in plan.changes().iter().enumerate() {
            cancel.check()?;
            let path = ProjectLayout::path(change.target())?;
            let old = match base.entries().get(&path) {
                Some(Observation::File(file)) => Some(file),
                _ => None,
            };
            if let Some(old) = old {
                let (parent, leaf) = native::parent(&root.directory, &path)?;
                let mut file = native::open_file(&parent, &leaf)?;
                ensure!(
                    native::identity(&file)? == old.object,
                    "Original changed before retaining recovery data"
                );
                let mut before = new_retained_file(&retained, &format!("before-{index}"))?;
                let (digest, bytes) = copy_bounded(&mut file, &mut before, old.bytes, cancel)?;
                ensure!(
                    digest == old.content && bytes == old.bytes,
                    "Original changed while retaining recovery data"
                );
                before.sync_all()?;
                retained_files.insert(format!("before-{index}"));
            }
            let after = match change {
                FileChange::Replace { after, .. } => Some(Fingerprint::from(after)),
                FileChange::Remove { .. } => None,
            };
            if after.is_some() {
                let mut file = new_retained_file(&retained, &format!("after-{index}"))?;
                stage.copy_verified(&path, &mut file, cancel)?;
                file.sync_all()?;
                retained_files.insert(format!("after-{index}"));
            }
            changes.push(Change {
                target: path.as_str().to_owned(),
                sibling: after
                    .as_ref()
                    .map(|_| format!(".empack-publish-{operation}-{index}")),
                before: old.map(|value| Fingerprint::from(&content(value))),
                before_object: old.map(|value| value.object.into()),
                after,
                #[cfg(unix)]
                unix_mode: old.map(|value| value.mode & 0o777),
                #[cfg(not(unix))]
                unix_mode: None,
                applied: false,
            });
        }
        for index in 0..changes.len() {
            retained_files.insert(format!("restore-{index}"));
        }
        sync_directory(&retained)?;
        hook(PublicationPoint::RecoveryDataDurable)?;
        root.revalidate(&base, cancel)?;
        // The read set can be broader than the write footprint (for example, local
        // sources outside pack/ during an artifact-only build). Retain their postconditions
        // in the journal without giving them a replacement or removal action.
        let mut expected: BTreeMap<_, _> = base
            .entries()
            .iter()
            .filter_map(|(path, observation)| {
                if let Observation::File(file) = observation {
                    Some((path.as_str().to_owned(), Fingerprint::from(&content(file))))
                } else {
                    None
                }
            })
            .collect();
        for change in plan.changes() {
            if matches!(change, FileChange::Remove { .. }) {
                expected.remove(ProjectLayout::path(change.target())?.as_str());
            }
        }
        for (target, file) in plan.expected() {
            expected.insert(
                ProjectLayout::path(target)?.as_str().to_owned(),
                Fingerprint::from(file),
            );
        }
        let mut journal = Journal {
            schema: JOURNAL_SCHEMA,
            root: root.binding.into(),
            operation,
            changes,
            groups: base
                .groups()
                .map(|group| JournalCapture {
                    scopes: group
                        .scopes
                        .iter()
                        .map(|path| path.as_str().to_owned())
                        .collect(),
                    limits: group.limits,
                    filter: group.filter.clone(),
                })
                .collect(),
            expected,
            restoring: false,
            retained_files,
            committed: false,
        };
        validate_journal(&journal, root)?;
        for change in &journal.changes {
            if let Some(sibling) = &change.sibling {
                let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
                match native::parent(&root.directory, &path) {
                    Ok((parent, _)) => match parent.symlink_metadata(sibling) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                        Ok(_) => anyhow::bail!("Publication scratch destination is occupied"),
                    },
                    Err(error)
                        if error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        // From this point cancellation is deferred to a recoverable boundary.
        write_journal(&state, &journal).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        hook(PublicationPoint::IntentDurable).with_context(|| RecoveryRequired {
            operation: journal.operation.clone(),
        })?;
        self.finish(root, &state, &retained, &mut journal, hook)
            .with_context(|| RecoveryRequired {
                operation: journal.operation.clone(),
            })
    }

    /// Roll forward retained verified bytes. Conflicting external edits are never overwritten.
    pub fn recover(&self, root: &ProjectReadRoot) -> Result<PublicationReceipt> {
        self.recover_checked(root, None)
    }
    fn recover_checked(
        &self,
        root: &ProjectReadRoot,
        expected: Option<&PreparedRecovery>,
    ) -> Result<PublicationReceipt> {
        let state = self.project_state(root)?;
        let _lock = lock(&state)?;
        let mut journal = load_journal(&state)?.context("No publication journal exists")?;
        validate_journal(&journal, root)?;
        if let Some(expected) = expected {
            expected.check(root, &journal)?;
        }
        if journal.committed {
            return Ok(receipt(&journal));
        }
        let retained = state.open_dir_nofollow(&journal.operation)?;
        native::reject_reparse(&retained.try_clone()?.into_std_file())?;
        self.finish(root, &state, &retained, &mut journal, &mut |_| Ok(()))
            .with_context(|| RecoveryRequired {
                operation: journal.operation.clone(),
            })
    }

    /// Restore only the journal's own applied files; unrelated edits remain conflicts.
    /// The inverse operation is persisted before its first replacement and can itself resume.
    pub fn restore_before_images(&self, root: &ProjectReadRoot) -> Result<PublicationReceipt> {
        self.restore_with_hook(root, &mut |_| Ok(()))
    }

    fn restore_with_hook(
        &self,
        root: &ProjectReadRoot,
        hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
    ) -> Result<PublicationReceipt> {
        self.restore_checked(root, None, hook)
    }
    fn restore_checked(
        &self,
        root: &ProjectReadRoot,
        expected: Option<&PreparedRecovery>,
        hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
    ) -> Result<PublicationReceipt> {
        let state = self.project_state(root)?;
        let _lock = lock(&state)?;
        let mut journal = load_journal(&state)?.context("No publication journal exists")?;
        validate_journal(&journal, root)?;
        if let Some(expected) = expected {
            expected.check(root, &journal)?;
        }
        if journal.committed {
            ensure!(
                journal.restoring,
                "A committed publication requires a newly planned rollback"
            );
            return Ok(receipt(&journal));
        }
        let retained = state.open_dir_nofollow(&journal.operation)?;
        native::reject_reparse(&retained.try_clone()?.into_std_file())?;
        if !journal.restoring {
            // Restoring a corrupt pending candidate needs before-images, not valid after-images.
            preflight_recovery(root, &retained, &journal, false)?;
            let mut inverse = Vec::new();
            let mut expected = journal.expected.clone();
            for (index, change) in journal.changes.iter().enumerate() {
                match &change.before {
                    Some(before) => {
                        expected.insert(change.target.clone(), before.clone());
                    }
                    None => {
                        expected.remove(&change.target);
                    }
                }
                let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
                let actual = current_file(root, &path, change)?;
                let fingerprint = actual
                    .as_ref()
                    .map(|file| Fingerprint::from(&content(file)));
                if fingerprints_match(fingerprint.as_ref(), change.before.as_ref()) {
                    continue;
                }
                ensure!(
                    fingerprints_match(fingerprint.as_ref(), change.after.as_ref()),
                    "Restoration conflict at {}",
                    change.target
                );
                let inverse_index = inverse.len();
                if let Some(before) = &change.before {
                    let name = format!("restore-{inverse_index}");
                    let mut source = native::open_file(&retained, &format!("before-{index}"))?;
                    // An earlier preparation may have stopped before changing journal direction.
                    discard_sibling(&retained, &name)?;
                    let mut candidate = new_retained_file(&retained, &name)?;
                    let (digest, bytes) = copy_bounded(
                        &mut source,
                        &mut candidate,
                        before.bytes,
                        &Cancellation::default(),
                    )?;
                    ensure!(
                        digest == before.sha256 && bytes == before.bytes,
                        "Before-image is corrupt"
                    );
                    candidate.sync_all()?;
                    journal.retained_files.insert(name);
                }
                inverse.push(Change {
                    target: change.target.clone(),
                    before: fingerprint,
                    before_object: actual.as_ref().map(|file| file.object.into()),
                    after: change.before.clone(),
                    sibling: change
                        .before
                        .as_ref()
                        .map(|_| format!(".empack-publish-{}-{inverse_index}", journal.operation)),
                    unix_mode: change.unix_mode,
                    applied: false,
                });
            }
            sync_directory(&retained)?;
            journal.changes = inverse;
            journal.expected = expected;
            journal.restoring = true;
            validate_journal(&journal, root)?;
            write_journal(&state, &journal)?;
            hook(PublicationPoint::IntentDurable)?;
        }
        self.finish(root, &state, &retained, &mut journal, hook)
            .with_context(|| RecoveryRequired {
                operation: journal.operation.clone(),
            })
    }

    /// Reclaim only files named by a committed host journal; the durable receipt remains.
    pub fn reclaim_committed(&self, root: &ProjectReadRoot) -> Result<u64> {
        let state = self.project_state(root)?;
        let _lock = lock(&state)?;
        let journal = load_journal(&state)?.context("No publication journal exists")?;
        validate_journal(&journal, root)?;
        ensure!(
            journal.committed,
            "Active recovery data cannot be reclaimed"
        );
        let retained = match state.open_dir_nofollow(&journal.operation) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        native::reject_reparse(&retained.try_clone()?.into_std_file())?;
        let mut bytes = 0u64;
        for name in &journal.retained_files {
            match retained.symlink_metadata(name) {
                Ok(metadata) => {
                    ensure!(
                        metadata.is_file() && !metadata.file_type().is_symlink(),
                        "Retained journal object is not a file"
                    );
                    retained.remove_file(name)?;
                    bytes = bytes
                        .checked_add(metadata.len())
                        .context("Reclaimed byte count overflow")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        sync_directory(&retained)?;
        drop(retained);
        // Unexpected objects prevent directory removal; never recursively delete them.
        state.remove_dir(&journal.operation)?;
        sync_directory(&state)?;
        Ok(bytes)
    }

    fn finish(
        &self,
        root: &ProjectReadRoot,
        state: &Dir,
        retained: &Dir,
        journal: &mut Journal,
        hook: &mut dyn FnMut(PublicationPoint) -> Result<()>,
    ) -> Result<PublicationReceipt> {
        preflight_recovery(root, retained, journal, true)?;
        for index in 0..journal.changes.len() {
            root.check_binding()?;
            let change = &journal.changes[index];
            let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
            let current = current_file(root, &path, change)?;
            let fingerprint = current
                .as_ref()
                .map(|value| Fingerprint::from(&content(value)));
            if !fingerprints_match(fingerprint.as_ref(), change.after.as_ref()) {
                ensure!(
                    fingerprints_match(fingerprint.as_ref(), change.before.as_ref())
                        && current.as_ref().map(|file| Binding::from(file.object))
                            == change.before_object,
                    "Publication conflict at {}",
                    change.target
                );
                let (parent, leaf) = publication_parent(&root.directory, &path)?;
                match &change.after {
                    Some(after) => {
                        let mut source =
                            native::open_file(retained, &candidate_name(journal, index))?;
                        let temporary = change
                            .sibling
                            .as_ref()
                            .context("Replacement has no journal-owned sibling")?;
                        discard_sibling(&parent, temporary)?;
                        let mut candidate = new_retained_file(&parent, temporary)?;
                        let result: Result<()> = (|| {
                            let (digest, bytes) = copy_bounded(
                                &mut source,
                                &mut candidate,
                                after.bytes,
                                &Cancellation::default(),
                            )?;
                            ensure!(
                                digest == after.sha256 && bytes == after.bytes,
                                "Retained candidate is corrupt"
                            );
                            set_permissions(&candidate, after, change.unix_mode)?;
                            hook(PublicationPoint::SiblingWritten)?;
                            candidate.sync_all()?;
                            hook(PublicationPoint::SiblingSynced)?;
                            drop(candidate);
                            // Recheck immediately before replacement, independently of progress flags.
                            check_pending(root, &path, change)?;
                            parent.rename(temporary, &parent, &leaf)?;
                            Ok(())
                        })();
                        if result.is_err() {
                            let _ = parent.remove_file(temporary);
                        }
                        result?;
                    }
                    None => {
                        check_pending(root, &path, change)?;
                        parent.remove_file(&leaf)?;
                    }
                }
                hook(PublicationPoint::TargetChanged)?;
                sync_directory(&parent)?;
                hook(PublicationPoint::DirectorySynced)?;
            }
            journal.changes[index].applied = true;
            write_journal(state, journal)?;
            hook(PublicationPoint::ProgressDurable)?;
        }
        verify_after(root, journal)?;
        hook(PublicationPoint::Verified)?;
        journal.committed = true;
        write_journal(state, journal)?;
        hook(PublicationPoint::Committed)?;
        Ok(receipt(journal))
    }

    fn project_state(&self, root: &ProjectReadRoot) -> Result<Dir> {
        root.check_binding()?;
        let key = root_key(root)?;
        match self.host.create_dir(&key) {
            Ok(()) => sync_directory(&self.host)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let directory = self.host.open_dir_nofollow(&key)?;
        private_directory(&directory)?;
        Ok(directory)
    }
}

fn candidate_name(journal: &Journal, index: usize) -> String {
    format!(
        "{}-{index}",
        if journal.restoring {
            "restore"
        } else {
            "after"
        }
    )
}

fn root_key(root: &ProjectReadRoot) -> Result<String> {
    binding_key(&root.binding.into())
}
fn binding_key(binding: &Binding) -> Result<String> {
    let (seconds, nanos) = binding.created.context(
        "Filesystem lacks durable directory creation identity; publication is unsupported",
    )?;
    Ok(format!(
        "project-{:016x}-{:032x}-{seconds:x}-{nanos:x}",
        binding.volume,
        u128::from_le_bytes(binding.object)
    ))
}

fn discard_sibling(parent: &Dir, name: &str) -> Result<()> {
    match parent.symlink_metadata(name) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Journal sibling is not a regular file"
            );
            parent.remove_file(name)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn preflight_recovery(
    root: &ProjectReadRoot,
    retained: &Dir,
    journal: &Journal,
    verify_candidates: bool,
) -> Result<()> {
    for (index, change) in journal.changes.iter().enumerate() {
        let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
        let actual = current_file(root, &path, change)?;
        let fingerprint = actual
            .as_ref()
            .map(|file| Fingerprint::from(&content(file)));
        if fingerprints_match(fingerprint.as_ref(), change.after.as_ref()) {
            continue;
        }
        check_pending(root, &path, change)?;
        if verify_candidates && let Some(after) = &change.after {
            let mut candidate = native::open_file(retained, &candidate_name(journal, index))?;
            let (hash, bytes) = copy_bounded(
                &mut candidate,
                &mut std::io::sink(),
                after.bytes,
                &Cancellation::default(),
            )?;
            ensure!(
                hash == after.sha256 && bytes == after.bytes,
                "Retained candidate is corrupt"
            );
        }
    }
    // Rebuild journal-owned scratch only after every retained candidate verifies.
    for change in &journal.changes {
        if let Some(sibling) = &change.sibling {
            let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
            match native::parent(&root.directory, &path) {
                Ok((parent, _)) => {
                    discard_sibling(&parent, sibling)?;
                    sync_directory(&parent)?;
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
    }
    let snapshot = capture_journal(root, journal)?;
    let changing: BTreeSet<_> = journal
        .changes
        .iter()
        .map(|change| change.target.as_str())
        .collect();
    let mut unchanged = BTreeMap::new();
    for (path, observation) in snapshot.entries() {
        if let Observation::File(file) = observation {
            if changing.contains(path.as_str()) {
                continue;
            }
            unchanged.insert(path.as_str().to_owned(), Fingerprint::from(&content(file)));
        }
    }
    let expected: BTreeMap<_, _> = journal
        .expected
        .iter()
        .filter(|(path, _)| !changing.contains(path.as_str()))
        .map(|(path, file)| (path.clone(), file.clone()))
        .collect();
    ensure!(
        inventories_match(&unchanged, &expected),
        "Unchanged project inputs conflict with publication recovery"
    );
    Ok(())
}

fn private_directory(directory: &Dir) -> Result<()> {
    #[cfg(windows)]
    super::windows_privacy::verify(directory)?;
    let file = directory.try_clone()?.into_std_file();
    native::reject_reparse(&file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        native::readable_directory(directory)?
            .set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn lock(directory: &Dir) -> Result<File> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .follow(FollowSymlinks::No);
    let file = directory.open_with("operation.lock", &options)?.into_std();
    ensure!(
        file.metadata()?.is_file(),
        "Operation lock is not a regular file"
    );
    file.try_lock().context("Project publication is busy")?;
    Ok(file)
}
fn new_retained_file(directory: &Dir, name: &str) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(directory.open_with(name, &options)?.into_std())
}
fn sync_directory(directory: &Dir) -> Result<()> {
    #[cfg(unix)]
    native::readable_directory(directory)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}
fn write_journal(directory: &Dir, journal: &Journal) -> Result<()> {
    let bytes = serde_json::to_vec(journal)?;
    ensure!(
        bytes.len() as u64 <= JOURNAL_LIMIT,
        "Publication journal exceeds byte limit"
    );
    let (temporary, mut file) = create_temporary(directory)?;
    let result: Result<()> = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        directory.rename(&temporary, directory, "journal.json")?;
        sync_directory(directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
}
fn load_journal(directory: &Dir) -> Result<Option<Journal>> {
    let mut file = match native::open_file(directory, "journal.json") {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    copy_bounded(
        &mut file,
        &mut bytes,
        JOURNAL_LIMIT,
        &Cancellation::default(),
    )?;
    let journal: Journal = serde_json::from_slice(&bytes).context("Corrupt publication journal")?;
    ensure!(
        journal.schema == JOURNAL_SCHEMA,
        "Unsupported publication journal schema"
    );
    Ok(Some(journal))
}
fn validate_journal(journal: &Journal, root: &ProjectReadRoot) -> Result<()> {
    ensure!(
        journal.schema == JOURNAL_SCHEMA && journal.root == Binding::from(root.binding),
        "Journal root or schema mismatch"
    );
    PortableRelPath::parse(&journal.operation, PathSyntax::ArtifactName)?;
    ensure!(
        journal.operation.starts_with("op-"),
        "Invalid operation identity"
    );
    let mut collisions = CollisionIndex::default();
    for (index, change) in journal.changes.iter().enumerate() {
        let expected_sibling = change
            .after
            .as_ref()
            .map(|_| format!(".empack-publish-{}-{index}", journal.operation));
        ensure!(
            change.sibling == expected_sibling,
            "Invalid journal-owned sibling"
        );
        let path = PortableRelPath::parse(&change.target, PathSyntax::ProjectContent)?;
        ProjectLayout::classify(&path)?;
        collisions.insert_file(&path)?;
        ensure!(
            change.before.is_some() == change.before_object.is_some(),
            "Invalid prior object binding"
        );
        ensure!(
            !fingerprints_match(change.before.as_ref(), change.after.as_ref()),
            "Journal contains a no-op change"
        );
        ensure!(
            journal.expected.get(&change.target) == change.after.as_ref(),
            "Journal postcondition mismatch"
        );
    }
    let mut expected = CollisionIndex::default();
    for path in journal.expected.keys() {
        let path = PortableRelPath::parse(path, PathSyntax::ProjectContent)?;
        expected.insert_file(&path)?;
    }
    for name in &journal.retained_files {
        PortableRelPath::parse(name, PathSyntax::ArtifactName)?;
        let (kind, index) = name
            .split_once('-')
            .context("Invalid retained object name")?;
        ensure!(
            matches!(kind, "before" | "after" | "restore") && index.parse::<usize>().is_ok(),
            "Invalid retained object name"
        );
    }
    for (index, change) in journal.changes.iter().enumerate() {
        if change.after.is_some() {
            ensure!(
                journal
                    .retained_files
                    .contains(&candidate_name(journal, index)),
                "Journal candidate lacks a retention record"
            );
        }
    }
    ensure!(!journal.groups.is_empty(), "Journal lacks capture groups");
    for group in &journal.groups {
        if let Some(filter) = &group.filter {
            filter.matcher()?;
        }
    }
    for path in journal.groups.iter().flat_map(|group| &group.scopes) {
        PortableRelPath::parse(path, PathSyntax::ProjectContent)?;
    }
    Ok(())
}
fn current_file(
    root: &ProjectReadRoot,
    path: &PortableRelPath,
    change: &Change,
) -> Result<Option<FileObservation>> {
    let parent = native::parent(&root.directory, path);
    let (parent, leaf) = match parent {
        Ok(value) => value,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut file = match native::open_file(&parent, &leaf) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let maximum = change
        .before
        .iter()
        .chain(change.after.iter())
        .map(|value| value.bytes)
        .max()
        .unwrap_or(0);
    Ok(Some(observe_file(
        &mut file,
        maximum,
        &Cancellation::default(),
    )?))
}
fn check_pending(root: &ProjectReadRoot, path: &PortableRelPath, change: &Change) -> Result<()> {
    root.check_binding()?;
    let current = current_file(root, path, change)?;
    ensure!(
        fingerprints_match(
            current
                .as_ref()
                .map(|value| Fingerprint::from(&content(value)))
                .as_ref(),
            change.before.as_ref()
        ) && current.as_ref().map(|value| Binding::from(value.object)) == change.before_object,
        "Publication conflict at {}",
        change.target
    );
    Ok(())
}
fn publication_parent(root: &Dir, path: &PortableRelPath) -> Result<(Dir, String)> {
    let mut directory = root.try_clone()?;
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return Ok((directory, part.to_owned()));
        }
        match directory.open_dir_nofollow(part) {
            Ok(next) => {
                native::reject_reparse(&next.try_clone()?.into_std_file())?;
                directory = next;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                directory.create_dir(part)?;
                sync_directory(&directory)?;
                directory = directory.open_dir_nofollow(part)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("portable paths are nonempty")
}
fn set_permissions(file: &File, desired: &Fingerprint, previous_mode: Option<u32>) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut mode = previous_mode.unwrap_or(0o644);
        if desired.executable {
            mode |= 0o100;
        } else {
            mode &= !0o100;
        }
        if desired.readonly {
            mode &= !0o222;
        } else {
            mode |= 0o200;
        }
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let _ = previous_mode;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(desired.readonly);
        file.set_permissions(permissions)?;
    }
    Ok(())
}
fn capture_journal(
    root: &ProjectReadRoot,
    journal: &Journal,
) -> Result<super::snapshot::NativeSnapshot> {
    let mut captured = None;
    for group in &journal.groups {
        let scopes = group
            .scopes
            .iter()
            .map(|path| PortableRelPath::parse(path, PathSyntax::ProjectContent))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let next = root.capture_filtered(
            &scopes,
            group.limits,
            group.filter.as_ref(),
            &Cancellation::default(),
        )?;
        captured = Some(match captured {
            None => next,
            Some(previous) => super::snapshot::NativeSnapshot::merge(previous, next)?,
        });
    }
    captured.context("Journal lacks capture groups")
}
fn verify_after(root: &ProjectReadRoot, journal: &Journal) -> Result<()> {
    let snapshot = capture_journal(root, journal)?;
    let actual: BTreeMap<_, _> = snapshot
        .entries()
        .iter()
        .filter_map(|(path, value)| {
            if let Observation::File(file) = value {
                Some((path.as_str().to_owned(), Fingerprint::from(&content(file))))
            } else {
                None
            }
        })
        .collect();
    ensure!(
        inventories_match(&actual, &journal.expected),
        "Published inventory differs from planned postconditions"
    );
    Ok(())
}
fn fingerprints_match(actual: Option<&Fingerprint>, expected: Option<&Fingerprint>) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            actual.sha256 == expected.sha256
                && actual.bytes == expected.bytes
                && actual.readonly == expected.readonly
                && (!cfg!(unix) || actual.executable == expected.executable)
        }
        (None, None) => true,
        _ => false,
    }
}
fn inventories_match(
    actual: &BTreeMap<String, Fingerprint>,
    expected: &BTreeMap<String, Fingerprint>,
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .all(|(path, file)| fingerprints_match(Some(file), expected.get(path)))
}

fn receipt(journal: &Journal) -> PublicationReceipt {
    PublicationReceipt {
        disposition: if journal.restoring {
            PublicationDisposition::Restored
        } else {
            PublicationDisposition::Published
        },
        operation: journal.operation.clone(),
        changed_files: journal.changes.len(),
        directory_synced: cfg!(unix),
        executable_bits_verified: cfg!(unix),
    }
}

#[cfg(test)]
pub(in crate::engine) mod tests;
