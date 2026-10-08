//! Disposable cache eviction binds native objects and never recursively removes storage.
use super::*;
use crate::engine::{native::ObjectIdentity, resources::AdmissionPermit};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheObject {
    pub kind: CacheObjectKind,
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheObjectKind {
    Content(ContentId),
    SourceHint(String),
    AbandonedCandidate(String),
}
impl CacheObject {
    /// Display identity only; an executable cleanup still requires its opaque native plan.
    pub fn name(&self) -> String {
        match &self.kind {
            CacheObjectKind::Content(id) => name(id),
            CacheObjectKind::AbandonedCandidate(name) | CacheObjectKind::SourceHint(name) => {
                name.clone()
            }
        }
    }
}
fn object_kind(name: &str) -> Option<CacheObjectKind> {
    parse_name(name)
        .map(CacheObjectKind::Content)
        .or_else(|| index::is_hint(name).then(|| CacheObjectKind::SourceHint(name.into())))
        .or_else(|| candidate_name(name).then(|| CacheObjectKind::AbandonedCandidate(name.into())))
}
#[derive(PartialEq, Eq)]
struct Binding {
    identity: ObjectIdentity,
    bytes: u64,
    modified: SystemTime,
}
struct SelectedObject {
    object: CacheObject,
    binding: Binding,
}
/// An opaque inspected selection. A cache address or serialized pathname cannot forge it.
pub struct CacheCleanupPlan {
    root: ObjectIdentity,
    selected: Vec<SelectedObject>,
    _reservation: Option<AdmissionPermit>,
}
impl CacheCleanupPlan {
    pub fn objects(&self) -> impl Iterator<Item = &CacheObject> {
        self.selected.iter().map(|entry| &entry.object)
    }
    pub fn bytes(&self) -> Result<u64> {
        self.selected.iter().try_fold(0u64, |sum, entry| {
            sum.checked_add(entry.object.bytes)
                .context("Cache cleanup size overflow")
        })
    }
}
/// A failed or cancelled eviction reports the objects already removed and those still retained.
pub struct CacheCleanupReceipt {
    pub removed: Vec<CacheObject>,
    pub retained: Vec<CacheObject>,
    pub failure: Option<anyhow::Error>,
    _reservation: Option<AdmissionPermit>,
}
impl FileContentLookup {
    /// Capture canonical blobs and abandoned publisher candidates under store coordination.
    /// Unknown neighbors remain untouched; active writers hold exclusive coordination.
    /// Limits bound enumeration; the returned plan charges its actual retained selection size.
    pub async fn plan_cleanup(&self, scope: &mut WorkScope) -> Result<CacheCleanupPlan> {
        // Count under shared coordination without accumulating selection metadata. Retain that
        // lock across admission so cooperating writers cannot grow the second pass.
        let lookup = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 10,
                open_files: 3,
                ..Default::default()
            },
            // Enumeration storage retires with this worker. Only the scalar count and
            // coordination descriptor survive; the second pass owns selection memory.
            ResourceRequest {
                open_files: 1,
                ..Default::default()
            },
            move |cancel| lookup.0.scan_cleanup(&cancel),
        )?;
        let scan = scope.accept(work.wait().await?)?.transpose()?;
        let maximum = metadata_reservation(scan.count)?;
        let lookup = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: maximum,
                open_files: 3,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: maximum,
                ..Default::default()
            },
            move |cancel| {
                let (scan, _reservation) = scan.into_parts();
                lookup.0.capture_cleanup(scan, &cancel)
            },
        )?;
        let (mut plan, permit) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
        plan._reservation = Some(permit);
        Ok(plan)
    }
}
impl FileContentStore {
    /// The writer must own the inspected store. Active content leases are independent verified
    /// copies; eviction cannot invalidate them. New objects outside the plan remain untouched.
    pub async fn evict(
        &self,
        scope: &mut WorkScope,
        plan: CacheCleanupPlan,
    ) -> Result<CacheCleanupReceipt> {
        let store = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 3,
                ..Default::default()
            },
            ResourceRequest::default(),
            move |cancel| store.0.evict(plan, &cancel),
        )?;
        // Once deletion starts, cancellation must not discard a receipt of completed effects.
        Ok(scope
            .accept_publication(work.wait().await?)?
            .transpose()?
            .into_parts()
            .0)
    }
}
fn metadata_reservation(entries: usize) -> Result<u64> {
    u64::try_from(entries)?
        .checked_mul(256)
        .and_then(|bytes| bytes.checked_add(64 << 10))
        .context("Cache cleanup metadata limit overflow")
}
fn binding(file: &File) -> Result<Binding> {
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "Cache object is not a regular file");
    Ok(Binding {
        identity: native::identity(file)?,
        bytes: metadata.len(),
        modified: metadata.modified()?,
    })
}
struct CleanupScan {
    count: usize,
    _lock: File,
}
impl Store {
    fn scan_cleanup(&self, cancel: &Cancellation) -> Result<CleanupScan> {
        cancel.check()?;
        let lock = self.lock(false)?;
        let mut count = 0usize;
        for (seen, entry) in self.root.entries()?.enumerate() {
            cancel.check()?;
            self.check_scan_limit(seen)?;
            if entry?.file_name().to_str().and_then(object_kind).is_some() {
                ensure!(
                    count < self.limits.entries,
                    "Content store exceeds object limit"
                );
                count += 1;
            }
        }
        Ok(CleanupScan { count, _lock: lock })
    }
    #[cfg(test)]
    fn plan_cleanup(&self, cancel: &Cancellation) -> Result<CacheCleanupPlan> {
        self.capture_cleanup(self.scan_cleanup(cancel)?, cancel)
    }
    fn capture_cleanup(
        &self,
        scan: CleanupScan,
        cancel: &Cancellation,
    ) -> Result<CacheCleanupPlan> {
        cancel.check()?;
        let mut selected = Vec::with_capacity(scan.count);
        for (seen, entry) in self.root.entries()?.enumerate() {
            cancel.check()?;
            self.check_scan_limit(seen)?;
            let name = entry?.file_name();
            let Some(kind) = name.to_str().and_then(object_kind) else {
                continue;
            };
            ensure!(
                selected.len() < scan.count,
                "Cache membership changed during inspection"
            );
            let file = native::open_native_file(&self.root, &name)?;
            let binding = binding(&file)?;
            // Cache contents may be corrupt or oversized; cleanup still owns their verified native
            // identities. It never reads or authenticates their payload as part of eviction.
            selected.push(SelectedObject {
                object: CacheObject {
                    kind,
                    bytes: binding.bytes,
                },
                binding,
            });
        }
        ensure!(
            selected.len() == scan.count,
            "Cache membership changed during inspection"
        );
        selected.sort_unstable_by_key(|entry| entry.object.name());
        Ok(CacheCleanupPlan {
            root: native::directory_identity(&self.root)?,
            selected,
            _reservation: None,
        })
    }
    fn evict(&self, plan: CacheCleanupPlan, cancel: &Cancellation) -> Result<CacheCleanupReceipt> {
        self.evict_with_hook(plan, cancel, &mut |_| Ok(()))
    }
    fn evict_with_hook(
        &self,
        plan: CacheCleanupPlan,
        cancel: &Cancellation,
        hook: &mut dyn FnMut(usize) -> Result<()>,
    ) -> Result<CacheCleanupReceipt> {
        cancel.check()?;
        ensure!(
            plan.root == native::directory_identity(&self.root)?,
            "Cache cleanup belongs to another store"
        );
        let _guard = self.lock(true)?;
        for entry in &plan.selected {
            cancel.check()?;
            let file = native::open_file(&self.root, &entry.object.name())?;
            ensure!(
                binding(&file)? == entry.binding,
                "Cache object changed after cleanup preparation"
            );
        }
        let mut receipt = CacheCleanupReceipt {
            removed: vec![],
            retained: vec![],
            failure: None,
            _reservation: plan._reservation,
        };
        let mut selected = plan.selected.into_iter();
        while let Some(entry) = selected.next() {
            let result = (|| {
                cancel.check()?;
                let target = entry.object.name();
                let file = native::open_file(&self.root, &target)?;
                ensure!(
                    binding(&file)? == entry.binding,
                    "Cache object changed during eviction"
                );
                drop(file);
                self.root.remove_file(&target)?;
                Ok(())
            })();
            if let Err(error) = result {
                receipt.failure = Some(error);
                receipt.retained.push(entry.object);
                receipt.retained.extend(selected.map(|entry| entry.object));
                break;
            }
            receipt.removed.push(entry.object);
            if let Err(error) = hook(receipt.removed.len()) {
                receipt.failure = Some(error);
                receipt.retained.extend(selected.map(|entry| entry.object));
                break;
            }
        }
        if !receipt.removed.is_empty() {
            let durable = sync(&self.root);
            if receipt.failure.is_none() {
                receipt.failure = durable.err();
            }
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests;
