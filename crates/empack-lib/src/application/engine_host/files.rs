//! Explicit local and URL file choices publish as one verified addition.
use super::*;
#[cfg(test)]
use crate::engine::addition::DirectFileSource;
use crate::{
    engine::{
        acquisition::HttpAcquisition,
        addition::{DirectFileInput, DirectFileLimits},
        api::ExistingDependencyPolicy,
        content::SourceEvidencePolicy,
        providers::ProviderCatalog,
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
    let catalog = ProviderCatalog::new(
        session
            .config()
            .app_config()
            .curseforge_api_client_key
            .clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    dependencies::add_with_services(
        session,
        NonEmpty::new(
            inputs
                .into_vec()
                .into_iter()
                .map(dependencies::AddHostInput::File)
                .collect(),
        )?,
        crate::engine::providers::ReleasePolicy::PreferStable,
        evidence,
        existing,
        dependencies::AdditionServices {
            catalog,
            transport,
            files: limits,
        },
    )
    .await
}

#[cfg(test)]
mod tests;
