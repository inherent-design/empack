//! Keep discovery facts and their exact evidence together until candidate interpretation.
use super::*;
use anyhow::Context;
use empack_core::model::{DependencyKey, ResolvedIdentity};

/// A bounded list of observed runtime-eligible versions, never application authority.
#[derive(Clone)]
pub struct UpdateVersionChoices {
    pub key: DependencyKey,
    pub name: String,
    pub current: ResolvedPin,
    pub versions: Vec<SelectionObservation>,
}
/// The host obtains a deliberate choice; the catalog validates that it was offered.
pub trait UpdateVersionSelector: Send {
    fn choose<'a>(
        &'a mut self,
        choices: &'a UpdateVersionChoices,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PinSelector>> + Send + 'a>>;
}
#[derive(Default)]
pub struct UpdateAnalysisOptions<'a> {
    pub releases: Option<ReleasePolicy>,
    pub limits: ClosureLimits,
    pub selector: Option<&'a mut dyn UpdateVersionSelector>,
}

/// Discovery can succeed partially while a coherent proposal is unavailable.
/// A normalized group is not a native preparation or authority to publish it.
pub struct ProviderUpdateAnalysis {
    pub report: UpdateAssessment,
    pub selected_pins: BTreeMap<DependencyKey, PinSelector>,
    pub proposal: Result<ProviderAdditionOutcome>,
}

#[derive(Debug, thiserror::Error)]
#[error("Dependency evidence is incomplete")]
struct IncompleteUpdate;

impl ProviderCatalog {
    /// Scan the selected existing providers once, retain partial findings and normalize
    /// the observed exact closure with the same file/participation rules as application.
    pub async fn analyze_provider_update(
        &self,
        scope: &mut WorkScope,
        current: &ResolvedProject,
        inputs: NonEmpty<ProviderAddInput>,
        options: UpdateAnalysisOptions<'_>,
        progress: impl Fn(AssessmentProgress) + Send + Sync,
    ) -> Result<ProviderUpdateAnalysis> {
        let mut request = AssessmentRequest {
            releases: options.releases,
            ..Default::default()
        };
        let mut choices = BTreeMap::new();
        for mut input in inputs.into_vec() {
            let key: &DependencyKey = input
                .key
                .as_ref()
                .context("Update requires an existing dependency key")?;
            let dependency = current
                .lock()
                .dependencies
                .get(key)
                .with_context(|| format!("Unknown dependency key: {}", key.as_str()))?;
            let ResolvedIdentity::Provider(identity) = &dependency.identity else {
                anyhow::bail!("Not a provider dependency: {}", key.as_str());
            };
            ensure!(
                input.selector.matches_identity(identity, "")
                    || input
                        .selector
                        .authored_selector()
                        .as_ref()
                        .is_some_and(|source| {
                            dependency.source_selector.as_ref() == Some(source)
                                || current
                                    .intent()
                                    .roots
                                    .get(key)
                                    .and_then(|root| root.source.selector())
                                    == Some(source)
                        }),
                "Update must retain the canonical provider identity for {}",
                key.as_str()
            );
            ensure!(
                input.kind.is_none_or(|kind| kind == dependency.kind),
                "Update cannot change the content kind for {}",
                key.as_str()
            );
            input.kind = Some(dependency.kind);
            let held = current
                .intent()
                .roots
                .get(key)
                .and_then(|root| match &root.selection {
                    DependencySelection::ProviderPin(pin) => Some(pin),
                    _ => None,
                });
            if let Some(pin) = &input.pin {
                if Some(pin) != held {
                    request.exact.insert(key.clone(), pin.clone());
                }
            } else {
                ensure!(
                    held.is_none(),
                    "Update cannot silently unpin {}",
                    key.as_str()
                );
            }
            request.keys.push(key.clone());
            ensure!(
                choices.insert(identity.clone(), input).is_none(),
                "Repeated provider identity in an update"
            );
        }
        let record = scope.progress_reporter();
        let ObservedUpdate {
            report,
            closure,
            failure,
            selected_pins,
        } = self
            .observe_updates(
                scope,
                current,
                request,
                options.limits,
                options.selector,
                |observation| {
                    record(observation.clone().into());
                    progress(observation);
                },
            )
            .await?;
        for input in choices.values_mut() {
            if let Some(pin) = input.key.as_ref().and_then(|key| selected_pins.get(key)) {
                input.pin = Some(pin.clone());
            }
        }
        let proposal = match closure {
            Some(closure) if report.is_complete() || !closure.complete_for_required() => {
                ensure!(
                    closure.roots.iter().eq(choices.keys()),
                    "Observed update roots differ from the requested identities"
                );
                ProviderAddition::from_observed_closure(scope, current, closure, choices, false)
            }
            _ => Err(failure.unwrap_or_else(|| anyhow::Error::new(IncompleteUpdate)).context(
                "Some dependency checks could not finish; no complete update proposal is available",
            )),
        };
        Ok(ProviderUpdateAnalysis {
            report,
            selected_pins,
            proposal,
        })
    }
}
