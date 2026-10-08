//! Selected provider requests and dependency mutation plans share native publication.
use super::*;
use crate::{
    engine::{
        api::{
            AddRequest, DependencyContent, ExistingDependencyPolicy, RemoveRequest, SyncRequest,
        },
        mrpack::LockedFileKey,
        project::ProjectReader,
        providers::{
            ClosureLimits, ProviderAddInput, ProviderAdditionOutcome, ProviderCatalog,
            ReleasePolicy,
        },
        publication::RecoveryReader,
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::model::NonEmpty;
use std::{collections::BTreeMap, sync::Arc};

/// Resolve every selected provider root and its required closure, then record exact references.
/// Search/UI selection and local/direct-file acquisition are separate host inputs. This path
/// never claims to have materialized remote payloads merely because their records were saved.
pub async fn add_providers(
    session: &dyn Session,
    inputs: NonEmpty<ProviderAddInput>,
    releases: ReleasePolicy,
    existing: ExistingDependencyPolicy,
) -> Result<()> {
    let catalog = ProviderCatalog::new(
        session
            .config()
            .app_config()
            .curseforge_api_client_key
            .clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    add_with_catalog(session, inputs, releases, existing, catalog).await
}
async fn add_with_catalog(
    session: &dyn Session,
    inputs: NonEmpty<ProviderAddInput>,
    releases: ReleasePolicy,
    existing: ExistingDependencyPolicy,
    catalog: ProviderCatalog,
) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let shared = governor(config);
    let engine = engine_with_governor(config, &invocation, shared.clone())?;
    let state = state_root(config, &invocation)?;
    let selected = project.clone();
    let mut limits = ClosureLimits::default();
    limits.selection.catalog.deadline = Duration::from_secs(config.net_timeout);
    let result = async {
        let (addition, revision) = scoped(session, shared, move |mut scope| async move {
            let work = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    memory_bytes: 64 << 20,
                    open_files: 16,
                    ..Default::default()
                },
                ResourceRequest {
                    memory_bytes: 64 << 20,
                    open_files: 4,
                    ..Default::default()
                },
                move |cancel| {
                    ProjectReader::new(RecoveryReader::new(state)).capture(
                        &selected,
                        &[],
                        SnapshotLimits::default(),
                        &cancel,
                    )
                },
            )?;
            let snapshot = scope.accept(work.wait().await?)?.transpose()?;
            let revision = snapshot.revision();
            let current = snapshot.require_resolved()?;
            let addition = catalog
                .resolve_addition(&mut scope, &current, inputs, releases, limits)
                .await?;
            Ok((addition, revision))
        })
        .await?;
        let addition = match addition {
            ProviderAdditionOutcome::Ready(addition) => addition,
            ProviderAdditionOutcome::NeedsInput(closure) => {
                for issue in &closure.issues {
                    session.display().status().warning(&format!(
                        "Required dependency {:?}: {:?}",
                        issue.from, issue.kind
                    ));
                }
                anyhow::bail!(
                    "Addition was not published: required dependency evidence needs a decision"
                );
            }
        };
        let mut content = BTreeMap::new();
        for (key, dependency) in &addition.project().lock().dependencies {
            session.display().status().info(&format!(
                "Record {}: {} ({:?})",
                key.as_str(),
                dependency.title,
                dependency.identity
            ));
            for file in dependency.files.as_slice() {
                let slot = LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                };
                content.insert(slot, DependencyContent::Reference);
            }
        }
        let prepared = ready(
            cancellable(
                session,
                engine.prepare(
                    project,
                    AddRequest {
                        source_revision: Some(revision),
                        group: addition.group().clone(),
                        content,
                        existing,
                    },
                ),
            )
            .await?,
        )?;
        drop(addition);
        publish_addition(session, &engine, prepared).await
    }
    .await;
    engine.shutdown().await;
    result
}
pub(super) async fn publish_addition(
    session: &dyn Session,
    engine: &Engine,
    prepared: PreparedOperation,
) -> Result<()> {
    let view = prepared.view().add().context("Missing addition preview")?;
    for (key, previous) in &view.replaced {
        session.display().status().info(&format!(
            "Replace {}: {} ({:?})",
            key.as_str(),
            previous.title,
            previous.identity
        ));
    }
    for unknown in &view.incomplete_evidence {
        session.display().status().warning(&format!(
            "Dependency evidence is incomplete for {}",
            unknown.as_str()
        ));
    }
    for (requested, canonical) in &view.bindings {
        if requested != canonical {
            session.display().status().info(&format!(
                "Use existing label {} for {}",
                canonical.as_str(),
                requested.as_str()
            ));
        }
    }
    if !view.references.is_empty() {
        session.display().status().info(&format!(
            "Record {} exact content references; payloads are verified when a build requires them",
            view.references.len()
        ));
    }
    show_changes(session, &view.files)?;
    apply(session, engine, prepared, "Addition", |receipt| {
        let ExecutionReceipt::Add(receipt) = receipt else {
            anyhow::bail!("Unexpected addition receipt");
        };
        Ok(format!(
            "Recorded {} dependency bindings and {} exact references",
            receipt.bindings.len(),
            receipt.references.len()
        ))
    })
    .await
}

pub(super) fn ready(preparation: Preparation) -> Result<PreparedOperation> {
    match preparation {
        Preparation::Ready(value) => Ok(value),
        Preparation::NeedsInput(_) => {
            anyhow::bail!("Dependency publication still requires content decisions")
        }
    }
}
/// Apply explicit logical selections; unknown dependency evidence requires its own policy.
pub async fn remove(session: &dyn Session, request: RemoveRequest) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let prepared = ready(cancellable(session, engine.prepare(project, request)).await?)?;
        let view = prepared
            .view()
            .remove()
            .context("Missing removal preview")?;
        session
            .display()
            .status()
            .info(&format!("Removal mode: {:?}", view.mode));
        for selected in &view.selected {
            session.display().status().info(&format!(
                "Select {}: {} ({:?})",
                selected.key.as_str(),
                selected.title,
                selected.identity
            ));
        }
        for unknown in &view.incomplete_evidence {
            session.display().status().warning(&format!(
                "Dependency evidence is incomplete for {}",
                unknown.as_str()
            ));
        }
        for observed in &view.observed {
            session.display().status().info(&format!(
                "Select observed {}: {:?} ({:?}, {:?})",
                observed.metadata_path.as_str(),
                observed.destination,
                observed.provider,
                observed.digest
            ));
        }
        for path in &view.untracked_evidence {
            session.display().status().warning(&format!(
                "Dependency evidence is incomplete for observed {}",
                path.as_str()
            ));
        }
        show_changes(session, &view.files)?;
        apply(session, &engine, prepared, "Removal", |receipt| {
            let ExecutionReceipt::Remove(receipt) = receipt else {
                anyhow::bail!("Unexpected removal receipt");
            };
            Ok(format!(
                "Applied {:?} to {} tracked and {} observed selections",
                receipt.mode,
                receipt.selected.len(),
                receipt.observed.len()
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
/// Publish a recorded or explicitly resolved synchronization with complete per-slot decisions.
pub async fn synchronize(session: &dyn Session, request: SyncRequest) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let prepared = ready(cancellable(session, engine.prepare(project, request)).await?)?;
        let view = prepared
            .view()
            .sync()
            .context("Missing synchronization preview")?;
        session.display().status().info(&format!(
            "Synchronize {} recorded selections ({} deferred references)",
            view.selected.len(),
            view.references.len()
        ));
        show_changes(session, &view.files)?;
        apply(session, &engine, prepared, "Synchronization", |receipt| {
            let ExecutionReceipt::Sync(receipt) = receipt else {
                anyhow::bail!("Unexpected synchronization receipt");
            };
            Ok(format!(
                "Synchronized {} recorded dependencies; {} managed file changes",
                receipt.project.lock().dependencies.len(),
                receipt.publication.changed_files
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}

#[cfg(test)]
mod tests;
