//! Explicit local and URL file choices publish as one verified addition.
use super::*;
#[cfg(test)]
use crate::engine::addition::DirectFileSource;
use crate::engine::{
    addition::DirectFileInput, api::ExistingDependencyPolicy, content::SourceEvidencePolicy,
};
#[cfg(test)]
use crate::{
    engine::{
        acquisition::HttpAcquisition, addition::DirectFileLimits, providers::ProviderCatalog,
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::model::NonEmpty;
#[cfg(test)]
use std::sync::Arc;

/// The caller explicitly chooses a direct representation; catalog errors never imply this choice.
/// Local paths resolve against invocation cwd, while placements resolve within the selected pack.
pub async fn add_files(
    session: &dyn Session,
    inputs: NonEmpty<DirectFileInput>,
    evidence: SourceEvidencePolicy,
    existing: ExistingDependencyPolicy,
) -> Result<()> {
    dependencies::add(
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
    )
    .await
}
#[cfg(test)]
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
