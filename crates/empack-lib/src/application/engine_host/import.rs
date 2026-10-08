//! Selected archives become verified candidates before native replacement is authorized.
use super::*;
use crate::{
    engine::{
        acquisition::{DownloadRequest, HttpAcquisition, LocalFileRequest, acquire_local_file},
        api::ImportRequest,
        content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
        import::{
            ImportCandidateOptions, ImportContentKey, ImportContentLimits, ImportContentOutcome,
            ImportContentPlan, ImportLimits, VerifiedImportContent, inspect_import,
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
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
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
async fn import_with_services(
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
    let evidence = request.evidence;
    let content = scoped(session, shared.clone(), move |mut scope| async move {
        let archive = match source {
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
        plan.acquire(&mut scope, &transport, request.supplied, evidence)
            .await
    })
    .await?;
    let content = match content {
        ImportContentOutcome::Ready(content) => content,
        ImportContentOutcome::NeedsInput { pending, .. } => {
            for input in &pending {
                session
                    .display()
                    .status()
                    .warning(&format!("Import input {:?}: {:?}", input.key, input.reason));
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
    let engine = engine_with_governor(config, &invocation, shared)?;
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
        apply(session, &engine, prepared, "Import", |receipt| {
            let ExecutionReceipt::Import(receipt) = receipt else {
                anyhow::bail!("Unexpected import receipt")
            };
            Ok(format!(
                "Import {:?}: {} published files",
                receipt.publication.disposition, receipt.publication.changed_files
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
