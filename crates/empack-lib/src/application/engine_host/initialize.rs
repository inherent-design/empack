//! Parsed host choices reach the same approved initialization used by embedding clients.
use super::*;
use crate::{
    application::InitArgs,
    engine::{
        acquisition::HttpAcquisition,
        api::InitializeRequest,
        initialize::{InitializeCandidate, default_templates},
        project_change::ProjectReplacementPolicy,
        runtime::{OperationRuntime, RetainedOutput, WorkScope},
        runtime_catalog::{LoaderVersions, RuntimeCatalog, RuntimeCatalogLimits},
    },
};
use empack_core::{
    model::{
        ContentKind, DistributionArchive, DistributionIntent, GameVersion, LoaderKind,
        LoaderVersion, NonEmpty, PackMetadata, ProjectIntent, RuntimeIntent, RuntimeResolution,
    },
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::collections::BTreeMap;

/// Native v0.5 initialization host. Import has its own normalization path; this entry point
/// accepts empty-project initialization only. The legacy dispatcher is replaced once the
/// complete cross-command host is connected, so it cannot create an unusable mixed-schema pack.
pub async fn initialize(session: &dyn Session, args: &InitArgs) -> Result<()> {
    initialize_with_catalog(session, args, RuntimeCatalog::new(HttpAcquisition::new()?)).await
}
async fn initialize_with_catalog(
    session: &dyn Session,
    args: &InitArgs,
    catalog: RuntimeCatalog,
) -> Result<()> {
    ensure!(args.from_source.is_none(), "Use the import host for --from");
    session.process().check_cancelled()?;
    let invocation = session.invocation().current_dir()?;
    let config = session.config().app_config();
    let base = absolute(
        &invocation,
        config.workdir.as_deref().unwrap_or(&invocation),
    );
    let selected = args.dir.as_ref().map_or(base.clone(), |dir| base.join(dir));
    let target = match std::fs::symlink_metadata(&selected) {
        Ok(_) => ProjectTarget::Existing(selected.clone()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ProjectTarget::New(selected.clone())
        }
        Err(error) => return Err(error.into()),
    };
    let requested_family = args.modloader.as_deref().map(parse_loader).transpose()?;
    ensure!(
        requested_family.is_some() || (!config.yes && session.interactive().can_choose()),
        "Noninteractive initialization requires --modloader"
    );
    ensure!(
        requested_family != Some(LoaderKind::Vanilla) || args.loader_version.is_none(),
        "Vanilla cannot have a loader version"
    );
    let requested_game = args
        .mc_version
        .as_deref()
        .map(GameVersion::parse)
        .transpose()?;
    ensure!(
        config.yes || session.interactive().can_choose() || requested_game.is_some(),
        "Noninteractive initialization requires --mc-version or --yes for the latest release"
    );
    let requested_loader = args
        .loader_version
        .as_deref()
        .map(LoaderVersion::parse)
        .transpose()?;
    ensure!(
        config.yes
            || session.interactive().can_choose()
            || requested_family == Some(LoaderKind::Vanilla)
            || requested_loader.is_some(),
        "Noninteractive initialization requires --loader-version or --yes for the latest compatible loader"
    );
    // Validate independent options before any discovery request or native preparation.
    let mut layout = BTreeMap::new();
    if let Some(folder) = &args.datapack_folder {
        layout.insert(
            ContentKind::DataPack,
            PortableRelPath::parse(folder, PathSyntax::ProjectContent)?,
        );
    }
    let mut acceptable_versions = args
        .game_versions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|value| GameVersion::parse(value))
        .collect::<Result<Vec<_>, _>>()?;
    let metadata = metadata(session, args, &selected).await?;
    let limits = RuntimeCatalogLimits {
        transfer: TransferLimits {
            deadline: Duration::from_secs(config.net_timeout),
            ..RuntimeCatalogLimits::default().transfer
        },
        ..Default::default()
    };
    let game = match requested_game {
        Some(game) => game,
        None => {
            let catalog = catalog.clone();
            let choices = discover(session, move |mut scope| async move {
                catalog.games(&mut scope, limits).await
            })
            .await?;
            if config.yes {
                choices.resolve(None)?
            } else {
                let values = choices
                    .versions()
                    .iter()
                    .map(|value| value.as_str().to_owned())
                    .collect::<Vec<_>>();
                let index = select_version(session, "Minecraft version", &values)?;
                choices.resolve(Some(&choices.versions()[index]))?
            }
        }
    };
    let (loader, available) = match requested_family {
        Some(loader) => (loader, None),
        None => {
            compatible_loader(
                session,
                catalog.clone(),
                game.clone(),
                requested_loader.clone(),
                limits,
            )
            .await?
        }
    };
    ensure!(
        loader != LoaderKind::Vanilla || requested_loader.is_none(),
        "Vanilla cannot have a loader version"
    );
    let requested_loader = requested_loader
        .map(|version| {
            if loader == LoaderKind::Forge {
                LoaderVersion::parse(
                    &crate::engine::runtime_versions::canonicalize_forge_loader_version(
                        game.as_str(),
                        version.as_str(),
                    ),
                )
            } else {
                Ok(version)
            }
        })
        .transpose()?;
    let runtime =
        if loader == LoaderKind::Vanilla || (requested_loader.is_some() && available.is_none()) {
            // Exact explicit selections need no catalog lookup. Executable availability and bytes
            // are verified during build; absence of a lookup is never a fallback version guess.
            RuntimeResolution {
                minecraft: game.clone(),
                loader,
                loader_version: requested_loader.clone(),
            }
        } else {
            let choices = match available {
                Some(choices) => choices,
                None => {
                    let selected_game = game.clone();
                    discover(session, move |mut scope| async move {
                        catalog
                            .loaders(&mut scope, selected_game, loader, limits)
                            .await
                    })
                    .await?
                }
            };
            if requested_loader.is_some() {
                choices.resolve(requested_loader.as_ref())?
            } else if config.yes {
                choices.resolve(None)?
            } else {
                let values = choices
                    .versions()
                    .iter()
                    .map(|value| value.as_str().to_owned())
                    .collect::<Vec<_>>();
                let index = select_version(session, "Loader version", &values)?;
                choices.resolve(Some(&choices.versions()[index]))?
            }
        };
    let mut seen = std::collections::BTreeSet::new();
    acceptable_versions.retain(|version| version != &game && seen.insert(version.clone()));
    let intent = ProjectIntent {
        metadata,
        runtime: RuntimeIntent {
            minecraft: game,
            acceptable_versions,
            loader,
            loader_version: requested_loader,
        },
        roots: BTreeMap::new(),
        layout,
        distribution: DistributionIntent {
            targets: NonEmpty::new(vec![
                BuildTarget::Mrpack,
                BuildTarget::Client,
                BuildTarget::Server,
                BuildTarget::ClientFull,
                BuildTarget::ServerFull,
            ])?,
            archive: DistributionArchive::Zip,
        },
        extensions: BTreeMap::new(),
    };
    let candidate = InitializeCandidate::new(intent, runtime, default_templates())?;
    let engine = engine(config, &invocation)?;
    let result = initialize_with_engine(session, &engine, target, candidate, args.force).await;
    engine.shutdown().await;
    result
}

fn select_version(session: &dyn Session, prompt: &str, values: &[String]) -> Result<usize> {
    ensure!(!values.is_empty(), "No compatible versions are available");
    let index = session
        .interactive()
        .fuzzy_select(prompt, values)?
        .ok_or(super::super::process_runtime::Interrupted)?;
    ensure!(index < values.len(), "Version selection is out of range");
    Ok(index)
}
pub(super) fn parse_loader(value: &str) -> Result<LoaderKind> {
    Ok(match value.to_ascii_lowercase().as_str() {
        "neoforge" => LoaderKind::NeoForge,
        "fabric" => LoaderKind::Fabric,
        "forge" => LoaderKind::Forge,
        "quilt" => LoaderKind::Quilt,
        "none" | "vanilla" => LoaderKind::Vanilla,
        _ => anyhow::bail!("Unknown loader: {value}"),
    })
}
type LoaderChoice = (LoaderKind, Option<RetainedOutput<LoaderVersions>>);
async fn compatible_loader(
    session: &dyn Session,
    catalog: RuntimeCatalog,
    game: GameVersion,
    pin: Option<LoaderVersion>,
    limits: RuntimeCatalogLimits,
) -> Result<LoaderChoice> {
    let deadline = std::time::Instant::now()
        .checked_add(limits.transfer.deadline)
        .context("Loader discovery deadline overflow")?;
    // Each catalog owns its worker scope, while all four share host admission and one
    // network deadline. Serial waits neither shorten a provider's time nor delay its start.
    let runtime = OperationRuntime::new(
        ResourceGovernor::new(ResourceRequest {
            jobs: 4,
            memory_bytes: 512 << 20,
            scratch_bytes: limits
                .transfer
                .file_bytes
                .checked_mul(4)
                .context("Catalog scratch allowance overflow")?,
            open_files: 64,
        }),
        4,
    );
    let result = async {
        let mut pending = Vec::new();
        for family in [
            LoaderKind::NeoForge,
            LoaderKind::Fabric,
            LoaderKind::Forge,
            LoaderKind::Quilt,
        ] {
            let catalog = catalog.clone();
            let game = game.clone();
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let handle = runtime.start(move |mut scope| async move {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let versions = if remaining.is_zero() {
                    Err(crate::engine::acquisition::TransferError::Deadline.into())
                } else {
                    let mut request_limits = limits;
                    request_limits.transfer.deadline = remaining;
                    catalog
                        .loaders(&mut scope, game, family, request_limits)
                        .await
                };
                let _ = sender.send(versions);
                Ok(())
            })?;
            pending.push((family, handle, receiver));
        }
        cancellable(session, async {
            let mut choices = Vec::new();
            for (family, mut handle, receiver) in pending {
                let result = match &*handle.wait().await {
                    OperationOutcome::Completed(()) => {
                        receiver.await.context("Loader discovery result was lost")?
                    }
                    OperationOutcome::Failed(error) => Err(error.clone().into()),
                };
                choices.push((family, result));
            }
            Ok(choices)
        })
        .await
    }
    .await;
    runtime.shutdown().await;
    let choices = result?;
    let mut supported = if pin.is_none() {
        vec![(LoaderKind::Vanilla, None)]
    } else {
        Vec::new()
    };
    for (family, result) in choices {
        match result {
            Ok(versions)
                if !versions.versions().is_empty()
                    && pin
                        .as_ref()
                        .is_none_or(|pin| versions.resolve(Some(pin)).is_ok()) =>
            {
                supported.push((family, Some(versions)))
            }
            Ok(_) => {}
            Err(error) => session.display().status().warning(&format!(
                "Could not inspect {family:?} compatibility: {error:#}"
            )),
        }
    }
    choose_loader(session, supported)
}
fn choose_loader(session: &dyn Session, mut supported: Vec<LoaderChoice>) -> Result<LoaderChoice> {
    ensure!(
        !supported.is_empty(),
        "No compatible loader family supports the requested selection"
    );
    let names = supported
        .iter()
        .map(|(family, _)| match family {
            LoaderKind::Vanilla => "Vanilla",
            LoaderKind::NeoForge => "NeoForge",
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Forge => "Forge",
            LoaderKind::Quilt => "Quilt",
        })
        .collect::<Vec<_>>();
    let index = session
        .interactive()
        .select("Compatible mod loader", &names)?;
    ensure!(index < supported.len(), "Loader selection is out of range");
    Ok(supported.remove(index))
}
async fn metadata(session: &dyn Session, args: &InitArgs, selected: &Path) -> Result<PackMetadata> {
    let default_name = selected
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Pack");
    let name = match &args.pack_name {
        Some(value) => value.clone(),
        None => session
            .interactive()
            .text_input("Modpack name", default_name.into())?,
    };
    let author = match &args.author {
        Some(value) => value.clone(),
        None => {
            let cwd = if selected.is_dir() {
                selected
            } else {
                selected.parent().unwrap_or(selected)
            };
            let default = session
                .process()
                .execute("git", &["config", "user.name"], cwd)
                .await
                .ok()
                .filter(|output| output.success)
                .map(|output| output.stdout.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_default();
            session.process().check_cancelled()?;
            session.interactive().text_input("Author", default)?
        }
    };
    let version = match &args.pack_version {
        Some(value) => value.clone(),
        None => session
            .interactive()
            .text_input("Version", "1.0.0".into())?,
    };
    ensure!(!name.trim().is_empty(), "Pack name cannot be empty");
    ensure!(!version.trim().is_empty(), "Pack version cannot be empty");
    Ok(PackMetadata {
        name,
        version,
        author: (!author.is_empty()).then_some(author),
        description: None,
    })
}

/// The owned discovery runtime is drained on success, failure, and host interruption.
pub(super) async fn discover<T, F, Fut>(session: &dyn Session, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(WorkScope) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
{
    scoped(
        session,
        ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 512 << 20,
            scratch_bytes: 32 << 20,
            open_files: 64,
        }),
        work,
    )
    .await
}

async fn initialize_with_engine(
    session: &dyn Session,
    engine: &Engine,
    target: ProjectTarget,
    candidate: InitializeCandidate,
    force: bool,
) -> Result<()> {
    let prepared = match cancellable(
        session,
        engine.prepare(
            target,
            InitializeRequest {
                candidate,
                replacement: if force {
                    ProjectReplacementPolicy::ReplaceManagedContent
                } else {
                    ProjectReplacementPolicy::RejectExisting
                },
            },
        ),
    )
    .await?
    {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(_) => {
            anyhow::bail!("Empty initialization unexpectedly requires content")
        }
    };
    let view = prepared
        .view()
        .initialize()
        .context("Missing initialization preview")?;
    session.display().status().info(&format!(
        "Initialize {} v{}: Minecraft {}, {:?}{}",
        view.metadata.name,
        view.metadata.version,
        view.runtime.minecraft.as_str(),
        view.runtime.loader,
        view.runtime
            .loader_version
            .as_ref()
            .map(|value| format!(" {}", value.as_str()))
            .unwrap_or_default()
    ));
    show_changes(session, &view.files)?;
    apply(session, engine, prepared, "Initialization", |receipt| {
        let ExecutionReceipt::Initialize(receipt) = receipt else {
            anyhow::bail!("Unexpected initialization receipt");
        };
        Ok(format!(
            "Initialized {}: {} managed files",
            receipt.project.intent().metadata.name,
            receipt.publication.changed_files
        ))
    })
    .await
}

#[cfg(test)]
mod tests;
