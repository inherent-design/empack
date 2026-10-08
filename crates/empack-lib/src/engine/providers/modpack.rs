//! Modpack projects resolve to archive inputs, never invented dependency content kinds.
use super::*;
use empack_core::{
    identity::{CurseForgeProjectId, ModrinthProjectId},
    model::ProviderKind,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// A selected provider page or bare project selector; URLs grant no transport authority.
#[derive(Clone)]
pub struct ModpackSelector {
    project: ProjectSelector,
    version: Option<String>,
}
impl ModpackSelector {
    pub fn parse(provider: ProviderKind, input: &str) -> Result<Self> {
        if !input.contains("://") {
            return Ok(Self {
                project: ProjectSelector::parse(provider, input)?,
                version: None,
            });
        }
        let url = reqwest::Url::parse(input).map_err(|_| CatalogError::InvalidSelector)?;
        ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            CatalogError::InvalidSelector
        );
        let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
        let (slug, version) = match (provider, url.host_str(), parts.as_slice()) {
            (
                ProviderKind::Modrinth,
                Some("modrinth.com" | "www.modrinth.com"),
                ["", "modpack", slug],
            ) => (*slug, None),
            (
                ProviderKind::Modrinth,
                Some("modrinth.com" | "www.modrinth.com"),
                ["", "modpack", slug, "version", version],
            ) => (*slug, Some(*version)),
            (
                ProviderKind::CurseForge,
                Some("curseforge.com" | "www.curseforge.com"),
                ["", "minecraft", "modpacks", slug],
            ) => (*slug, None),
            (
                ProviderKind::CurseForge,
                Some("curseforge.com" | "www.curseforge.com"),
                ["", "minecraft", "modpacks", slug, "files", version],
            ) => (*slug, Some(*version)),
            _ => return Err(CatalogError::InvalidSelector.into()),
        };
        let value = Self {
            project: ProjectSelector::slug(provider, slug)?,
            version: None,
        };
        version.map_or(Ok(value.clone()), |version| value.with_version(version))
    }
    /// Modrinth accepts a project-scoped version number or ID; CurseForge requires a file ID.
    pub fn with_version(mut self, version: &str) -> Result<Self> {
        ensure!(
            !version.is_empty()
                && version.len() <= 256
                && !matches!(version, "." | "..")
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b)),
            CatalogError::InvalidSelector
        );
        if self.project.provider() == ProviderKind::CurseForge {
            empack_core::identity::CurseForgeFileId::parse(version)?;
        }
        ensure!(
            self.version.as_deref().is_none_or(|prior| prior == version),
            CatalogError::InvalidSelector
        );
        self.version = Some(version.into());
        Ok(self)
    }
}
#[derive(Clone)]
pub struct ModpackProject {
    pub id: ProviderProjectId,
    pub slug: String,
    pub title: String,
}
struct Candidate {
    pin: ResolvedPin,
    file: ProviderFile,
    rank: (u8, std::cmp::Reverse<OffsetDateTime>, ResolvedPin),
}
struct Page {
    candidates: Vec<Candidate>,
    seen: Vec<(ResolvedPin, [u8; 32])>,
    count: usize,
    next: Option<usize>,
    total: Option<usize>,
}
/// Retains catalog evidence and its admission; transient origins are never serialized or Debug.
pub struct ModpackArchive {
    project: RetainedOutput<ModpackProject>,
    pages: Vec<RetainedOutput<Page>>,
    selected: (usize, usize),
}
impl ModpackArchive {
    pub fn project(&self) -> &ModpackProject {
        &self.project
    }
    pub fn pin(&self) -> &ResolvedPin {
        &self.choice().pin
    }
    pub fn file(&self) -> &ProviderFile {
        &self.choice().file
    }
    fn choice(&self) -> &Candidate {
        &self.pages[self.selected.0].candidates[self.selected.1]
    }
}
enum Request {
    Project(ProjectSelector),
    Files {
        project: ProviderProjectId,
        index: usize,
        version: Option<String>,
    },
}
impl ProviderCatalog {
    /// One cumulative network budget covers project lookup, bounded pages and exact selection.
    /// Latest selection is deliberate import resolution, not an update to installed content.
    pub async fn resolve_modpack_archive(
        &self,
        scope: &mut WorkScope,
        selector: ModpackSelector,
        releases: ReleasePolicy,
        limits: SelectionLimits,
    ) -> Result<ModpackArchive> {
        ensure!(
            limits.pages > 0 && limits.candidates > 0,
            CatalogError::Limit
        );
        let raw = fetch(
            self.transport.clone(),
            scope,
            Request::Project(selector.project.clone()),
            transport::RequestBudget::new(limits.catalog)?,
            limits.catalog,
        )
        .await?;
        let retained = parse_resources(raw.0.len() as u64)?;
        let project_selector = selector.project;
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let ((bytes, budget), _permit) = raw.into_parts();
                let project = parse_project(&project_selector, &bytes)?;
                budget.check_deadline()?;
                Ok::<_, anyhow::Error>((project, budget))
            },
        )?;
        let (project, mut budget) = split_budget(scope.accept(worker.wait().await?)?.transpose()?);
        let mut pages: Vec<RetainedOutput<Page>> = Vec::new();
        let mut selected: Option<(usize, usize)> = None;
        let mut seen = BTreeMap::new();
        let mut total = None;
        let mut index = 0;
        let mut count = 0usize;
        loop {
            ensure!(pages.len() < limits.pages, CatalogError::Limit);
            let raw = fetch(
                self.transport.clone(),
                scope,
                Request::Files {
                    project: project.id.clone(),
                    index,
                    version: selector.version.clone(),
                },
                budget,
                limits.catalog,
            )
            .await?;
            let retained = parse_resources(raw.0.len() as u64)?;
            let id = project.id.clone();
            let version = selector.version.clone();
            let maximum = limits.candidates.saturating_sub(count);
            let worker = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    ..retained
                },
                retained,
                move |cancel| {
                    let ((bytes, budget), _permit) = raw.into_parts();
                    let page = parse_page(
                        &id,
                        version.as_deref(),
                        releases,
                        &bytes,
                        index,
                        maximum,
                        &cancel,
                    )?;
                    budget.check_deadline()?;
                    Ok::<_, anyhow::Error>((page, budget))
                },
            )?;
            let (page, next_budget) =
                split_budget(scope.accept(worker.wait().await?)?.transpose()?);
            budget = next_budget;
            count = count.checked_add(page.count).ok_or(CatalogError::Limit)?;
            if let Some(expected) = total {
                ensure!(page.total == Some(expected), CatalogError::InvalidRecord);
            }
            total = page.total;
            for (pin, fingerprint) in &page.seen {
                // Overlapping pages cannot silently change which published archive was selected.
                if let Some(prior) = seen.insert(pin.clone(), *fingerprint) {
                    ensure!(prior == *fingerprint, CatalogError::Identity);
                    anyhow::bail!(CatalogError::InvalidRecord);
                }
            }
            for (candidate_index, candidate) in page.candidates.iter().enumerate() {
                if selected.is_none_or(|(p, c)| {
                    let previous = if p == pages.len() {
                        &page.candidates[c]
                    } else {
                        &pages[p].candidates[c]
                    };
                    candidate.rank < previous.rank
                }) {
                    selected = Some((pages.len(), candidate_index));
                }
            }
            let next = page.next;
            pages.push(page);
            match next {
                Some(next) => index = next,
                None => break,
            }
        }
        Ok(ModpackArchive {
            project,
            pages,
            selected: selected.ok_or(CatalogError::NoCompatibleSelection)?,
        })
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
    let worker = scope.spawn(
        ResourceRequest {
            jobs: 1,
            open_files: 1,
            ..retained
        },
        retained,
        move |cancel| async move {
            let bytes = match request {
                Request::Project(selector)
                    if selector.provider() == ProviderKind::CurseForge && selector.is_slug() =>
                {
                    transport
                        .get(
                            ProviderKind::CurseForge,
                            &["mods", "search"],
                            &[
                                ("gameId", "432"),
                                ("classId", "4471"),
                                ("slug", &selector.value()),
                                ("pageSize", "2"),
                            ],
                            &mut budget,
                            &cancel,
                        )
                        .await?
                }
                Request::Project(selector) => {
                    transport.project(&selector, &mut budget, &cancel).await?
                }
                Request::Files {
                    project: ProviderProjectId::Modrinth(id),
                    version,
                    ..
                } => {
                    let mut path = vec!["project", id.as_str(), "version"];
                    if let Some(version) = &version {
                        path.push(version);
                    }
                    transport
                        .get(ProviderKind::Modrinth, &path, &[], &mut budget, &cancel)
                        .await?
                }
                Request::Files {
                    project: ProviderProjectId::CurseForge(id),
                    index,
                    version,
                } => {
                    let id = id.to_string();
                    let mut path = vec!["mods", id.as_str(), "files"];
                    let index = index.to_string();
                    let query = if let Some(version) = &version {
                        path.push(version);
                        vec![]
                    } else {
                        vec![("index", index.as_str()), ("pageSize", "50")]
                    };
                    transport
                        .get(
                            ProviderKind::CurseForge,
                            &path,
                            &query,
                            &mut budget,
                            &cancel,
                        )
                        .await?
                }
            };
            Ok::<_, anyhow::Error>((bytes, budget))
        },
    )?;
    scope.accept(worker.wait().await?)?.transpose()
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| CatalogError::InvalidRecord.into())
}
fn number(value: &Value, field: &str) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| CatalogError::InvalidRecord.into())
}
fn parse_project(selector: &ProjectSelector, bytes: &[u8]) -> Result<ModpackProject> {
    let mut value: Value = json(bytes)?;
    let project = match selector.provider() {
        ProviderKind::Modrinth => {
            ensure!(
                text(&value, "project_type")? == "modpack",
                CatalogError::UnsupportedKind
            );
            ModpackProject {
                id: ProviderProjectId::Modrinth(ModrinthProjectId::parse(text(&value, "id")?)?),
                slug: text(&value, "slug")?.into(),
                title: text(&value, "title")?.into(),
            }
        }
        ProviderKind::CurseForge => {
            value = value
                .get_mut("data")
                .ok_or(CatalogError::InvalidRecord)?
                .take();
            if selector.is_slug() {
                let values = value.as_array_mut().ok_or(CatalogError::InvalidRecord)?;
                ensure!(!values.is_empty(), CatalogError::NotFound);
                ensure!(values.len() == 1, CatalogError::Ambiguous);
                value = values.remove(0);
            }
            ensure!(
                number(&value, "gameId")? == 432 && number(&value, "classId")? == 4471,
                CatalogError::UnsupportedKind
            );
            ModpackProject {
                id: ProviderProjectId::CurseForge(CurseForgeProjectId::parse(
                    &number(&value, "id")?.to_string(),
                )?),
                slug: text(&value, "slug")?.into(),
                title: text(&value, "name")?.into(),
            }
        }
    };
    ensure!(
        !project.slug.is_empty() && !project.title.trim().is_empty(),
        CatalogError::InvalidRecord
    );
    ensure!(
        selector.matches_identity(&project.id, &project.slug),
        CatalogError::Identity
    );
    Ok(project)
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
    project: &ProviderProjectId,
    version: Option<&str>,
    releases: ReleasePolicy,
    bytes: &[u8],
    index: usize,
    maximum: usize,
    cancel: &crate::application::process_runtime::Cancellation,
) -> Result<Page> {
    let (values, next, total) = match (project, version) {
        (ProviderProjectId::Modrinth(_), Some(_)) => (vec![json(bytes)?], None, None),
        (ProviderProjectId::Modrinth(_), None) => (json::<Vec<Value>>(bytes)?, None, None),
        (ProviderProjectId::CurseForge(_), Some(_)) => {
            let mut value: Value = json(bytes)?;
            (
                vec![
                    value
                        .get_mut("data")
                        .ok_or(CatalogError::InvalidRecord)?
                        .take(),
                ],
                None,
                None,
            )
        }
        (ProviderProjectId::CurseForge(_), None) => {
            let page: CurseForgePage = json(bytes)?;
            let p = page.pagination;
            let end = index
                .checked_add(page.data.len())
                .ok_or(CatalogError::Limit)?;
            ensure!(
                p.index == index
                    && p.page_size > 0
                    && p.page_size <= 50
                    && p.result_count == page.data.len()
                    && page.data.len() <= p.page_size
                    && end <= p.total_count
                    && (!page.data.is_empty() || end == p.total_count),
                CatalogError::InvalidRecord
            );
            (
                page.data,
                (end < p.total_count).then_some(end),
                Some(p.total_count),
            )
        }
    };
    ensure!(values.len() <= maximum, CatalogError::Limit);
    let mut page = Page {
        count: values.len(),
        next,
        total,
        candidates: Vec::new(),
        seen: Vec::new(),
    };
    for value in values {
        cancel.check()?;
        let (id, published, channel, available) = match project {
            ProviderProjectId::Modrinth(owner) => {
                ensure!(
                    text(&value, "project_id")? == owner.as_str(),
                    CatalogError::Identity
                );
                let id = text(&value, "id")?.to_owned();
                if let Some(requested) = version {
                    ensure!(
                        requested == id
                            || value.get("version_number").and_then(Value::as_str)
                                == Some(requested),
                        CatalogError::Identity
                    );
                }
                let channel = match text(&value, "version_type")? {
                    "release" => 0,
                    "beta" | "alpha" => 1,
                    _ => return Err(CatalogError::InvalidRecord.into()),
                };
                let status = text(&value, "status")?;
                ensure!(
                    matches!(
                        status,
                        "listed" | "archived" | "unlisted" | "draft" | "scheduled" | "unknown"
                    ),
                    CatalogError::InvalidRecord
                );
                (
                    id,
                    text(&value, "date_published")?,
                    channel,
                    status == "listed"
                        || (version.is_some() && matches!(status, "archived" | "unlisted")),
                )
            }
            ProviderProjectId::CurseForge(owner) => {
                ensure!(
                    number(&value, "modId")? == owner.get() && number(&value, "gameId")? == 432,
                    CatalogError::Identity
                );
                let id = number(&value, "id")?.to_string();
                if let Some(requested) = version {
                    ensure!(requested == id, CatalogError::Identity);
                }
                let channel = match number(&value, "releaseType")? {
                    1 => 0,
                    2 | 3 => 1,
                    _ => return Err(CatalogError::InvalidRecord.into()),
                };
                let available = value
                    .get("isAvailable")
                    .and_then(Value::as_bool)
                    .ok_or(CatalogError::InvalidRecord)?;
                let server = match value.get("isServerPack") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(value)) => *value,
                    _ => return Err(CatalogError::InvalidRecord.into()),
                };
                (id, text(&value, "fileDate")?, channel, available && !server)
            }
        };
        let pin = ResolvedPin {
            project: project.clone(),
            selection: project.parse_pin(&id)?,
        };
        let date =
            OffsetDateTime::parse(published, &Rfc3339).map_err(|_| CatalogError::InvalidRecord)?;
        page.seen.push((
            pin.clone(),
            Sha256::digest(serde_json::to_vec(&value)?).into(),
        ));
        if !available
            || (version.is_none() && releases == ReleasePolicy::StableOnly && channel != 0)
        {
            continue;
        }
        let file = match project {
            ProviderProjectId::Modrinth(_) => {
                let files = modrinth::archive_files(&pin, &serde_json::to_vec(&value)?)?;
                let mut files = files.into_vec();
                let index = files.iter().position(|file| file.primary).unwrap_or(0);
                let file = files.remove(index);
                ensure!(
                    file.filename.to_ascii_lowercase().ends_with(".mrpack"),
                    CatalogError::UnsupportedKind
                );
                file
            }
            ProviderProjectId::CurseForge(_) => {
                let file = curseforge::archive_file(
                    &pin,
                    &serde_json::to_vec(&serde_json::json!({"data":value}))?,
                )?;
                ensure!(
                    file.filename.to_ascii_lowercase().ends_with(".zip"),
                    CatalogError::UnsupportedKind
                );
                file
            }
        };
        let rank = if version.is_none() && releases == ReleasePolicy::PreferStable {
            channel
        } else {
            0
        };
        page.candidates.push(Candidate {
            pin: pin.clone(),
            file,
            rank: (rank, std::cmp::Reverse(date), pin),
        });
    }
    Ok(page)
}

#[cfg(test)]
mod tests;
