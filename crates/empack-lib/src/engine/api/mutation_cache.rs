//! Approved mutations cache verified private staged bytes, never reread live project files.
use super::*;
use crate::engine::{
    content::{InitialObservation, cache::ContentCache, verify_stream},
    layout::ProjectLayout,
    runtime::WorkScope,
    staging::FrozenStage,
};
use empack_core::{files::ManagedPath, model::ResolvedProject};

pub(super) trait StagedMutation {
    fn cache_parts(&mut self) -> (&ResolvedProject, &mut FrozenStage);
}
/// All candidate files are already staged and verified. Only execution calls this service;
/// disposable insertion is not a project publication or evidence that the operation completed.
pub(super) async fn publish<T: StagedMutation + Send + 'static>(
    prepared: RetainedOutput<T>,
    cache: Option<ContentCache>,
    scope: &mut WorkScope,
) -> Result<RetainedOutput<T>> {
    let Some(cache) = cache else {
        return Ok(prepared);
    };
    let (mut value, permit) = prepared.into_parts();
    let (project, stage) = value.cache_parts();
    let count = project
        .lock()
        .dependencies
        .values()
        .try_fold(0u64, |count, dep| {
            count
                .checked_add(dep.files.as_slice().len() as u64)
                .context("Cache inventory overflow")
        })?;
    let _index = scope.reserve_storage(ResourceRequest {
        memory_bytes: count
            .checked_mul(4096)
            .context("Cache inventory overflow")?,
        ..Default::default()
    })?;
    let mut sources = Vec::new();
    for dependency in project.lock().dependencies.values() {
        for file in dependency.files.as_slice() {
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                if let Some(Observation::File(observed)) = stage.inventory().get(&path) {
                    sources.push((path, file.expected.clone(), observed.clone()));
                    break;
                }
            }
        }
    }
    let mut prepared = RetainedOutput::from_parts(value, permit);
    for (path, expected, observed) in sources {
        scope.cancellation().check()?;
        let bytes = observed.bytes;
        if bytes > scope.available_scratch_bytes() {
            tracing::debug!("Staged content exceeds spare cache-copy allowance");
            continue;
        }
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 256 << 10,
                open_files: 5,
                scratch_bytes: bytes,
            },
            ResourceRequest {
                open_files: 1,
                scratch_bytes: bytes,
                ..Default::default()
            },
            move |cancel| {
                let (mut value, permit) = prepared.into_parts();
                let (_, stage) = value.cache_parts();
                let content = verify_stream(
                    &mut stage.reader(&path)?,
                    &expected,
                    bytes,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::Accepted,
                    &cancel,
                )?;
                ensure!(
                    *content.lease().id().bytes() == observed.content,
                    "Staged cache source changed"
                );
                Ok::<_, anyhow::Error>((RetainedOutput::from_parts(value, permit), content))
            },
        )?;
        let ((next, content), _content_permit) =
            scope.accept(work.wait().await?)?.transpose()?.into_parts();
        prepared = next;
        cache.publish(scope, vec![content]).await?;
    }
    Ok(prepared)
}
