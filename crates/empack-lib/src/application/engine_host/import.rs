//! Selected archives become verified candidates before native replacement is authorized.
use super::*;
use crate::{
    engine::{
        acquisition::{DownloadRequest, HttpAcquisition, LocalFileRequest, acquire_local_file},
        api::ImportRequest,
        content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
        import::{
            ImportCandidateOptions, ImportContentKey, ImportContentLimits, ImportContentOutcome,
            ImportContentPlan, ImportLimits, ImportLocalFile, VerifiedImportContent,
            inspect_import,
        },
        project_change::ProjectReplacementPolicy,
        providers::{ModpackSelector, ProviderCatalog, ReleasePolicy, SelectionLimits},
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::model::{ExpectedContent, NonEmpty};
use std::{collections::BTreeMap, sync::Arc};

/// Explicit archive selection. Transient download URLs are never formatted or serialized here.
pub enum ImportSource {
    Saved,
    Provider {
        selector: ModpackSelector,
        releases: ReleasePolicy,
        /// Explicit manual archive selection, verified against this provider's exact assertions.
        supplied_archive: Option<PathBuf>,
    },
    Local {
        path: PathBuf,
        expected: ExpectedContent,
    },
    Download {
        alternatives: NonEmpty<String>,
        expected: ExpectedContent,
    },
}
pub struct ImportHostRequest {
    pub source: ImportSource,
    /// Relative destinations resolve against the selected workdir, without changing cwd.
    pub destination: Option<PathBuf>,
    pub replacement: ProjectReplacementPolicy,
    pub evidence: SourceEvidencePolicy,
    /// Explicit associations for restricted inputs; no filename guessing or provider bypass.
    pub supplied: BTreeMap<ImportContentKey, AcquiredContent>,
    /// Explicit host file associations; paths resolve against the invocation directory.
    pub local_files: Vec<ImportLocalFile>,
}

/// The decision callback receives verified source evidence, not a live project writer.
/// It must preserve participation and supply explicit representation/optional choices.
/// Provider pages resolve through the archive catalog. Durable pending-input storage and
/// CLI selection remain separate host services.
pub async fn import(
    session: &dyn Session,
    request: ImportHostRequest,
    decide: impl FnOnce(&VerifiedImportContent) -> Result<ImportCandidateOptions>,
) -> Result<()> {
    let catalog = ProviderCatalog::new(
        session
            .config()
            .app_config()
            .curseforge_api_client_key
            .clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let transport = acquisition_with_cache_lookup(
        session,
        catalog.configure_acquisition(HttpAcquisition::new()?),
    )?;
    import_with_services(
        session,
        request,
        decide,
        catalog,
        transport,
        ImportLimits::default(),
    )
    .await
}
pub(super) async fn import_with_services(
    session: &dyn Session,
    request: ImportHostRequest,
    decide: impl FnOnce(&VerifiedImportContent) -> Result<ImportCandidateOptions>,
    catalog: ProviderCatalog,
    transport: HttpAcquisition,
    inspection: ImportLimits,
) -> Result<()> {
    session.process().check_cancelled()?;
    let (invocation, base) = project_path(session)?;
    let selected = request
        .destination
        .as_ref()
        .map_or(base.clone(), |path| absolute(&base, path));
    let selected_path = selected.clone();
    let target = match std::fs::symlink_metadata(&selected) {
        Ok(_) => ProjectTarget::Existing(selected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProjectTarget::New(selected),
        Err(error) => return Err(error.into()),
    };
    let config = session.config().app_config();
    let shared = governor(config);
    let deadline = Duration::from_secs(config.net_timeout);
    let mut content_limits = ImportContentLimits::default();
    content_limits.catalog.deadline = deadline;
    content_limits.transfer.deadline = deadline;
    content_limits.archive = inspection.archive;
    let source = match request.source {
        ImportSource::Local { path, expected } => ImportSource::Local {
            path: absolute(&invocation, &path),
            expected,
        },
        ImportSource::Provider {
            selector,
            releases,
            supplied_archive,
        } => ImportSource::Provider {
            selector,
            releases,
            supplied_archive: supplied_archive.map(|path| absolute(&invocation, &path)),
        },
        value => value,
    };
    let local_files = request
        .local_files
        .into_iter()
        .map(|file| ImportLocalFile {
            selector: file.selector,
            source: absolute(&invocation, &file.source),
        })
        .collect::<Vec<_>>();
    let evidence = request.evidence;
    let state = state_root(config, &invocation)?;
    let load_state = state.clone();
    let load_target = selected_path.clone();
    let (content, mut saved) = scoped(session, shared.clone(), move |mut scope| async move {
        let mut resumed = None;
        let archive = match source {
            ImportSource::Saved => {
                let saved =
                    crate::engine::import::load_pending_import(&mut scope, load_state, load_target)
                        .await?
                        .context("No saved import exists for this destination")?;
                let archive = saved.archive.clone();
                resumed = Some(saved);
                archive
            }
            ImportSource::Provider {
                selector,
                releases,
                supplied_archive,
            } => {
                let selected = catalog
                    .resolve_modpack_archive(
                        &mut scope,
                        selector,
                        releases,
                        SelectionLimits {
                            catalog: content_limits.catalog,
                            ..Default::default()
                        },
                    )
                    .await?;
                if let Some(source) = supplied_archive {
                    acquire_local_file(
                        &mut scope,
                        LocalFileRequest {
                            source,
                            expected: selected.file().expected.clone(),
                            maximum: inspection.archive.compressed_bytes,
                            evidence,
                            initial: InitialObservation::RequireEvidence,
                        },
                    )
                    .await?
                    .content
                } else {
                    ensure!(
                        !selected.file().alternatives.is_empty(),
                        "Modpack archive {:?} requires explicit manual acquisition",
                        selected.pin()
                    );
                    transport
                        .acquire(
                            &mut scope,
                            DownloadRequest {
                                alternatives: NonEmpty::new(selected.file().alternatives.clone())?,
                                expected: selected.file().expected.clone(),
                                limits: TransferLimits {
                                    file_bytes: inspection.archive.compressed_bytes,
                                    transfer_bytes: inspection.archive.compressed_bytes,
                                    deadline,
                                    ..Default::default()
                                },
                                evidence,
                                initial: InitialObservation::RequireEvidence,
                            },
                        )
                        .await?
                }
            }
            ImportSource::Local { path, expected } => {
                acquire_local_file(
                    &mut scope,
                    LocalFileRequest {
                        source: path,
                        expected,
                        maximum: inspection.archive.compressed_bytes,
                        evidence,
                        initial: InitialObservation::Accepted,
                    },
                )
                .await?
                .content
            }
            ImportSource::Download {
                alternatives,
                expected,
            } => {
                transport
                    .acquire(
                        &mut scope,
                        DownloadRequest {
                            alternatives,
                            expected,
                            limits: TransferLimits {
                                file_bytes: inspection.archive.compressed_bytes,
                                transfer_bytes: inspection.archive.compressed_bytes,
                                deadline,
                                ..Default::default()
                            },
                            evidence,
                            initial: InitialObservation::Accepted,
                        },
                    )
                    .await?
            }
        };
        let inspected = inspect_import(&mut scope, archive, inspection).await?;
        let plan =
            ImportContentPlan::resolve(&mut scope, inspected, &catalog, content_limits).await?;
        let supplied = plan
            .acquire_local_files(&mut scope, &local_files, request.supplied, evidence)
            .await?;
        let (supplied, saved) = if let Some(resumed) = resumed {
            let (supplied, saved) = resumed
                .restore(&mut scope, &plan, supplied, evidence)
                .await?;
            (supplied, Some(saved))
        } else {
            (supplied, None)
        };
        Ok((
            plan.acquire(&mut scope, &transport, supplied, evidence)
                .await?,
            saved,
        ))
    })
    .await?;
    let content = match content {
        ImportContentOutcome::Ready(content) => content,
        ImportContentOutcome::NeedsInput {
            plan,
            pending,
            provided,
        } => {
            for input in &pending {
                session.display().status().warning(&format!(
                    "Import input {:?}: {:?}",
                    input.key.selector(),
                    input.reason
                ));
            }
            if approve(session, "Save incomplete import")? {
                scoped(session, shared.clone(), move |mut scope| async move {
                    crate::engine::import::save_pending_import(
                        &mut scope,
                        state,
                        selected_path,
                        plan,
                        provided,
                        evidence,
                        saved,
                    )
                    .await
                })
                .await?;
                session.display().status().info("Import continuation was saved; use init --continue with the same destination and --import-file SELECTOR=PATH");
            }
            anyhow::bail!(
                "Import was not published: {} content obligations need explicit input",
                pending.len()
            );
        }
    };
    for diagnostic in &content.plan().imported().diagnostics {
        session.display().status().warning(&format!(
            "{}: {}",
            diagnostic.location.member.as_str(),
            diagnostic.message
        ));
    }
    let options = decide(&content)?;
    let candidate = scoped(session, shared.clone(), move |mut scope| async move {
        content.into_candidate(&mut scope, options)
    })
    .await?;
    let engine = engine_with_governor(config, &invocation, shared.clone())?;
    let result = async {
        let prepared = match cancellable(
            session,
            engine.prepare(
                target,
                ImportRequest {
                    candidate,
                    replacement: request.replacement,
                },
            ),
        )
        .await?
        {
            Preparation::Ready(value) => value,
            Preparation::NeedsInput(_) => {
                anyhow::bail!("Verified import unexpectedly requires content")
            }
        };
        let view = prepared.view().import().context("Missing import preview")?;
        session.display().status().info(&format!(
            "Import {} {} ({:?})",
            view.metadata.name, view.metadata.version, view.runtime.loader
        ));
        show_changes(session, &view.files)?;
        if !approve(session, "Import")? {
            return Ok(());
        }
        execute_approved(session, &engine, prepared, "Import", |receipt| {
            let ExecutionReceipt::Import(receipt) = receipt else {
                anyhow::bail!("Unexpected import receipt")
            };
            Ok(format!(
                "Import {:?}: {} published files",
                receipt.publication.disposition, receipt.publication.changed_files
            ))
        })
        .await?;
        if let Some(saved) = saved.take() {
            scoped(session, shared, move |mut scope| async move {
                crate::engine::import::discard_pending_import(&mut scope, saved).await
            })
            .await?;
        }
        Ok(())
    }
    .await;
    engine.shutdown().await;
    result
}

#[cfg(test)]
mod tests;
