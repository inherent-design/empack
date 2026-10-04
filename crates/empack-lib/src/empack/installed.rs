//! Observed provider identity, independent of manifest labels and metadata filenames.
use crate::empack::config::{DependencySource, ProjectSpec};
use crate::primitives::{ProjectPlatform, ProjectType};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DependencyIdentity {
    pub platform: ProjectPlatform,
    pub project_id: String,
    pub project_type: ProjectType,
}

impl DependencyIdentity {
    pub fn from_spec(spec: &ProjectSpec) -> Option<Self> {
        match &spec.source {
            DependencySource::Platform {
                project_id,
                project_platform,
                ..
            } => Some(Self {
                platform: *project_platform,
                project_id: project_id.clone(),
                project_type: spec.project_type,
            }),
            DependencySource::Local { .. } | DependencySource::Url(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDependency {
    pub key: String,
    /// Unmanaged metadata can be retained, but cannot satisfy a provider declaration.
    pub identity: Option<DependencyIdentity>,
    pub version: Option<String>,
}

impl InstalledDependency {
    pub fn from_metadata(
        key: String,
        project_type: ProjectType,
        metadata: &toml::Value,
    ) -> anyhow::Result<Self> {
        let mut result = Self {
            key,
            identity: None,
            version: None,
        };
        for (platform, id_field, version_field) in [
            (ProjectPlatform::Modrinth, "mod-id", "version"),
            (ProjectPlatform::CurseForge, "project-id", "file-id"),
        ] {
            let Some(update) = metadata
                .get("update")
                .and_then(|v| v.get(platform.to_string()))
            else {
                continue;
            };
            anyhow::ensure!(
                result.identity.is_none(),
                "Ambiguous provider identity for {}",
                result.key
            );
            let field = |key| {
                update.get(key).and_then(|v| match v {
                    toml::Value::String(s) => Some(s.clone()),
                    toml::Value::Integer(n) => Some(n.to_string()),
                    _ => None,
                })
            };
            let project_id = field(id_field)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Missing provider identity in {}", result.key))?;
            result.identity = Some(DependencyIdentity {
                platform,
                project_id,
                project_type,
            });
            result.version = field(version_field);
        }
        Ok(result)
    }
}
