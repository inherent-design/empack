//! CLI input decisions. These adapters nominate engine requests, never backend mutations.
use super::*;
use crate::{
    application::cli::{CliProjectType, SearchPlatform},
    engine::{
        acquisition::HttpAcquisition,
        addition::{DirectFileLimits, DirectFileSource},
        api::ExistingDependencyPolicy,
        content::SourceEvidencePolicy,
        project::ProjectReader,
        providers::{
            ProjectCandidate, ProjectSelector, ProviderAddInput, ProviderCatalog, ProviderFiles,
            ReleasePolicy, SearchLimits, SearchQuery,
        },
        publication::RecoveryReader,
    },
    networking::rate_budget::HostBudgetRegistry,
};
use empack_core::{
    identity::{CurseForgeFileId, ModrinthVersionId, PinSelector},
    model::{ContentKind, NonEmpty, ProviderKind, ResolvedProject},
    requirements::{Requirement, Requirements},
};
use std::{collections::BTreeSet, sync::Arc};

mod file_plan;
mod files;
mod import;
mod synchronization;
mod update;
pub use import::initialize;
pub use synchronization::synchronize;
pub use update::{adopt, update};

/// CLI flags remain input selectors until catalog responses establish canonical identity.
pub struct AddOptions {
    pub inputs: Vec<String>,
    pub force: bool,
    pub platform: Option<SearchPlatform>,
    pub kind: Option<CliProjectType>,
    pub version_id: Option<String>,
    pub file_id: Option<String>,
    pub file_plan: Option<PathBuf>,
    pub download_as_local: bool,
}
impl AddOptions {
    fn pin(&self) -> Result<Option<PinSelector>> {
        ensure!(!self.inputs.is_empty(), "Specify at least one dependency");
        ensure!(
            self.file_plan.is_none() || self.inputs.len() == 1,
            "A file plan requires exactly one provider input"
        );
        ensure!(
            self.version_id.is_none() || self.file_id.is_none(),
            "--version-id and --file-id select different providers"
        );
        ensure!(
            self.inputs.len() == 1 || (self.version_id.is_none() && self.file_id.is_none()),
            "An exact pin requires one dependency input"
        );
        if let Some(value) = &self.version_id {
            ensure!(
                self.platform != Some(SearchPlatform::Curseforge),
                "--version-id requires Modrinth"
            );
            return Ok(Some(PinSelector::ModrinthVersion(
                ModrinthVersionId::parse(value)?,
            )));
        }
        if let Some(value) = &self.file_id {
            ensure!(
                self.platform != Some(SearchPlatform::Modrinth),
                "--file-id requires CurseForge"
            );
            return Ok(Some(PinSelector::CurseForgeFile(CurseForgeFileId::parse(
                value,
            )?)));
        }
        Ok(None)
    }
}
fn kind(value: &CliProjectType) -> ContentKind {
    match value {
        CliProjectType::Mod => ContentKind::Mod,
        CliProjectType::Datapack => ContentKind::DataPack,
        CliProjectType::ResourcePack => ContentKind::ResourcePack,
        CliProjectType::Shader => ContentKind::ShaderPack,
        CliProjectType::World => ContentKind::World,
    }
}
fn required() -> Requirements {
    Requirements {
        client: Requirement::Required,
        server: Requirement::Required,
    }
}
fn provider(platform: Option<&SearchPlatform>, pin: Option<&PinSelector>) -> Option<ProviderKind> {
    match pin {
        Some(PinSelector::ModrinthVersion(_)) => Some(ProviderKind::Modrinth),
        Some(PinSelector::CurseForgeFile(_)) => Some(ProviderKind::CurseForge),
        None => match platform {
            Some(SearchPlatform::Modrinth) => Some(ProviderKind::Modrinth),
            Some(SearchPlatform::Curseforge) => Some(ProviderKind::CurseForge),
            _ => None,
        },
    }
}
/// A provider-looking URL must parse as that provider's selector; malformed URLs never search.
fn url_provider(input: &str) -> Result<Option<ProviderKind>> {
    if !input.contains("://") {
        return Ok(None);
    }
    let url = reqwest::Url::parse(input).context("Invalid dependency URL")?;
    Ok(match url.host_str() {
        Some("modrinth.com" | "www.modrinth.com") => Some(ProviderKind::Modrinth),
        Some("curseforge.com" | "www.curseforge.com") => Some(ProviderKind::CurseForge),
        _ => None,
    })
}
async fn current(
    session: &dyn Session,
) -> Result<crate::engine::runtime::RetainedOutput<ResolvedProject>> {
    current_for(session, InputOperation::Add).await
}
async fn current_for(
    session: &dyn Session,
    operation: InputOperation,
) -> Result<crate::engine::runtime::RetainedOutput<ResolvedProject>> {
    let (invocation, project) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    initialize::discover(session, move |mut scope| async move {
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
                let snapshot = ProjectReader::new(RecoveryReader::new(state)).capture(
                    &project,
                    &[],
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                if operation == InputOperation::Adopt && snapshot.prior_lock().is_none() {
                    dependencies::adoption_context(snapshot.intent())
                } else {
                    snapshot.require_resolved()
                }
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await
}

pub async fn add(session: &dyn Session, options: AddOptions) -> Result<()> {
    let catalog = ProviderCatalog::new(
        session
            .config()
            .app_config()
            .curseforge_api_client_key
            .clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    add_with_catalog(session, options, catalog).await
}
async fn add_with_catalog(
    session: &dyn Session,
    options: AddOptions,
    catalog: ProviderCatalog,
) -> Result<()> {
    selected_with_catalog(session, options, catalog, InputOperation::Add).await
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputOperation {
    Add,
    Adopt,
}

/// Describe new installed content, then use the same native observed-adoption publisher.
pub async fn adopt_inputs(session: &dyn Session, options: AddOptions) -> Result<()> {
    let catalog = dependencies::configured_services(session)?.catalog;
    selected_with_catalog(session, options, catalog, InputOperation::Adopt).await
}
async fn selected_with_catalog(
    session: &dyn Session,
    options: AddOptions,
    catalog: ProviderCatalog,
    operation: InputOperation,
) -> Result<()> {
    ensure!(
        operation != InputOperation::Adopt || (!options.force && !options.download_as_local),
        "Adoption cannot force updates or convert downloads to local ownership"
    );
    let pin = options.pin()?;
    let preferred = provider(options.platform.as_ref(), pin.as_ref());
    let current = current_for(session, operation).await?;
    let invocation = session.filesystem().current_dir()?;
    let file_plan = match &options.file_plan {
        Some(path) => Some(file_plan::read(session, absolute(&invocation, path)).await?),
        None => None,
    };
    let mut inputs = Vec::new();
    for value in &options.inputs {
        session.process().check_cancelled()?;
        let from_url = url_provider(value)?;
        if let (Some(requested), Some(actual)) = (preferred, from_url) {
            ensure!(
                requested == actual,
                "Provider URL conflicts with the selected provider or pin"
            );
        }
        let source = files::classify(&invocation, value, from_url.is_some())?;
        let source = if options.download_as_local {
            ensure!(
                options.platform.is_none(),
                "--download-as-local cannot retain provider ownership"
            );
            match source {
                Some(DirectFileSource::Download { alternatives, .. }) => {
                    Some(DirectFileSource::DownloadAsLocal { alternatives })
                }
                _ => anyhow::bail!("--download-as-local requires a direct HTTPS file URL"),
            }
        } else {
            source
        };
        if let Some(source) = source {
            ensure!(
                pin.is_none(),
                "Provider pins cannot be applied to a direct file"
            );
            ensure!(
                file_plan.is_none() || options.platform.is_some(),
                "A file plan requires provider identification for supplied files"
            );
            inputs.push(match &options.platform {
                None => AddHostInput::File(files::input(
                    session,
                    &current,
                    value,
                    source,
                    options.kind.as_ref(),
                )?),
                Some(platform) => AddHostInput::IdentifiedFile {
                    source,
                    kind: options.kind.as_ref().map(kind),
                    file_plan: file_plan.as_ref().map(|plan| (**plan).clone()),
                    providers: NonEmpty::new(match platform {
                        SearchPlatform::Modrinth => vec![ProviderKind::Modrinth],
                        SearchPlatform::Curseforge => vec![ProviderKind::CurseForge],
                        SearchPlatform::Both => {
                            vec![ProviderKind::Modrinth, ProviderKind::CurseForge]
                        }
                    })?,
                },
            });
            continue;
        }
        ensure!(
            operation != InputOperation::Adopt || pin.is_some(),
            "New provider adoption needs an exact --version-id/--file-id or a local file for provider identification"
        );
        let selected_kind = options.kind.as_ref().map(kind);
        let search_text =
            from_url.is_none() && pin.is_none() && value.chars().any(char::is_whitespace);
        let (selector, selected_kind) = if let Some(selected) =
            from_url.or(preferred).filter(|_| !search_text)
        {
            (ProjectSelector::parse(selected, value)?, selected_kind)
        } else {
            ensure!(
                options.platform != Some(SearchPlatform::Both) || catalog.availability().curseforge,
                "Searching both providers requires CurseForge credentials"
            );
            let (selector, kind) =
                search(session, &catalog, &current, value, selected_kind, preferred).await?;
            (selector, Some(kind))
        };
        inputs.push(AddHostInput::Provider(ProviderAddInput {
            selector,
            key: None,
            kind: selected_kind,
            pin: pin.clone(),
            requirements: file_plan
                .as_ref()
                .map(|plan| plan.requirements.clone())
                .unwrap_or_else(required),
            folder: None,
            files: file_plan
                .as_ref()
                .map(|plan| ProviderFiles::Placed(plan.files.clone()))
                .unwrap_or_default(),
        }));
    }
    if operation == InputOperation::Adopt {
        for input in &mut inputs {
            if let AddHostInput::File(input) = input {
                if let DirectFileSource::Download { origins, .. } = &input.source {
                    let placement = &input.placements.as_slice()[0];
                    let path = crate::engine::layout::ProjectLayout::path(
                        &empack_core::files::ManagedPath::Content {
                            layer: placement.layer,
                            path: placement.destination.relative().clone(),
                        },
                    )?;
                    let (_, project) = project_path(session)?;
                    input.source = DirectFileSource::ObservedUrl {
                        path: project.join(path.as_str()),
                        origins: origins.clone(),
                    };
                }
            } else if let AddHostInput::IdentifiedFile { source, .. } = input {
                ensure!(
                    matches!(source, DirectFileSource::Local(_)),
                    "Adoption identifies installed local bytes; use a local file instead of a download URL"
                );
            }
        }
    }
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut limits = DirectFileLimits::default();
    limits.transfer.deadline = Duration::from_secs(session.config().app_config().net_timeout);
    let services = dependencies::AdditionServices {
        catalog,
        transport,
        files: limits,
    };
    if operation == InputOperation::Adopt {
        return dependencies::adopt_with_services(session, NonEmpty::new(inputs)?, services).await;
    }
    dependencies::add_with_services(
        session,
        NonEmpty::new(inputs)?,
        ReleasePolicy::PreferStable,
        SourceEvidencePolicy::Compatibility,
        if options.force {
            ExistingDependencyPolicy::UpdateSameIdentity
        } else {
            ExistingDependencyPolicy::RejectExisting
        },
        services,
    )
    .await
}

async fn search(
    session: &dyn Session,
    catalog: &ProviderCatalog,
    current: &ResolvedProject,
    text: &str,
    selected_kind: Option<ContentKind>,
    preferred: Option<ProviderKind>,
) -> Result<(ProjectSelector, ContentKind)> {
    let mut providers = vec![ProviderKind::Modrinth];
    if catalog.availability().curseforge {
        providers.push(ProviderKind::CurseForge);
    }
    if let Some(provider) = preferred {
        providers = vec![provider];
    }
    search_providers(session, catalog, current, text, selected_kind, providers).await
}
async fn search_providers(
    session: &dyn Session,
    catalog: &ProviderCatalog,
    current: &ResolvedProject,
    text: &str,
    selected_kind: Option<ContentKind>,
    providers: Vec<ProviderKind>,
) -> Result<(ProjectSelector, ContentKind)> {
    ensure!(
        !session.config().app_config().yes && session.interactive().can_choose(),
        "Search requires a deliberate choice; supply a project URL or --platform with a slug/ID in headless mode"
    );
    ensure!(
        !providers.contains(&ProviderKind::CurseForge) || catalog.availability().curseforge,
        "Requested CurseForge search requires credentials"
    );
    let kinds = selected_kind.map_or_else(
        || {
            vec![
                ContentKind::Mod,
                ContentKind::ResourcePack,
                ContentKind::ShaderPack,
                ContentKind::DataPack,
                ContentKind::World,
            ]
        },
        |kind| vec![kind],
    );
    let mut games = vec![current.lock().runtime.minecraft.clone()];
    games.extend(current.intent().runtime.acceptable_versions.iter().cloned());
    let mut seen_games = BTreeSet::new();
    games.retain(|game| seen_games.insert(game.clone()));
    for kind in kinds {
        let mut windows = vec![(providers.clone(), games.clone(), 0)];
        while !windows.is_empty() {
            let mut candidates: Vec<ProjectCandidate> = Vec::new();
            let mut next = Vec::new();
            let mut seen = BTreeSet::new();
            for (providers, games, offset) in windows {
                let query = SearchQuery {
                    text: text.into(),
                    providers: NonEmpty::new(providers)?,
                    kind,
                    game_versions: NonEmpty::new(games)?,
                    loader: current.lock().runtime.loader,
                    offset,
                };
                let catalog = catalog.clone();
                let limits = SearchLimits {
                    catalog: crate::engine::providers::CatalogLimits {
                        deadline: Duration::from_secs(session.config().app_config().net_timeout),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let results = initialize::discover(session, move |mut scope| async move {
                    catalog.search_projects(&mut scope, query, limits).await
                })
                .await?;
                for page in &results.pages {
                    for candidate in &page.candidates {
                        if seen.insert(candidate.project.clone()) {
                            candidates.push(candidate.clone());
                        }
                    }
                    if let Some(offset) = page.next_offset {
                        next.push((vec![page.provider], vec![page.game.clone()], offset));
                    }
                }
            }
            if candidates.is_empty() && next.is_empty() {
                break;
            }
            let mut labels = candidates
                .iter()
                .map(|value| format!("{} — {} ({})", value.title, value.slug, value.project))
                .collect::<Vec<_>>();
            let more = !next.is_empty();
            if more {
                labels.push("More results".into());
            }
            labels.push("Try the next content type / cancel".into());
            let index = session
                .interactive()
                .fuzzy_select(&format!("Select {kind:?} for {text}"), &labels)?
                .ok_or(crate::application::process_runtime::Interrupted)?;
            ensure!(index < labels.len(), "Search selection is out of range");
            if let Some(candidate) = candidates.get(index) {
                return Ok((ProjectSelector::canonical(candidate.project.clone()), kind));
            }
            if more && index == candidates.len() {
                windows = next;
            } else {
                break;
            }
        }
    }
    anyhow::bail!("No dependency selected; nothing was published")
}

#[cfg(test)]
mod tests;

/// Optional participation and lossy export decisions are independent of execution approval.
pub async fn build(session: &dyn Session, args: &crate::application::BuildArgs) -> Result<()> {
    if args.continue_build {
        ensure!(
            !args.optional_defaults
                && args.optional_choices.is_empty()
                && !args.allow_optional_metadata_loss,
            "Continuation uses saved optional choices; prepare a new build to change them"
        );
        return super::continue_build(session, args).await;
    }
    let mut choices = std::collections::BTreeMap::new();
    for selected in &args.optional_choices {
        let (key, value) = selected
            .rsplit_once('=')
            .context("Optional choice requires CHOICE=true|false")?;
        empack_core::requirements::ChoiceKey::parse(key)?;
        let value = match value {
            "true" => true,
            "false" => false,
            _ => anyhow::bail!("Optional choice requires true or false"),
        };
        ensure!(
            choices.insert(key.to_owned(), value).is_none(),
            "Repeated optional choice"
        );
    }
    let optional = if !choices.is_empty() || args.optional_defaults {
        empack_core::inventory::OptionalPolicy::Resolve {
            choices,
            use_defaults: args.optional_defaults,
        }
    } else {
        empack_core::inventory::OptionalPolicy::Preserve
    };
    super::build(
        session,
        args,
        BuildDecisions {
            optional,
            mrpack_optional: if args.allow_optional_metadata_loss {
                crate::engine::mrpack::OptionalConversion::AcknowledgedMetadataLoss
            } else {
                crate::engine::mrpack::OptionalConversion::RejectMetadataLoss
            },
            ..Default::default()
        },
        Default::default(),
    )
    .await
}
