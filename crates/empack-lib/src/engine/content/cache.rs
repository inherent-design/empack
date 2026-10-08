//! Host-selected disposable storage. Inspection never creates it; publication is explicit.
use super::{
    AcquiredContent,
    store::{ContentStoreLimits, FileContentLookup, FileContentStore},
};
use crate::engine::{
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use anyhow::{Result, ensure};
use empack_core::{digest::IntegrityEvidence, model::ExpectedContent};
use std::path::PathBuf;

#[derive(Clone)]
pub struct ContentCache {
    root: PathBuf,
    limits: ContentStoreLimits,
}
impl ContentCache {
    pub fn new(root: PathBuf, limits: ContentStoreLimits) -> Result<Self> {
        ensure!(root.is_absolute(), "Content cache root must be absolute");
        Ok(Self { root, limits })
    }
    /// A missing, busy or damaged disposable store does not prevent authoritative work.
    pub async fn lookup(
        &self,
        scope: &mut WorkScope,
    ) -> Result<RetainedOutput<Option<FileContentLookup>>> {
        let cache = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 4,
                ..Default::default()
            },
            ResourceRequest {
                open_files: 1,
                ..Default::default()
            },
            move |cancel| {
                cancel.check()?;
                FileContentLookup::open_existing(&cache.root, cache.limits)
            },
        )?;
        let result = scope.accept(work.wait().await?)?;
        scope.cancellation().check()?;
        Ok(result.map(|result| match result {
            Ok(lookup) => lookup,
            Err(_) => {
                tracing::debug!("Disposable content cache is unavailable");
                None
            }
        }))
    }
    /// The caller must already own execution approval. Only verified content enters storage.
    pub async fn publish(&self, scope: &mut WorkScope, files: Vec<AcquiredContent>) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let cache = self.clone();
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                open_files: 4,
                ..Default::default()
            },
            ResourceRequest {
                open_files: 1,
                ..Default::default()
            },
            move |cancel| {
                cancel.check()?;
                FileContentStore::open(&cache.root, cache.limits)
            },
        )?;
        let store = scope.accept(work.wait().await?)?;
        scope.cancellation().check()?;
        let store = store.map(Result::ok);
        let Some(store) = store.as_ref() else {
            tracing::debug!("Disposable content cache could not be opened for publication");
            return Ok(());
        };
        for file in files {
            scope.cancellation().check()?;
            let expected = match file.evidence() {
                IntegrityEvidence::MatchedExpected { expected, .. } => ExpectedContent {
                    digests: Some(expected.clone()),
                    size: Some(file.lease().len()),
                    accepted_observation: None,
                },
                IntegrityEvidence::ObservedOnly { actual } => ExpectedContent {
                    digests: None,
                    size: Some(file.lease().len()),
                    accepted_observation: Some(actual.clone()),
                },
            };
            if store.publish_expected(scope, file, expected).await.is_err() {
                scope.cancellation().check()?;
                tracing::debug!("Verified content was not retained in the disposable cache");
                // Capacity, corruption and contention do not authorize cleanup or retry loops.
                break;
            }
        }
        Ok(())
    }
}
