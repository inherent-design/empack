//! Packwiz wire observations. Metadata describes an installation; it is not a byte proof.
pub(in crate::engine) mod index;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ExpectedDigest,
    identity::{CurseForgeProjectId, ModrinthProjectId, PinSelector, ProviderProjectId},
    model::{ContentLayer, DependencyKey, ResolvedFile, ResolvedPin, ResolvedProject},
    path::{InstallDestination, PortableRelPath},
    requirements::{Environments, Requirements},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderObservation {
    pub project: ProviderProjectId,
    /// Missing pins remain observable, but cannot satisfy an exact selection.
    pub selection: Option<PinSelector>,
}
/// One parser is shared by command adapters and normalized backend observation.
pub fn provider_observation(
    metadata: &toml::Value,
    origin: &str,
) -> Result<Option<ProviderObservation>> {
    let mut result = None;
    for (provider, id_field, version_field) in [
        ("modrinth", "mod-id", "version"),
        ("curseforge", "project-id", "file-id"),
    ] {
        let Some(update) = metadata.get("update").and_then(|value| value.get(provider)) else {
            continue;
        };
        ensure!(result.is_none(), "Ambiguous provider identity in {origin}");
        let field = |name| {
            update
                .get(name)
                .map(|value| match value {
                    toml::Value::String(value) => Ok(value.clone()),
                    toml::Value::Integer(value) => Ok(value.to_string()),
                    _ => anyhow::bail!("Invalid provider field {name} in {origin}"),
                })
                .transpose()
        };
        let id = field(id_field)?.context("Missing provider project identity")?;
        let project = if provider == "modrinth" {
            ProviderProjectId::Modrinth(ModrinthProjectId::parse(&id)?)
        } else {
            ProviderProjectId::CurseForge(CurseForgeProjectId::parse(&id)?)
        };
        let selection = field(version_field)?
            .map(|value| project.parse_pin(&value))
            .transpose()?;
        result = Some(ProviderObservation { project, selection });
    }
    Ok(result)
}

/// Keep locators out of Debug output: observed backend documents may contain secrets.
pub enum BackendDownload {
    Url(String),
    CurseForgeMetadata,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendOptional {
    pub default_enabled: bool,
    pub description: Option<String>,
}
/// What established agreement with a derivative backend record. This is not source authenticity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestComparisonBasis {
    AcquiredBytes,
    SameAlgorithmDeclaration,
    /// The lock has independent export evidence, but no comparable digest algorithm.
    IndependentLockedReference,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDigestComparison {
    pub metadata_path: PortableRelPath,
    pub declared: ExpectedDigest,
    pub basis: DigestComparisonBasis,
}
pub struct BackendFile {
    pub metadata_path: PortableRelPath,
    pub destination: InstallDestination,
    pub provider: Option<ProviderObservation>,
    pub digest: ExpectedDigest,
    pub environments: Environments,
    pub optional: Option<BackendOptional>,
    pub download: BackendDownload,
}
impl BackendFile {
    /// Path is relative to the pack root. The filename cannot authorize a directory or escape.
    /// Additional backend fields remain uninterpreted instead of becoming mutation authority.
    pub fn parse(metadata_path: PortableRelPath, bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "Backend metadata exceeds 16 MiB limit"
        );
        ensure!(
            metadata_path.as_str().ends_with(".pw.toml"),
            "Not a backend metadata filename"
        );
        let metadata: toml::Value = toml::from_str(std::str::from_utf8(bytes)?)?;
        let destination = destination(&metadata_path, required_text(&metadata, "filename")?)?;
        let provider = provider_observation(&metadata, metadata_path.as_str())?;
        let download = metadata
            .get("download")
            .context("Backend metadata lacks download description")?;
        let digest = ExpectedDigest::parse(
            required_text(download, "hash-format")?,
            required_text(download, "hash")?,
        )?;
        let download = match optional_text(download, "mode")?.unwrap_or("url") {
            "" | "url" => BackendDownload::Url(required_text(download, "url")?.to_owned()),
            "metadata:curseforge" => {
                ensure!(
                    matches!(
                        provider.as_ref().map(|value| &value.project),
                        Some(ProviderProjectId::CurseForge(_))
                    ),
                    "CurseForge download lacks its provider identity"
                );
                BackendDownload::CurseForgeMetadata
            }
            _ => anyhow::bail!("Unsupported backend download mode"),
        };
        let environments = match optional_text(&metadata, "side")?.unwrap_or("both") {
            "" | "both" => Environments::Both,
            "client" => Environments::Client,
            "server" => Environments::Server,
            _ => anyhow::bail!("Unknown backend environment"),
        };
        let optional = metadata
            .get("option")
            .map(|option| -> Result<_> {
                ensure!(option.is_table(), "Backend option must be a table");
                if optional_bool(option, "optional")?.unwrap_or(false) {
                    Ok(Some(BackendOptional {
                        default_enabled: optional_bool(option, "default")?.unwrap_or(false),
                        description: optional_text(option, "description")?
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                    }))
                } else {
                    Ok(None)
                }
            })
            .transpose()?
            .flatten();
        Ok(Self {
            metadata_path,
            destination,
            provider,
            digest,
            environments,
            optional,
            download,
        })
    }
    /// Bind derivative metadata to one exact locked file, independent of its filename.
    /// An unclaimed observation remains untracked; conflicting claims are never guessed.
    pub(super) fn locked_owner<'a>(
        &self,
        project: &'a ResolvedProject,
    ) -> Result<Option<(&'a DependencyKey, &'a ResolvedFile)>> {
        let mut owner = None;
        let mut claimed = false;
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                for placement in file.placements.as_slice() {
                    if placement.layer != ContentLayer::Common
                        || placement.destination != self.destination
                    {
                        continue;
                    }
                    claimed = true;
                    if self.matches_selection_and_requirements(
                        dependency.selected.as_ref(),
                        &placement.requirements,
                    )? {
                        ensure!(
                            owner.is_none(),
                            "Backend metadata has multiple locked owners: {}",
                            self.metadata_path.as_str()
                        );
                        owner = Some((key, file));
                    }
                }
            }
        }
        ensure!(
            !claimed || owner.is_some(),
            "Backend metadata does not identify one exact locked file: {}",
            self.metadata_path.as_str()
        );
        Ok(owner)
    }
    /// Verify the semantic facts this wire format can represent. This does not prove content bytes.
    pub fn matches_selection_and_requirements(
        &self,
        selected: Option<&ResolvedPin>,
        requirements: &Requirements,
    ) -> Result<bool> {
        let selection_matches = match (&self.provider, selected) {
            (Some(actual), Some(expected)) => {
                actual.project == expected.project
                    && actual.selection.as_ref() == Some(&expected.selection)
            }
            (None, None) => true,
            _ => false,
        };
        let expected = requirements.uniform()?;
        let choice_matches = match (&self.optional, expected.choice) {
            (Some(actual), Some(expected)) => {
                actual.default_enabled == expected.default_enabled
                    && actual.description.as_deref().unwrap_or("")
                        == expected.description.as_deref().unwrap_or("")
            }
            (None, None) => true,
            _ => false,
        };
        Ok(selection_matches && self.environments == expected.environments && choice_matches)
    }
}
/// Packwiz resolves filenames against the metadata parent; `.index` is not magic.
/// Relative subpaths are supported only while the normalized result stays inside the pack root.
fn destination(metadata: &PortableRelPath, filename: &str) -> Result<InstallDestination> {
    ensure!(
        !filename.is_empty() && !filename.contains('\\') && !filename.starts_with('/'),
        "Backend filename is not a portable relative file"
    );
    ensure!(
        !matches!(filename.rsplit('/').next(), Some("" | "." | "..")),
        "Backend filename must identify a file"
    );
    let mut components: Vec<_> = metadata.as_str().split('/').collect();
    components.pop();
    for component in filename.split('/') {
        match component {
            "" => anyhow::bail!("Backend filename has an empty component"),
            "." => {}
            ".." => {
                ensure!(
                    components.pop().is_some(),
                    "Backend filename escapes pack root"
                );
            }
            value => {
                PortableRelPath::parse(value, empack_core::path::PathSyntax::ArtifactName)?;
                components.push(value);
            }
        }
    }
    Ok(InstallDestination::parse(&components.join("/"))?)
}
fn optional_text<'a>(table: &'a toml::Value, name: &str) -> Result<Option<&'a str>> {
    table
        .get(name)
        .map(|value| {
            value
                .as_str()
                .with_context(|| format!("Backend field {name} must be text"))
        })
        .transpose()
}
fn required_text<'a>(table: &'a toml::Value, name: &str) -> Result<&'a str> {
    optional_text(table, name)?
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Missing backend field {name}"))
}
fn optional_bool(table: &toml::Value, name: &str) -> Result<Option<bool>> {
    table
        .get(name)
        .map(|value| {
            value
                .as_bool()
                .with_context(|| format!("Backend field {name} must be boolean"))
        })
        .transpose()
}

#[cfg(test)]
mod tests;
