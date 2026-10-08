use super::*;
use crate::engine::{
    bootstrap_tools::InstallerAssets,
    build::{
        batch::{DistributionRequest, prepare_build_batch_with_cleanup},
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
    owner: Arc<()>,
    mut scope: WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancel = scope.cancellation();
    let result = execute(prepared, config, transport, catalog, owner, &mut scope).await;
    Ok(match result {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(recovery) = error.downcast_ref::<RecoveryRequired>() {
                ExecutionOutcome::RecoveryRequired {
                    operation: recovery.operation.clone(),
                    cause: error,
                }
            } else if error.downcast_ref::<PublicationWorkerFailed>().is_some() {
                ExecutionOutcome::ExecutionUncertain(error)
            } else if cancel.is_cancelled() {
                ExecutionOutcome::InterruptedBeforePublication
            } else {
                ExecutionOutcome::FailedBeforePublication(error)
            }
        }
    })
}
fn pending(
    owner: Arc<()>,
    mut build: PreparedBuild,
    permit: crate::engine::resources::AdmissionPermit,
) -> ExecutionOutcome {
    let requirements: Vec<_> = build
        .acquisition
        .pending
        .iter()
        .filter(|need| !matches!(need.source, BuildContentSource::Download(_)))
        .map(describe)
        .collect();
    build.view.content = build.acquisition.pending.iter().map(describe).collect();
    build.view.unresolved = build
        .acquisition
        .pending
        .iter()
        .filter(|need| !matches!(need.source, BuildContentSource::Download(_)))
        .map(|need| need.key.clone())
        .collect();
    let data = RetainedOutput::from_parts(PreparedKind::Build(Box::new(build)), permit);
    let continuation = PreparationContinuation {
        prepared: PreparedOperation {
            owner,
            view: Box::new(data.view()),
            data: Box::new(data),
        },
    };
    ExecutionOutcome::NeedsInput(ExecutionInput {
        requirements,
        continuation: std::sync::Mutex::new(Some(continuation)),
    })
}

async fn execute(
    prepared: RetainedOutput<PreparedBuild>,
    config: EngineConfig,
    transport: HttpAcquisition,
    catalog: Option<(ProviderCatalog, CatalogLimits)>,
    owner: Arc<()>,
    scope: &mut WorkScope,
) -> Result<ExecutionOutcome> {
    let (prepared, prepared_permit) = prepared.into_parts();
    let PreparedBuild {
        view,
        workspace,
        request,
        acquisition,
        project,
        acquired_permit: previous_acquired,
    } = prepared;
    let archive = config.archive;
    let evidence = request.evidence;
    let held = previous_acquired
        .as_ref()
        .map(|permit| permit.reserved())
        .unwrap_or_default();
    let desired = config.resources.local_acquisition;
    let additional = ResourceRequest {
        jobs: desired
            .jobs
            .checked_sub(held.jobs)
            .context("Acquisition job reservation changed")?,
        memory_bytes: desired
            .memory_bytes
            .checked_sub(held.memory_bytes)
            .context("Acquisition memory reservation changed")?,
        scratch_bytes: desired
            .scratch_bytes
            .checked_sub(held.scratch_bytes)
            .context("Acquisition scratch reservation changed")?,
        open_files: desired
            .open_files
            .checked_sub(held.open_files)
            .context("Acquisition descriptor reservation changed")?,
    };
    let retained = if previous_acquired.is_some() {
        ResourceRequest::default()
    } else {
        config.resources.acquired
    };
    let local = scope.spawn_blocking(additional, retained, move |cancel| {
        workspace
            .root()
            .revalidate(workspace.observations(), &cancel)?;
        let result = acquisition.acquire_embedded(&workspace, archive, evidence, &cancel)?;
        Ok::<_, anyhow::Error>((workspace, result, prepared_permit))
    })?;
    let local = scope.accept(local.wait().await?)?.transpose()?;
    let ((workspace, acquired, prepared_permit), acquired_permit) = local.into_parts();
    let acquired_permit = previous_acquired.unwrap_or(acquired_permit);
    let acquired = if let Some((catalog, limits)) = catalog {
        acquired
            .refresh_provider_locators(&catalog, scope, limits)
            .await?
    } else {
        acquired
    };
    let acquired = acquired.use_saved_provider_alternatives();
    let missing = acquired
        .pending
        .iter()
        .filter(|need| !matches!(need.source, BuildContentSource::Download(_)))
        .count();
    if missing != 0 {
        return Ok(pending(
            owner,
            PreparedBuild {
                project,
                view,
                workspace,
                request,
                acquisition: acquired,
                acquired_permit: Some(acquired_permit),
            },
            prepared_permit,
        ));
    }
    let acquired = acquired
        .acquire_http(&transport, scope, evidence, config.transfer)
        .await?;
    if !acquired.pending.is_empty() {
        return Ok(pending(
            owner,
            PreparedBuild {
                project,
                view,
                workspace,
                request,
                acquisition: acquired,
                acquired_permit: Some(acquired_permit),
            },
            prepared_permit,
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
    let mut assembly = config.resources.assembly;
    assembly.scratch_bytes = assembly
        .scratch_bytes
        .checked_add(
            view.cleanup
                .as_ref()
                .map_or(0, |cleanup| cleanup.removed_bytes),
        )
        .context("Build cleanup recovery storage overflow")?;
    let work = scope.spawn_blocking(assembly, config.resources.receipt, move |cancel| {
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
        let removals = view
            .cleanup
            .as_ref()
            .into_iter()
            .flat_map(|cleanup| cleanup.files.changes())
            .map(|change| change.target().clone())
            .collect::<std::collections::BTreeSet<_>>();
        let removed_artifacts = removals
            .iter()
            .map(|path| match path {
                empack_core::files::ManagedPath::Artifact(path) => Ok(path.clone()),
                _ => anyhow::bail!("Build cleanup escaped the artifact namespace"),
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let prepared = prepare_build_batch_with_cleanup(
            workspace,
            NonEmpty::new(requests)?,
            &acquired.acquired,
            &removals,
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
            removed_artifacts,
        })
    })?;
    let result = work.wait().await.map_err(PublicationWorkerFailed)?;
    // Cancellation after a completed publication must preserve its committed receipt.
    Ok(ExecutionOutcome::Completed(ExecutionReceipt::Build(
        Box::new(scope.accept_publication(result)?.transpose()?),
    )))
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
