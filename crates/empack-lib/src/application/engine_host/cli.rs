//! CLI input decisions. These adapters nominate engine requests, never backend mutations.
use super::*;
use crate::{
    application::cli::{CliProjectType, SearchPlatform},
    engine::{
        acquisition::HttpAcquisition,
        addition::DirectFileLimits,
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

mod files;
mod import;
pub use import::initialize;

/// CLI flags remain input selectors until catalog responses establish canonical identity.
pub struct AddOptions {
    pub inputs: Vec<String>,
    pub force: bool,
    pub platform: Option<SearchPlatform>,
    pub kind: Option<CliProjectType>,
    pub version_id: Option<String>,
    pub file_id: Option<String>,
}
impl AddOptions {
    fn pin(&self) -> Result<Option<PinSelector>> {
        ensure!(!self.inputs.is_empty(), "Specify at least one dependency");
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
                ProjectReader::new(RecoveryReader::new(state))
                    .capture(&project, &[], SnapshotLimits::default(), &cancel)?
                    .require_resolved()
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
    let pin = options.pin()?;
    let preferred = provider(options.platform.as_ref(), pin.as_ref());
    let current = current(session).await?;
    let invocation = session.filesystem().current_dir()?;
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
        if let Some(source) = source {
            ensure!(
                pin.is_none(),
                "Provider pins cannot be applied to a direct file"
            );
            ensure!(
                options.platform.is_none(),
                "Direct file input cannot also select a provider; identify the content first"
            );
            inputs.push(AddHostInput::File(files::input(
                session,
                &current,
                value,
                source,
                options.kind.as_ref(),
            )?));
            continue;
        }
        let selector = if let Some(selected) = from_url.or(preferred) {
            ProjectSelector::parse(selected, value)?
        } else {
            search(
                session,
                &catalog,
                &current,
                value,
                options.kind.as_ref().map(kind),
            )
            .await?
        };
        inputs.push(AddHostInput::Provider(ProviderAddInput {
            selector,
            key: None,
            kind: options.kind.as_ref().map(kind),
            pin: pin.clone(),
            requirements: required(),
            folder: None,
            files: ProviderFiles::Primary,
        }));
    }
    let transport = catalog.configure_acquisition(HttpAcquisition::new()?);
    let mut limits = DirectFileLimits::default();
    limits.transfer.deadline = Duration::from_secs(session.config().app_config().net_timeout);
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
        dependencies::AdditionServices {
            catalog,
            transport,
            files: limits,
        },
    )
    .await
}

async fn search(
    session: &dyn Session,
    catalog: &ProviderCatalog,
    current: &ResolvedProject,
    text: &str,
    selected_kind: Option<ContentKind>,
) -> Result<ProjectSelector> {
    ensure!(
        !session.config().app_config().yes && session.interactive().can_choose(),
        "Search requires a deliberate choice; supply a project URL or --platform with a slug/ID in headless mode"
    );
    let mut providers = vec![ProviderKind::Modrinth];
    if catalog.availability().curseforge {
        providers.push(ProviderKind::CurseForge);
    }
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
                return Ok(ProjectSelector::canonical(candidate.project.clone()));
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
