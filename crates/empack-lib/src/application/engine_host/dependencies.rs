//! Selected provider requests and dependency mutation plans share native publication.
use super::*;
use crate::{
    engine::{
        acquisition::HttpAcquisition,
        addition::{
            DirectFileInput, DirectFileLimits, DirectFileSource, FileAddition,
            ResolvedAdditionBatch,
        },
        api::{
            AddRequest, AdoptObservedRequest, ExistingDependencyPolicy, RemoveRequest, Request,
            SyncRequest, UpdateRequest,
        },
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
mod adoption;
mod identification;

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
    /// Provider identification is explicit; failed discovery cannot become a local fallback.
    IdentifiedFile {
        source: DirectFileSource,
        kind: Option<empack_core::model::ContentKind>,
        file_plan: Option<crate::engine::documents::ProviderFileSelection>,
        providers: NonEmpty<empack_core::model::ProviderKind>,
    },
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
    let services = configured_services(session)?;
    add_with_services(session, inputs, releases, evidence, existing, services).await
}
/// Deliberately refresh selected installed identities while preserving authored intent and pins.
/// Inputs describe the requested selections; an unknown identity cannot become an implicit add.
pub async fn update(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
) -> Result<()> {
    let services = configured_services(session)?;
    update_with_services(session, inputs, releases, evidence, services).await
}
pub(super) async fn update_with_services(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
    services: AdditionServices,
) -> Result<()> {
    change_with_services(
        session,
        inputs,
        releases,
        evidence,
        Change::Update,
        services,
    )
    .await
}

pub(super) async fn adopt_with_services(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    services: AdditionServices,
) -> Result<()> {
    change_with_services(
        session,
        inputs,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        Change::Adopt,
        services,
    )
    .await
}
pub(super) fn configured_services(session: &dyn Session) -> Result<AdditionServices> {
    let config = session.config().app_config();
    let catalog = ProviderCatalog::new(
        config.curseforge_api_client_key.clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut files = DirectFileLimits::default();
    files.transfer.deadline = Duration::from_secs(config.net_timeout);
    Ok(AdditionServices {
        catalog,
        transport,
        files,
    })
}
enum Change {
    Add(ExistingDependencyPolicy),
    Update,
    Adopt,
}

pub(super) async fn add_with_services(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
    services: AdditionServices,
) -> Result<()> {
    change_with_services(
        session,
        inputs,
        releases,
        evidence,
        Change::Add(existing),
        services,
    )
    .await
}
async fn change_with_services(
    session: &dyn Session,
    inputs: NonEmpty<AddHostInput>,
    releases: ReleasePolicy,
    evidence: SourceEvidencePolicy,
    change: Change,
    services: AdditionServices,
) -> Result<()> {
    let update = matches!(&change, Change::Update);
    let adopt = matches!(&change, Change::Adopt);
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let shared = governor(config);
    let engine = engine_with_governor(config, &invocation, shared.clone())?;
    let state = state_root(config, &invocation)?;
    let mut providers = Vec::new();
    let mut files = Vec::new();
    let mut identify = Vec::new();
    for input in inputs.into_vec() {
        match input {
            AddHostInput::Provider(input) => providers.push(input),
            AddHostInput::IdentifiedFile {
                mut source,
                kind,
                file_plan,
                providers,
            } => {
                if let DirectFileSource::Local(path) = &mut source {
                    *path = absolute(&invocation, path);
                }
                identify.push(identification::Input {
                    source,
                    kind,
                    file_plan,
                    providers,
                });
            }
            AddHostInput::File(mut input) => {
                if let DirectFileSource::Local(path) | DirectFileSource::ObservedUrl { path, .. } =
                    &mut input.source
                {
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
            let supplied = identification::resolve(
                &mut scope, identify, &mut providers, &services, evidence
            ).await?;
            let provider = if providers.is_empty() {
                None
            } else {
                let resolved = services.catalog.resolve_addition(
                    &mut scope, &current, NonEmpty::new(providers)?, releases, limits
                ).await?;
                match resolved {
                    ProviderAdditionOutcome::Ready(addition) => Some(addition),
                    ProviderAdditionOutcome::NeedsInput(closure) => anyhow::bail!(
                        "Requested dependencies were not published: required dependency evidence needs a decision: {:?}",
                        closure.issues
                    ),
                }
            };
            let provider_content = match &provider {
                Some(provider) if !supplied.is_empty() => Some(identification::content(
                    &mut scope, provider, supplied, &services, evidence
                ).await?),
                _ => None,
            };
            let files = if files.is_empty() {
                None
            } else {
                Some(FileAddition::acquire(
                    &mut scope, &current, NonEmpty::new(files)?, &services.transport,
                    evidence, services.files
                ).await?)
            };
            let addition = ResolvedAdditionBatch::combine_with_content(
                &mut scope, current, provider, provider_content, files
            ).await?;
            Ok((addition, revision))
        })
        .await?;
        for (key, dependency) in addition.group().dependencies() {
            session.display().status().info(&format!(
                "Record {}: {} ({:?})", key.as_str(), dependency.title, dependency.identity
            ));
        }
        let request: Request = match change {
            Change::Add(existing) => AddRequest {
                source_revision: Some(revision), group: addition.group().clone(),
                content: addition.content().clone(), existing,
            }.into(),
            Change::Update => UpdateRequest {
                source_revision: Some(revision), group: addition.group().clone(),
                content: addition.content().clone(),
            }.into(),
            Change::Adopt => AdoptObservedRequest { group: addition.group().clone() }.into(),
        };
        let prepared = ready(cancellable(session, engine.prepare(project, request)).await?)?;
        drop(addition);
        if adopt { publish_adoption(session, &engine, prepared).await }
        else if update { publish_update(session, &engine, prepared).await }
        else { publish_addition(session, &engine, prepared).await }

    }
    .await;
    engine.shutdown().await;
    result
}
async fn publish_update(
    session: &dyn Session,
    engine: &Engine,
    prepared: PreparedOperation,
) -> Result<()> {
    let view = prepared.view().update().context("Missing update preview")?;
    for (requested, canonical) in &view.bindings {
        if !view.selected.contains(canonical) {
            continue;
        }
        session.display().status().info(&format!(
            "Update {} as {}",
            requested.as_str(),
            canonical.as_str(),
        ));
    }
    if !view.references.is_empty() {
        session.display().status().info(&format!(
            "Record {} exact references; payloads verify when acquired",
            view.references.len(),
        ));
    }
    show_changes(session, &view.files)?;
    apply(session, engine, prepared, "Update", |receipt| {
        let ExecutionReceipt::Update(receipt) = receipt else {
            anyhow::bail!("Unexpected update receipt");
        };
        Ok(format!(
            "Updated {} selected dependencies; preserved authored intent",
            receipt.selected.len()
        ))
    })
    .await
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
/// Record explicitly described, already present content. The engine verifies every selected
/// payload and backend owner before offering document changes; this host never installs bytes.
pub async fn adopt_observed(session: &dyn Session, request: AdoptObservedRequest) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        for (key, dependency) in request.group.dependencies() {
            session.display().status().info(&format!(
                "Adopt {}: {} ({:?}, {:?}, {} files)",
                key.as_str(),
                dependency.title,
                dependency.kind,
                dependency.identity,
                dependency.files.as_slice().len()
            ));
        }
        let prepared = ready(cancellable(session, engine.prepare(project, request)).await?)?;
        publish_adoption(session, &engine, prepared).await
    }
    .await;
    engine.shutdown().await;
    result
}
async fn publish_adoption(
    session: &dyn Session,
    engine: &Engine,
    prepared: PreparedOperation,
) -> Result<()> {
    let view = prepared
        .view()
        .adoption()
        .context("Missing adoption preview")?;
    for (requested, canonical) in &view.bindings {
        if requested != canonical {
            session.display().status().info(&format!(
                "Retain logical key {} for {}",
                canonical.as_str(),
                requested.as_str()
            ));
        }
    }
    adoption::describe(view, |line| session.display().status().info(&line));
    show_changes(session, &view.files)?;
    apply(session, engine, prepared, "Adoption", |receipt| {
        let ExecutionReceipt::AdoptObserved(receipt) = receipt else {
            anyhow::bail!("Unexpected adoption receipt");
        };
        Ok(format!(
            "Adopted {} verified dependency bindings; {} managed document changes",
            receipt.bindings.len(),
            receipt.publication.changed_files
        ))
    })
    .await
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
