//! Fresh exact records share the closure interpreter, without missing-version discovery.
use super::*;
use crate::engine::providers::{ClosureRequest, ClosureRoot, UnresolvedDependencies};
use empack_core::identity::ProviderProjectId;

pub(super) async fn inspect(
    scope: &mut WorkScope,
    inspection: &mut Inspection,
    catalog: &ProviderCatalog,
    limits: ClosureLimits,
) -> Result<()> {
    let current = inspection.project.clone().expect("validated project");
    let mut keys = BTreeMap::<ProviderProjectId, DependencyKey>::new();
    let mut roots = Vec::new();
    for (key, dependency) in &current.lock().dependencies {
        if let Some(pin) = &dependency.selected {
            keys.insert(pin.project.clone(), key.clone());
            roots.push(ClosureRoot {
                pin: pin.clone(),
                kind: dependency.kind,
            });
        }
    }
    if roots.is_empty() {
        inspection.finding("Fresh provider evidence",CheckState::Unverified,
            "No provider selections to check; manual and local content has no provider compatibility evidence");
        return Ok(());
    }
    let progress = scope.progress_reporter();
    let evidence = catalog
        .check_exact_selections(
            scope,
            ClosureRequest {
                roots: NonEmpty::new(roots)?,
                // Recheck the policy under which the exact selection was accepted, not a new runtime.
                game_versions: NonEmpty::new(
                    std::iter::once(current.lock().runtime.minecraft.clone())
                        .chain(current.intent().runtime.acceptable_versions.iter().cloned())
                        .collect(),
                )?,
                loader: current.lock().runtime.loader,
                releases: current.intent().dependency_policy,
            },
            limits,
            |identity| {
                progress(WorkProgress::CheckingRequirements {
                    dependency: Some(
                        keys.get(identity)
                            .map_or_else(|| identity.to_string(), |key| key.as_str().to_owned()),
                    ),
                });
            },
        )
        .await?;
    if !evidence.complete_for_required() {
        inspection.failed(
            "Fresh required dependencies",
            UnresolvedDependencies {
                issues: evidence.issues.clone(),
            }
            .into(),
        );
    }
    let mut fresh = current.lock().clone();
    // Replace provider observations only in this private validation copy. Keep manual edges,
    // identities, authored holds, placements and byte assertions exactly as recorded.
    for (identity, selection) in &evidence.selections {
        let key = keys
            .get(identity)
            .context("Fresh metadata introduced an unlocked provider selection")?;
        let dependency = fresh.dependencies.get_mut(key).expect("locked identity");
        ensure!(
            dependency.selected.as_ref() == Some(&selection.resolution.pin),
            "Fresh record changed an exact selection"
        );
        fresh
            .coverage
            .insert(key.clone(), selection.resolution.coverage);
        dependency.incompatibilities = selection
            .resolution
            .dependencies
            .iter()
            .filter_map(|relation| relation.incompatibility())
            .collect();
        let mut required = std::collections::BTreeSet::new();
        for pin in evidence
            .required_edges
            .get(&selection.resolution.pin)
            .into_iter()
            .flatten()
        {
            required.insert(
                keys.get(&pin.project)
                    .context("Required dependency is not in the current lock")?
                    .clone(),
            );
        }
        fresh.required_edges.insert(key.clone(), required);
        for file in dependency.files.as_slice() {
            let (slot, expected, observed) = match &file.acquisition {
                AcquisitionSpec::Provider { slot, .. } => (
                    slot,
                    &file.expected,
                    inspection.observed.get(&(key.clone(), file.slot.clone())),
                ),
                AcquisitionSpec::ProviderArchiveMember { archive, .. } => {
                    (&archive.slot, &archive.expected, None)
                }
                // A manual provider-bound file still needs a uniquely matching declared role.
                AcquisitionSpec::Manual { .. } => (
                    &file.slot,
                    &file.expected,
                    inspection.observed.get(&(key.clone(), file.slot.clone())),
                ),
                _ => continue,
            };
            let checked = (|| {
                let declared = selection.resolution.file_for_slot(slot, expected)?;
                compare_assertions(expected, &declared.expected, observed)
            })();
            match checked {
                Ok(()) => inspection.passed(
                    format!("{} / {}", key.as_str(), file.slot.as_str()),
                    "Fresh provider file assertions agree with the locked content",
                ),
                Err(error) => {
                    inspection.failed(format!("{} / {}", key.as_str(), file.slot.as_str()), error)
                }
            }
        }
    }
    // The existing model owns participation-aware incompatibility and required-edge validation.
    let validated = ResolvedProject::validate(
        current.intent().clone(),
        fresh,
        current.lock().intent_revision,
    );
    match validated {
        Ok(_) if evidence.complete_for_required() => inspection.passed(
            "Fresh relationships",
            "Exact provider selections and their participating relationships are consistent",
        ),
        Ok(_) => {}
        Err(error) => inspection.failed("Fresh relationships", error.into()),
    }
    Ok(())
}
pub(super) fn compare_assertions(
    locked: &ExpectedContent,
    fresh: &ExpectedContent,
    observed: Option<&CheckedContent>,
) -> Result<()> {
    ensure!(
        locked.size.zip(fresh.size).is_none_or(|(a, b)| a == b),
        "Provider file size changed since resolution"
    );
    if let (Some(expected), Some(observed)) = (&fresh.digests, observed) {
        ensure!(
            fresh.size.is_none_or(|size| size == observed.bytes),
            "Fresh provider size differs from verified local bytes"
        );
        expected.check(observed.digests.values())?;
        return Ok(());
    }
    let left = locked
        .digests
        .as_ref()
        .context("Locked file lacks independently comparable provider digests")?;
    let right = fresh
        .digests
        .as_ref()
        .context("Fresh provider record lacks comparable file digests")?;
    let mut comparable = false;
    for old in left.values() {
        if let Some(new) = right
            .values()
            .iter()
            .find(|new| new.algorithm() == old.algorithm())
        {
            comparable = true;
            ensure!(old == new, "Provider file digest changed since resolution");
        }
    }
    ensure!(
        comparable,
        "Fresh and locked digests have no comparable algorithm; materialize and verify the exact bytes before claiming agreement"
    );
    Ok(())
}
