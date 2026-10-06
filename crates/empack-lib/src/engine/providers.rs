//! Read-only provider records. Selectors, canonical identities and exact byte assertions
//! remain distinct; this catalog has no project, cache-write or publication capability.
use super::{
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use crate::networking::rate_budget::HostBudgetRegistry;
use anyhow::{Result, ensure};
use empack_core::{
    identity::{PinSelector, ProviderProjectId},
    model::{ContentKind, Coverage, ExpectedContent, NonEmpty, ResolvedPin},
};
use std::{sync::Arc, time::Duration};

mod compatible;
pub use compatible::{
    CompatibleRequest, CompatibleSelection, ReleaseChannel, ReleasePolicy, SelectionLimits,
};
mod identify;
pub use identify::{Identification, IdentificationLimits, IdentifiedSelection};
mod curseforge;
mod modrinth;
mod refresh;
mod selector;
mod transport;
pub use selector::ProjectSelector;
pub use transport::CatalogError;

#[derive(Clone, Copy)]
pub struct CatalogLimits {
    pub response_bytes: u64,
    /// Across every response and retry in one resolution, not per endpoint.
    pub transfer_bytes: u64,
    pub deadline: Duration,
}
impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            response_bytes: 4 << 20,
            transfer_bytes: 16 << 20,
            deadline: Duration::from_secs(60),
        }
    }
}
/// Provider facts remain separate from the user's desired per-side requirements.
/// In particular, a provider's "optional on server" is not permission to make a
/// required root optional for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEvidence {
    pub version: Option<String>,
    pub client: Option<String>,
    pub server: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalProject {
    pub id: ProviderProjectId,
    pub slug: String,
    pub title: String,
    pub kind: ContentKind,
    pub environment: EnvironmentEvidence,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyRelation {
    Required,
    Optional,
    Incompatible,
    Embedded,
    Tool,
    Include,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDependency {
    pub project: Option<ProviderProjectId>,
    pub pin: Option<PinSelector>,
    pub filename: Option<String>,
    pub relation: DependencyRelation,
}
/// No Debug implementation: transient locators may include signed query values.
pub struct ProviderFile {
    pub filename: String,
    pub primary: bool,
    pub role: Option<String>,
    pub expected: ExpectedContent,
    pub alternatives: Vec<String>,
}
pub struct ProviderResolution {
    pub project: CanonicalProject,
    pub pin: ResolvedPin,
    /// All declared files; selecting a primary file never drops additional records here.
    pub files: NonEmpty<ProviderFile>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub environment: EnvironmentEvidence,
    pub dependencies: Vec<ProviderDependency>,
    /// Catalog completeness is not proof that the transitive closure was resolved.
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderAvailability {
    pub modrinth: bool,
    pub curseforge: bool,
}
impl ProviderAvailability {
    pub fn supports(self, project: &ProviderProjectId) -> bool {
        match project {
            ProviderProjectId::Modrinth(_) => self.modrinth,
            ProviderProjectId::CurseForge(_) => self.curseforge,
        }
    }
}
#[derive(Clone)]
pub struct ProviderCatalog {
    transport: transport::CatalogTransport,
}
impl ProviderCatalog {
    /// Read-only capability description; it makes no network request or authentication claim.
    pub fn availability(&self) -> ProviderAvailability {
        ProviderAvailability {
            modrinth: true,
            curseforge: self.transport.has_curseforge_key(),
        }
    }
    #[cfg(test)]
    pub(in crate::engine) fn for_loopback_tests(origin: &str, key: Option<String>) -> Self {
        Self {
            transport: transport::CatalogTransport::test(origin, key),
        }
    }
    pub fn new(curseforge_key: Option<String>, budgets: Arc<HostBudgetRegistry>) -> Result<Self> {
        Ok(Self {
            transport: transport::CatalogTransport::new(curseforge_key, budgets)?,
        })
    }
    /// A slug/URL is validated against returned metadata, never stored as a canonical ID.
    /// Body buffering and parsing are separately owned, admitted work.
    pub async fn resolve_selector(
        &self,
        scope: &mut WorkScope,
        selector: ProjectSelector,
        limits: CatalogLimits,
    ) -> Result<RetainedOutput<CanonicalProject>> {
        let transport = self.transport.clone();
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
                let mut budget = transport::RequestBudget::new(limits)?;
                let bytes = transport.project(&selector, &mut budget, &cancel).await?;
                Ok::<_, anyhow::Error>((selector, bytes))
            },
        )?;
        let bytes = scope.accept(work.wait().await?)?.transpose()?;
        let retained = parse_resources(bytes.1.len() as u64)?;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let result = decode_project(&bytes.0, &bytes.1)?;
                cancel.check()?;
                Ok::<_, anyhow::Error>(result)
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }
    /// Resolve only the requested project-owned pin. Compatibility selection is a separate
    /// planning decision; an exact request cannot quietly pick a different file/version.
    pub async fn resolve_exact(
        &self,
        scope: &mut WorkScope,
        pin: ResolvedPin,
        limits: CatalogLimits,
    ) -> Result<RetainedOutput<ProviderResolution>> {
        pin.validate()?;
        let transport = self.transport.clone();
        let retained = ResourceRequest {
            memory_bytes: limits
                .response_bytes
                .checked_mul(2)
                .ok_or(CatalogError::Limit)?,
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
                let mut budget = transport::RequestBudget::new(limits)?;
                let selector = ProjectSelector::canonical(pin.project.clone());
                let project = transport.project(&selector, &mut budget, &cancel).await?;
                let file = transport.selection(&pin, &mut budget, &cancel).await?;
                Ok::<_, anyhow::Error>((selector, pin, project, file))
            },
        )?;
        let bytes = scope.accept(work.wait().await?)?.transpose()?;
        let retained = parse_resources(
            (bytes.2.len() as u64)
                .checked_add(bytes.3.len() as u64)
                .ok_or(CatalogError::Limit)?,
        )?;
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let project = decode_project(&bytes.0, &bytes.2)?;
                let result = match &bytes.1.project {
                    ProviderProjectId::Modrinth(_) => {
                        modrinth::selection(project, &bytes.1, &bytes.3)
                    }
                    ProviderProjectId::CurseForge(_) => {
                        curseforge::selection(project, &bytes.1, &bytes.3)
                    }
                }?;
                cancel.check()?;
                Ok::<_, anyhow::Error>(result)
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    }
}
fn parse_resources(response_bytes: u64) -> Result<ResourceRequest> {
    // Admission estimate for DTO and normalized strings; response bytes are independently bounded.
    Ok(ResourceRequest {
        memory_bytes: response_bytes
            .checked_mul(16)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or(CatalogError::Limit)?,
        ..Default::default()
    })
}
fn decode_project(selector: &ProjectSelector, bytes: &[u8]) -> Result<CanonicalProject> {
    let project = match selector.provider() {
        empack_core::model::ProviderKind::Modrinth => modrinth::project(bytes),
        empack_core::model::ProviderKind::CurseForge => {
            curseforge::project(bytes, selector.is_slug())
        }
    }?;
    ensure!(selector.matches(&project), CatalogError::Identity);
    Ok(project)
}
fn filename(value: &str) -> Result<()> {
    let path = empack_core::path::PortableRelPath::parse(
        value,
        empack_core::path::PathSyntax::ProjectContent,
    )
    .map_err(|_| CatalogError::InvalidRecord)?;
    ensure!(!path.as_str().contains('/'), CatalogError::InvalidRecord);
    Ok(())
}
fn download_locator(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|_| CatalogError::InvalidRecord)?;
    let allowed = url.scheme() == "https";
    #[cfg(test)]
    let allowed = allowed
        || (url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            }));
    ensure!(
        allowed
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        CatalogError::InvalidRecord
    );
    // Execution-only locators may be signed. Durable document encoding separately rejects them.
    Ok(())
}
fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    // Serde's message can contain untrusted values, including signed URLs. Keep it out of errors.
    serde_json::from_slice(bytes).map_err(|_| CatalogError::InvalidRecord.into())
}
#[cfg(test)]
mod tests;
