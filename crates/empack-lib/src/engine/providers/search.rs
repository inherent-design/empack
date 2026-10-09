//! Search offers bounded choices; ranking never grants identity or installation authority.
use super::*;
use empack_core::{
    identity::ModrinthProjectId,
    model::{GameVersion, LoaderKind, ProviderKind},
};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct SearchQuery {
    pub text: String,
    /// Preference order, retained even when another provider has a closer title.
    pub providers: NonEmpty<ProviderKind>,
    pub kind: ContentKind,
    pub game_versions: NonEmpty<GameVersion>,
    pub loader: LoaderKind,
    /// Applies to each provider/game window. Follow a page's next offset explicitly.
    pub offset: u32,
}
#[derive(Clone, Copy)]
pub struct SearchLimits {
    pub catalog: CatalogLimits,
    pub page_size: u32,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            catalog: CatalogLimits::default(),
            page_size: 20,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCandidate {
    pub project: ProviderProjectId,
    pub slug: String,
    pub title: String,
    /// Display ordering only; even 100 is not proof of identity or compatibility.
    pub similarity: u8,
    pub provider_rank: u32,
}
pub struct SearchPage {
    pub provider: ProviderKind,
    pub game: GameVersion,
    pub offset: u32,
    pub total: u64,
    /// True even when the provider paging ceiling prevents requesting another window.
    pub has_more: bool,
    /// None at exhaustion or the provider's paging ceiling; compare total with this window.
    pub next_offset: Option<u32>,
    pub candidates: Vec<ProjectCandidate>,
}
pub struct ProjectSearch {
    /// Provider-preference order, then explicitly accepted game-version order.
    pub pages: Vec<RetainedOutput<SearchPage>>,
    /// A missing provider capability is distinct from an empty successful search.
    pub unsupported: Vec<ProviderKind>,
}
impl ProviderCatalog {
    /// Bounded search windows, not an exhaustive catalog or automatic best-match selection.
    /// A failure in any requested supported provider discards all earlier result windows.
    pub async fn search_projects(
        &self,
        scope: &mut WorkScope,
        query: SearchQuery,
        limits: SearchLimits,
    ) -> Result<ProjectSearch> {
        ensure!(
            !query.text.trim().is_empty()
                && query.text.len() <= 256
                && !query.text.chars().any(char::is_control),
            CatalogError::InvalidSelector
        );
        ensure!(
            (1..=50).contains(&limits.page_size)
                && query
                    .offset
                    .checked_add(limits.page_size)
                    .is_some_and(|end| end <= 10_000)
                && query.game_versions.as_slice().len() <= 16
                && query
                    .game_versions
                    .as_slice()
                    .iter()
                    .all(|game| game.as_str().len() <= 128)
                && query.providers.as_slice().len() <= 2,
            CatalogError::Limit
        );
        ensure!(
            !matches!(query.kind, ContentKind::Config | ContentKind::OtherFile),
            CatalogError::UnsupportedKind
        );
        ensure!(
            !query
                .providers
                .as_slice()
                .contains(&ProviderKind::CurseForge)
                || self.transport.has_curseforge_key(),
            CatalogError::Unauthorized
        );
        let mut budget = transport::RequestBudget::new(limits.catalog)?;
        let mut pages = Vec::new();
        let mut unsupported = Vec::new();
        let mut providers = BTreeSet::new();
        for provider in query.providers.as_slice().iter().copied() {
            if !providers.insert(provider) {
                continue;
            }
            if provider == ProviderKind::Modrinth && query.kind == ContentKind::World {
                unsupported.push(provider);
                continue;
            }
            let mut games = BTreeSet::new();
            for game in query.game_versions.as_slice() {
                if !games.insert(game.clone()) {
                    continue;
                }
                let params = parameters(&query, game, provider, limits.page_size)?;
                let transport = self.transport.clone();
                let retained = ResourceRequest {
                    memory_bytes: limits.catalog.response_bytes,
                    ..Default::default()
                };
                let worker = scope.spawn(
                    ResourceRequest {
                        jobs: 1,
                        open_files: 1,
                        ..retained
                    },
                    retained,
                    move |cancel| async move {
                        let path: &[&str] = match provider {
                            ProviderKind::Modrinth => &["search"],
                            ProviderKind::CurseForge => &["mods", "search"],
                        };
                        let refs: Vec<_> = params
                            .iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect();
                        let bytes = transport
                            .get(provider, path, &refs, &mut budget, &cancel)
                            .await?;
                        Ok::<_, anyhow::Error>((bytes, budget))
                    },
                )?;
                let raw = scope.accept(worker.wait().await?)?.transpose()?;
                let retained = parse_resources(raw.0.len() as u64)?;
                let game = game.clone();
                let query = query.clone();
                let worker = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        ..retained
                    },
                    retained,
                    move |cancel| {
                        cancel.check()?;
                        let ((bytes, budget), _permit) = raw.into_parts();
                        let page = parse_page(&bytes, provider, game, &query, limits.page_size)?;
                        cancel.check()?;
                        budget.check_deadline()?;
                        Ok::<_, anyhow::Error>((page, budget))
                    },
                )?;
                let parsed = scope.accept(worker.wait().await?)?.transpose()?;
                let mut next_budget = None;
                pages.push(parsed.map(|(page, b)| {
                    next_budget = Some(b);
                    page
                }));
                budget = next_budget.expect("search page returns cumulative budget");
            }
        }
        scope.cancellation().check()?;
        budget.check_deadline()?;
        Ok(ProjectSearch { pages, unsupported })
    }
}
fn parameters(
    query: &SearchQuery,
    game: &GameVersion,
    provider: ProviderKind,
    size: u32,
) -> Result<Vec<(String, String)>> {
    let loader = match query.loader {
        LoaderKind::Vanilla => None,
        LoaderKind::Fabric => Some(("fabric", "4")),
        LoaderKind::Quilt => Some(("quilt", "5")),
        LoaderKind::Forge => Some(("forge", "1")),
        LoaderKind::NeoForge => Some(("neoforge", "6")),
    };
    Ok(match provider {
        ProviderKind::Modrinth => {
            let kind = match query.kind {
                ContentKind::Mod => "mod",
                ContentKind::ResourcePack => "resourcepack",
                ContentKind::ShaderPack => "shader",
                ContentKind::DataPack => "datapack",
                _ => return Err(CatalogError::UnsupportedKind.into()),
            };
            let mut facets = vec![
                vec![format!("all_project_types:{kind}")],
                vec![format!("versions:{}", game.as_str())],
            ];
            if query.kind == ContentKind::Mod
                && let Some((name, _)) = loader
            {
                facets.push(vec![format!("categories:{name}")]);
            }
            vec![
                ("query".into(), query.text.clone()),
                ("facets".into(), serde_json::to_string(&facets)?),
                ("index".into(), "relevance".into()),
                ("limit".into(), size.to_string()),
                ("offset".into(), query.offset.to_string()),
            ]
        }
        ProviderKind::CurseForge => {
            let class = match query.kind {
                ContentKind::Mod => 6,
                ContentKind::ResourcePack => 12,
                ContentKind::ShaderPack => 6552,
                ContentKind::DataPack => 6945,
                ContentKind::World => 17,
                _ => return Err(CatalogError::UnsupportedKind.into()),
            };
            let mut params = vec![
                ("gameId".into(), "432".into()),
                ("classId".into(), class.to_string()),
                ("searchFilter".into(), query.text.clone()),
                ("gameVersion".into(), game.as_str().into()),
                ("sortField".into(), "1".into()),
                ("sortOrder".into(), "desc".into()),
                ("pageSize".into(), size.to_string()),
                ("index".into(), query.offset.to_string()),
            ];
            if query.kind == ContentKind::Mod
                && let Some((_, code)) = loader
            {
                params.push(("modLoaderType".into(), code.into()));
            }
            params
        }
    })
}
#[derive(Deserialize)]
struct MrSearch {
    hits: Vec<MrHit>,
    offset: u32,
    limit: u32,
    total_hits: u64,
}
#[derive(Deserialize)]
struct MrHit {
    project_id: String,
    slug: String,
    title: String,
    project_type: String,
    #[serde(default)]
    all_project_types: Vec<String>,
    categories: Vec<String>,
    versions: Vec<String>,
}
#[derive(Deserialize)]
struct CfSearch {
    data: Vec<serde_json::Value>,
    pagination: Pagination,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: u32,
    page_size: u32,
    result_count: u32,
    total_count: u64,
}
fn parse_page(
    bytes: &[u8],
    provider: ProviderKind,
    game: GameVersion,
    query: &SearchQuery,
    size: u32,
) -> Result<SearchPage> {
    let (raw, total) = match provider {
        ProviderKind::Modrinth => {
            let response: MrSearch = json(bytes)?;
            ensure!(
                response.offset == query.offset && response.limit == size,
                CatalogError::InvalidRecord
            );
            ensure!(response.hits.len() <= size as usize, CatalogError::Limit);
            let mut rows = Vec::new();
            for hit in response.hits {
                let expected = match query.kind {
                    ContentKind::Mod => "mod",
                    ContentKind::DataPack => "datapack",
                    ContentKind::ResourcePack => "resourcepack",
                    ContentKind::ShaderPack => "shader",
                    _ => return Err(CatalogError::UnsupportedKind.into()),
                };
                let kind_matches = if hit.all_project_types.is_empty() {
                    hit.project_type == expected
                        || (query.kind == ContentKind::DataPack
                            && hit.project_type == "mod"
                            && hit.categories.iter().any(|v| v == "datapack"))
                } else {
                    hit.all_project_types.iter().any(|v| v == expected)
                };
                ensure!(
                    kind_matches && hit.versions.iter().any(|v| v == game.as_str()),
                    CatalogError::InvalidRecord
                );
                rows.push((
                    ProviderProjectId::Modrinth(ModrinthProjectId::parse(&hit.project_id)?),
                    hit.slug,
                    hit.title,
                ));
            }
            (rows, response.total_hits)
        }
        ProviderKind::CurseForge => {
            let response: CfSearch = json(bytes)?;
            ensure!(
                response.pagination.index == query.offset
                    && response.pagination.page_size == size
                    && response.pagination.result_count as usize == response.data.len(),
                CatalogError::InvalidRecord
            );
            ensure!(response.data.len() <= size as usize, CatalogError::Limit);
            let mut rows = Vec::new();
            for value in response.data {
                let project = curseforge::project(
                    &serde_json::to_vec(&serde_json::json!({"data":value}))?,
                    false,
                )?;
                ensure!(
                    project.kinds.as_slice().contains(&query.kind),
                    CatalogError::ContentKindMismatch
                );
                rows.push((project.id, project.slug, project.title));
            }
            (rows, response.pagination.total_count)
        }
    };
    let count = u32::try_from(raw.len()).map_err(|_| CatalogError::Limit)?;
    let end = query.offset.checked_add(count).ok_or(CatalogError::Limit)?;
    ensure!(
        count <= size
            && u64::from(end) <= total.max(u64::from(query.offset))
            && (count > 0 || u64::from(query.offset) >= total),
        CatalogError::InvalidRecord
    );
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for (rank, (project, slug, title)) in raw.into_iter().enumerate() {
        ensure!(
            seen.insert(project.clone())
                && !title.trim().is_empty()
                && title.len() <= 512
                && !title.chars().any(char::is_control),
            CatalogError::InvalidRecord
        );
        ensure!(
            slug.len() <= 256 && !slug.contains("://"),
            CatalogError::InvalidRecord
        );
        ProjectSelector::parse(provider, &slug).map_err(|_| CatalogError::InvalidRecord)?;
        candidates.push(ProjectCandidate {
            project,
            similarity: similarity(&query.text, &title).max(similarity(&query.text, &slug)),
            slug,
            title,
            provider_rank: query.offset + rank as u32,
        });
    }
    candidates.sort_by_key(|v| (std::cmp::Reverse(v.similarity), v.provider_rank));
    Ok(SearchPage {
        provider,
        game,
        offset: query.offset,
        total,
        has_more: u64::from(end) < total,
        next_offset: (u64::from(end) < total
            && end.checked_add(size).is_some_and(|last| last <= 10_000))
        .then_some(end),
        candidates,
    })
}
/// Bounded caller inputs and rolling rows keep fuzzy ranking independent of quadratic storage.
fn similarity(query: &str, label: &str) -> u8 {
    let a: Vec<_> = query.trim().to_lowercase().chars().collect();
    let b: Vec<_> = label.trim().to_lowercase().chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            row[j + 1] = (previous[j + 1] + 1)
                .min(row[j] + 1)
                .min(previous[j] + usize::from(left != right));
        }
        std::mem::swap(&mut row, &mut previous);
    }
    let length = a.len().max(b.len());
    (previous[b.len()] * 100)
        .checked_div(length)
        .map_or(0, |difference| (100 - difference) as u8)
}
#[cfg(test)]
mod tests;
