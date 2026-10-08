use super::*;
use empack_core::{
    digest::DigestSet,
    identity::{CurseForgeProjectId, ProviderProjectId},
};
use serde::{
    Deserialize,
    de::{MapAccess, Visitor},
};
use std::fmt;

type Roots = Vec<(PortableRelPath, ContentLayer)>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mrpack {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    summary: Option<String>,
    files: Vec<MrFile>,
    #[serde(deserialize_with = "unique_map")]
    dependencies: BTreeMap<String, String>,
    // Historical producer extensions remain explicit; standard archives use the fixed defaults.
    overrides: Option<String>,
    #[serde(rename = "client-overrides")]
    client_overrides: Option<String>,
    #[serde(rename = "server-overrides")]
    server_overrides: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrFile {
    path: String,
    #[serde(deserialize_with = "unique_map")]
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    env: MrEnvironment,
    downloads: Vec<String>,
    file_size: u64,
}
#[derive(Deserialize, Default)]
struct MrEnvironment {
    client: Option<String>,
    server: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForge {
    manifest_type: String,
    manifest_version: Option<u32>,
    minecraft: CfRuntime,
    files: Vec<CfFile>,
    overrides: Option<String>,
    name: Option<String>,
    version: Option<String>,
    author: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfRuntime {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfLoader>,
}
#[derive(Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}
#[derive(Deserialize)]
struct CfFile {
    #[serde(rename = "projectID")]
    project: u64,
    #[serde(rename = "fileID")]
    file: u64,
    required: bool,
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| {
        anyhow::anyhow!(
            "Invalid manifest JSON at line {}, column {}",
            e.line(),
            e.column()
        )
    })
}
fn unique_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Unique;
    impl<'de> Visitor<'de> for Unique {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object with unique string fields")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if result.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate field"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Unique)
}
pub(super) fn parse(
    format: ImportFormat,
    bytes: &[u8],
    source: AcquiredContent,
    maximum: usize,
    cancel: &Cancellation,
) -> Result<(ImportedProject, Roots)> {
    match format {
        ImportFormat::Modrinth => mrpack(bytes, source, maximum, cancel),
        ImportFormat::CurseForge => curseforge(bytes, source, maximum, cancel),
    }
}
fn blank(
    format: ImportFormat,
    metadata: ImportedMetadata,
    runtime: ImportedRuntime,
    source: AcquiredContent,
) -> ImportedProject {
    ImportedProject {
        format,
        metadata,
        runtime,
        providers: Vec::new(),
        files: Vec::new(),
        overrides: Vec::new(),
        dependency_coverage: Coverage::Unknown,
        diagnostics: Vec::new(),
        auxiliary_members: Vec::new(),
        source,
    }
}
fn text(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && !value.chars().any(char::is_control),
        "Blank or control-containing metadata field"
    );
    Ok(())
}
fn root(value: Option<String>, default: &str) -> Result<PortableRelPath> {
    let value = value
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.into());
    path(&value)
}
fn loader(value: &str) -> Result<LoaderKind> {
    Ok(match value {
        "fabric" | "fabric-loader" => LoaderKind::Fabric,
        "quilt" | "quilt-loader" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => anyhow::bail!("Unsupported runtime dependency"),
    })
}
fn mrpack(
    bytes: &[u8],
    source: AcquiredContent,
    maximum: usize,
    cancel: &Cancellation,
) -> Result<(ImportedProject, Roots)> {
    let value: Mrpack = decode(bytes)?;
    ensure!(
        value.format_version == 1 && value.game == "minecraft",
        "Unsupported mrpack format version or game"
    );
    ensure!(value.files.len() <= maximum, "Import exceeds record limit");
    text(&value.name).context("/name")?;
    text(&value.version_id).context("/versionId")?;
    let minecraft = GameVersion::parse(
        value
            .dependencies
            .get("minecraft")
            .context("Missing /dependencies/minecraft")?,
    )?;
    let mut loaders = Vec::new();
    for (name, version) in &value.dependencies {
        if name == "minecraft" {
            continue;
        }
        let kind = loader(name).with_context(|| format!("/dependencies/{name}"))?;
        ensure!(
            !loaders
                .iter()
                .any(|entry: &ImportedLoader| entry.kind == kind),
            "Duplicate loader family in import"
        );
        loaders.push(ImportedLoader {
            kind,
            version: LoaderVersion::parse(version)?,
            primary: false,
        });
    }
    let roots = vec![
        (
            root(value.overrides, "overrides")?,
            ContentLayer::CommonOverride,
        ),
        (
            root(value.client_overrides, "client-overrides")?,
            ContentLayer::Client,
        ),
        (
            root(value.server_overrides, "server-overrides")?,
            ContentLayer::Server,
        ),
    ];
    validate_roots(&roots)?;
    let mut result = blank(
        ImportFormat::Modrinth,
        ImportedMetadata {
            name: Some(value.name),
            version: Some(value.version_id),
            author: None,
            summary: value.summary,
        },
        ImportedRuntime { minecraft, loaders },
        source,
    );
    for (index, file) in value.files.into_iter().enumerate() {
        cancel.check()?;
        let origin = location("modrinth.index.json", Some(format!("/files/{index}")))?;
        let parsed = (|| -> Result<ImportedFile> {
            let destination = InstallDestination::parse(&file.path)?;
            let requirements = ImportedRequirements {
                client: requirement(file.env.client.as_deref())?,
                server: requirement(file.env.server.as_deref())?,
            };
            ensure!(
                requirements.client != ImportedRequirement::Unsupported
                    || requirements.server != ImportedRequirement::Unsupported,
                "File is unsupported in every environment"
            );
            // Preserve every supported assertion. Compatibility archives may provide fewer than
            // the spec's SHA-1 + SHA-512 pair; that limitation is recorded rather than hidden.
            let digests =
                DigestSet::parse(file.hashes.iter().map(|(a, h)| (a.as_str(), h.as_str())))?;
            let acquisition = if file.downloads.is_empty() {
                ImportedAcquisition::Embedded(path(&file.path)?)
            } else {
                for (download_index, url) in file.downloads.iter().enumerate() {
                    ensure!(
                        !url.chars().any(|c| c.is_whitespace() || c.is_control()),
                        "Invalid download locator"
                    );
                    let parsed = reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!("Invalid download locator"))?;
                    ensure!(matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
                        && parsed.username().is_empty() && parsed.password().is_none() && parsed.fragment().is_none(),
                        "Invalid download locator");
                    if super::super::documents::validate_download_url(url).is_err() {
                        result.diagnostics.push(ImportDiagnostic {
                            location: location("modrinth.index.json", Some(format!("/files/{index}/downloads/{download_index}")))?,
                            message: if parsed.scheme() == "http" {
                                "Source declares HTTP; preparation must resolve transport policy before acquisition or persistence"
                            } else {
                                "Source locator is transient; retain it only for acquisition and resolve durable provenance separately"
                            }.into(),
                        });
                    }
                }
                ImportedAcquisition::Downloads(file.downloads)
            };
            Ok(ImportedFile {
                destination,
                layer: ContentLayer::Common,
                requirements,
                expected: ExpectedContent {
                    digests: Some(digests),
                    size: Some(file.file_size),
                    accepted_observation: None,
                },
                acquisition,
                permissions: None,
                source: origin.clone(),
            })
        })()
        .with_context(|| format!("modrinth.index.json /files/{index}"))?;
        if !(file.hashes.contains_key("sha1") && file.hashes.contains_key("sha512")) {
            result.diagnostics.push(ImportDiagnostic { location: origin, message: "Source omits the mrpack SHA-1/SHA-512 pair; retain its declared integrity level".into() });
        }
        result.files.push(parsed);
    }
    Ok((result, roots))
}
fn requirement(value: Option<&str>) -> Result<ImportedRequirement> {
    Ok(match value.unwrap_or("required") {
        "required" => ImportedRequirement::Required,
        "optional" => ImportedRequirement::Optional,
        "unsupported" => ImportedRequirement::Unsupported,
        _ => anyhow::bail!("Unknown environment requirement"),
    })
}
fn curseforge(
    bytes: &[u8],
    source: AcquiredContent,
    maximum: usize,
    cancel: &Cancellation,
) -> Result<(ImportedProject, Roots)> {
    let mut value: CurseForge = decode(bytes)?;
    ensure!(
        value.manifest_type == "minecraftModpack" && value.manifest_version.is_none_or(|v| v == 1),
        "Unsupported CurseForge manifest type or version"
    );
    ensure!(value.files.len() <= maximum, "Import exceeds record limit");
    let mut diagnostics = Vec::new();
    for (name, field) in [
        ("name", &mut value.name),
        ("version", &mut value.version),
        ("author", &mut value.author),
    ] {
        if let Some(value) = field {
            ensure!(
                !value.chars().any(char::is_control),
                "Control-containing metadata field /{name}"
            );
            if value.trim().is_empty() {
                *field = None;
                diagnostics.push(ImportDiagnostic {
                    location: location("manifest.json", Some(format!("/{name}")))?,
                    message: format!(
                        "Blank optional {name} has no value; supply a project value during import"
                    ),
                });
            }
        }
    }
    let mut loaders = Vec::new();
    for (index, entry) in value.minecraft.mod_loaders.into_iter().enumerate() {
        let (name, version) = entry
            .id
            .split_once('-')
            .context("Invalid /minecraft/modLoaders id")?;
        let kind = loader(name).with_context(|| format!("/minecraft/modLoaders/{index}"))?;
        ensure!(
            !loaders.iter().any(|v: &ImportedLoader| v.kind == kind),
            "Duplicate loader family in import"
        );
        loaders.push(ImportedLoader {
            kind,
            version: LoaderVersion::parse(version)?,
            primary: entry.primary,
        });
    }
    ensure!(
        loaders.iter().filter(|v| v.primary).count() <= 1,
        "Import declares competing primary loaders"
    );
    let runtime = ImportedRuntime {
        minecraft: GameVersion::parse(&value.minecraft.version)?,
        loaders,
    };
    let roots = vec![(
        root(value.overrides, "overrides")?,
        ContentLayer::CommonOverride,
    )];
    let mut result = blank(
        ImportFormat::CurseForge,
        ImportedMetadata {
            name: value.name,
            version: value.version,
            author: value.author,
            summary: None,
        },
        runtime,
        source,
    );
    result.diagnostics.extend(diagnostics);
    let mut projects = BTreeSet::new();
    for (index, file) in value.files.into_iter().enumerate() {
        cancel.check()?;
        let project =
            ProviderProjectId::CurseForge(CurseForgeProjectId::parse(&file.project.to_string())?);
        ensure!(
            projects.insert(project.clone()),
            "CurseForge manifest repeats a project identity"
        );
        let selection = ResolvedPin {
            selection: project.parse_pin(&file.file.to_string())?,
            project,
        };
        let requirement = if file.required {
            ImportedRequirement::Required
        } else {
            ImportedRequirement::Optional
        };
        result.providers.push(ImportedProvider {
            selection,
            requirements: ImportedRequirements {
                client: requirement,
                server: requirement,
            },
            source: location("manifest.json", Some(format!("/files/{index}")))?,
        });
    }
    Ok((result, roots))
}
fn validate_roots(roots: &Roots) -> Result<()> {
    for (index, (a, _)) in roots.iter().enumerate() {
        for (b, _) in roots.iter().skip(index + 1) {
            ensure!(
                a != b
                    && !a.as_str().starts_with(&format!("{}/", b.as_str()))
                    && !b.as_str().starts_with(&format!("{}/", a.as_str())),
                "Import override roots overlap"
            );
        }
    }
    Ok(())
}
