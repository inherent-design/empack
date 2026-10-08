//! Selected provider requests and dependency mutation plans share native publication.
use super::*;
use crate::{
    engine::{
        acquisition::HttpAcquisition,
        addition::{
            DirectFileInput, DirectFileLimits, DirectFileSource, FileAddition,
            ResolvedAdditionBatch,
        },
        api::{AddRequest, ExistingDependencyPolicy, RemoveRequest, SyncRequest},
        content::SourceEvidencePolicy,
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
use std::sync::Arc;

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
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut limits = DirectFileLimits::default();
    limits.transfer.deadline = Duration::from_secs(session.config().app_config().net_timeout);
    add_with_services(
        session,
        NonEmpty::new(
            inputs
                .into_vec()
                .into_iter()
                .map(AddHostInput::Provider)
                .collect(),
        )?,
        releases,
        SourceEvidencePolicy::Compatibility,
        existing,
        AdditionServices {
            catalog,
            transport,
            files: limits,
        },
    )
    .await
}

/// All requested provider roots and direct files resolve into one publication plan.
pub enum AddHostInput {
    Provider(ProviderAddInput),
    File(DirectFileInput),
}
pub(super) struct AdditionServices {
    pub catalog: ProviderCatalog,
    pub transport: HttpAcquisition,
    pub files: DirectFileLimits,
}
pub async fn add(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
) -> Result<()> {
    let config = session.config().app_config();
    let catalog = ProviderCatalog::new(
        config.curseforge_api_client_key.clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut files = DirectFileLimits::default();
    files.transfer.deadline = Duration::from_secs(config.net_timeout);
    add_with_services(
        session,
        inputs,
        releases,
        evidence,
        existing,
        AdditionServices {
            catalog,
            transport,
            files,
        },
    )
    .await
}
pub(super) async fn add_with_services(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
    services: AdditionServices,
) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let shared = governor(config);
    let engine = engine_with_governor(config, &invocation, shared.clone())?;
    let state = state_root(config, &invocation)?;
    let mut providers = Vec::new();
    let mut files = Vec::new();
    for input in inputs.into_vec() {
        match input {
            AddHostInput::Provider(input) => providers.push(input),
            AddHostInput::File(mut input) => {
                if let DirectFileSource::Local(path) = &mut input.source {
                    *path = absolute(&invocation, path);
                }
                files.push(input);
            }
        }
    }
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
            let provider = if providers.is_empty() {
                None
            } else {
                let resolved = services.catalog.resolve_addition(
                    &mut scope, &current, NonEmpty::new(providers)?, releases, limits
                ).await?;
                match resolved {
                    ProviderAdditionOutcome::Ready(addition) => Some(addition),
                    ProviderAdditionOutcome::NeedsInput(closure) => anyhow::bail!(
                        "Addition was not published: required dependency evidence needs a decision: {:?}",
                        closure.issues
                    ),
                }
            };
            let files = if files.is_empty() {
                None
            } else {
                Some(FileAddition::acquire(
                    &mut scope, &current, NonEmpty::new(files)?, &services.transport,
                    evidence, services.files
                ).await?)
            };
            let addition = ResolvedAdditionBatch::combine(
                &mut scope, current, provider, files
            ).await?;
            Ok((addition, revision))
        })
        .await?;
        for (key, dependency) in addition.group().dependencies() {
            session.display().status().info(&format!(
                "Record {}: {} ({:?})", key.as_str(), dependency.title, dependency.identity
            ));
        }
        let prepared = ready(
            cancellable(
                session,
                engine.prepare(
                    project,
                    AddRequest {
                        source_revision: Some(revision),
                        group: addition.group().clone(),
                        content: addition.content().clone(),
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
