//! Selections outside the fresh closure still ship, so retaining them requires evidence.
use super::*;
use crate::engine::{
    providers::{CatalogError, ClosureLimits},
    runtime::WorkScope,
};
use std::collections::BTreeSet;

pub(in crate::application::engine_host::cli) async fn validate_retained_providers(
    scope: &mut WorkScope,
    catalog: &ProviderCatalog,
    proposed: &ResolvedProject,
    checked: &BTreeSet<DependencyKey>,
    limits: ClosureLimits,
) -> Result<()> {
    let retained: Vec<_> = proposed
        .lock()
        .dependencies
        .iter()
        .filter(|(key, dependency)| !checked.contains(*key) && dependency.selected.is_some())
        .take(limits.projects.saturating_add(1))
        .collect();
    ensure!(
        retained.len() <= limits.projects,
        "Retained provider validation exceeds the bounded catalog project limit; nothing was published"
    );
    if retained.is_empty() {
        return Ok(());
    }
    let keys = retained
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let pins = retained
        .iter()
        .map(|(_, dependency)| {
            dependency
                .selected
                .clone()
                .expect("filtered provider selection")
        })
        .collect::<Vec<_>>();
    let evidence = catalog.resolve_exact_batch(scope, &pins, limits.projects, limits.selection.catalog).await
        .with_context(|| format!("Cannot establish compatibility of retained provider selections [{keys}]; provider evidence is unavailable or invalid. Retry when evidence is available, or deliberately remove/replace these dependencies before changing runtime. Nothing was published"))?;
    let mut games = vec![proposed.intent().runtime.minecraft.clone()];
    games.extend(proposed.intent().runtime.acceptable_versions.clone());
    let games = NonEmpty::new(games)?;
    for (key, dependency) in retained {
        scope.cancellation().check()?;
        let pin = dependency
            .selected
            .as_ref()
            .expect("filtered provider selection");
        let result = evidence.records()[pin].verify_compatibility(
            dependency.kind,
            &games,
            proposed.intent().runtime.loader,
        );
        if let Err(error) = result {
            let status = if matches!(
                error.downcast_ref::<CatalogError>(),
                Some(CatalogError::NoCompatibleSelection | CatalogError::ContentKindMismatch)
            ) {
                "is incompatible"
            } else {
                "has unknown compatibility"
            };
            return Err(error.context(format!("Retained dependency '{}' {status} with the proposed runtime and still has shipped placements. Deliberately remove or replace it before retrying; dependencies are never deleted automatically. Nothing was published", key.as_str())));
        }
    }
    Ok(())
}
