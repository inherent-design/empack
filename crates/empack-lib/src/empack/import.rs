use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;
use thiserror::Error;

use crate::application::session::Session;
use crate::empack::config::{DependencyEntry, DependencyRecord, DependencyStatus};
use crate::empack::content::{OverrideCategory, OverrideSide, SideEnv, SideRequirement};
use crate::empack::parsing::ModLoader;
use crate::primitives::ProjectPlatform;
use crate::Result;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Parsed intermediate representation of a modpack manifest.
///
/// Produced by platform-specific parsers (CurseForge zip, Modrinth mrpack).
/// Consumed by the resolver and executor.
#[derive(Debug)]
pub struct ModpackManifest {
    pub identity: PackIdentity,
    pub target: RuntimeTarget,
    pub content: Vec<ContentEntry>,
    pub overrides: Vec<OverrideEntry>,
    pub source_platform: ProjectPlatform,
    pub archive_path: PathBuf,
}

#[derive(Debug)]
pub struct PackIdentity {
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug)]
pub struct RuntimeTarget {
    pub minecraft_version: String,
    pub loader: ModLoader,
    pub loader_version: String,
}

#[derive(Debug)]
pub enum ContentEntry {
    PlatformReferenced(PlatformRef),
    EmbeddedJar(EmbeddedJar),
}

#[derive(Debug)]
pub struct PlatformRef {
    pub destination_path: String,
    pub platform: ProjectPlatform,
    pub project_id: String,
    pub file_id: Option<String>,
    pub hashes: HashMap<String, String>,
    pub download_urls: Vec<String>,
    pub env: SideEnv,
    pub required: bool,
    pub resolved_name: Option<String>,
    pub resolved_type: Option<crate::primitives::ProjectType>,
}

#[derive(Debug)]
pub struct EmbeddedJar {
    pub source_path: String,
    pub destination_path: String,
    pub hashes: HashMap<String, String>,
    pub file_size: u64,
    pub env: SideEnv,
}

#[derive(Debug)]
pub struct OverrideEntry {
    pub source_path: String,
    pub destination_path: String,
    pub side: OverrideSide,
    pub category: OverrideCategory,
}

/// A manifest after API resolution.
#[derive(Debug)]
pub struct ResolvedManifest {
    pub manifest: ModpackManifest,
    pub warnings: Vec<String>,
}

/// Configuration for the import executor.
#[derive(Debug)]
pub struct ImportConfig {
    pub target_dir: PathBuf,
    pub pack_name: String,
    pub author: String,
    pub version: String,
}

/// Result of executing an import.
#[derive(Debug)]
pub struct ImportResult {
    pub project_dir: PathBuf,
    pub stats: ImportStats,
}

#[derive(Debug)]
pub struct ImportStats {
    pub platform_referenced: usize,
    pub embedded_jars_identified: usize,
    pub embedded_jars_unidentified: usize,
    pub overrides_copied: usize,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("failed to read modpack archive: {0}")]
    ArchiveRead(String),

    #[error("manifest.json not found in CurseForge archive")]
    CurseForgeManifestMissing,

    #[error("modrinth.index.json not found in mrpack archive")]
    ModrinthManifestMissing,

    #[error("failed to parse manifest: {0}")]
    ParseFailed(String),

    #[error("missing required field: {field}")]
    MissingField { field: String },

    #[error("unknown mod loader: {0}")]
    UnknownLoader(String),

    #[error("source is already an empack project (empack.yml exists)")]
    AlreadyEmpackProject,

    #[error("cannot detect source type for: {0}")]
    UnrecognizedSource(String),

    #[error("remote CurseForge modpack download is not yet supported; download the .zip manually and pass the local path")]
    RemoteCurseForgeNotSupported,

    #[error("failed to download modpack: {0}")]
    DownloadFailed(String),
}

// ---------------------------------------------------------------------------
// CurseForge manifest JSON shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(default)]
    files: Vec<CfFile>,
    #[serde(rename = "manifestType")]
    manifest_type: String,
    #[serde(default)]
    overrides: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    author: Option<String>,
}

#[derive(Deserialize)]
struct CfMinecraft {
    #[serde(default, rename = "modLoaders")]
    mod_loaders: Vec<CfModLoader>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
struct CfModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct CfFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default)]
    required: bool,
}

// ---------------------------------------------------------------------------
// Modrinth manifest JSON shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct MrManifest {
    #[serde(default)]
    dependencies: HashMap<String, serde_json::Value>,
    #[serde(default)]
    files: Vec<MrFile>,
    #[serde(default)]
    overrides: Option<String>,
    #[serde(default, rename = "client-overrides")]
    client_overrides: Option<String>,
    #[serde(default, rename = "server-overrides")]
    server_overrides: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Deserialize)]
struct MrFile {
    path: String,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: MrEnv,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Deserialize, Default)]
struct MrEnv {
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    server: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse a CurseForge modpack archive (zip containing `manifest.json`).
pub fn parse_curseforge_zip(archive_path: &Path) -> Result<ModpackManifest> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ImportError::ArchiveRead(e.to_string()))?;

    let manifest_entry = archive
        .by_name("manifest.json")
        .map_err(|_| ImportError::CurseForgeManifestMissing)?;

    let manifest_content = read_zip_entry_to_string(manifest_entry)?;
    let cf: CfManifest =
        serde_json::from_str(&manifest_content).map_err(|e| ImportError::ParseFailed(e.to_string()))?;

    if cf.manifest_type != "minecraftModpack" {
        return Err(ImportError::ParseFailed(format!(
            "expected manifestType 'minecraftModpack', got '{}'",
            cf.manifest_type
        ))
        .into());
    }

    let mc_version = cf
        .minecraft
        .version
        .ok_or_else(|| ImportError::MissingField {
            field: "minecraft.version".to_string(),
        })?
        .to_string();

    let (loader, loader_version) = parse_cf_loader(&cf.minecraft.mod_loaders)?;

    let name = cf
        .name
        .unwrap_or_else(|| archive_path.file_stem().and_then(|s| s.to_str()).unwrap_or("Pack").to_string());
    let version = cf.version.unwrap_or_else(|| "1.0.0".to_string());

    let overrides_dir = if cf.overrides.is_empty() {
        "overrides".to_string()
    } else {
        cf.overrides
    };

    let mut override_entries = Vec::new();
    collect_override_entries(&mut archive, &overrides_dir, OverrideSide::Both, &mut override_entries)?;

    let content: Vec<ContentEntry> = cf
        .files
        .into_iter()
        .map(|f| {
            ContentEntry::PlatformReferenced(PlatformRef {
                destination_path: format!("mods/{}.jar", f.project_id),
                platform: ProjectPlatform::CurseForge,
                project_id: f.project_id.to_string(),
                file_id: Some(f.file_id.to_string()),
                hashes: HashMap::new(),
                download_urls: Vec::new(),
                env: SideEnv {
                    client: SideRequirement::Required,
                    server: SideRequirement::Required,
                },
                required: f.required,
                resolved_name: None,
                resolved_type: None,
            })
        })
        .collect();

    Ok(ModpackManifest {
        identity: PackIdentity {
            name,
            version,
            author: cf.author,
            summary: None,
        },
        target: RuntimeTarget {
            minecraft_version: mc_version,
            loader,
            loader_version,
        },
        content,
        overrides: override_entries,
        source_platform: ProjectPlatform::CurseForge,
        archive_path: archive_path.to_path_buf(),
    })
}

/// Parse a Modrinth modpack archive (mrpack containing `modrinth.index.json`).
pub fn parse_modrinth_mrpack(file_path: &Path) -> Result<ModpackManifest> {
    let file = std::fs::File::open(file_path)
        .with_context(|| format!("opening mrpack: {}", file_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ImportError::ArchiveRead(e.to_string()))?;

    let manifest_entry = archive
        .by_name("modrinth.index.json")
        .map_err(|_| ImportError::ModrinthManifestMissing)?;

    let manifest_content = read_zip_entry_to_string(manifest_entry)?;
    let mr: MrManifest =
        serde_json::from_str(&manifest_content).map_err(|e| ImportError::ParseFailed(e.to_string()))?;

    let mc_version = mr
        .dependencies
        .get("minecraft")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ImportError::MissingField {
            field: "dependencies.minecraft".to_string(),
        })?
        .to_string();

    let loader_id = mr
        .dependencies
        .keys()
        .find(|k| {
            let k = k.as_str();
            k == "forge"
                || k == "neoforge"
                || k == "fabric-loader"
                || k == "quilt-loader"
        })
        .ok_or_else(|| ImportError::MissingField {
            field: "dependencies.<loader>".to_string(),
        })?;

    let loader_version = mr
        .dependencies
        .get(loader_id)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let loader = ModLoader::parse_from_platform_id(loader_id)
        .map_err(|_| ImportError::UnknownLoader(loader_id.clone()))?;

    let name = mr
        .name
        .unwrap_or_else(|| file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("Pack").to_string());
    let version = mr.version_id.unwrap_or_else(|| "1.0.0".to_string());

    let mut override_entries = Vec::new();

    // Modrinth mrpack spec uses hardcoded archive directory names for overrides;
    // the JSON index does not contain these keys, so fall back to the convention.
    let overrides_dir = mr.overrides.as_deref().unwrap_or("overrides");
    collect_override_entries(&mut archive, overrides_dir, OverrideSide::Both, &mut override_entries)?;

    let client_dir = mr.client_overrides.as_deref().unwrap_or("client-overrides");
    collect_override_entries(&mut archive, client_dir, OverrideSide::ClientOnly, &mut override_entries)?;

    let server_dir = mr.server_overrides.as_deref().unwrap_or("server-overrides");
    collect_override_entries(&mut archive, server_dir, OverrideSide::ServerOnly, &mut override_entries)?;

    let content: Vec<ContentEntry> = mr
        .files
        .into_iter()
        .map(|f| {
            let env = SideEnv {
                client: mr_side_requirement(f.env.client.as_deref()),
                server: mr_side_requirement(f.env.server.as_deref()),
            };
            if !f.downloads.is_empty() {
                ContentEntry::PlatformReferenced(PlatformRef {
                    destination_path: f.path.clone(),
                    platform: ProjectPlatform::Modrinth,
                    project_id: String::new(),
                    file_id: None,
                    hashes: f.hashes,
                    download_urls: f.downloads,
                    env,
                    required: true,
                    resolved_name: None,
                    resolved_type: None,
                })
            } else {
                ContentEntry::EmbeddedJar(EmbeddedJar {
                    source_path: f.path.clone(),
                    destination_path: f.path.clone(),
                    hashes: f.hashes,
                    file_size: f.file_size.unwrap_or(0),
                    env,
                })
            }
        })
        .collect();

    Ok(ModpackManifest {
        identity: PackIdentity {
            name,
            version,
            author: None,
            summary: mr.summary,
        },
        target: RuntimeTarget {
            minecraft_version: mc_version,
            loader,
            loader_version,
        },
        content,
        overrides: override_entries,
        source_platform: ProjectPlatform::Modrinth,
        archive_path: file_path.to_path_buf(),
    })
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// Enrich a raw manifest via platform APIs to resolve names, types, and
/// identify embedded JARs.
pub async fn resolve_manifest(
    manifest: ModpackManifest,
    modrinth_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api_key: Option<&str>,
) -> Result<ResolvedManifest> {
    let mut warnings = Vec::new();
    let mut resolved_content = Vec::new();

    for entry in manifest.content.into_iter() {
        match entry {
            ContentEntry::PlatformReferenced(mut pref) => {
                if pref.platform == ProjectPlatform::Modrinth && pref.project_id.is_empty() && pref.download_urls.is_empty() {
                    warnings.push(format!(
                        "Modrinth file '{}' has no project ID and no download URL; \
                         skipping platform resolution",
                        pref.destination_path
                    ));
                }
                resolve_platform_ref(&mut pref, modrinth_api, curseforge_api, curseforge_api_key, &mut warnings).await;
                resolved_content.push(ContentEntry::PlatformReferenced(pref));
            }
            ContentEntry::EmbeddedJar(embed) => {
                warnings.push(format!(
                    "embedded JAR '{}' cannot be identified while inside archive; \
                     resolve after extraction",
                    embed.source_path
                ));
                resolved_content.push(ContentEntry::EmbeddedJar(embed));
            }
        }
    }

    Ok(ResolvedManifest {
        manifest: ModpackManifest {
            content: resolved_content,
            ..manifest
        },
        warnings,
    })
}

async fn resolve_platform_ref(
    pref: &mut PlatformRef,
    modrinth_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api_key: Option<&str>,
    warnings: &mut Vec<String>,
) {
    if pref.resolved_name.is_some() {
        return;
    }

    match pref.platform {
        ProjectPlatform::Modrinth => {
            if pref.project_id.is_empty() {
                // Modrinth files without project IDs are likely pre-modrinth or
                // embedded content; nothing to resolve.
                return;
            }
            resolve_modrinth_project(pref, modrinth_api, warnings).await;
        }
        ProjectPlatform::CurseForge => {
            resolve_curseforge_project(pref, curseforge_api, curseforge_api_key, warnings).await;
        }
    }
}

#[derive(Deserialize)]
struct MrProjectResponse {
    title: String,
    #[serde(default)]
    project_type: Option<String>,
}

async fn resolve_modrinth_project(
    pref: &mut PlatformRef,
    api: &dyn crate::application::session::NetworkProvider,
    warnings: &mut Vec<String>,
) {
    let client = match api.http_client() {
        Ok(c) => c,
        Err(_) => return,
    };

    let url = format!(
        "https://api.modrinth.com/v2/project/{}",
        pref.project_id
    );

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            warnings.push(format!("Modrinth API lookup failed for '{}': {}", pref.project_id, e));
            return;
        }
    };

    if !response.status().is_success() {
        warnings.push(format!(
            "Modrinth API returned {} for project '{}'",
            response.status(),
            pref.project_id
        ));
        return;
    }

    let body: MrProjectResponse = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("failed to parse Modrinth project response: {}", e));
            return;
        }
    };

    pref.resolved_name = Some(body.title.clone());

    pref.resolved_type = body.project_type.as_deref().map(|pt| match pt {
        "mod" => crate::primitives::ProjectType::Mod,
        "resourcepack" => crate::primitives::ProjectType::ResourcePack,
        "shader" => crate::primitives::ProjectType::Shader,
        "datapack" => crate::primitives::ProjectType::Datapack,
        _ => crate::primitives::ProjectType::Mod,
    });
}

#[derive(Deserialize)]
struct CfDataEnvelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct CfModResponse {
    name: String,
    #[serde(rename = "classId", default)]
    class_id: Option<u32>,
}

async fn resolve_curseforge_project(
    pref: &mut PlatformRef,
    api: &dyn crate::application::session::NetworkProvider,
    curseforge_api_key: Option<&str>,
    warnings: &mut Vec<String>,
) {
    let api_key = match curseforge_api_key {
        Some(k) => k,
        None => {
            warnings.push(format!(
                "CurseForge API key missing; cannot resolve mod '{}'",
                pref.project_id
            ));
            return;
        }
    };

    let client = match api.http_client() {
        Ok(c) => c,
        Err(_) => return,
    };

    let url = format!(
        "https://api.curseforge.com/v1/mods/{}",
        pref.project_id
    );

    let response = match client
        .get(&url)
        .header("x-api-key", api_key)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warnings.push(format!("CurseForge API lookup failed for '{}': {}", pref.project_id, e));
            return;
        }
    };

    if !response.status().is_success() {
        warnings.push(format!(
            "CurseForge API returned {} for mod '{}'",
            response.status(),
            pref.project_id
        ));
        return;
    }

    let envelope: CfDataEnvelope<CfModResponse> = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("failed to parse CurseForge mod response: {}", e));
            return;
        }
    };
    let body = envelope.data;

    pref.resolved_name = Some(body.name.clone());

    pref.resolved_type = body.class_id.map(|cid| match cid {
        6 => crate::primitives::ProjectType::Mod,
        5 => crate::primitives::ProjectType::Mod, // Bukkit Plugins
        12 => crate::primitives::ProjectType::ResourcePack,
        17 => crate::primitives::ProjectType::Datapack, // Worlds (no World variant; closest match)
        6945 => crate::primitives::ProjectType::Datapack,
        6552 => crate::primitives::ProjectType::Shader,
        _ => crate::primitives::ProjectType::Mod,
    });
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Transform a resolved manifest into an empack project on disk.
pub async fn execute_import(
    resolved: ResolvedManifest,
    config: ImportConfig,
    session: &dyn Session,
) -> Result<ImportResult> {
    let mut stats = ImportStats {
        platform_referenced: 0,
        embedded_jars_identified: 0,
        embedded_jars_unidentified: 0,
        overrides_copied: 0,
        warnings: resolved.warnings.clone(),
    };

    session.filesystem().create_dir_all(&config.target_dir)?;

    let init_config = crate::primitives::InitializationConfig {
        name: &config.pack_name,
        author: &config.author,
        version: &config.version,
        modloader: resolved.manifest.target.loader.as_str(),
        mc_version: &resolved.manifest.target.minecraft_version,
        loader_version: &resolved.manifest.target.loader_version,
    };

    let empack_yml_content = format_empack_yml(
        &config.pack_name,
        &config.author,
        &config.version,
        &resolved.manifest.target.minecraft_version,
        resolved.manifest.target.loader.as_str(),
        &resolved.manifest.target.loader_version,
    );

    session
        .filesystem()
        .write_file(&config.target_dir.join("empack.yml"), &empack_yml_content)?;

    let manager = crate::empack::state::PackStateManager::new(
        config.target_dir.clone(),
        session.filesystem(),
    );

    let transition_result = manager
        .execute_transition(
            session.process(),
            &*session.packwiz(),
            crate::primitives::StateTransition::Initialize(init_config),
        )
        .await
        .context("failed to initialize modpack project during import")?;

    for w in &transition_result.warnings {
        session.display().status().warning(w);
    }

    let pack_dir = config.target_dir.join("pack");
    let config_manager = session.filesystem().config_manager(config.target_dir.clone());

    // Process content entries
    for entry in &resolved.manifest.content {
        match entry {
            ContentEntry::PlatformReferenced(pref) => {
                let added = add_platform_ref(pref, &pack_dir, session).await?;
                if added {
                    stats.platform_referenced += 1;
                    if let Some(name) = &pref.resolved_name {
                        let record = DependencyRecord {
                            status: DependencyStatus::Resolved,
                            title: name.clone(),
                            platform: pref.platform,
                            project_id: pref.project_id.clone(),
                            project_type: pref.resolved_type.unwrap_or(crate::primitives::ProjectType::Mod),
                            version: pref.file_id.clone(),
                        };
                        let dep_key = name.to_lowercase().replace(' ', "-");
                        if let Err(e) = config_manager.add_dependency(&dep_key, record) {
                            session.display().status().warning(&format!("failed to update empack.yml: {}", e));
                        }
                    }
                }
            }
            ContentEntry::EmbeddedJar(embed) => {
                let dest = sanitize_archive_path(&pack_dir, &embed.destination_path)?;
                extract_embedded_from_archive(
                    &resolved.manifest.archive_path,
                    &embed.source_path,
                    &dest,
                    session.filesystem(),
                )?;
                stats.embedded_jars_unidentified += 1;
            }
        }
    }

    // Copy override files from archive
    for override_entry in &resolved.manifest.overrides {
        let dest = sanitize_archive_path(&pack_dir, &override_entry.destination_path)?;
        extract_embedded_from_archive(
            &resolved.manifest.archive_path,
            &override_entry.source_path,
            &dest,
            session.filesystem(),
        )?;
        stats.overrides_copied += 1;
    }

    Ok(ImportResult {
        project_dir: config.target_dir,
        stats,
    })
}

async fn add_platform_ref(
    pref: &PlatformRef,
    pack_dir: &Path,
    session: &dyn Session,
) -> Result<bool> {
    match pref.platform {
        ProjectPlatform::Modrinth => {
            if pref.download_urls.is_empty() {
                session.display().status().warning(&format!(
                    "no download URL for '{}'; skipping",
                    pref.destination_path
                ));
                return Ok(false);
            }

            let args = [
                "modrinth".to_string(),
                "add".to_string(),
                "--url".to_string(),
                pref.download_urls[0].clone(),
                "-y".to_string(),
            ];

            let output = session.process().execute(
                "packwiz",
                &args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                pack_dir,
            )?;

            if output.success {
                return Ok(true);
            }

            session.display().status().warning(&format!(
                "packwiz modrinth add failed for '{}': {}",
                pref.destination_path, output.stderr
            ));
            Ok(false)
        }
        ProjectPlatform::CurseForge => {
            let mod_id = &pref.project_id;
            let file_id = pref
                .file_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("CurseForge ref missing file_id"))?;

            let args = [
                "curseforge".to_string(),
                "add".to_string(),
                "--mod-id".to_string(),
                mod_id.clone(),
                "--file-id".to_string(),
                file_id.to_string(),
                "-y".to_string(),
            ];

            let output = session.process().execute(
                "packwiz",
                &args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                pack_dir,
            )?;

            if output.success {
                return Ok(true);
            }

            session.display().status().warning(&format!(
                "packwiz curseforge add failed for '{}': {}",
                pref.destination_path, output.stderr
            ));
            Ok(false)
        }
    }
}

/// Validate that a relative path from an archive does not escape the target directory.
fn sanitize_archive_path(base: &Path, relative: &str) -> Result<PathBuf> {
    let joined = base.join(relative);
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let canonical_dest = joined.canonicalize().unwrap_or_else(|_| {
        // File doesn't exist yet; normalize by resolving .. components manually
        let mut components = Vec::new();
        for c in joined.components() {
            match c {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                _ => components.push(c),
            }
        }
        components.iter().collect()
    });
    if !canonical_dest.starts_with(&canonical_base) {
        anyhow::bail!(
            "path traversal detected: '{}' escapes target directory",
            relative
        );
    }
    Ok(joined)
}

fn extract_embedded_from_archive(
    archive_path: &Path,
    source_path: &str,
    dest_path: &Path,
    fs: &dyn crate::application::session::FileSystemProvider,
) -> Result<()> {
    if let Some(parent) = dest_path.parent() {
        fs.create_dir_all(parent)?;
    }

    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ImportError::ArchiveRead(e.to_string()))?;

    let mut entry = archive
        .by_name(source_path)
        .with_context(|| format!("entry '{}' not found in archive", source_path))?;

    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    fs.write_bytes(dest_path, &bytes)?;

    Ok(())
}

fn format_empack_yml(
    name: &str,
    author: &str,
    version: &str,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
) -> String {
    use std::collections::BTreeMap;

    let loader_enum = ModLoader::parse(loader).ok();

    #[derive(serde::Serialize)]
    struct Yml<'a> {
        empack: Fields<'a>,
    }

    #[derive(serde::Serialize)]
    struct Fields<'a> {
        name: &'a str,
        author: &'a str,
        version: &'a str,
        minecraft_version: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        loader: Option<ModLoader>,
        #[serde(skip_serializing_if = "str::is_empty")]
        loader_version: &'a str,
        dependencies: BTreeMap<String, DependencyEntry>,
    }

    let config = Yml {
        empack: Fields {
            name,
            author,
            version,
            minecraft_version,
            loader: loader_enum,
            loader_version,
            dependencies: BTreeMap::new(),
        },
    };

    serde_saphyr::to_string(&config).expect("serializing import config should never fail")
}

// ---------------------------------------------------------------------------
// Override classification
// ---------------------------------------------------------------------------

/// Classify an override file path into an [`OverrideCategory`].
pub fn classify_override(path: &str) -> OverrideCategory {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_lowercase();

    if lower.starts_with("config/") || lower.starts_with("defaultconfigs/") {
        return OverrideCategory::Config;
    }
    if lower.starts_with("kubejs/") || lower.starts_with("scripts/") {
        return OverrideCategory::Script;
    }
    if lower.starts_with("resourcepacks/") {
        return OverrideCategory::ResourcePack;
    }
    if lower.starts_with("shaderpacks/") {
        return OverrideCategory::ShaderPack;
    }
    if lower.starts_with("datapacks/") || lower.starts_with("data/") {
        return OverrideCategory::DataPack;
    }
    if lower.starts_with("world/") || lower.starts_with("dim-") {
        return OverrideCategory::World;
    }
    if lower == "server.properties"
        || lower.starts_with("server-config/")
    {
        return OverrideCategory::ServerConfig;
    }
    if lower == "options.txt"
        || lower.ends_with("/options.txt")
        || lower == "optionsof.txt"
        || lower.ends_with("/optionsof.txt")
    {
        return OverrideCategory::ClientConfig;
    }
    if lower.starts_with("mods/") {
        return OverrideCategory::ModData;
    }

    OverrideCategory::Other
}

fn collect_override_entries(
    archive: &mut zip::ZipArchive<std::fs::File>,
    prefix: &str,
    side: OverrideSide,
    entries: &mut Vec<OverrideEntry>,
) -> Result<()> {
    let prefix_trimmed = prefix.trim_end_matches('/');
    let prefix_with_slash = format!("{}/", prefix_trimmed);

    let mut i = 0;
    while i < archive.len() {
        let name = match archive.by_index(i) {
            Ok(entry) => entry.name().to_string(),
            Err(_) => {
                i += 1;
                continue;
            }
        };

        if name.starts_with(&prefix_with_slash) && !name.ends_with('/') {
            let relative = &name[prefix_with_slash.len()..];
            if !relative.is_empty() {
                entries.push(OverrideEntry {
                    source_path: name.clone(),
                    destination_path: relative.to_string(),
                    side: side.clone(),
                    category: classify_override(relative),
                });
            }
        }

        i += 1;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_cf_loader(
    loaders: &[CfModLoader],
) -> Result<(ModLoader, String)> {
    let primary = loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| loaders.first());

    let loader = primary.ok_or_else(|| ImportError::MissingField {
        field: "minecraft.modLoaders".to_string(),
    })?;

    let (loader_type, loader_version) = loader
        .id
        .split_once('-')
        .ok_or_else(|| ImportError::ParseFailed(format!("invalid loader ID: {}", loader.id)))?;

    let mod_loader = ModLoader::parse(loader_type)
        .map_err(|_| ImportError::UnknownLoader(loader_type.to_string()))?;

    Ok((mod_loader, loader_version.to_string()))
}

fn read_zip_entry_to_string<R: std::io::Read>(
    mut entry: zip::read::ZipFile<'_, R>,
) -> Result<String> {
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut entry, &mut buf)
        .map_err(|e| ImportError::ArchiveRead(e.to_string()))?;
    Ok(buf)
}

fn mr_side_requirement(value: Option<&str>) -> SideRequirement {
    match value {
        Some("required") => SideRequirement::Required,
        Some("optional") => SideRequirement::Optional,
        Some("unsupported") => SideRequirement::Unsupported,
        _ => SideRequirement::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Source detection
// ---------------------------------------------------------------------------

/// Detect the type of a local source (file or directory).
pub fn detect_local_source(path: &Path) -> Result<SourceKind> {
    if !path.exists() {
        return Err(ImportError::UnrecognizedSource(path.display().to_string()).into());
    }

    if path.is_file() {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        return match ext.as_str() {
            "mrpack" => Ok(SourceKind::ModrinthMrpack),
            "zip" => Ok(SourceKind::CurseForgeZip),
            _ => Err(ImportError::UnrecognizedSource(path.display().to_string()).into()),
        };
    }

    if path.is_dir() {
        let empack_yml = path.join("empack.yml");
        if empack_yml.exists() {
            return Err(ImportError::AlreadyEmpackProject.into());
        }

        let pack_toml = path.join("pack.toml");
        if pack_toml.exists() {
            return Ok(SourceKind::PackwizDirectory);
        }
    }

    Err(ImportError::UnrecognizedSource(path.display().to_string()).into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    CurseForgeZip,
    ModrinthMrpack,
    PackwizDirectory,
    ModrinthRemote { slug: String, version: Option<String> },
    CurseForgeRemote { slug: String },
}

#[cfg(test)]
mod tests {
    include!("import.test.rs");
}
