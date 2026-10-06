//! Append-only private backing. Logical source evidence stays on each acquired content value.
use super::*;
use crate::engine::{
    resources::{AdmissionError, AdmissionPermit, ResourceRequest},
    runtime::{RuntimeError, WorkScope},
    staging::PrivateFile,
};
use std::collections::BTreeMap;

struct State {
    file: PrivateFile,
    bytes: u64,
    // A failed or still-running append cannot admit another writer. Existing ranges stay readable.
    unavailable: bool,
    members: BTreeMap<ContentId, (u64, u64)>,
    _ranges: Vec<AdmissionPermit>,
}
pub(super) struct Storage {
    state: Mutex<State>,
    _handles: Option<AdmissionPermit>,
}
impl Storage {
    pub(super) fn read_at(
        &self,
        start: u64,
        bytes: u64,
        position: u64,
        output: &mut [u8],
    ) -> io::Result<usize> {
        if position >= bytes {
            return Ok(0);
        }
        let absolute = start
            .checked_add(position)
            .ok_or_else(|| io::Error::other("Content range overflow"))?;
        let maximum = output
            .len()
            .min(usize::try_from(bytes - position).unwrap_or(usize::MAX));
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.file.file().seek(SeekFrom::Start(absolute))?;
        state.file.file().read(&mut output[..maximum])
    }
}
/// Retain many independent verified files behind one backing and its private directory handle.
/// Readers and content clones keep the entire backing charged, including retired member ranges.
pub struct ContentPool {
    storage: Arc<Storage>,
    maximum: u64,
}
impl ContentPool {
    /// Synchronous assembly callers cover this storage with their enclosing worker reservation.
    /// No native path or writer is exposed; only verified, bounded input can be inserted.
    pub(in crate::engine) fn new(maximum: u64) -> Result<Self> {
        Ok(Self {
            storage: Arc::new(Storage {
                state: Mutex::new(State {
                    file: PrivateFile::new()?,
                    bytes: 0,
                    unavailable: false,
                    members: BTreeMap::new(),
                    _ranges: vec![],
                }),
                _handles: None,
            }),
            maximum,
        })
    }
    /// Async acquisition retains storage reservations on the backing rather than on a UI handle.
    pub async fn owned(scope: &mut WorkScope, maximum: u64) -> Result<Self> {
        let retained = ResourceRequest {
            memory_bytes: 4096,
            open_files: 2,
            ..Default::default()
        };
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 4,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                Self::new(maximum)
            },
        )?;
        let (mut pool, permit) = scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts();
        Arc::get_mut(&mut pool.storage)
            .expect("unshared new pool")
            ._handles = Some(permit);
        Ok(pool)
    }
    /// Consolidate when temporary overlap can be admitted. Otherwise preserve an already
    /// charged source lease; reducing handles must not require additional scratch capacity.
    pub async fn consolidate_owned(
        &mut self,
        scope: &mut WorkScope,
        content: AcquiredContent,
    ) -> Result<AcquiredContent> {
        let exists = {
            let state = self
                .storage
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            ensure!(
                !state.unavailable,
                "Content pool has an unfinished or failed append"
            );
            state.members.contains_key(&content.lease().id())
        };
        let reservation = if exists {
            None
        } else {
            match scope.reserve_storage(ResourceRequest {
                scratch_bytes: content.lease().len(),
                memory_bytes: 1024,
                ..Default::default()
            }) {
                Ok(permit) => Some(permit),
                Err(RuntimeError::Admission(
                    AdmissionError::Busy { .. } | AdmissionError::TooLarge { .. },
                )) if content.lease.0._reservation.is_some()
                    || matches!(
                        &content.lease.0.backing, ContentBacking::Packed { storage, .. } if storage._handles.is_some()
                    ) =>
                {
                    return Ok(content);
                }
                Err(error) => return Err(error.into()),
            }
        };
        let storage = self.storage.clone();
        let maximum = self.maximum;
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 10,
                ..Default::default()
            },
            ResourceRequest::default(),
            move |cancel| insert(storage, maximum, content, reservation, &cancel),
        )?;
        Ok(scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts()
            .0)
    }
    pub(in crate::engine) fn insert(
        &mut self,
        content: AcquiredContent,
        cancel: &Cancellation,
    ) -> Result<AcquiredContent> {
        insert(self.storage.clone(), self.maximum, content, None, cancel)
    }
}
fn insert(
    storage: Arc<Storage>,
    maximum: u64,
    content: AcquiredContent,
    reservation: Option<AdmissionPermit>,
    cancel: &Cancellation,
) -> Result<AcquiredContent> {
    cancel.check()?;
    let id = content.lease().id();
    let bytes = content.lease().len();
    let offset = {
        let mut state = storage
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ensure!(
            !state.unavailable,
            "Content pool has an unfinished or failed append"
        );
        if let Some(&(offset, stored_bytes)) = state.members.get(&id) {
            ensure!(
                stored_bytes == bytes,
                "Content address has conflicting sizes"
            );
            drop(state);
            return Ok(rebind(content, storage, offset));
        }
        let end = state
            .bytes
            .checked_add(bytes)
            .context("Content pool size overflow")?;
        ensure!(end <= maximum, "Content pool exceeds its byte allowance");
        ensure!(
            state.file.file().metadata()?.len() == state.bytes,
            "Content pool backing changed size"
        );
        // Retain before writing. Cancellation, I/O failure or panic cannot release the charge
        // while any prior reader still keeps a partially appended backing alive.
        if let Some(permit) = reservation {
            state._ranges.push(permit);
        }
        state.unavailable = true;
        state.bytes
    };
    let mut writer = PoolWriter {
        storage: &storage,
        position: offset,
        remaining: bytes,
    };
    // Source and destination locks are never held together, even between two different pools.
    content.lease().copy_verified(&mut writer, cancel)?;
    cancel.check()?;
    {
        let mut state = storage
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ensure!(writer.remaining == 0, "Content pool copy ended early");
        state.file.file().sync_all()?;
        ensure!(
            state.file.file().metadata()?.len() == writer.position,
            "Content pool backing changed size"
        );
        state.bytes = writer.position;
        state.members.insert(id, (offset, bytes));
        state.unavailable = false;
    }
    Ok(rebind(content, storage, offset))
}
fn rebind(content: AcquiredContent, storage: Arc<Storage>, offset: u64) -> AcquiredContent {
    let id = content.lease.id();
    let bytes = content.lease.len();
    // Only bytes are shared. Weaker source assertions and observed-only status never inherit
    // another logical file's stronger evidence through content-address deduplication.
    AcquiredContent {
        lease: ContentLease(Arc::new(ContentObject {
            backing: ContentBacking::Packed { storage, offset },
            id,
            bytes,
            _reservation: None,
        })),
        evidence: content.evidence,
        observed: content.observed,
    }
}
struct PoolWriter<'a> {
    storage: &'a Storage,
    position: u64,
    remaining: u64,
}
impl Write for PoolWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("Content pool write exceeds member"));
        }
        let mut state = self
            .storage
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.file.file().seek(SeekFrom::Start(self.position))?;
        let count = state.file.file().write(bytes)?;
        self.position = self
            .position
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Content pool offset overflow"))?;
        self.remaining -= count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
