//! Durable verified bytes are disposable cache objects, never recovery or project authority.
use super::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream};
use crate::{
    application::process_runtime::Cancellation,
    engine::{native, resources::ResourceRequest, runtime::WorkScope, staging::create_temporary},
};
use anyhow::{Context, Result, ensure};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use empack_core::{
    digest::{ContentId, ExpectedDigest},
    model::ExpectedContent,
};
use std::{fs::File, path::Path, sync::Arc};

mod cleanup;
pub use cleanup::{CacheCleanupPlan, CacheCleanupReceipt, CacheObject, CacheObjectKind};

#[derive(Clone, Copy)]
pub struct ContentStoreLimits {
    pub file_bytes: u64,
    /// Durable objects and recognized incomplete publication candidates share this capacity.
    pub total_bytes: u64,
    /// Maximum stored objects/candidates; traversal separately bounds unknown neighbors.
    pub entries: usize,
}
impl Default for ContentStoreLimits {
    fn default() -> Self {
        Self {
            file_bytes: 8 << 30,
            total_bytes: 64 << 30,
            entries: 100_000,
        }
    }
}
struct Store {
    root: Dir,
    limits: ContentStoreLimits,
}
/// Read-only capability. It cannot create the store, insert bytes, or evict content.
#[derive(Clone)]
pub struct FileContentLookup(Arc<Store>);
/// Explicit host capability to publish verified cache content.
#[derive(Clone)]
pub struct FileContentStore(Arc<Store>);
/// The address selects bytes; the unchanged source expectation determines their assurance.
#[derive(Clone)]
pub struct CachedFileRequest {
    pub id: ContentId,
    pub expected: ExpectedContent,
    pub maximum: u64,
    pub evidence: SourceEvidencePolicy,
    pub initial: InitialObservation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentStored {
    pub id: ContentId,
    pub bytes: u64,
    pub already_present: bool,
}
struct Selected {
    file: File,
    bytes: u64,
    _lock: File,
}
impl FileContentLookup {
    /// Missing cache state is a miss; inspection never creates directories or coordination files.
    pub fn open_existing(path: &Path, limits: ContentStoreLimits) -> Result<Option<Self>> {
        match open(path, false, limits) {
            Ok(store) => match native::open_file(&store.root, "store.lock") {
                Ok(_) => Ok(Some(Self(Arc::new(store)))),
                Err(error) if missing(&error) => Ok(None),
                Err(error) => Err(error),
            },
            Err(error) if missing(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Native lookup and copying run in admitted workers. Private copied bytes survive eviction.
    pub async fn retain(
        &self,
        scope: &mut WorkScope,
        request: CachedFileRequest,
    ) -> Result<Option<AcquiredContent>> {
        let lookup = self.clone();
        let id = request.id.clone();
        let maximum = request.maximum;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 3,
                ..Default::default()
            },
            ResourceRequest {
                open_files: 2,
                ..Default::default()
            },
            move |cancel| {
                cancel.check()?;
                lookup.0.select(&id, maximum)
            },
        )?;
        let selected = scope.accept(work.wait().await?)?.transpose()?;
        let Some(bytes) = selected.as_ref().map(|selected| selected.bytes) else {
            return Ok(None);
        };
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 256 << 10,
                open_files: 6,
                scratch_bytes: bytes,
            },
            ResourceRequest {
                open_files: 1,
                scratch_bytes: bytes,
                ..Default::default()
            },
            move |cancel| {
                let (selected, _guard) = selected.into_parts();
                verify_selected(
                    selected.context("Selected cache object disappeared")?,
                    &request,
                    &cancel,
                )
            },
        )?;
        Ok(Some(AcquiredContent::retain_resources(
            scope.accept(work.wait().await?)?.transpose()?,
        )?))
    }
}
impl FileContentStore {
    /// Open an already initialized host store without creating storage or coordination files.
    /// Mutation still requires the explicit store capability and an approved operation.
    pub fn open_existing(path: &Path, limits: ContentStoreLimits) -> Result<Option<Self>> {
        Ok(FileContentLookup::open_existing(path, limits)?.map(|lookup| Self(lookup.0)))
    }
    /// The composition root selects host-private storage, never a path from imported metadata.
    pub fn open(path: &Path, limits: ContentStoreLimits) -> Result<Self> {
        let store = open(path, true, limits)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let (lock, created) = match store.root.open_with("store.lock", &options) {
            Ok(file) => (file.into_std(), true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                (native::open_file(&store.root, "store.lock")?, false)
            }
            Err(error) => return Err(error.into()),
        };
        native::reject_reparse(&lock)?;
        ensure!(
            lock.metadata()?.is_file(),
            "Content store lock is not a regular file"
        );
        if created {
            lock.sync_all()?;
            sync(&store.root)?;
        }
        Ok(Self(Arc::new(store)))
    }
    pub fn lookup(&self) -> FileContentLookup {
        FileContentLookup(self.0.clone())
    }
    /// Only an acquired lease can enter the store; it is hashed again before atomic publication.
    /// Persistent candidate bytes consume the store's locked capacity, independently of the
    /// source lease's private-scratch reservation. Failed candidate remnants also consume capacity.
    pub async fn publish_verified(
        &self,
        scope: &mut WorkScope,
        content: AcquiredContent,
    ) -> Result<ContentStored> {
        let bytes = content.lease().len();
        let id = content.lease().id();
        let probe = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 128 << 10,
                open_files: 4,
                ..Default::default()
            },
            ResourceRequest::default(),
            move |cancel| {
                cancel.check()?;
                let _guard = probe.0.lock(false)?;
                probe.0.existing(&id, bytes, &cancel)
            },
        )?;
        if let Some(receipt) = scope
            .accept(work.wait().await?)?
            .transpose()?
            .into_parts()
            .0
        {
            return Ok(receipt);
        }
        let store = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 128 << 10,
                open_files: 5,
                // The store reserves its own bounded capacity under its exclusive lock.
                // The source lease already owns private scratch admission.
                scratch_bytes: 0,
            },
            ResourceRequest::default(),
            move |cancel| store.0.publish(&content, &cancel),
        )?;
        Ok(scope
            .accept(work.wait().await?)?
            .transpose()?
            .into_parts()
            .0)
    }
}
impl Store {
    fn lock(&self, exclusive: bool) -> Result<File> {
        let file = native::open_file(&self.root, "store.lock")?;
        if exclusive {
            file.try_lock().context("Content store is busy")?;
        } else {
            file.try_lock_shared().context("Content store is busy")?;
        }
        Ok(file)
    }
    fn select(&self, id: &ContentId, maximum: u64) -> Result<Option<Selected>> {
        let lock = self.lock(false)?;
        let file = match native::open_file(&self.root, &name(id)) {
            Ok(file) => file,
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let bytes = file.metadata()?.len();
        ensure!(
            bytes <= maximum.min(self.limits.file_bytes),
            "Cached object exceeds byte limit"
        );
        Ok(Some(Selected {
            file,
            bytes,
            _lock: lock,
        }))
    }
    fn check_scan_limit(&self, seen: usize) -> Result<()> {
        // Stream unknown names without granting them object ownership or unbounded scan time.
        const UNOWNED_SCAN_ALLOWANCE: usize = 100_000;
        ensure!(
            seen < self
                .limits
                .entries
                .saturating_add(UNOWNED_SCAN_ALLOWANCE)
                .saturating_add(1),
            "Content store exceeds directory scan limit"
        );
        Ok(())
    }
    fn usage(&self, cancel: &Cancellation) -> Result<(usize, u64)> {
        let mut count = 0usize;
        let mut bytes = 0u64;
        for (seen, entry) in self.root.entries()?.enumerate() {
            cancel.check()?;
            self.check_scan_limit(seen)?;
            let name = entry?.file_name();
            let is_object = name.to_str().and_then(parse_name).is_some();
            let is_candidate = name.to_str().is_some_and(candidate_name);
            if !is_object && !is_candidate {
                continue;
            }
            let file = native::open_native_file(&self.root, &name)?;
            let length = file.metadata()?.len();
            ensure!(
                length <= self.limits.file_bytes,
                "Cached object exceeds byte limit"
            );
            count = count.checked_add(1).context("Content count overflow")?;
            ensure!(
                count <= self.limits.entries,
                "Content store exceeds object limit"
            );
            bytes = bytes.checked_add(length).context("Content size overflow")?;
        }
        Ok((count, bytes))
    }
    // Caller holds shared or exclusive store coordination for the complete verification.
    fn existing(
        &self,
        id: &ContentId,
        bytes: u64,
        cancel: &Cancellation,
    ) -> Result<Option<ContentStored>> {
        ensure!(
            bytes <= self.limits.file_bytes,
            "Content exceeds store file limit"
        );
        let mut file = match native::open_file(&self.root, &name(id)) {
            Ok(file) => file,
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let (digest, count) =
            crate::engine::io::copy_bounded(&mut file, &mut std::io::sink(), bytes, cancel)?;
        ensure!(
            digest == *id.bytes() && count == bytes,
            "Existing cache object is corrupt"
        );
        Ok(Some(ContentStored {
            id: id.clone(),
            bytes,
            already_present: true,
        }))
    }
    fn publish(&self, content: &AcquiredContent, cancel: &Cancellation) -> Result<ContentStored> {
        cancel.check()?;
        let bytes = content.lease().len();
        ensure!(
            bytes <= self.limits.file_bytes,
            "Content exceeds store file limit"
        );
        let id = content.lease().id();
        let target = name(&id);
        let _lock = self.lock(true)?;
        if let Some(receipt) = self.existing(&id, bytes, cancel)? {
            return Ok(receipt);
        }
        let (count, used) = self.usage(cancel)?;
        ensure!(
            count < self.limits.entries
                && used <= self.limits.total_bytes
                && bytes <= self.limits.total_bytes - used,
            "Content store capacity exceeded"
        );
        let (temporary, mut file) = create_temporary(&self.root)?;
        let result = (|| {
            content.lease().copy_verified(&mut file, cancel)?;
            file.sync_all()?;
            // Close the temporary before rename, including on Windows.
            drop(file);
            cancel.check()?;
            self.root.rename(&temporary, &self.root, &target)?;
            sync(&self.root)?;
            Ok(ContentStored {
                id,
                bytes,
                already_present: false,
            })
        })();
        if result.is_err() {
            let _ = self.root.remove_file(&temporary);
        }
        result
    }
}
fn verify_selected(
    mut selected: Selected,
    request: &CachedFileRequest,
    cancel: &Cancellation,
) -> Result<AcquiredContent> {
    let content = verify_stream(
        &mut selected.file,
        &request.expected,
        selected.bytes,
        request.evidence,
        request.initial,
        cancel,
    )?;
    ensure!(
        content.lease().id() == request.id && content.lease().len() == selected.bytes,
        "Cached bytes do not match their content address"
    );
    Ok(content)
}
fn name(id: &ContentId) -> String {
    format!("{}.blob", ExpectedDigest::Sha256(*id.bytes()).hex())
}
fn parse_name(name: &str) -> Option<ContentId> {
    let value = name.strip_suffix(".blob")?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    match ExpectedDigest::parse("sha256", value).ok()? {
        ExpectedDigest::Sha256(bytes) => Some(ContentId::from_sha256(bytes)),
        _ => None,
    }
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}
fn sync(root: &Dir) -> Result<()> {
    #[cfg(unix)]
    native::readable_directory(root)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}
fn open(path: &Path, create: bool, limits: ContentStoreLimits) -> Result<Store> {
    ensure!(path.is_absolute(), "Content store root must be absolute");
    if create && !path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
        }
        #[cfg(windows)]
        crate::engine::windows_privacy::create(path)?;
    }
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Content store root is not a directory"
    );
    let root = Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    let file = root.try_clone()?.into_std_file();
    native::reject_reparse(&file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        // SAFETY: geteuid has no preconditions.
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
            "Content store must be owned by this user and private (0700)"
        );
    }
    #[cfg(windows)]
    crate::engine::windows_privacy::verify(&root)?;
    Ok(Store { root, limits })
}

#[cfg(test)]
mod tests;

// Only the private publisher owns this namespace. Cooperating publications hold the
// exclusive store lock for the entire candidate lifetime.
fn candidate_name(name: &str) -> bool {
    name.strip_prefix(".empack-candidate-")
        .and_then(|suffix| suffix.split_once('-'))
        .is_some_and(|(process, sequence)| {
            !process.is_empty()
                && !sequence.is_empty()
                && process.bytes().all(|byte| byte.is_ascii_digit())
                && sequence.bytes().all(|byte| byte.is_ascii_digit())
        })
}
