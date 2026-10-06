//! Bounded compatible selection is a resolver operation, never an implicit build/update effect.
use super::*;
use crate::application::process_runtime::Cancellation;
use empack_core::model::{GameVersion, LoaderKind, ProviderKind};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
    Release,
    Beta,
    Alpha,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReleasePolicy {
    StableOnly,
    #[default]
    PreferStable,
    Any,
}
#[derive(Debug, Clone)]
pub struct CompatibleRequest {
    pub project: ProviderProjectId,
    pub kind: ContentKind,
    /// Primary version first, then explicitly accepted alternatives. These are not SemVer ranges.
    pub game_versions: NonEmpty<GameVersion>,
    pub loader: LoaderKind,
    pub releases: ReleasePolicy,
}
#[derive(Clone, Copy)]
pub struct SelectionLimits {
    pub catalog: CatalogLimits,
    /// Across pages and alternate game-version queries, including duplicate records.
    pub candidates: usize,
    pub pages: usize,
}
impl Default for SelectionLimits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits::default(),
            candidates: 10_000,
            pages: 128,
        }
    }
}
pub struct CompatibleSelection {
    /// The requested kind satisfied by the selected version.
    pub kind: ContentKind,
    pub resolution: ProviderResolution,
    pub channel: ReleaseChannel,
    pub published_at: String,
    pub matched_game: GameVersion,
}
struct Candidate {
    selected: CompatibleSelection,
    rank: (u8, usize, std::cmp::Reverse<OffsetDateTime>, ResolvedPin),
}
struct Page {
    candidates: Vec<Candidate>,
    /// All record identities, including filtered records, to detect conflicting observations.
    seen: Vec<(ResolvedPin, [u8; 32])>,
    count: usize,
    next: Option<usize>,
    total: Option<usize>,
}
enum Request {
    Project(ProviderProjectId),
    Versions {
        project: ProviderProjectId,
        games: Vec<String>,
        index: usize,
    },
}
impl ProviderCatalog {
    /// Resolve a fresh exact selection. Sync callers retain a valid existing lock; only missing
    /// resolution or an explicit update should call this. Every page shares one byte/deadline budget.
    pub async fn resolve_compatible(
        &self,
        scope: &mut WorkScope,
        request: CompatibleRequest,
        limits: SelectionLimits,
    ) -> Result<RetainedOutput<CompatibleSelection>> {
        Ok(self
            .resolve_compatible_budget(
                scope,
                request,
                limits,
                transport::RequestBudget::new(limits.catalog)?,
            )
            .await?
            .0)
    }
    pub(super) async fn resolve_compatible_budget(
        &self,
        scope: &mut WorkScope,
        request: CompatibleRequest,
        limits: SelectionLimits,
        budget: transport::RequestBudget,
    ) -> Result<(
        RetainedOutput<CompatibleSelection>,
        transport::RequestBudget,
    )> {
        ensure!(
            limits.candidates > 0 && limits.pages > 0,
            CatalogError::Limit
        );
        let raw = fetch(
            self.transport.clone(),
            scope,
            Request::Project(request.project.clone()),
            budget,
            limits.catalog,
        )
        .await?;
        let project_bytes = raw.0.len() as u64;
        let retained = parse_resources(project_bytes)?;
        let selector = ProjectSelector::canonical(request.project.clone());
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let ((bytes, budget), _permit) = raw.into_parts();
                Ok::<_, anyhow::Error>((decode_project(&selector, &bytes)?, budget))
            },
        )?;
        let parsed = scope.accept(work.wait().await?)?.transpose()?;
        let ((project, mut budget), _project_permit) = parsed.into_parts();
        ensure!(
            project.kinds.as_slice().contains(&request.kind),
            CatalogError::ContentKindMismatch
        );
        let mut unique_games = BTreeSet::new();
        let normalized_project_bytes = project_storage(&project)?;
        let games: Vec<String> = request
            .game_versions
            .as_slice()
            .iter()
            .map(|value| value.as_str().to_owned())
            .filter(|value| unique_games.insert(value.clone()))
            .collect();
        // CurseForge accepts one gameVersion filter. Modrinth accepts the whole explicit list.
        let queries = match &request.project {
            ProviderProjectId::Modrinth(_) => vec![games],
            ProviderProjectId::CurseForge(_) => games.into_iter().map(|game| vec![game]).collect(),
        };
        let mut pages: Vec<RetainedOutput<Page>> = Vec::new();
        let mut seen = BTreeMap::new();
        let mut count = 0usize;
        let mut best: Option<(usize, usize)> = None;
        for games in queries {
            let mut index = 0;
            let mut total = None;
            let mut query_seen = BTreeSet::new();
            loop {
                ensure!(pages.len() < limits.pages, CatalogError::Limit);
                let raw = fetch(
                    self.transport.clone(),
                    scope,
                    Request::Versions {
                        project: request.project.clone(),
                        games: games.clone(),
                        index,
                    },
                    budget,
                    limits.catalog,
                )
                .await?;
                let retained = parse_resources(
                    normalized_project_bytes
                        .checked_add(raw.0.len() as u64)
                        .ok_or(CatalogError::Limit)?,
                )?;
                let project = project.clone();
                let request = request.clone();
                let remaining = limits.candidates.saturating_sub(count);
                let worker = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        ..retained
                    },
                    retained,
                    move |cancel| {
                        let ((bytes, budget), _permit) = raw.into_parts();
                        let page =
                            parse_page(&project, &request, &bytes, index, remaining, &cancel)?;
                        Ok::<_, anyhow::Error>((page, budget))
                    },
                )?;
                let parsed = scope.accept(worker.wait().await?)?.transpose()?;
                let mut next_budget = None;
                let page = parsed.map(|(page, value)| {
                    next_budget = Some(value);
                    page
                });
                budget = next_budget.expect("parsed page returns its budget");
                count = count.checked_add(page.count).ok_or(CatalogError::Limit)?;
                if let Some(previous) = total {
                    ensure!(page.total == Some(previous), CatalogError::InvalidRecord);
                }
                total = page.total;
                let next = page.next;
                let page_index = pages.len();
                for (pin, fingerprint) in &page.seen {
                    ensure!(query_seen.insert(pin.clone()), CatalogError::InvalidRecord);
                    if let Some(previous) = seen.insert(pin.clone(), *fingerprint) {
                        ensure!(previous == *fingerprint, CatalogError::InvalidRecord);
                    }
                }
                // Keep admitted pages alive through comparison; never select from a truncated list.
                pages.push(page);
                for (candidate_index, candidate) in pages[page_index].candidates.iter().enumerate()
                {
                    if best.is_none_or(|(p, c)| candidate.rank < pages[p].candidates[c].rank) {
                        best = Some((page_index, candidate_index));
                    }
                }
                match next {
                    Some(value) => index = value,
                    None => break,
                }
            }
        }
        scope.cancellation().check()?;
        budget.check_deadline()?;
        let (page, candidate) = best.ok_or(CatalogError::NoCompatibleSelection)?;
        Ok((
            pages
                .swap_remove(page)
                .map(|mut page| page.candidates.swap_remove(candidate).selected),
            budget,
        ))
    }
}

async fn fetch(
    transport: transport::CatalogTransport,
    scope: &mut WorkScope,
    request: Request,
    mut budget: transport::RequestBudget,
    limits: CatalogLimits,
) -> Result<RetainedOutput<(Vec<u8>, transport::RequestBudget)>> {
    let retained = ResourceRequest {
        memory_bytes: limits.response_bytes,
        ..Default::default()
    };
    let work = scope.spawn(
        ResourceRequest {
            jobs: 1,
            open_files: 1,
            ..retained
        },
        retained,
        move |cancel| async move {
            let bytes = match request {
                Request::Project(project) => {
                    transport
                        .project(&ProjectSelector::canonical(project), &mut budget, &cancel)
                        .await?
                }
                Request::Versions {
                    project,
                    games,
                    index,
                } => match project {
                    ProviderProjectId::Modrinth(project) => {
                        let games = serde_json::to_string(&games)?;
                        transport
                            .get(
                                ProviderKind::Modrinth,
                                &["project", project.as_str(), "version"],
                                &[("game_versions", &games), ("include_changelog", "false")],
                                &mut budget,
                                &cancel,
                            )
                            .await?
                    }
                    ProviderProjectId::CurseForge(project) => {
                        let index = index.to_string();
                        transport
                            .get(
                                ProviderKind::CurseForge,
                                &["mods", &project.to_string(), "files"],
                                &[
                                    ("gameVersion", &games[0]),
                                    ("index", &index),
                                    ("pageSize", "50"),
                                ],
                                &mut budget,
                                &cancel,
                            )
                            .await?
                    }
                },
            };
            Ok::<_, anyhow::Error>((bytes, budget))
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: usize,
    page_size: usize,
    result_count: usize,
    total_count: usize,
}
#[derive(Deserialize)]
struct CurseForgePage {
    data: Vec<Value>,
    pagination: Pagination,
}
fn parse_page(
    project: &CanonicalProject,
    request: &CompatibleRequest,
    bytes: &[u8],
    index: usize,
    maximum: usize,
    cancel: &Cancellation,
) -> Result<Page> {
    let (values, next, total) = match project.id {
        ProviderProjectId::Modrinth(_) => (json::<Vec<Value>>(bytes)?, None, None),
        ProviderProjectId::CurseForge(_) => {
            let page: CurseForgePage = json(bytes)?;
            let pagination = page.pagination;
            let end = index
                .checked_add(page.data.len())
                .ok_or(CatalogError::Limit)?;
            ensure!(
                pagination.index == index
                    && pagination.page_size > 0
                    && pagination.page_size <= 50
                    && pagination.result_count == page.data.len()
                    && page.data.len() <= pagination.page_size
                    && end <= pagination.total_count,
                CatalogError::InvalidRecord
            );
            ensure!(
                !page.data.is_empty() || end == pagination.total_count,
                CatalogError::InvalidRecord
            );
            (
                page.data,
                (end < pagination.total_count).then_some(end),
                Some(pagination.total_count),
            )
        }
    };
    ensure!(values.len() <= maximum, CatalogError::Limit);
    let count = values.len();
    let mut candidates = Vec::new();
    let mut seen = Vec::new();
    for value in values {
        cancel.check()?;
        let (pin, published, channel, available) = match &project.id {
            ProviderProjectId::Modrinth(_) => {
                let pin = project.id.parse_pin(text_field(&value, "id")?)?;
                let channel = match text_field(&value, "version_type")? {
                    "release" => ReleaseChannel::Release,
                    "beta" => ReleaseChannel::Beta,
                    "alpha" => ReleaseChannel::Alpha,
                    _ => return Err(CatalogError::InvalidRecord.into()),
                };
                let status = text_field(&value, "status")?;
                ensure!(
                    matches!(
                        status,
                        "listed" | "archived" | "draft" | "unlisted" | "scheduled" | "unknown"
                    ),
                    CatalogError::InvalidRecord
                );
                (
                    pin,
                    text_field(&value, "date_published")?.to_owned(),
                    channel,
                    status == "listed",
                )
            }
            ProviderProjectId::CurseForge(_) => {
                let id = value
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or(CatalogError::InvalidRecord)?;
                let pin = project.id.parse_pin(&id.to_string())?;
                let channel = match value.get("releaseType").and_then(Value::as_u64) {
                    Some(1) => ReleaseChannel::Release,
                    Some(2) => ReleaseChannel::Beta,
                    Some(3) => ReleaseChannel::Alpha,
                    _ => return Err(CatalogError::InvalidRecord.into()),
                };
                (
                    pin,
                    text_field(&value, "fileDate")?.to_owned(),
                    channel,
                    value
                        .get("isAvailable")
                        .and_then(Value::as_bool)
                        .ok_or(CatalogError::InvalidRecord)?,
                )
            }
        };
        let pin = ResolvedPin {
            project: project.id.clone(),
            selection: pin,
        };
        let date =
            OffsetDateTime::parse(&published, &Rfc3339).map_err(|_| CatalogError::InvalidRecord)?;
        let fingerprint = fingerprint(&project.id, &value)?;
        seen.push((pin.clone(), fingerprint));
        // Unavailable records remain identity observations, not downloadable selections.
        // Providers may omit their payload evidence; that cannot poison an eligible sibling.
        let owned = match &project.id {
            ProviderProjectId::Modrinth(id) => {
                value.get("project_id").and_then(Value::as_str) == Some(id.as_str())
            }
            ProviderProjectId::CurseForge(id) => {
                value.get("modId").and_then(Value::as_u64) == Some(id.get())
                    && value.get("gameId").and_then(Value::as_u64) == Some(432)
            }
        };
        ensure!(owned, CatalogError::Identity);
        if !available {
            continue;
        }
        let resolution = match &project.id {
            ProviderProjectId::Modrinth(_) => {
                modrinth::selection(project.clone(), &pin, &serde_json::to_vec(&value)?)?
            }
            ProviderProjectId::CurseForge(_) => curseforge::selection(
                project.clone(),
                &pin,
                &serde_json::to_vec(&serde_json::json!({"data":value}))?,
            )?,
        };
        let channel_rank = match (request.releases, channel) {
            (ReleasePolicy::StableOnly, ReleaseChannel::Beta | ReleaseChannel::Alpha) => continue,
            (ReleasePolicy::PreferStable, ReleaseChannel::Beta | ReleaseChannel::Alpha) => 1,
            _ => 0,
        };
        let Some(game) = request
            .game_versions
            .as_slice()
            .iter()
            .position(|game| resolution.game_versions.iter().any(|v| v == game.as_str()))
        else {
            continue;
        };
        if !resolution.kinds.as_slice().contains(&request.kind)
            || !loader_matches(&resolution, request)
        {
            continue;
        }
        let candidate = Candidate {
            rank: (channel_rank, game, std::cmp::Reverse(date), pin),
            selected: CompatibleSelection {
                kind: request.kind,
                resolution,
                channel,
                published_at: published,
                matched_game: request.game_versions.as_slice()[game].clone(),
            },
        };
        if candidates
            .first()
            .is_none_or(|previous: &Candidate| candidate.rank < previous.rank)
        {
            candidates.clear();
            candidates.push(candidate);
        }
    }
    Ok(Page {
        candidates,
        seen,
        count,
        next,
        total,
    })
}
fn project_storage(project: &CanonicalProject) -> Result<u64> {
    [
        Some(project.slug.as_str()),
        Some(project.title.as_str()),
        project.environment.version.as_deref(),
        project.environment.client.as_deref(),
        project.environment.server.as_deref(),
    ]
    .into_iter()
    .flatten()
    .try_fold(1024u64, |sum, value| {
        sum.checked_add(value.len() as u64)
            .ok_or_else(|| CatalogError::Limit.into())
    })
}
fn text_field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| CatalogError::InvalidRecord.into())
}
pub(super) fn loader_matches(resolution: &ProviderResolution, request: &CompatibleRequest) -> bool {
    match request.kind {
        ContentKind::Mod => {
            let loader = match request.loader {
                LoaderKind::Vanilla => return false,
                LoaderKind::Fabric => "fabric",
                LoaderKind::Quilt => "quilt",
                LoaderKind::Forge => "forge",
                LoaderKind::NeoForge => "neoforge",
            };
            resolution.loaders.iter().any(|value| value == loader)
        }
        // Game-specific files are not rejected merely because the pack uses a mod loader.
        // Shader backend/environment facts remain visible for the planner's placement decisions.
        ContentKind::ResourcePack
        | ContentKind::ShaderPack
        | ContentKind::DataPack
        | ContentKind::World => true,
        ContentKind::Config | ContentKind::OtherFile => false,
    }
}
fn fingerprint(project: &ProviderProjectId, value: &Value) -> Result<[u8; 32]> {
    // Ignore changing download/popularity counts; retain all selection and file declarations.
    let fields: &[&str] = match project {
        ProviderProjectId::Modrinth(_) => &[
            "id",
            "project_id",
            "files",
            "game_versions",
            "loaders",
            "environment",
            "dependencies",
            "date_published",
            "version_type",
            "status",
        ],
        ProviderProjectId::CurseForge(_) => &[
            "id",
            "gameId",
            "modId",
            "fileName",
            "fileLength",
            "downloadUrl",
            "hashes",
            "gameVersions",
            "dependencies",
            "fileDate",
            "releaseType",
            "isAvailable",
        ],
    };
    let selected: BTreeMap<_, _> = fields
        .iter()
        .map(|name| (*name, value.get(*name)))
        .collect();
    Ok(Sha256::digest(serde_json::to_vec(&selected)?).into())
}
#[cfg(test)]
mod tests;
