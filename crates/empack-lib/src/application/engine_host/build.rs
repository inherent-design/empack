//! Native build host: explicit recipes, visible effects and one approved publication.
use super::*;
use crate::engine::build::acquisition::AcquisitionKey;
use crate::{
    application::{BuildArgs, cli::CliArchiveFormat},
    engine::{
        acquisition::discovery::{DiscoveryLimits, discover_downloads},
        api::{BuildOutput, BuildPreparationRequest, BuildRequest},
        build::BuildAcquisitions,
        content::SourceEvidencePolicy,
        mrpack::OptionalConversion,
        packwiz::InstallerInteraction,
        project::ProjectReader,
        providers::{CatalogLimits, ProviderCatalog},
        publication::RecoveryReader,
        templates::TemplateOptions,
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::{
    inventory::OptionalPolicy,
    model::{DistributionArchive, NonEmpty, ProjectIntent},
    path::{ArtifactStem, PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::{collections::BTreeMap, sync::Arc};

/// Explicit decisions which cannot be inferred from a filename or `--yes`.
/// Preserve optional participation unless the host supplies reviewed materialization choices.
#[derive(Clone)]
pub struct BuildDecisions {
    pub optional: OptionalPolicy,
    pub mrpack_optional: OptionalConversion,
    pub templates: TemplateOptions,
    pub evidence: SourceEvidencePolicy,
    pub interaction: InstallerInteraction,
}
impl Default for BuildDecisions {
    fn default() -> Self {
        Self {
            optional: OptionalPolicy::Preserve,
            mrpack_optional: OptionalConversion::RejectMetadataLoss,
            templates: TemplateOptions::default(),
            evidence: SourceEvidencePolicy::Compatibility,
            interaction: InstallerInteraction::Headless,
        }
    }
}

/// Build a native v0.5 project. Supplied bytes retain their exact logical slot and evidence.
/// Selected download roots are scanned without mutation. Durable continuation and explicit
/// CLI association syntax remain separate host services; unsupported flags fail explicitly.
pub async fn build(
    session: &dyn Session,
    args: &BuildArgs,
    decisions: BuildDecisions,
    supplied: BuildAcquisitions,
) -> Result<()> {
    build_with_inputs(session, args, decisions, supplied, BTreeMap::new()).await
}
/// Explicit file associations use logical obligations; host paths resolve against invocation cwd.
pub async fn build_with_local_files(
    session: &dyn Session,
    args: &BuildArgs,
    decisions: BuildDecisions,
    files: BTreeMap<AcquisitionKey, PathBuf>,
) -> Result<()> {
    build_with_inputs(
        session,
        args,
        decisions,
        BuildAcquisitions::default(),
        files,
    )
    .await
}
async fn build_with_inputs(
    session: &dyn Session,
    args: &BuildArgs,
    decisions: BuildDecisions,
    supplied: BuildAcquisitions,
    files: BTreeMap<AcquisitionKey, PathBuf>,
) -> Result<()> {
    ensure!(
        !args.continue_build && args.associate_downloads.is_empty(),
        "This host requires explicit verified content; durable continuation and CLI association parsing are not connected yet"
    );
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let config = session.config().app_config();
    let state = state_root(config, &invocation)?;
    let selected = project.clone();
    // Only documents enter this preliminary read: payload capture belongs to Engine::prepare.
    let intent = initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 20,
                open_files: 16,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 64 << 20,
                ..Default::default()
            },
            move |cancel| {
                let captured = ProjectReader::new(RecoveryReader::new(state)).capture(
                    &selected,
                    &[],
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                captured.require_resolved()
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    let request = request(intent.intent(), args, decisions)?
        .with_content(supplied)
        .with_local_files(
            files
                .into_iter()
                .map(|(key, path)| (key, absolute(&invocation, &path)))
                .collect(),
        )
        .require_intent(intent.lock().intent_revision);
    drop(intent);
    let engine = configured_engine(config, &invocation)?;
    let downloads = args
        .downloads_dir
        .as_ref()
        .map(|path| absolute(&invocation, Path::new(path)));
    let result = build_with_engine(session, &engine, project, request, downloads).await;
    engine.shutdown().await;
    result
}

fn configured_engine(config: &AppConfig, invocation: &Path) -> Result<Engine> {
    let catalog = ProviderCatalog::new(
        config.curseforge_api_client_key.clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let engine = engine(config, invocation)?.with_provider_catalog(
        catalog,
        CatalogLimits {
            deadline: Duration::from_secs(config.net_timeout),
            ..Default::default()
        },
    );
    Ok(engine)
}

/// Continue an explicitly saved native build. Saved choices remain intact; new recipe flags
/// are rejected, and every file association is checked against current captured obligations.
pub async fn continue_build(session: &dyn Session, args: &BuildArgs) -> Result<()> {
    ensure!(
        args.continue_build && args.targets.is_empty() && args.format.is_none() && !args.clean,
        "Continuation uses its saved recipe; targets, archive overrides and clean require a new build"
    );
    session.process().check_cancelled()?;
    let (invocation, project) = project_path(session)?;
    let engine = configured_engine(session.config().app_config(), &invocation)?;
    let result = async {
        let resumed = match cancellable(session, engine.resume_saved_build(project)).await? {
            crate::engine::api::SavedBuildResume::Missing => {
                anyhow::bail!("There is no saved native build for this project")
            }
            crate::engine::api::SavedBuildResume::Stale => anyhow::bail!(
                "Saved build inputs changed; prepare a new build (saved state was retained)"
            ),
            crate::engine::api::SavedBuildResume::Prepared(resumed) => *resumed,
        };
        let mut prepared = resumed.preparation;
        if !args.associate_downloads.is_empty() {
            let view = match &prepared {
                Preparation::Ready(value) => value.view(),
                Preparation::NeedsInput(value) => value.view(),
            };
            let files = associations(
                view.build().context("Missing build preview")?,
                &args.associate_downloads,
                &invocation,
            )?;
            let Preparation::NeedsInput(pending) = prepared else {
                anyhow::bail!("Explicit files do not match unresolved build input");
            };
            prepared =
                cancellable(session, engine.resume_with_local_files(*pending, files)).await?;
        }
        let downloads = args
            .downloads_dir
            .as_ref()
            .map(|path| absolute(&invocation, Path::new(path)));
        let published = finish_build(session, &engine, prepared, downloads).await?;
        if published {
            cancellable(session, engine.discard_saved_build(resumed.saved))
                .await
                .context("Build completed, but saved-state cleanup failed")?;
        }
        Ok(())
    }
    .await;
    engine.shutdown().await;
    result
}
fn input_selector(key: &AcquisitionKey) -> String {
    let encode = |value: &str| {
        percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
    };
    match key {
        AcquisitionKey::Locked(key) => format!(
            "locked:{}:{}",
            encode(key.dependency.as_str()),
            encode(key.slot.as_str())
        ),
        AcquisitionKey::Observed(path) => format!("observed:{}", encode(path.as_str())),
    }
}
fn associations(
    view: &crate::engine::api::BuildPreview,
    values: &[String],
    invocation: &Path,
) -> Result<BTreeMap<AcquisitionKey, PathBuf>> {
    let mut files = BTreeMap::new();
    for value in values {
        let (name, path) = value
            .split_once('=')
            .context("Use FILENAME=PATH for each download association")?;
        ensure!(
            !name.is_empty() && !path.is_empty(),
            "Download association needs a filename and source path"
        );
        let matches: Vec<_> = view
            .file_names()
            .iter()
            .filter(|(key, names)| {
                view.unresolved.contains(key)
                    && (names.contains(name) || input_selector(key) == name)
            })
            .map(|(key, _)| key)
            .collect();
        ensure!(
            matches.len() == 1,
            "Download selector must match exactly one pending obligation; use its displayed locked: or observed: selector when filenames are ambiguous"
        );
        ensure!(
            files
                .insert(matches[0].clone(), absolute(invocation, Path::new(path)))
                .is_none(),
            "Download association repeats an obligation"
        );
    }
    Ok(files)
}

fn request(
    intent: &ProjectIntent,
    args: &BuildArgs,
    decisions: BuildDecisions,
) -> Result<BuildRequest> {
    const ALL: [BuildTarget; 5] = [
        BuildTarget::Mrpack,
        BuildTarget::Client,
        BuildTarget::Server,
        BuildTarget::ClientFull,
        BuildTarget::ServerFull,
    ];
    let mut targets = Vec::new();
    // Validate every spelling, including values after `all`, before expanding the selection.
    for name in &args.targets {
        let selected: &[BuildTarget] = match name.as_str() {
            "all" => &ALL,
            "mrpack" => &[BuildTarget::Mrpack],
            "client" => &[BuildTarget::Client],
            "server" => &[BuildTarget::Server],
            "client-full" => &[BuildTarget::ClientFull],
            "server-full" => &[BuildTarget::ServerFull],
            _ => anyhow::bail!("Unknown build target: {name}"),
        };
        for target in selected {
            if !targets.contains(target) {
                targets.push(*target);
            }
        }
    }
    if targets.is_empty() {
        targets.extend(intent.distribution.targets.as_slice());
    }
    let name = ArtifactStem::parse(&intent.metadata.name)
        .context("Pack name cannot form a portable artifact name")?;
    let version = ArtifactStem::parse(&intent.metadata.version)
        .context("Pack version cannot form a portable artifact name")?;
    let archive = match args.format {
        Some(CliArchiveFormat::Zip) => DistributionArchive::Zip,
        Some(CliArchiveFormat::TarGz) => DistributionArchive::TarGz,
        Some(CliArchiveFormat::SevenZ) => DistributionArchive::SevenZip,
        None => intent.distribution.archive,
    };
    let extension = match archive {
        DistributionArchive::Zip => "zip",
        DistributionArchive::TarGz => "tar.gz",
        DistributionArchive::SevenZip => "7z",
    };
    let outputs = targets
        .into_iter()
        .map(|target| {
            let (suffix, extension) = match target {
                BuildTarget::Mrpack => ("", "mrpack"),
                BuildTarget::Client => ("-client", extension),
                BuildTarget::Server => ("-server", extension),
                BuildTarget::ClientFull => ("-client-full", extension),
                BuildTarget::ServerFull => ("-server-full", extension),
            };
            Ok(BuildOutput {
                target,
                artifact: PortableRelPath::parse(
                    &format!("{}-{}{suffix}.{extension}", name.as_str(), version.as_str()),
                    PathSyntax::ArtifactName,
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BuildRequest {
        clean: args.clean,
        outputs: NonEmpty::new(outputs).context("No build targets selected")?,
        archive,
        optional: decisions.optional,
        mrpack_optional: decisions.mrpack_optional,
        templates: decisions.templates,
        evidence: decisions.evidence,
        interaction: decisions.interaction,
    })
}

async fn build_with_engine(
    session: &dyn Session,
    engine: &Engine,
    project: PathBuf,
    request: BuildPreparationRequest,
    downloads: Option<PathBuf>,
) -> Result<()> {
    let prepared = cancellable(session, engine.prepare(project, request)).await?;
    finish_build(session, engine, prepared, downloads)
        .await
        .map(|_| ())
}
async fn finish_build(
    session: &dyn Session,
    engine: &Engine,
    prepared: Preparation,
    downloads: Option<PathBuf>,
) -> Result<bool> {
    let prepared = match (prepared, downloads) {
        (Preparation::NeedsInput(pending), Some(downloads)) => {
            let view = pending.view().build().context("Missing build preview")?;
            let evidence = view.request().evidence;
            let requirements = view
                .content
                .iter()
                .filter(|need| view.unresolved.contains(&need.key))
                .map(|need| (need.key.clone(), need.expected.clone()))
                .collect();
            let found = initialize::discover(session, move |mut scope| async move {
                discover_downloads(
                    &mut scope,
                    vec![downloads],
                    requirements,
                    evidence,
                    DiscoveryLimits::default(),
                )
                .await
            })
            .await?;
            let files = found.unique_files();
            session.display().status().info(&format!(
                "Download scan: {} verified associations; {} candidates inspected; {} entries skipped",
                files.len(), found.inspected_files, found.skipped_files
            ));
            if found
                .matches
                .values()
                .any(|candidates| candidates.len() > 1)
            {
                session.display().status().warning("Different candidate bytes match an obligation; explicit association is required");
            }
            drop(found);
            if files.is_empty() {
                Preparation::NeedsInput(pending)
            } else {
                cancellable(session, engine.resume_with_local_files(*pending, files)).await?
            }
        }
        (prepared, _) => prepared,
    };
    let view = match &prepared {
        Preparation::Ready(value) => value.view(),
        Preparation::NeedsInput(value) => value.view(),
    }
    .build()
    .context("Missing build preview")?;
    session.display().status().info(&format!(
        "Build for Minecraft {} ({:?})",
        view.runtime.minecraft.as_str(),
        view.runtime.loader,
    ));
    session.display().status().info(&format!(
        "Optional content: {:?}; mrpack conversion: {:?}; source evidence: {:?}; installer UI: {:?}",
        view.request().optional, view.request().mrpack_optional, view.request().evidence,
        view.request().interaction,
    ));
    for output in &view.outputs {
        session.display().status().info(&format!(
            "build {:?}: dist/{}",
            output.target,
            output.artifact.as_str()
        ));
    }
    if let Some(cleanup) = &view.cleanup {
        show_changes(session, &cleanup.files)?;
    }
    if view.needs_network {
        session
            .display()
            .status()
            .info("This build requires verified network acquisition");
    }
    if view.runs_installer {
        session
            .display()
            .status()
            .info("This build executes the selected runtime installer in private staging");
    }
    for input in &view.content {
        // AcquisitionKey contains only logical dependency/slot or observed metadata names.
        session.display().status().info(&format!(
            "content {}: {:?} {:?}",
            input_selector(&input.key),
            input.kind,
            view.file_names().get(&input.key)
        ));
    }
    if !view.unresolved.is_empty() {
        session.display().status().warning(&format!(
            "{} content obligations require verified supplied files",
            view.unresolved.len()
        ));
    }
    if let Preparation::NeedsInput(pending) = prepared {
        if session.config().app_config().dry_run {
            session.display().status().complete(
                "Dry run complete - missing content remains unresolved; no changes applied",
            );
            return Ok(false);
        }
        session.display().status().info("The build remains unpublished; saving retains its recipe and verified files for continuation");
        if !approve(session, "Save pending build")? {
            return Ok(false);
        }
        let saved = cancellable(session, engine.suspend_build(*pending)).await?;
        session.display().status().info(&format!("Saved pending build with {} verified files; continue after supplying the missing downloads",saved.retained_files));
        anyhow::bail!(
            "Build was not published: supply the displayed missing content before approving a new plan; continuation was saved"
        );
    }
    let Preparation::Ready(prepared) = prepared else {
        unreachable!()
    };
    let mut published = false;
    apply(session, engine, prepared, "Build", |receipt| {
        let ExecutionReceipt::Build(receipt) = receipt else {
            anyhow::bail!("Unexpected build receipt");
        };
        published = true;
        for artifact in &receipt.artifacts {
            session.display().status().info(&format!(
                "built dist/{} ({} bytes)",
                artifact.artifact.as_str(),
                artifact.bytes
            ));
        }
        Ok(format!(
            "Built {} verified distributions; removed {} obsolete artifacts",
            receipt.artifacts.len(),
            receipt.removed_artifacts.len()
        ))
    })
    .await?;
    Ok(published)
}

#[cfg(test)]
mod tests;
