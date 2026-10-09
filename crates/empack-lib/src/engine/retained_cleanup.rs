//! Reclaim continuation payloads only when their entire record category is empty.
//! A shared save guard spans content insertion and record publication; cleanup takes it exclusively.
use super::{
    content::store::{CacheCleanupPlan, ContentStoreLimits, FileContentStore},
    native::{self, ObjectIdentity},
    publication::open_private_directory,
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use cap_std::fs::OpenOptions;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

const LOCK: &str = "continuation-content.lock";
const CATEGORIES: [(&str, &str); 3] = [
    ("pending-builds", "pending-content"),
    ("pending-imports", "pending-import-content"),
    ("pending-sync", "pending-sync-content"),
];
fn resources() -> ResourceRequest {
    ResourceRequest {
        jobs: 1,
        memory_bytes: 64 << 10,
        open_files: 6,
        ..Default::default()
    }
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
}
fn guard(state: &Path, exclusive: bool, cancel: &Cancellation) -> Result<(File, ObjectIdentity)> {
    cancel.check()?;
    let directory = open_private_directory(state, !exclusive)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match directory.open_with(LOCK, &options) {
        Ok(file) => {
            file.sync_all()?;
            #[cfg(unix)]
            native::readable_directory(&directory)?.sync_all()?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let file = native::open_file(&directory, LOCK)?;
    if exclusive {
        file.try_lock().context("Continuation storage is busy")?;
    } else {
        file.try_lock_shared()
            .context("Continuation storage is being cleaned")?;
    }
    Ok((file, native::directory_identity(&directory)?))
}
/// Authorized retention only. Hold this until the saved record has been durably published.
pub(in crate::engine) async fn begin_save(
    scope: &mut WorkScope,
    state: PathBuf,
) -> Result<RetainedOutput<File>> {
    let work = scope.spawn_blocking(
        resources(),
        ResourceRequest {
            open_files: 1,
            ..Default::default()
        },
        move |cancel| guard(&state, false, &cancel).map(|(file, _)| file),
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
fn has_records(state: &Path, category: &str, cancel: &Cancellation) -> Result<bool> {
    let directory = match open_private_directory(&state.join(category), false) {
        Ok(directory) => directory,
        Err(error) if missing(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in directory.entries()? {
        cancel.check()?;
        // Unknown or incomplete records conservatively retain the whole category as well.
        if entry?.file_name() != "state.lock" {
            return Ok(true);
        }
    }
    Ok(false)
}
struct StorePlan {
    records: &'static str,
    category: &'static str,
    store: FileContentStore,
    cleanup: CacheCleanupPlan,
}
/// Native cleanup capabilities only; serialized record addresses cannot grant deletion.
pub struct RetainedCleanupPlan {
    state: PathBuf,
    root: ObjectIdentity,
    stores: Vec<StorePlan>,
    preserved: Vec<&'static str>,
    _handles: super::resources::AdmissionPermit,
}
impl RetainedCleanupPlan {
    pub fn preserved(&self) -> &[&'static str] {
        &self.preserved
    }
    pub fn selected(&self) -> Result<Vec<(&'static str, usize, u64)>> {
        self.stores
            .iter()
            .map(|store| {
                Ok((
                    store.category,
                    store.cleanup.objects().count(),
                    store.cleanup.bytes()?,
                ))
            })
            .collect()
    }
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }
}
/// Read-only inspection never creates the state root, stores or save coordination file.
pub async fn prepare(scope: &mut WorkScope, state: PathBuf) -> Result<Option<RetainedCleanupPlan>> {
    let selected = state.clone();
    let work = scope.spawn_blocking(
        resources(),
        ResourceRequest {
            open_files: 3,
            ..Default::default()
        },
        move |cancel| {
            let directory = match open_private_directory(&selected, false) {
                Ok(directory) => directory,
                Err(error) if missing(&error) => return Ok(None),
                Err(error) => return Err(error),
            };
            let root = native::directory_identity(&directory)?;
            let mut stores = Vec::new();
            let mut preserved = Vec::new();
            for (records, content) in CATEGORIES {
                cancel.check()?;
                if has_records(&selected, records, &cancel)? {
                    preserved.push(content);
                } else if let Some(store) = FileContentStore::open_existing(
                    &selected.join(content),
                    ContentStoreLimits::default(),
                )? {
                    stores.push((records, content, store));
                }
            }
            Ok::<_, anyhow::Error>(Some((root, stores, preserved)))
        },
    )?;
    let retained = scope.accept(work.wait().await?)?.transpose()?;
    let (Some((root, stores, preserved)), permit) = retained.into_parts() else {
        return Ok(None);
    };
    let mut planned = Vec::new();
    for (records, category, store) in stores {
        let cleanup = store.lookup().plan_cleanup(scope).await?;
        if cleanup.objects().next().is_some() {
            planned.push(StorePlan {
                records,
                category,
                store,
                cleanup,
            });
        }
    }
    Ok(Some(RetainedCleanupPlan {
        state,
        root,
        stores: planned,
        preserved,
        _handles: permit,
    }))
}
/// Recheck every category under exclusion before deleting any captured store objects.
pub async fn execute(
    scope: &mut WorkScope,
    plan: RetainedCleanupPlan,
) -> Result<Vec<(&'static str, u64)>> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let work = scope.spawn_blocking(resources(), ResourceRequest::default(), move |cancel| {
        // Ownership stays inside the worker even if its caller stops observing the result.
        let RetainedCleanupPlan {
            state,
            root,
            stores,
            _handles,
            preserved: _,
        } = plan;
        let (_guard, current) = guard(&state, true, &cancel)?;
        ensure!(current == root, "Continuation state root changed");
        for store in &stores {
            ensure!(
                !has_records(&state, store.records, &cancel)?,
                "A saved operation now retains {}",
                store.records
            );
        }
        let mut completed = Vec::new();
        for store in stores {
            let receipt = store
                .store
                .evict_captured(store.cleanup, &cancel)
                .with_context(|| {
                    format!("Retained cleanup failed; earlier completed scopes: {completed:?}")
                })?;
            let bytes = receipt.removed.iter().try_fold(0u64, |sum, object| {
                sum.checked_add(object.bytes)
                    .context("Reclaimed byte count overflow")
            })?;
            completed.push((store.category, bytes));
            if let Some(error) = receipt.failure {
                return Err(error.context(format!(
                    "Retained cleanup was partial; removed bytes by scope: {completed:?}"
                )));
            }
        }
        Ok::<_, anyhow::Error>(completed)
    })?;
    Ok(scope
        .accept_publication(work.wait().await?)?
        .transpose()?
        .into_parts()
        .0)
}

#[cfg(test)]
mod tests;
