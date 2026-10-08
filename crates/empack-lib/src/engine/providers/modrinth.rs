use super::*;
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
        !value.title.trim().is_empty() && !value.slug.is_empty(),
        CatalogError::InvalidRecord
    );
    Ok(CanonicalProject {
        id: ProviderProjectId::Modrinth(ModrinthProjectId::parse(&value.id)?),
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
fn mod_kinds(loaders: &[String]) -> Result<NonEmpty<ContentKind>> {
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
            _ => return Err(CatalogError::InvalidRecord.into()),
        };
        let project = dep
            .project_id
            .map(|id| ModrinthProjectId::parse(&id).map(ProviderProjectId::Modrinth))
            .transpose()?;
        let pin = dep
            .version_id
            .map(|id| ModrinthVersionId::parse(&id).map(PinSelector::ModrinthVersion))
            .transpose()?;
        if project.is_none() && pin.is_none() {
            coverage = Coverage::Partial;
        }
        // A version-only edge still requires its ownership to be resolved before graph closure.
        if let Some(name) = &dep.file_name {
            filename(name)?;
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
    let kinds = if project
        .kinds
        .as_slice()
        .iter()
        .any(|kind| matches!(kind, ContentKind::Mod | ContentKind::DataPack))
        && !version.loaders.is_empty()
    {
        mod_kinds(&version.loaders)?
    } else {
        project.kinds.clone()
    };
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

fn checked_version(pin: &ResolvedPin, bytes: &[u8]) -> Result<Version> {
    let version: Version = json(bytes)?;
    ensure!(
        ProviderProjectId::Modrinth(ModrinthProjectId::parse(&version.project_id)?) == pin.project
            && PinSelector::ModrinthVersion(ModrinthVersionId::parse(&version.id)?)
                == pin.selection,
        CatalogError::Identity
    );
    Ok(version)
}
fn version_files(input: Vec<File>) -> Result<NonEmpty<ProviderFile>> {
    ensure!(
        input.iter().filter(|file| file.primary).count() <= 1,
        CatalogError::InvalidRecord
    );
    let mut names = std::collections::BTreeSet::new();
    let mut files = Vec::new();
    for file in input {
        filename(&file.filename)?;
        ensure!(
            names.insert(file.filename.clone()),
            CatalogError::InvalidRecord
        );
        download_locator(&file.url)?;
        let expected = DigestSet::parse(
            file.hashes
                .iter()
                .map(|(algorithm, value)| (algorithm.as_str(), value.as_str())),
        )
        .map_err(|_| CatalogError::InvalidRecord)?;
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
    let files = value
        .get("files")
        .and_then(serde_json::Value::as_array)
        .ok_or(CatalogError::InvalidRecord)?;
    ensure!(!files.is_empty(), CatalogError::InvalidRecord);
    let mut primary = None;
    for (index, file) in files.iter().enumerate() {
        let selected = file
            .get("primary")
            .and_then(serde_json::Value::as_bool)
            .ok_or(CatalogError::InvalidRecord)?;
        if selected {
            ensure!(
                primary.replace(index).is_none(),
                CatalogError::InvalidRecord
            );
        }
    }
    let selected = &files[primary.unwrap_or(0)];
    let name = selected
        .get("filename")
        .and_then(serde_json::Value::as_str)
        .ok_or(CatalogError::InvalidRecord)?;
    filename(name)?;
    if !name.to_ascii_lowercase().ends_with(".mrpack") {
        return Ok(None);
    }
    let mut selected_version = value.clone();
    selected_version["files"] = serde_json::json!([selected]);
    let version = checked_version(pin, &serde_json::to_vec(&selected_version)?)?;
    Ok(Some(version_files(version.files)?.into_vec().remove(0)))
}
