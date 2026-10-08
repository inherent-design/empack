//! Explicit local and URL file choices publish as one verified addition.
use super::*;
use crate::{
    engine::{
        acquisition::HttpAcquisition,
        addition::{DirectFileInput, DirectFileLimits, DirectFileSource, FileAddition},
        api::{AddRequest, ExistingDependencyPolicy},
        content::SourceEvidencePolicy,
        project::ProjectReader,
        providers::ProviderCatalog,
        publication::RecoveryReader,
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::model::NonEmpty;
use std::sync::Arc;

/// The caller explicitly chooses a direct representation; catalog errors never imply this choice.
/// Local paths resolve against invocation cwd, while placements resolve within the selected pack.
pub async fn add_files(
    session: &dyn Session,
    inputs: NonEmpty<DirectFileInput>,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
) -> Result<()> {
    let config = session.config().app_config();
    let catalog = ProviderCatalog::new(
        config.curseforge_api_client_key.clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut limits = DirectFileLimits::default();
    limits.transfer.deadline = Duration::from_secs(config.net_timeout);
    add_with_transport(session, inputs, evidence, existing, transport, limits).await
}
async fn add_with_transport(
    session: &dyn Session,
    inputs: NonEmpty<DirectFileInput>,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
    transport: HttpAcquisition,
    limits: DirectFileLimits,
) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let shared = governor(config);
    let engine = engine_with_governor(config, &invocation, shared.clone())?;
    let state = state_root(config, &invocation)?;
    let mut inputs = inputs.into_vec();
    for input in &mut inputs {
        if let DirectFileSource::Local(path) = &mut input.source {
            *path = absolute(&invocation, path);
        }
    }
    let selected = project.clone();
    let result = async {
        let (addition, revision) = scoped(session, shared, move |mut scope| async move {
            let worker = scope.spawn_blocking(
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
            let snapshot = scope.accept(worker.wait().await?)?.transpose()?;
            let revision = snapshot.revision();
            let current = snapshot.require_resolved()?;
            let addition = FileAddition::acquire(
                &mut scope,
                &current,
                NonEmpty::new(inputs)?,
                &transport,
                evidence,
                limits,
            )
            .await?;
            Ok((addition, revision))
        })
        .await?;
        let prepared = dependencies::ready(
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
        dependencies::publish_addition(session, &engine, prepared).await
    }
    .await;
    engine.shutdown().await;
    result
}
#[cfg(test)]
mod tests;
