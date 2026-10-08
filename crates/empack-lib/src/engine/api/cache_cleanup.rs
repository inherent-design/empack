//! Host-only maintenance uses the same owned approval lifecycle without a project writer.
use super::*;
use crate::engine::{
    content::store::{
        CacheCleanupPlan, CacheCleanupReceipt, CacheObject, FileContentLookup, FileContentStore,
    },
    runtime::WorkScope,
};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub enum CacheCleanRequest {
    /// Evict the canonical objects present during inspection; preserve unknown neighbors.
    All,
}
#[derive(Clone)]
pub struct CacheCleanPreview {
    pub plan: PlanId,
    pub objects: Vec<CacheObject>,
    pub bytes: u64,
    pub replacement: ReplacementSummary,
}
pub struct CacheCleanReceipt {
    pub plan: PlanId,
    /// Native eviction retains its admission reservation and records every completed effect.
    pub objects: CacheCleanupReceipt,
}
pub(super) struct PreparedCacheCleanup {
    pub(super) view: CacheCleanPreview,
    selection: CacheCleanupPlan,
}
impl Engine {
    /// Explicit host wiring. Only the read-only lookup enters maintenance preparation.
    pub fn with_content_store(mut self, store: FileContentStore) -> Self {
        self.content_store = Some(store);
        self
    }
    pub async fn preview_cache_cleanup(
        &self,
        request: CacheCleanRequest,
    ) -> Result<CacheCleanPreview> {
        let prepared = self.prepare_cache_cleanup(request).await?;
        Ok(prepared
            .view()
            .cache_clean()
            .expect("cache preparation")
            .clone())
    }
    pub async fn prepare_cache_cleanup(
        &self,
        request: CacheCleanRequest,
    ) -> Result<PreparedOperation> {
        let lookup = self
            .content_store
            .as_ref()
            .context("Host content store is not configured")?
            .lookup();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = prepare(lookup, request, &mut scope).await.map(|data| {
                    let data = data.map(|value| PreparedKind::CacheClean(Box::new(value)));
                    PreparedOperation {
                        owner,
                        view: Box::new(data.view()),
                        data: Box::new(data),
                    }
                });
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Cache preparation result was not retained")?
    }
}
async fn prepare(
    lookup: FileContentLookup,
    request: CacheCleanRequest,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<PreparedCacheCleanup>> {
    let selection = match request {
        CacheCleanRequest::All => lookup.plan_cleanup(scope).await?,
    };
    // The opaque selection already retains its metadata reservation through execution/receipt.
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            ..Default::default()
        },
        ResourceRequest::default(),
        move |cancel| {
            cancel.check()?;
            let objects: Vec<_> = selection.objects().cloned().collect();
            let mut digest = Sha256::new();
            digest.update(b"empack-cache-cleanup-v2\0");
            for object in &objects {
                let name = object.name();
                digest.update((name.len() as u64).to_le_bytes());
                digest.update(name.as_bytes());
                digest.update(object.bytes.to_le_bytes());
            }
            let view = CacheCleanPreview {
                plan: PlanId(
                    NEXT_PLAN
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                ),
                bytes: selection.bytes()?,
                objects,
                replacement: ReplacementSummary::from_digest(digest.finalize().into()),
            };
            Ok::<_, anyhow::Error>(PreparedCacheCleanup { view, selection })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedCacheCleanup>,
    store: FileContentStore,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let (prepared, _reservation) = prepared.into_parts();
    match store.evict(&mut scope, prepared.selection).await {
        Ok(mut objects) => {
            let failure = objects.failure.take();
            let receipt = ExecutionReceipt::CacheClean(Box::new(CacheCleanReceipt {
                plan: prepared.view.plan,
                objects,
            }));
            Ok(match failure {
                Some(cause) => ExecutionOutcome::PartiallyCompleted { receipt, cause },
                None => ExecutionOutcome::Completed(receipt),
            })
        }
        // A lost worker may have performed deletion without returning its receipt.
        Err(error) if error.downcast_ref::<RuntimeError>().is_some() => {
            Ok(ExecutionOutcome::ExecutionUncertain(error))
        }
        Err(_) if cancel.is_cancelled() => Ok(ExecutionOutcome::InterruptedBeforePublication),
        Err(error) => Ok(ExecutionOutcome::FailedBeforePublication(error)),
    }
}

#[cfg(test)]
mod tests;
