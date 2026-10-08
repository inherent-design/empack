//! Project artifact cleanup and disposable content eviction have separate receipts.
use super::*;
use crate::engine::{
    api::{CacheCleanRequest, CleanRequest, SavedBuildRecord},
    content::store::{ContentStoreLimits, FileContentStore},
};

#[derive(Clone, Copy)]
struct Selection {
    artifacts: bool,
    cache: bool,
    continuation: bool,
}
impl Selection {
    fn parse(targets: &[String]) -> Result<Self> {
        let mut value = Self {
            artifacts: targets.is_empty(),
            cache: false,
            continuation: false,
        };
        for target in targets {
            match target.as_str() {
                "builds" => value.artifacts = true,
                "cache" => value.cache = true,
                "continuation" => value.continuation = true,
                "all" => {
                    value.artifacts = true;
                    value.cache = true;
                }
                _ => {
                    anyhow::bail!(
                        "Unknown cleanup target {target:?}; choose builds, cache, continuation or all"
                    )
                }
            }
        }
        Ok(value)
    }
}
/// Inspect every selected scope before approval. Artifact publication and cache eviction are
/// separate operations; a later failure reports earlier completed work without claiming rollback.
pub async fn clean(session: &dyn Session, targets: &[String]) -> Result<()> {
    let selected = Selection::parse(targets)?;
    let invocation = session.filesystem().current_dir()?;
    let cache = if selected.cache {
        Some(content_cache_root(
            session.config().app_config(),
            &invocation,
        )?)
    } else {
        None
    };
    clean_selected(session, selected, cache).await
}
async fn clean_selected(
    session: &dyn Session,
    selected: Selection,
    cache: Option<PathBuf>,
) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let store = if let Some(cache) = &cache {
        let store = FileContentStore::open_existing(cache, ContentStoreLimits::default())?;
        if store.is_some() && selected.artifacts && project.join("dist").exists() {
            let cache = cache.canonicalize()?;
            let artifacts = project.join("dist").canonicalize()?;
            ensure!(
                !cache.starts_with(&artifacts) && !artifacts.starts_with(&cache),
                "Artifact and content-cache cleanup scopes overlap"
            );
        }
        store
    } else {
        None
    };
    let has_store = store.is_some();
    let engine = engine(config, &invocation)?;
    let engine = match store {
        Some(store) => engine.with_content_store(store),
        None => engine,
    };
    let result = async {
        let mut plans = Vec::new();
        if selected.artifacts {
            let prepared =
                match cancellable(session, engine.prepare(project.clone(), CleanRequest::Artifacts)).await?
                {
                    Preparation::Ready(value) => value,
                    Preparation::NeedsInput(_) => {
                        anyhow::bail!("Artifact cleanup unexpectedly requires content")
                    }
                };
            let view = prepared.view().clean().context("Missing cleanup preview")?;
            show_changes(session, &view.files)?;
            session.display().status().info(&format!(
                "Remove {} bytes from dist; recovery retains before-images",
                view.removed_bytes
            ));
            if !view.files.changes().is_empty() {
                plans.push(("Artifact cleanup", prepared));
            }
        }
        if selected.cache {
            if has_store {
                let prepared = cancellable(
                    session,
                    engine.prepare_cache_cleanup(CacheCleanRequest::All),
                )
                .await?;
                let view = prepared
                    .view()
                    .cache_clean()
                    .context("Missing cache cleanup preview")?;
                for object in &view.objects {
                    session.display().status().info(&format!(
                        "Evict {} ({} bytes)",
                        object.name(),
                        object.bytes
                    ));
                }
                session.display().status().info(&format!(
                    "Evict up to {} bytes from {} content-store entries",
                    view.bytes,
                    view.objects.len()
                ));
                if !view.objects.is_empty() {
                    plans.push(("Content cache cleanup", prepared));
                }
            } else {
                session
                    .display()
                    .status()
                    .info("No initialized content cache to clean");
            }
        }
        let saved = if selected.continuation {
            let saved = cancellable(session, engine.observe_saved_build(project)).await?;
            session.display().status().info(if saved.is_some() {
                "Discard this project's saved build recipe; retained content and recovery journals remain"
            } else {
                "No saved build recipe to discard"
            });
            saved
        } else { None };
        apply_cleanup(session, &engine, plans, saved).await
    }
    .await;
    engine.shutdown().await;
    result
}
async fn apply_cleanup(
    session: &dyn Session,
    engine: &Engine,
    plans: Vec<(&'static str, PreparedOperation)>,
    saved: Option<SavedBuildRecord>,
) -> Result<()> {
    if plans.is_empty() && saved.is_none() {
        session
            .display()
            .status()
            .complete("No selected cleanup changes");
        return Ok(());
    }
    if plans.len() + usize::from(saved.is_some()) > 1 {
        session.display().status().info("Selected cleanup scopes are separate operations; completed work is retained if a later operation fails");
    }
    if !approve(session, "Cleanup")? {
        return Ok(());
    }
    let mut completed = Vec::new();
    for (label, prepared) in plans {
        execute_approved(session, engine, prepared, label, describe_receipt)
            .await
            .with_context(|| {
                if completed.is_empty() {
                    "Cleanup did not complete any earlier scope".to_owned()
                } else {
                    format!(
                        "Cleanup is incomplete; already completed: {}",
                        completed.join(", ")
                    )
                }
            })?;
        completed.push(label);
    }
    if let Some(saved) = saved {
        let result = cancellable(session, engine.discard_saved_build(saved)).await;
        match result {
            Ok(true) => session
                .display()
                .status()
                .complete("Discarded saved build recipe"),
            Ok(false) => anyhow::bail!(
                "Saved build record changed or disappeared; no record was discarded; earlier completed scopes: {}",
                completed.join(", ")
            ),
            Err(error) => {
                return Err(error.context(format!(
                    "Saved build cleanup failed; earlier completed scopes: {}",
                    completed.join(", ")
                )));
            }
        }
    }
    Ok(())
}
fn describe_receipt(receipt: &ExecutionReceipt) -> Result<String> {
    match receipt {
        ExecutionReceipt::Clean(receipt) => Ok(format!(
            "Removed {} artifact files ({} bytes); recovery data retained",
            receipt.publication.changed_files, receipt.removed_bytes
        )),
        ExecutionReceipt::CacheClean(receipt) => Ok(format!(
            "Evicted {} content objects ({} bytes); retained {} objects",
            receipt.objects.removed.len(),
            receipt
                .objects
                .removed
                .iter()
                .map(|object| u128::from(object.bytes))
                .sum::<u128>(),
            receipt.objects.retained.len()
        )),
        _ => anyhow::bail!("Unexpected cleanup receipt"),
    }
}

#[cfg(test)]
mod tests;
