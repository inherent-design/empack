use super::{CanonicalProject, CatalogError};
use anyhow::{Result, ensure};
use empack_core::{
    identity::{CurseForgeProjectId, ProviderProjectId},
    model::ProviderKind,
};
use reqwest::Url;

/// Input syntax only. Eight-character slugs are still selectors until the provider answers.
#[derive(Clone)]
pub struct ProjectSelector {
    value: Selector,
}
#[derive(Clone)]
enum Selector {
    Canonical(ProviderProjectId),
    Slug(ProviderKind, String),
}
impl ProjectSelector {
    pub fn canonical(id: ProviderProjectId) -> Self {
        Self {
            value: Selector::Canonical(id),
        }
    }
    pub fn parse(provider: ProviderKind, input: &str) -> Result<Self> {
        if input.contains("://") {
            let url = Url::parse(input).map_err(|_| CatalogError::InvalidSelector)?;
            ensure!(
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                CatalogError::InvalidSelector
            );
            let parts: Vec<_> = url
                .path()
                .trim_end_matches('/')
                .split('/')
                .filter(|v| !v.is_empty())
                .collect();
            let slug = match (provider, url.host_str(), parts.as_slice()) {
                (
                    ProviderKind::Modrinth,
                    Some("modrinth.com" | "www.modrinth.com"),
                    [
                        "mod" | "resourcepack" | "shader" | "datapack" | "plugin",
                        slug,
                    ],
                ) => *slug,
                (
                    ProviderKind::CurseForge,
                    Some("curseforge.com" | "www.curseforge.com"),
                    [
                        "minecraft",
                        "mc-mods" | "texture-packs" | "customization" | "worlds" | "shaders"
                        | "data-packs",
                        slug,
                    ],
                ) => *slug,
                _ => return Err(CatalogError::InvalidSelector.into()),
            };
            return Self::slug(provider, slug);
        }
        if provider == ProviderKind::CurseForge && input.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(Self::canonical(ProviderProjectId::CurseForge(
                CurseForgeProjectId::parse(input)?,
            )));
        }
        Self::slug(provider, input)
    }
    fn slug(provider: ProviderKind, input: &str) -> Result<Self> {
        ensure!(
            !input.is_empty()
                && input.len() <= 256
                && input != "."
                && input != ".."
                && input
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b)),
            CatalogError::InvalidSelector
        );
        Ok(Self {
            value: Selector::Slug(provider, input.into()),
        })
    }
    pub(super) fn provider(&self) -> ProviderKind {
        match &self.value {
            Selector::Canonical(ProviderProjectId::Modrinth(_)) => ProviderKind::Modrinth,
            Selector::Canonical(ProviderProjectId::CurseForge(_)) => ProviderKind::CurseForge,
            Selector::Slug(kind, _) => *kind,
        }
    }
    pub(super) fn is_slug(&self) -> bool {
        matches!(self.value, Selector::Slug(_, _))
    }
    pub(super) fn value(&self) -> String {
        match &self.value {
            Selector::Canonical(id) => id.to_string(),
            Selector::Slug(_, slug) => slug.clone(),
        }
    }
    pub(super) fn matches(&self, project: &CanonicalProject) -> bool {
        match &self.value {
            Selector::Canonical(id) => id == &project.id,
            // The Modrinth lookup route accepts both IDs and slugs. The response disambiguates.
            Selector::Slug(ProviderKind::Modrinth, value) => {
                project.slug == *value || project.id.to_string() == *value
            }
            Selector::Slug(ProviderKind::CurseForge, value) => project.slug == *value,
        }
    }
}
