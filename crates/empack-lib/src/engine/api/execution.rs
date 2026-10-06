use super::*;
#[derive(Debug, thiserror::Error)]
#[error("Publication worker failed; inspect recovery before retrying")]
struct PublicationWorkerFailed(#[source] RuntimeError);

use crate::engine::{
    bootstrap_tools::InstallerAssets,
    build::{
        batch::{DistributionRequest, prepare_build_batch},
        client::{ClientBootstrap, ClientOptions},
        server::{ServerBootstrap, ServerOptions},
    },
    publication::{Publisher, RecoveryRequired},
    runtime::WorkScope,
    server_runtime::{
        PreparedServerRuntime, VanillaServerPlan, installer::InstallerServerPlan,
        library::LibraryServerPlan,
    },
};

pub(super) async fn run(
    prepared: RetainedOutput<PreparedBuild>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    mut scope: WorkScope,
) -> Result<BuildOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, transport, catalog, &mut scope).await;
    Ok(match result {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(recovery) = error.downcast_ref::<RecoveryRequired>() {
                BuildOutcome::RecoveryRequired {
                    operation: recovery.operation.clone(),
                    cause: error,
                }
            } else if error.downcast_ref::<PublicationWorkerFailed>().is_some() {
                BuildOutcome::ExecutionUncertain(error)
            } else if cancel.is_cancelled() {
                BuildOutcome::InterruptedBeforePublication
            } else {
                BuildOutcome::FailedBeforePublication(error)
            }
        }
    })
}
async fn execute(
    prepared: RetainedOutput<PreparedBuild>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    scope: &mut WorkScope,
) -> Result<BuildOutcome> {
    let (prepared, prepared_permit) = prepared.into_parts();
    let PreparedBuild {
        view,
        workspace,
        request,
        acquisition,
    } = prepared;
    let archive = config.archive;
    let evidence = request.evidence;
    let local = scope.spawn_blocking(
        config.resources.local_acquisition,
        config.resources.acquired,
        move |cancel| {
            workspace
                .root()
                .revalidate(workspace.observations(), &cancel)?;
            let result = acquisition
                .begin()
                .acquire_embedded(&workspace, archive, evidence, &cancel)?;
            Ok::<_, anyhow::Error>((workspace, result, prepared_permit))
        },
    )?;
    let local = scope.accept(local.wait().await?)?.transpose()?;
    let ((workspace, acquired, prepared_permit), acquired_permit) = local.into_parts();
    let acquired = if let Some((catalog, limits)) = catalog {
        acquired
            .refresh_provider_locators(&catalog, scope, limits)
            .await?
    } else {
        acquired
    };
    let acquired = acquired
        .acquire_http(&transport, scope, evidence, config.transfer)
        .await?;
    if !acquired.pending.is_empty() {
        return Ok(BuildOutcome::NeedsInput(
            acquired.pending.iter().map(describe).collect(),
        ));
    }
    let has = |target| {
        request
            .outputs
            .as_slice()
            .iter()
            .any(|output| output.target == target)
    };
    let assets = if has(BuildTarget::Client) || has(BuildTarget::Server) {
        Some(InstallerAssets::acquire(&transport, scope, config.transfer).await?)
    } else {
        None
    };
    let runtime = if has(BuildTarget::Server) || has(BuildTarget::ServerFull) {
        Some(prepare_runtime(&transport, scope, view.runtime.clone(), evidence, &config).await?)
    } else {
        None
    };
    let work = scope.spawn_blocking(
        config.resources.assembly,
        config.resources.receipt,
        move |cancel| {
            // These owners remain charged through assembly and publication, even if the handle drops.
            let _owners = (prepared_permit, acquired_permit);
            let mut requests = Vec::new();
            for output in request.outputs.as_slice() {
                let artifact = output.artifact.clone();
                let client_options = || ClientOptions {
                    archive: request.archive,
                    optional: request.optional.clone(),
                    templates: request.templates.clone(),
                    evidence,
                    limits: config.archive,
                };
                let server_options = || ServerOptions {
                    archive: request.archive,
                    optional: request.optional.clone(),
                    templates: request.templates.clone(),
                    evidence,
                    limits: config.archive,
                };
                requests.push(match output.target {
                    BuildTarget::Mrpack => DistributionRequest::Mrpack {
                        artifact,
                        optional: request.mrpack_optional,
                        evidence,
                    },
                    BuildTarget::ClientFull => DistributionRequest::ClientFull {
                        artifact,
                        options: client_options(),
                    },
                    BuildTarget::Client => DistributionRequest::Client {
                        artifact,
                        options: client_options(),
                        bootstrap: ClientBootstrap {
                            assets: assets
                                .as_ref()
                                .context("Missing acquired installer assets")?
                                .clone(),
                            interaction: request.interaction,
                        },
                    },
                    BuildTarget::ServerFull => DistributionRequest::ServerFull {
                        artifact,
                        options: server_options(),
                        runtime: runtime
                            .as_ref()
                            .context("Missing prepared server runtime")?
                            .clone(),
                    },
                    BuildTarget::Server => DistributionRequest::Server {
                        artifact,
                        options: server_options(),
                        runtime: runtime
                            .as_ref()
                            .context("Missing prepared server runtime")?
                            .clone(),
                        bootstrap: ServerBootstrap {
                            assets: assets
                                .as_ref()
                                .context("Missing acquired installer assets")?
                                .clone(),
                            interaction: request.interaction,
                        },
                    },
                });
            }
            let prepared = prepare_build_batch(
                workspace,
                NonEmpty::new(requests)?,
                &acquired.acquired,
                &cancel,
            )?;
            cancel.check()?;
            // This is the first point with durable host-state and project publication authority.
            let publisher = Publisher::open(&config.state_root)?;
            let (publication, artifacts) = prepared.publish_with_evidence(&publisher, &cancel)?;
            Ok::<_, anyhow::Error>(BuildReceipt {
                plan: view.plan,
                publication,
                artifacts,
            })
        },
    )?;
    let result = work.wait().await.map_err(PublicationWorkerFailed)?;
    // Cancellation after a completed publication must preserve its committed receipt.
    Ok(BuildOutcome::Completed(
        scope.accept_publication(result)?.transpose()?,
    ))
}
async fn prepare_runtime(
    transport: &HttpAcquisition,
    scope: &mut WorkScope,
    runtime: RuntimeResolution,
    evidence: SourceEvidencePolicy,
    config: &EngineConfig,
) -> Result<PreparedServerRuntime> {
    match runtime.loader {
        LoaderKind::Vanilla => {
            VanillaServerPlan::resolve(transport, scope, runtime, config.transfer)
                .await?
                .acquire(transport, scope, config.transfer, config.archive, evidence)
                .await
        }
        LoaderKind::Fabric | LoaderKind::Quilt => {
            LibraryServerPlan::resolve(transport, scope, runtime, config.transfer)
                .await?
                .acquire(transport, scope, config.transfer, config.archive, evidence)
                .await
        }
        LoaderKind::Forge | LoaderKind::NeoForge => {
            InstallerServerPlan::resolve(
                transport,
                scope,
                runtime,
                config.transfer,
                config.archive,
                evidence,
            )
            .await?
            .prepare(
                transport,
                scope,
                config.transfer,
                config.archive,
                evidence,
                config.installer.clone(),
            )
            .await
        }
    }
}
