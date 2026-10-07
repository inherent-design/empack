//! Disposable cache eviction binds native objects and never recursively removes storage.
use super::*;
use crate::engine::{native::ObjectIdentity, resources::AdmissionPermit};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheObject {
    pub id: ContentId,
    pub bytes: u64,
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
    /// Capture only canonical cache-object names. Unknown neighbors and scratch remain untouched.
    /// Limits bound enumeration; the returned plan charges its actual retained selection size.
    pub async fn plan_cleanup(&self, scope: &mut WorkScope) -> Result<CacheCleanupPlan> {
        let lookup = self.clone();
        let maximum = metadata_reservation(self.0.limits.entries)?;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: maximum,
                open_files: 4,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: maximum,
                ..Default::default()
            },
            move |cancel| lookup.0.plan_cleanup(&cancel),
        )?;
        let (mut plan, mut permit) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
        plan._reservation = Some(permit.split(ResourceRequest {
            memory_bytes: metadata_reservation(plan.selected.len())?,
            ..Default::default()
        })?);
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
impl Store {
    fn plan_cleanup(&self, cancel: &Cancellation) -> Result<CacheCleanupPlan> {
        cancel.check()?;
        let _guard = self.lock(false)?;
        let mut selected = Vec::new();
        for (seen, entry) in self.root.entries()?.enumerate() {
            cancel.check()?;
            ensure!(
                seen < self.limits.entries.saturating_add(1),
                "Content store exceeds entry limit"
            );
            let Some(id) = entry?.file_name().to_str().and_then(parse_name) else {
                continue;
            };
            let file = native::open_file(&self.root, &name(&id))?;
            let binding = binding(&file)?;
            // Cache contents may be corrupt or oversized; cleanup still owns their verified native
            // identities. It never reads or authenticates their payload as part of eviction.
            selected.push(SelectedObject {
                object: CacheObject {
                    id,
                    bytes: binding.bytes,
                },
                binding,
            });
        }
        selected.sort_unstable_by(|a, b| a.object.id.cmp(&b.object.id));
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
            let file = native::open_file(&self.root, &name(&entry.object.id))?;
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
                let target = name(&entry.object.id);
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
