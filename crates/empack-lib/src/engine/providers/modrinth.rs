use super::*;
use anyhow::Context;
use empack_core::{
    digest::DigestSet,
    identity::{ModrinthProjectId, ModrinthVersionId},
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Project {
    id: String,
    slug: String,
    title: String,
    project_type: String,
    #[serde(default)]
    loaders: Vec<String>,
    client_side: Option<String>,
    server_side: Option<String>,
}
pub(super) fn project(bytes: &[u8]) -> Result<CanonicalProject> {
    let value: Project = json(bytes)?;
    let kinds = match value.project_type.as_str() {
        "mod" => mod_kinds(&value.loaders)?,
        "resourcepack" => NonEmpty::new(vec![ContentKind::ResourcePack])?,
        "shader" => NonEmpty::new(vec![ContentKind::ShaderPack])?,
        "datapack" => NonEmpty::new(vec![ContentKind::DataPack])?,
        _ => return Err(CatalogError::UnsupportedKind.into()),
    };
    ensure!(
        !value.title.trim().is_empty(),
        record_error("title", RecordInvariant::NonEmpty)
    );
    ensure!(
        !value.slug.is_empty(),
        record_error("slug", RecordInvariant::NonEmpty)
    );
    Ok(CanonicalProject {
        id: ProviderProjectId::Modrinth(
            ModrinthProjectId::parse(&value.id)
                .map_err(|_| record_error("id", RecordInvariant::CanonicalIdentity))?,
        ),
        slug: value.slug,
        title: value.title,
        kinds,
        environment: EnvironmentEvidence {
            version: None,
            client: value.client_side,
            server: value.server_side,
        },
    })
}
pub(super) fn mod_kinds(loaders: &[String]) -> Result<NonEmpty<ContentKind>> {
    let datapack = loaders.iter().any(|value| value == "datapack");
    let mut kinds = Vec::new();
    if !datapack || loaders.iter().any(|value| value != "datapack") {
        kinds.push(ContentKind::Mod);
    }
    if datapack {
        kinds.push(ContentKind::DataPack);
    }
    Ok(NonEmpty::new(kinds)?)
}
#[derive(Deserialize)]
struct Version {
    id: String,
    project_id: String,
    files: Vec<File>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    environment: Option<String>,
    dependencies: Option<Vec<Dependency>>,
}
#[derive(Deserialize)]
struct File {
    filename: String,
    primary: bool,
    hashes: BTreeMap<String, String>,
    url: String,
    size: u64,
    file_type: Option<String>,
}
#[derive(Deserialize)]
struct Dependency {
    project_id: Option<String>,
    version_id: Option<String>,
    file_name: Option<String>,
    dependency_type: String,
}
pub(super) fn selection(
    project: CanonicalProject,
    pin: &ResolvedPin,
    bytes: &[u8],
) -> Result<ProviderResolution> {
    decode_selection(project, pin, bytes).with_context(|| selection_context(pin))
}
fn decode_selection(
    project: CanonicalProject,
    pin: &ResolvedPin,
    bytes: &[u8],
) -> Result<ProviderResolution> {
    let version = checked_version(pin, bytes)?;
    let files = version_files(version.files)?;
    let mut coverage = if version.dependencies.is_some() {
        Coverage::CompleteForSelection
    } else {
        Coverage::Unknown
    };
    let mut dependencies = Vec::new();
    for dep in version.dependencies.unwrap_or_default() {
        let relation = match dep.dependency_type.as_str() {
            "required" => DependencyRelation::Required,
            "optional" => DependencyRelation::Optional,
            "incompatible" => DependencyRelation::Incompatible,
            "embedded" => DependencyRelation::Embedded,
            _ => {
                return Err(record_error(
                    "dependencies[].dependency_type",
                    RecordInvariant::RecognizedValue,
                ));
            }
        };
        let project = dep
            .project_id
            .map(|id| {
                ModrinthProjectId::parse(&id)
                    .map(ProviderProjectId::Modrinth)
                    .map_err(|_| {
                        record_error(
                            "dependencies[].project_id",
                            RecordInvariant::CanonicalIdentity,
                        )
                    })
            })
            .transpose()?;
        let pin = dep
            .version_id
            .map(|id| {
                ModrinthVersionId::parse(&id)
                    .map(PinSelector::ModrinthVersion)
                    .map_err(|_| {
                        record_error(
                            "dependencies[].version_id",
                            RecordInvariant::CanonicalIdentity,
                        )
                    })
            })
            .transpose()?;
        if project.is_none() && pin.is_none() {
            coverage = Coverage::Partial;
        }
        // A version-only edge still requires its ownership to be resolved before graph closure.
        if let Some(name) = &dep.file_name {
            filename(name).map_err(|_| {
                record_error(
                    "dependencies[].file_name",
                    RecordInvariant::PortableFilename,
                )
            })?;
        }
        dependencies.push(ProviderDependency {
            project,
            pin,
            filename: dep.file_name,
            relation,
        });
    }
    let mut environment = project.environment.clone();
    environment.version = version.environment;
    let kinds = version_kinds(&project, &version.loaders)?;
    Ok(ProviderResolution {
        project,
        kinds,
        pin: pin.clone(),
        files,
        game_versions: version.game_versions,
        loaders: version.loaders,
        environment,
        dependencies,
        coverage,
    })
}

pub(super) fn version_kinds(
    project: &CanonicalProject,
    loaders: &[String],
) -> Result<NonEmpty<ContentKind>> {
    if !loaders.is_empty()
        && project
            .kinds
            .as_slice()
            .iter()
            .any(|kind| matches!(kind, ContentKind::Mod | ContentKind::DataPack))
    {
        mod_kinds(loaders)
    } else {
        Ok(project.kinds.clone())
    }
}
fn checked_version(pin: &ResolvedPin, bytes: &[u8]) -> Result<Version> {
    let version: Version = json(bytes)?;
    let project = ModrinthProjectId::parse(&version.project_id)
        .map_err(|_| record_error("project_id", RecordInvariant::CanonicalIdentity))?;
    let selection = ModrinthVersionId::parse(&version.id)
        .map_err(|_| record_error("id", RecordInvariant::CanonicalIdentity))?;
    ensure!(
        ProviderProjectId::Modrinth(project) == pin.project,
        identity_error("project_id")
    );
    ensure!(
        PinSelector::ModrinthVersion(selection) == pin.selection,
        identity_error("id")
    );
    Ok(version)
}
fn version_files(input: Vec<File>) -> Result<NonEmpty<ProviderFile>> {
    ensure!(
        !input.is_empty(),
        record_error("files", RecordInvariant::NonEmpty)
    );
    ensure!(
        input.iter().filter(|file| file.primary).count() <= 1,
        record_error("files[].primary", RecordInvariant::AtMostOnePrimary)
    );
    let mut names = std::collections::BTreeSet::new();
    let mut files = Vec::new();
    for (index, file) in input.into_iter().enumerate() {
        filename(&file.filename).map_err(|_| {
            record_error(
                format!("files[{index}].filename"),
                RecordInvariant::PortableFilename,
            )
        })?;
        ensure!(
            names.insert(file.filename.clone()),
            record_error(
                format!("files[{index}].filename"),
                RecordInvariant::UniqueFilename
            )
        );
        download_locator(&file.url).map_err(|_| {
            record_error(
                format!("files[{index}].url"),
                RecordInvariant::DownloadLocator,
            )
        })?;
        let expected = DigestSet::parse(
            file.hashes
                .iter()
                .map(|(algorithm, value)| (algorithm.as_str(), value.as_str())),
        )
        .map_err(|_| record_error(format!("files[{index}].hashes"), RecordInvariant::DigestSet))?;
        files.push(ProviderFile {
            filename: file.filename,
            primary: file.primary,
            role: file.file_type,
            expected: ExpectedContent {
                digests: Some(expected),
                size: Some(file.size),
                accepted_observation: None,
            },
            alternatives: vec![file.url],
        });
    }
    Ok(NonEmpty::new(files)?)
}
/// Select the archive role before validating payload assertions belonging to that role.
/// Unselected notes/signatures are not dependencies of the chosen archive.
pub(super) fn archive_file(
    pin: &ResolvedPin,
    value: &serde_json::Value,
) -> Result<Option<ProviderFile>> {
    decode_archive_file(pin, value).with_context(|| selection_context(pin))
}
fn decode_archive_file(
    pin: &ResolvedPin,
    value: &serde_json::Value,
) -> Result<Option<ProviderFile>> {
    let files = value
        .get("files")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| record_error("files", RecordInvariant::JsonType))?;
    ensure!(
        !files.is_empty(),
        record_error("files", RecordInvariant::NonEmpty)
    );
    let mut primary = None;
    for (index, file) in files.iter().enumerate() {
        let selected = file
            .get("primary")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| {
                record_error(format!("files[{index}].primary"), RecordInvariant::JsonType)
            })?;
        if selected {
            ensure!(
                primary.replace(index).is_none(),
                record_error("files[].primary", RecordInvariant::AtMostOnePrimary)
            );
        }
    }
    let selected = &files[primary.unwrap_or(0)];
    let name = selected
        .get("filename")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            record_error(
                format!("files[{}].filename", primary.unwrap_or(0)),
                RecordInvariant::JsonType,
            )
        })?;
    filename(name).map_err(|_| {
        record_error(
            format!("files[{}].filename", primary.unwrap_or(0)),
            RecordInvariant::PortableFilename,
        )
    })?;
    if !name.to_ascii_lowercase().ends_with(".mrpack") {
        return Ok(None);
    }
    let mut selected_version = value.clone();
    selected_version["files"] = serde_json::json!([selected]);
    let version = checked_version(pin, &serde_json::to_vec(&selected_version)?)?;
    Ok(Some(version_files(version.files)?.into_vec().remove(0)))
}
