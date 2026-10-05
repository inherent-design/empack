//! Observed provider identity, independent of manifest labels and metadata filenames.
use crate::empack::config::{DependencySource, ProjectSpec};
use crate::primitives::{ProjectPlatform, ProjectType};
use empack_core::identity::{CurseForgeProjectId, ModrinthProjectId, ProviderProjectId};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DependencyIdentity {
    pub project: ProviderProjectId,
    pub project_type: ProjectType,
}

impl DependencyIdentity {
    pub fn parse(
        platform: ProjectPlatform,
        project_id: &str,
        project_type: ProjectType,
    ) -> anyhow::Result<Self> {
        let project = match platform {
            ProjectPlatform::Modrinth => {
                ProviderProjectId::Modrinth(ModrinthProjectId::parse(project_id)?)
            }
            ProjectPlatform::CurseForge => {
                ProviderProjectId::CurseForge(CurseForgeProjectId::parse(project_id)?)
            }
        };
        Ok(Self {
            project,
            project_type,
        })
    }

    pub fn from_record(record: &super::config::DependencyRecord) -> anyhow::Result<Self> {
        Self::parse(record.platform, &record.project_id, record.project_type)
    }

    pub fn from_spec(spec: &ProjectSpec) -> anyhow::Result<Option<Self>> {
        match &spec.source {
            DependencySource::Platform {
                project_id,
                project_platform,
                ..
            } if !project_id.is_empty() => {
                Self::parse(*project_platform, project_id, spec.project_type).map(Some)
            }
            // The old planning DTO also carries search requests. They have no identity.
            DependencySource::Platform { .. }
            | DependencySource::Local { .. }
            | DependencySource::Url(_) => Ok(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDependency {
    pub key: String,
    /// Unmanaged metadata can be retained, but cannot satisfy a provider declaration.
    pub identity: Option<DependencyIdentity>,
    pub version: Option<empack_core::identity::PinSelector>,
}

impl InstalledDependency {
    pub fn from_metadata(
        key: String,
        project_type: ProjectType,
        metadata: &toml::Value,
    ) -> anyhow::Result<Self> {
        let provider = crate::engine::backend::provider_observation(metadata, &key)?;
        Ok(Self {
            key,
            version: provider.as_ref().and_then(|value| value.selection.clone()),
            identity: provider.map(|value| DependencyIdentity {
                project: value.project,
                project_type,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_observation_cannot_satisfy_or_authorize_removal() {
        for update in [
            "[update.modrinth]\nmod-id = 'sodium'",
            "[update.modrinth]\nmod-id = 'AANobbMI'\nversion = 'latest'",
            "[update.modrinth]\nmod-id = 'AANobbMI'\nversion = true",
            "[update.curseforge]\nproject-id = 238222\nfile-id = []",
            "[update.curseforge]\nproject-id = -1",
            "[update.curseforge]\nproject-id = '0238222'",
            "[update.curseforge]\nproject-id = 238222\nfile-id = 0",
            "[update.modrinth]\nmod-id = 'AANobbMI'\n[update.curseforge]\nproject-id = 238222",
        ] {
            let metadata = toml::from_str(update).unwrap();
            assert!(
                InstalledDependency::from_metadata("alias".into(), ProjectType::Mod, &metadata)
                    .is_err(),
                "{update}"
            );
        }
    }

    #[test]
    fn observation_keeps_metadata_key_separate_from_provider_identity() {
        let metadata =
            toml::from_str("[update.modrinth]\nmod-id = 'AANobbMI'\nversion = 'Version1'").unwrap();
        let observed = InstalledDependency::from_metadata(
            "renderer-alias".into(),
            ProjectType::Mod,
            &metadata,
        )
        .unwrap();
        assert_eq!(observed.key, "renderer-alias");
        let identity = observed.identity.unwrap();
        assert_eq!(identity.project.to_string(), "AANobbMI");
        assert_eq!(
            observed.version,
            Some(identity.project.parse_pin("Version1").unwrap())
        );
    }
}
