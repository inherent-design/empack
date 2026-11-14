use super::*;
use anyhow::Context;
use empack_core::{
    digest::DigestSet,
    identity::{CurseForgeFileId, CurseForgeProjectId},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: u64,
    game_id: u64,
    slug: String,
    name: String,
    class_id: Option<u64>,
}
pub(super) fn project(bytes: &[u8], search: bool) -> Result<CanonicalProject> {
    let value = if search {
        let mut response: Envelope<Vec<Project>> = json(bytes)?;
        ensure!(!response.data.is_empty(), CatalogError::NotFound);
        ensure!(response.data.len() == 1, CatalogError::Ambiguous);
        response.data.remove(0)
    } else {
        json::<Envelope<Project>>(bytes)?.data
    };
    ensure!(
        value.game_id == 432,
        record_error("data.gameId", RecordInvariant::CanonicalIdentity)
    );
    ensure!(
        !value.name.trim().is_empty(),
        record_error("data.name", RecordInvariant::NonEmpty)
    );
    ensure!(
        !value.slug.is_empty(),
        record_error("data.slug", RecordInvariant::NonEmpty)
    );
    let kind = match value.class_id {
        Some(6) => ContentKind::Mod,
        Some(12) => ContentKind::ResourcePack,
        Some(17) => ContentKind::World,
        Some(6552) => ContentKind::ShaderPack,
        Some(6945) => ContentKind::DataPack,
        _ => return Err(CatalogError::UnsupportedKind.into()),
    };
    Ok(CanonicalProject {
        id: ProviderProjectId::CurseForge(
            CurseForgeProjectId::parse(&value.id.to_string())
                .map_err(|_| record_error("data.id", RecordInvariant::CanonicalIdentity))?,
        ),
        slug: value.slug,
        title: value.name,
        kinds: NonEmpty::new(vec![kind])?,
        environment: EnvironmentEvidence {
            version: None,
            client: None,
            server: None,
        },
    })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    id: u64,
    game_id: u64,
    mod_id: u64,
    file_name: String,
    file_length: u64,
    download_url: Option<String>,
    hashes: Vec<Hash>,
    game_versions: Vec<String>,
    dependencies: Option<Vec<Dependency>>,
}
#[derive(Deserialize)]
struct Hash {
    value: String,
    algo: u8,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dependency {
    mod_id: u64,
    relation_type: u8,
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
    let file = checked_file(pin, bytes)?;
    let selected = provider_file(&file)?;
    let coverage = if file.dependencies.is_some() {
        Coverage::CompleteForSelection
    } else {
        Coverage::Unknown
    };
    let mut dependencies = Vec::new();
    for dep in file.dependencies.unwrap_or_default() {
        let relation = match dep.relation_type {
            1 => DependencyRelation::Embedded,
            2 => DependencyRelation::Optional,
            3 => DependencyRelation::Required,
            4 => DependencyRelation::Tool,
            5 => DependencyRelation::Incompatible,
            6 => DependencyRelation::Include,
            _ => {
                return Err(record_error(
                    "data.dependencies[].relationType",
                    RecordInvariant::RecognizedValue,
                ));
            }
        };
        dependencies.push(ProviderDependency {
            project: Some(ProviderProjectId::CurseForge(
                CurseForgeProjectId::parse(&dep.mod_id.to_string()).map_err(|_| {
                    record_error(
                        "data.dependencies[].modId",
                        RecordInvariant::CanonicalIdentity,
                    )
                })?,
            )),
            pin: None,
            filename: None,
            relation,
        });
    }
    // CurseForge mixes game and loader labels in gameVersions. Preserve all source labels,
    // and expose known loader labels separately without inventing compatibility for unknowns.
    let loaders = loader_labels(&file.game_versions);
    let environment = project.environment.clone();
    Ok(ProviderResolution {
        kinds: project.kinds.clone(),
        project,
        pin: pin.clone(),
        files: NonEmpty::new(vec![selected])?,
        game_versions: file.game_versions,
        loaders,
        environment,
        dependencies,
        coverage,
    })
}

/// CurseForge mixes game versions and loader labels; one interpretation serves
/// metadata filtering and full selected-file normalization.
pub(super) fn loader_labels(versions: &[String]) -> Vec<String> {
    versions
        .iter()
        .filter(|value| {
            matches!(
                value.as_str(),
                "Forge" | "Fabric" | "Quilt" | "NeoForge" | "Cauldron" | "LiteLoader"
            )
        })
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn expected_content(assertions: &[Hash], size: u64) -> Result<ExpectedContent> {
    let mut hashes = Vec::new();
    for hash in assertions {
        let algorithm = match hash.algo {
            1 => "sha1",
            2 => "md5",
            _ => {
                return Err(record_error(
                    "data.hashes[].algo",
                    RecordInvariant::RecognizedValue,
                ));
            }
        };
        hashes.push((algorithm, hash.value.as_str()));
    }
    let digests = DigestSet::parse(hashes)
        .map_err(|_| record_error("data.hashes", RecordInvariant::DigestSet))?;
    Ok(ExpectedContent {
        digests: Some(digests),
        size: Some(size),
        accepted_observation: None,
    })
}
/// Fingerprints nominate files across all content classes. Reject byte mismatches
/// before resolving a nominee's project or requiring supported content semantics.
pub(super) fn matches_content(bytes: &[u8], size: u64, observed: &DigestSet) -> Result<bool> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Assertions {
        file_length: u64,
        hashes: Vec<Hash>,
    }
    let file: Assertions = json::<Envelope<Assertions>>(bytes)?.data;
    let expected = expected_content(&file.hashes, file.file_length)?;
    Ok(expected.size == Some(size)
        && expected
            .digests
            .as_ref()
            .is_some_and(|hashes| hashes.check(observed.values()).is_ok()))
}

fn checked_file(pin: &ResolvedPin, bytes: &[u8]) -> Result<File> {
    let file: File = json::<Envelope<File>>(bytes)?.data;
    let owner = CurseForgeProjectId::parse(&file.mod_id.to_string())
        .map_err(|_| record_error("data.modId", RecordInvariant::CanonicalIdentity))?;
    let selection = CurseForgeFileId::parse(&file.id.to_string())
        .map_err(|_| record_error("data.id", RecordInvariant::CanonicalIdentity))?;
    ensure!(file.game_id == 432, identity_error("data.gameId"));
    ensure!(
        ProviderProjectId::CurseForge(owner) == pin.project,
        identity_error("data.modId")
    );
    ensure!(
        PinSelector::CurseForgeFile(selection) == pin.selection,
        identity_error("data.id")
    );
    Ok(file)
}
fn provider_file(file: &File) -> Result<ProviderFile> {
    filename(&file.file_name)
        .map_err(|_| record_error("data.fileName", RecordInvariant::PortableFilename))?;
    let expected = expected_content(&file.hashes, file.file_length)?;
    let mut alternatives = Vec::new();
    if let Some(url) = &file.download_url {
        download_locator(url)
            .map_err(|_| record_error("data.downloadUrl", RecordInvariant::DownloadLocator))?;
        alternatives.push(url.clone());
    }
    Ok(ProviderFile {
        filename: file.file_name.clone(),
        primary: true,
        role: None,
        expected,
        alternatives,
    })
}
pub(super) fn archive_file(pin: &ResolvedPin, bytes: &[u8]) -> Result<ProviderFile> {
    (|| provider_file(&checked_file(pin, bytes)?))().with_context(|| selection_context(pin))
}
