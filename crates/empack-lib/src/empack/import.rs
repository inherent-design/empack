use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use serde::Deserialize;
use thiserror::Error;

use crate::Result;
use crate::application::session::{Session, execute_process_with_live_issues};
use crate::empack::config::{DependencyRecord, DependencyStatus};
use crate::empack::content::{OverrideCategory, OverrideSide, SideEnv, SideRequirement};
use crate::empack::parsing::ModLoader;
use crate::networking::rate_budget::RateBudget;
use crate::primitives::ProjectPlatform;
use tracing::instrument;

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

#[derive(Debug, Clone)]
pub enum ContentEntry {
    PlatformReferenced(PlatformRef),
    EmbeddedJar(EmbeddedJar),
}

#[derive(Debug, Clone)]
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
    /// Platform slug used by packwiz for .pw.toml filename (e.g., "jei" not "Just Enough Items").
    pub resolved_slug: Option<String>,
    pub resolved_type: Option<crate::primitives::ProjectType>,
    /// CurseForge classId for content-type routing (e.g. 6945 for Data Packs).
    pub cf_class_id: Option<u32>,
}

#[derive(Debug, Clone)]
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
    /// Folder for datapacks relative to pack root (written to empack.yml and pack.toml).
    pub datapack_folder: Option<String>,
    /// Additional accepted MC versions (written to empack.yml and pack.toml).
    pub acceptable_game_versions: Option<Vec<String>>,
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
    pub platform_failed: usize,
    pub platform_skipped: usize,
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
    #[serde(default, rename = "versionId")]
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
    #[serde(default, rename = "fileSize")]
    file_size: Option<u64>,
    #[serde(default, rename = "projectId")]
    project_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct MrEnv {
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    server: Option<String>,
}

/// Extract a Modrinth project ID from a CDN download URL.
///
/// CDN URLs follow `https://cdn.modrinth.com/data/{project_id}/versions/{version_id}/filename`.
/// Returns `None` if the URL does not match the expected structure.
fn extract_modrinth_project_id(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    let data_pos = parts.iter().position(|&s| s == "data")?;
    let pid = parts.get(data_pos + 1)?;
    if pid.is_empty() {
        return None;
    }
    Some(pid.to_string())
}

/// Extract a CurseForge file ID from a ForgeCD CDN download URL.
///
/// CDN URLs follow `https://edge.forgecdn.net/files/{part1}/{part2}/filename`.
/// The file ID is the concatenation of `part1` and `part2` (e.g. `3193` + `541` = `3193541`).
/// Returns `None` if the URL does not match the expected structure.
fn extract_forgecdn_file_id(url: &str) -> Option<String> {
    if !url.contains("forgecdn.net") && !url.contains("curseforge.com") {
        return None;
    }
    let parts: Vec<&str> = url.split('/').collect();
    let files_pos = parts.iter().position(|&s| s == "files")?;
    let p1 = parts.get(files_pos + 1)?;
    let p2 = parts.get(files_pos + 2)?;
    if p1.is_empty() || p2.is_empty() {
        return None;
    }
    if p1.chars().all(|c| c.is_ascii_digit()) && p2.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("{}{}", p1, p2))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse a CurseForge modpack archive (zip containing `manifest.json`).
pub fn parse_curseforge_zip(archive_path: &Path) -> Result<ModpackManifest> {
    let bytes = std::fs::read(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    parse_curseforge_zip_bytes(bytes, archive_path)
}

pub fn parse_curseforge_zip_with_filesystem(
    fs: &dyn crate::application::session::FileSystemProvider,
    archive_path: &Path,
) -> Result<ModpackManifest> {
    let bytes = fs
        .read_bytes(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    parse_curseforge_zip_bytes(bytes, archive_path)
}

fn parse_curseforge_zip_bytes(
    archive_bytes: Vec<u8>,
    archive_path: &Path,
) -> Result<ModpackManifest> {
    let mut archive = open_zip_archive(archive_bytes)?;
    let manifest_entry = archive
        .by_name("manifest.json")
        .map_err(|_| ImportError::CurseForgeManifestMissing)?;

    let manifest_content = read_zip_entry_to_string(manifest_entry)?;
    let cf: CfManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| ImportError::ParseFailed(e.to_string()))?;

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

    let name = cf.name.unwrap_or_else(|| {
        archive_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Pack")
            .to_string()
    });
    let version = cf.version.unwrap_or_else(|| "1.0.0".to_string());

    let overrides_dir = if cf.overrides.is_empty() {
        "overrides".to_string()
    } else {
        cf.overrides
    };

    let mut override_entries = Vec::new();
    collect_override_entries(
        &mut archive,
        &overrides_dir,
        OverrideSide::Both,
        &mut override_entries,
    )?;
    prune_packwiz_override_metadata(&mut archive, &mut override_entries);

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
                resolved_slug: None,
                resolved_type: None,
                cf_class_id: None,
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
    let bytes = std::fs::read(file_path)
        .with_context(|| format!("opening mrpack: {}", file_path.display()))?;
    parse_modrinth_mrpack_bytes(bytes, file_path)
}

pub fn parse_modrinth_mrpack_with_filesystem(
    fs: &dyn crate::application::session::FileSystemProvider,
    file_path: &Path,
) -> Result<ModpackManifest> {
    let bytes = fs
        .read_bytes(file_path)
        .with_context(|| format!("opening mrpack: {}", file_path.display()))?;
    parse_modrinth_mrpack_bytes(bytes, file_path)
}

fn parse_modrinth_mrpack_bytes(
    archive_bytes: Vec<u8>,
    file_path: &Path,
) -> Result<ModpackManifest> {
    let mut archive = open_zip_archive(archive_bytes)?;
    let manifest_entry = archive
        .by_name("modrinth.index.json")
        .map_err(|_| ImportError::ModrinthManifestMissing)?;

    let manifest_content = read_zip_entry_to_string(manifest_entry)?;
    let mr: MrManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| ImportError::ParseFailed(e.to_string()))?;

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
            k == "forge" || k == "neoforge" || k == "fabric-loader" || k == "quilt-loader"
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

    let name = mr.name.unwrap_or_else(|| {
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Pack")
            .to_string()
    });
    let version = mr.version_id.unwrap_or_else(|| "1.0.0".to_string());

    let mut override_entries = Vec::new();

    // Modrinth mrpack spec uses hardcoded archive directory names for overrides;
    // the JSON index does not contain these keys, so fall back to the convention.
    let overrides_dir = mr.overrides.as_deref().unwrap_or("overrides");
    collect_override_entries(
        &mut archive,
        overrides_dir,
        OverrideSide::Both,
        &mut override_entries,
    )?;

    let client_dir = mr.client_overrides.as_deref().unwrap_or("client-overrides");
    collect_override_entries(
        &mut archive,
        client_dir,
        OverrideSide::ClientOnly,
        &mut override_entries,
    )?;

    let server_dir = mr.server_overrides.as_deref().unwrap_or("server-overrides");
    collect_override_entries(
        &mut archive,
        server_dir,
        OverrideSide::ServerOnly,
        &mut override_entries,
    )?;
    prune_packwiz_override_metadata(&mut archive, &mut override_entries);

    let content: Vec<ContentEntry> = mr
        .files
        .into_iter()
        .map(|f| {
            let env = SideEnv {
                client: mr_side_requirement(f.env.client.as_deref()),
                server: mr_side_requirement(f.env.server.as_deref()),
            };
            if !f.downloads.is_empty() {
                let first_url = f.downloads.first().map(|s| s.as_str()).unwrap_or("");

                // Classify by download URL origin. CurseForge CDN URLs in a
                // Modrinth mrpack are reclassified so the CurseForge add path
                // handles them. Modrinth CDN and unknown URLs stay as Modrinth
                // (unknown URLs fall through to the packwiz url add path).
                let (platform, project_id, file_id) =
                    if let Some(cf_file_id) = extract_forgecdn_file_id(first_url) {
                        (ProjectPlatform::CurseForge, String::new(), Some(cf_file_id))
                    } else {
                        let pid = f
                            .project_id
                            .clone()
                            .filter(|s| !s.is_empty())
                            .or_else(|| extract_modrinth_project_id(first_url))
                            .unwrap_or_default();
                        (ProjectPlatform::Modrinth, pid, None)
                    };

                ContentEntry::PlatformReferenced(PlatformRef {
                    destination_path: f.path.clone(),
                    platform,
                    project_id,
                    file_id,
                    hashes: f.hashes,
                    download_urls: f.downloads,
                    env,
                    required: true,
                    resolved_name: None,
                    resolved_slug: None,
                    resolved_type: None,
                    cf_class_id: None,
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

/// Concurrency bound for the resolve phase. Adaptive budgets handle request
/// pacing; the semaphore now only caps task and connection pressure.
const RESOLVE_CONCURRENCY: usize = 10;

#[derive(Clone, Debug)]
struct ResolveApiBases {
    modrinth: String,
    curseforge: String,
}

impl ResolveApiBases {
    fn production() -> Self {
        Self {
            modrinth: "https://api.modrinth.com".to_string(),
            curseforge: "https://api.curseforge.com".to_string(),
        }
    }

    fn modrinth_url(&self, path: &str) -> String {
        format!("{}/{}", self.modrinth.trim_end_matches('/'), path)
    }

    fn curseforge_url(&self, path: &str) -> String {
        format!("{}/{}", self.curseforge.trim_end_matches('/'), path)
    }
}

#[derive(Clone)]
struct ResolveRateContext {
    api_bases: ResolveApiBases,
    modrinth_budget: Option<Arc<dyn RateBudget>>,
    curseforge_budget: Option<Arc<dyn RateBudget>>,
}

/// Enrich a raw manifest via platform APIs to resolve names, types, and
/// identify embedded JARs.
///
/// Platform references are resolved concurrently (bounded by
/// [`RESOLVE_CONCURRENCY`]) to reduce wall-clock time on large modpacks.
/// The output order matches the input order.
#[instrument(skip_all, fields(content_count = manifest.content.len()))]
pub async fn resolve_manifest(
    manifest: ModpackManifest,
    modrinth_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api_key: Option<&str>,
    display: &dyn crate::display::providers::DisplayProvider,
    rate_budgets: &crate::networking::rate_budget::HostBudgetRegistry,
) -> Result<ResolvedManifest> {
    resolve_manifest_with_api_bases(
        manifest,
        modrinth_api,
        curseforge_api,
        curseforge_api_key,
        display,
        rate_budgets,
        ResolveApiBases::production(),
    )
    .await
}

async fn resolve_manifest_with_api_bases(
    manifest: ModpackManifest,
    modrinth_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api: &dyn crate::application::session::NetworkProvider,
    curseforge_api_key: Option<&str>,
    display: &dyn crate::display::providers::DisplayProvider,
    rate_budgets: &crate::networking::rate_budget::HostBudgetRegistry,
    api_bases: ResolveApiBases,
) -> Result<ResolvedManifest> {
    let resolve_start = std::time::Instant::now();
    let mut warnings = Vec::new();
    let total = manifest.content.len();
    let progress = display.progress().bar(total as u64);
    progress.set_message("Resolving platform references");

    let mr_client = modrinth_api.http_client()?;
    let cf_client = curseforge_api.http_client()?;
    let cf_key: Option<String> = curseforge_api_key.map(|k| k.to_string());

    let cf_file_ids: Vec<u64> = manifest
        .content
        .iter()
        .filter_map(|e| match e {
            ContentEntry::PlatformReferenced(p)
                if p.platform == ProjectPlatform::CurseForge && p.project_id.is_empty() =>
            {
                p.file_id.as_ref()?.parse::<u64>().ok()
            }
            _ => None,
        })
        .collect();

    let rate_context = ResolveRateContext {
        api_bases: api_bases.clone(),
        modrinth_budget: rate_budgets.for_host("api.modrinth.com"),
        curseforge_budget: rate_budgets.for_host("api.curseforge.com"),
    };

    let cf_file_to_mod = if !cf_file_ids.is_empty() {
        resolve_curseforge_file_ids(
            &cf_file_ids,
            curseforge_api,
            curseforge_api_key,
            &mut warnings,
            &rate_context.api_bases,
            rate_context.curseforge_budget.as_ref(),
        )
        .await
    } else {
        std::collections::HashMap::new()
    };

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(RESOLVE_CONCURRENCY));
    type ResolveHandle = (
        usize,
        ContentEntry,
        tokio::task::JoinHandle<(ContentEntry, Vec<String>)>,
    );
    let mut handles: Vec<ResolveHandle> = Vec::new();
    let mut passthrough: Vec<(usize, ContentEntry, Vec<String>)> = Vec::new();

    for (i, entry) in manifest.content.into_iter().enumerate() {
        match entry {
            ContentEntry::PlatformReferenced(mut pref) => {
                if pref.platform == ProjectPlatform::CurseForge
                    && pref.project_id.is_empty()
                    && let Some(fid) = pref.file_id.as_ref().and_then(|s| s.parse::<u64>().ok())
                    && let Some(mod_id) = cf_file_to_mod.get(&fid)
                {
                    pref.project_id = mod_id.to_string();
                }

                let backup = ContentEntry::PlatformReferenced(pref.clone());
                let mr = mr_client.clone();
                let cf = cf_client.clone();
                let key = cf_key.clone();
                let sem = semaphore.clone();
                let task_context = rate_context.clone();

                handles.push((
                    i,
                    backup,
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.expect("semaphore closed");
                        let resolve_start = std::time::Instant::now();
                        let mut task_warnings = Vec::new();

                        if pref.platform == ProjectPlatform::Modrinth
                            && pref.project_id.is_empty()
                            && pref.download_urls.is_empty()
                        {
                            task_warnings.push(format!(
                                "Modrinth file '{}' has no project ID and no download URL; \
                             skipping platform resolution",
                                pref.destination_path
                            ));
                        } else {
                            resolve_platform_ref_with_client(
                                &mut pref,
                                &mr,
                                &cf,
                                &task_context,
                                key.as_deref(),
                                &mut task_warnings,
                            )
                            .await;
                        }

                        tracing::trace!(
                            slug = %pref.project_id,
                            platform = %pref.platform,
                            resolve_ms = resolve_start.elapsed().as_millis() as u64,
                            "ref resolved"
                        );

                        (ContentEntry::PlatformReferenced(pref), task_warnings)
                    }),
                ));
            }
            ContentEntry::EmbeddedJar(embed) => {
                let w = vec![format!(
                    "embedded JAR '{}' cannot be identified while inside archive; \
                     resolve after extraction",
                    embed.source_path
                )];
                passthrough.push((i, ContentEntry::EmbeddedJar(embed), w));
            }
        }
    }

    let mut results: Vec<Option<(ContentEntry, Vec<String>)>> = (0..total).map(|_| None).collect();

    for (i, entry, w) in passthrough {
        results[i] = Some((entry, w));
        progress.inc();
    }

    let mut pending: std::collections::HashMap<usize, ContentEntry> = handles
        .iter()
        .map(|(i, backup, _)| (*i, backup.clone()))
        .collect();
    let mut join_set = tokio::task::JoinSet::new();
    for (i, _backup, handle) in handles {
        join_set.spawn(async move { (i, handle.await) });
    }
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((i, Ok((entry, w)))) => {
                results[i] = Some((entry, w));
            }
            Ok((i, Err(e))) => {
                let msg = format!("resolve task panicked: {}", e);
                warnings.push(msg.clone());
                if let Some(backup) = pending.remove(&i) {
                    results[i] = Some((backup, vec![msg]));
                }
            }
            Err(e) => {
                warnings.push(format!("resolve join error: {}", e));
            }
        }
        progress.inc();
    }
    progress.finish("Platform references resolved");

    let missing_results = results.iter().filter(|entry| entry.is_none()).count();
    debug_assert_eq!(
        missing_results, 0,
        "resolve manifest should preserve one result slot per input content entry"
    );
    if missing_results > 0 {
        anyhow::bail!(
            "resolve manifest lost {} content entr{} during task join",
            missing_results,
            if missing_results == 1 { "y" } else { "ies" }
        );
    }

    let mut resolved_content = Vec::with_capacity(total);
    for (entry, w) in results.into_iter().flatten() {
        warnings.extend(w);
        resolved_content.push(entry);
    }

    let resolve_elapsed = resolve_start.elapsed();
    let resolved_count = resolved_content
        .iter()
        .filter(|e| matches!(e, ContentEntry::PlatformReferenced(_)))
        .count();
    tracing::info!(
        phase = "resolve_manifest",
        total_ms = resolve_elapsed.as_millis() as u64,
        count = resolved_count,
        concurrency = RESOLVE_CONCURRENCY,
        "resolve phase complete"
    );

    Ok(ResolvedManifest {
        manifest: ModpackManifest {
            content: resolved_content,
            ..manifest
        },
        warnings,
    })
}

/// Resolve a single platform reference using pre-extracted HTTP clients.
///
/// This variant accepts `reqwest::Client` directly so it can be called from
/// `tokio::spawn` tasks (which require `Send` futures).
async fn resolve_platform_ref_with_client(
    pref: &mut PlatformRef,
    modrinth_client: &reqwest::Client,
    curseforge_client: &reqwest::Client,
    rate_context: &ResolveRateContext,
    curseforge_api_key: Option<&str>,
    warnings: &mut Vec<String>,
) {
    #[cfg(feature = "test-utils")]
    if pref.project_id == "__panic__" {
        panic!("forced resolve task panic for test coverage");
    }

    if pref.resolved_name.is_some() {
        return;
    }

    match pref.platform {
        ProjectPlatform::Modrinth => {
            if pref.project_id.is_empty() {
                return;
            }
            resolve_modrinth_project_with_client(
                pref,
                modrinth_client,
                &rate_context.api_bases,
                warnings,
                rate_context.modrinth_budget.as_ref(),
            )
            .await;
        }
        ProjectPlatform::CurseForge => {
            if pref.project_id.is_empty() {
                return;
            }
            resolve_curseforge_project_with_client(
                pref,
                curseforge_client,
                &rate_context.api_bases,
                curseforge_api_key,
                warnings,
                rate_context.curseforge_budget.as_ref(),
            )
            .await;
        }
    }
}

async fn apply_rate_budget(budget: Option<&Arc<dyn RateBudget>>) {
    let Some(budget) = budget else {
        return;
    };

    let delay = budget.acquire();
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

fn record_rate_budget(budget: Option<&Arc<dyn RateBudget>>, response: &reqwest::Response) {
    if let Some(budget) = budget {
        budget.record_response(response.headers(), response.status());
    }
}

#[derive(Deserialize)]
struct MrProjectResponse {
    title: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    project_type: Option<String>,
}

#[derive(Deserialize)]
struct MrVersionFileResponse {
    id: String,
}

async fn resolve_modrinth_project_with_client(
    pref: &mut PlatformRef,
    client: &reqwest::Client,
    api_bases: &ResolveApiBases,
    warnings: &mut Vec<String>,
    budget: Option<&Arc<dyn RateBudget>>,
) {
    if pref.file_id.is_none()
        && let Some(sha1) = pref.hashes.get("sha1")
    {
        let url = api_bases.modrinth_url(&format!("v2/version_file/{}?algorithm=sha1", sha1));
        apply_rate_budget(budget).await;
        if let Ok(resp) = client.get(&url).send().await {
            record_rate_budget(budget, &resp);
            if resp.status().is_success()
                && let Ok(body) = resp.json::<MrVersionFileResponse>().await
            {
                pref.file_id = Some(body.id);
            }
        }
    }

    let url = api_bases.modrinth_url(&format!("v2/project/{}", pref.project_id));

    apply_rate_budget(budget).await;
    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            warnings.push(format!(
                "Modrinth API lookup failed for '{}': {}",
                pref.project_id, e
            ));
            return;
        }
    };
    record_rate_budget(budget, &response);

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
    pref.resolved_slug = body.slug.clone();

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
    #[serde(default)]
    slug: Option<String>,
    #[serde(rename = "classId", default)]
    class_id: Option<u32>,
}

async fn resolve_curseforge_project_with_client(
    pref: &mut PlatformRef,
    client: &reqwest::Client,
    api_bases: &ResolveApiBases,
    curseforge_api_key: Option<&str>,
    warnings: &mut Vec<String>,
    budget: Option<&Arc<dyn RateBudget>>,
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

    let url = api_bases.curseforge_url(&format!("v1/mods/{}", pref.project_id));

    apply_rate_budget(budget).await;
    let response = match client.get(&url).header("x-api-key", api_key).send().await {
        Ok(r) => r,
        Err(e) => {
            warnings.push(format!(
                "CurseForge API lookup failed for '{}': {}",
                pref.project_id, e
            ));
            return;
        }
    };
    record_rate_budget(budget, &response);

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
    pref.resolved_slug = body.slug.clone();
    pref.cf_class_id = body.class_id;

    pref.resolved_type = body.class_id.map(|cid| match cid {
        6 => crate::primitives::ProjectType::Mod,
        5 => crate::primitives::ProjectType::Mod,
        12 => crate::primitives::ProjectType::ResourcePack,
        17 => crate::primitives::ProjectType::Datapack,
        6945 => crate::primitives::ProjectType::Datapack,
        6552 => crate::primitives::ProjectType::Shader,
        _ => crate::primitives::ProjectType::Mod,
    });
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CfFileResponse {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "classId", default)]
    class_id: Option<u32>,
}

/// Batch-resolve CurseForge file IDs to mod IDs via `POST /v1/mods/files`.
///
/// Returns a map from file_id to mod_id for all successfully resolved entries.
async fn resolve_curseforge_file_ids(
    file_ids: &[u64],
    api: &dyn crate::application::session::NetworkProvider,
    api_key: Option<&str>,
    warnings: &mut Vec<String>,
    api_bases: &ResolveApiBases,
    budget: Option<&Arc<dyn RateBudget>>,
) -> std::collections::HashMap<u64, u64> {
    let mut result = std::collections::HashMap::new();

    let api_key = match api_key {
        Some(k) => k,
        None => {
            warnings.push(
                "CurseForge API key missing; cannot resolve file IDs from ForgeCD URLs".to_string(),
            );
            return result;
        }
    };

    let client = match api.http_client() {
        Ok(c) => c,
        Err(_) => return result,
    };

    // CF API accepts batches; process in chunks of 50.
    for chunk in file_ids.chunks(50) {
        let body = serde_json::json!({ "fileIds": chunk });
        apply_rate_budget(budget).await;
        let response = match client
            .post(api_bases.curseforge_url("v1/mods/files"))
            .header("x-api-key", api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("CurseForge batch file lookup failed: {}", e));
                continue;
            }
        };
        record_rate_budget(budget, &response);

        if !response.status().is_success() {
            warnings.push(format!(
                "CurseForge batch file lookup returned {}",
                response.status()
            ));
            continue;
        }

        if let Ok(envelope) = response.json::<CfDataEnvelope<Vec<CfFileResponse>>>().await {
            for f in envelope.data {
                result.insert(f.id, f.mod_id);
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Transform a resolved manifest into an empack project on disk.
#[instrument(skip_all, fields(content_count = resolved.manifest.content.len()))]
pub async fn execute_import(
    resolved: ResolvedManifest,
    config: ImportConfig,
    session: &dyn Session,
) -> Result<ImportResult> {
    let mut stats = ImportStats {
        platform_referenced: 0,
        platform_failed: 0,
        platform_skipped: 0,
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

    let datapack_folder = config
        .datapack_folder
        .clone()
        .or_else(|| detect_datapack_folder(&resolved.manifest));

    let empack_yml_content = format_empack_yml(
        &config.pack_name,
        &config.author,
        &config.version,
        &resolved.manifest.target.minecraft_version,
        resolved.manifest.target.loader.as_str(),
        &resolved.manifest.target.loader_version,
        datapack_folder.as_deref(),
        config.acceptable_game_versions.as_deref(),
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

    if datapack_folder.is_some() || config.acceptable_game_versions.is_some() {
        let pack_toml_path = config.target_dir.join("pack").join("pack.toml");
        crate::empack::packwiz::write_pack_toml_options(
            &pack_toml_path,
            datapack_folder.as_deref(),
            config.acceptable_game_versions.as_deref(),
            session.filesystem(),
        )
        .map_err(|e| anyhow::anyhow!("failed to write pack.toml options: {}", e))?;

        session
            .packwiz()
            .run_packwiz_refresh(&config.target_dir)
            .map_err(|e| anyhow::anyhow!("failed to refresh index after writing options: {}", e))?;
    }

    let pack_dir = config.target_dir.join("pack");
    let config_manager = session
        .filesystem()
        .config_manager(config.target_dir.clone());

    let mut content_dirs: Vec<&str> = vec!["mods", "resourcepacks", "shaderpacks", "datapacks"];
    if let Some(ref df) = datapack_folder
        && !content_dirs.contains(&df.as_str())
    {
        content_dirs.push(df);
    }

    // Build a set of override basenames so we can identify platform refs
    // that are already covered by override files (e.g. datapacks distributed
    // via Paxi that also appear in the mrpack files[] array).
    let override_basenames: std::collections::HashSet<String> = resolved
        .manifest
        .overrides
        .iter()
        .filter_map(|o| {
            std::path::Path::new(&o.destination_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    let content_total = resolved.manifest.content.len();
    let use_no_refresh = content_total > 1;
    let content_progress = session.display().progress().bar(content_total as u64);
    content_progress.set_message("Adding mods");

    let mut add_durations: Vec<std::time::Duration> = Vec::with_capacity(content_total);

    let scan_start = std::time::Instant::now();
    let pre_stems = scan_pw_toml_stems(&pack_dir, &content_dirs, session.filesystem());
    let pre_scan_ms = scan_start.elapsed().as_millis() as u64;

    struct PendingDep {
        derived_key: String,
        title: String,
        platform: ProjectPlatform,
        project_id: String,
        project_type: crate::primitives::ProjectType,
        version: Option<String>,
    }
    let mut pending_deps: Vec<PendingDep> = Vec::new();

    for entry in &resolved.manifest.content {
        match entry {
            ContentEntry::PlatformReferenced(pref) => {
                content_progress.tick(&pref.destination_path);

                let add_start = std::time::Instant::now();
                let result = add_platform_ref_with_retry(
                    pref,
                    &pack_dir,
                    session,
                    datapack_folder.as_deref(),
                    use_no_refresh,
                )
                .await?;
                add_durations.push(add_start.elapsed());

                match result {
                    AddRefResult::Skipped => {
                        stats.platform_skipped += 1;
                        session.display().status().warning(&format!(
                            "no project ID or download URL for '{}'; skipping",
                            pref.destination_path
                        ));
                    }
                    AddRefResult::Failed(detail) => {
                        let basename = std::path::Path::new(&pref.destination_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        if override_basenames.contains(basename) {
                            session.display().status().info(&format!(
                                "skipped packwiz add for '{}' (already in overrides)",
                                pref.destination_path
                            ));
                        } else {
                            stats.platform_failed += 1;
                            session.display().status().warning(&detail);
                        }
                    }
                    AddRefResult::Added => {
                        stats.platform_referenced += 1;

                        let derived_key = derive_dep_key(
                            pref.resolved_slug.as_deref(),
                            pref.resolved_name.as_deref(),
                            &pref.destination_path,
                        );
                        let title = pref
                            .resolved_name
                            .clone()
                            .unwrap_or_else(|| derived_key.clone());

                        pending_deps.push(PendingDep {
                            derived_key,
                            title,
                            platform: pref.platform,
                            project_id: pref.project_id.clone(),
                            project_type: pref
                                .resolved_type
                                .unwrap_or(crate::primitives::ProjectType::Mod),
                            version: pref.file_id.clone(),
                        });
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
        content_progress.inc();
    }
    content_progress.finish(&format!("{} platform references processed", content_total));

    if use_no_refresh {
        let pack_toml = pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in pack.toml path"))?;
        let refresh_output = execute_process_with_live_issues(
            session,
            session.packwiz_bin(),
            &["--pack-file", pack_toml_str, "refresh"],
            &pack_dir,
        )?;
        if !refresh_output.success {
            anyhow::bail!(
                "packwiz refresh failed after batch import: {}",
                refresh_output.error_output()
            );
        }
    }

    let scan_start = std::time::Instant::now();
    let post_stems = scan_pw_toml_stems(&pack_dir, &content_dirs, session.filesystem());
    let post_scan_ms = scan_start.elapsed().as_millis() as u64;

    let new_stems: std::collections::HashSet<_> =
        post_stems.difference(&pre_stems).cloned().collect();

    for dep in &pending_deps {
        let record = DependencyRecord {
            status: DependencyStatus::Resolved,
            title: dep.title.clone(),
            platform: dep.platform,
            project_id: dep.project_id.clone(),
            project_type: dep.project_type,
            version: dep.version.clone(),
        };
        if let Err(e) = config_manager.add_dependency(&dep.derived_key, record) {
            session
                .display()
                .status()
                .warning(&format!("failed to update empack.yml: {}", e));
        }
    }

    if !add_durations.is_empty() {
        let total: std::time::Duration = add_durations.iter().sum();
        let min = add_durations.iter().min().unwrap();
        let max = add_durations.iter().max().unwrap();
        tracing::info!(
            phase = "content_loop",
            total_ms = total.as_millis() as u64,
            count = add_durations.len(),
            min_ms = min.as_millis() as u64,
            max_ms = max.as_millis() as u64,
            avg_ms = (total.as_millis() as u64) / (add_durations.len() as u64),
            "content loop complete"
        );
    }

    tracing::info!(
        phase = "scan_pw_toml",
        pre_scan_ms,
        post_scan_ms,
        new_stems = new_stems.len(),
        pending_deps = pending_deps.len(),
        "batched scan_pw_toml_stems (2 scans total)"
    );

    for dep in &pending_deps {
        if !new_stems.contains(&dep.derived_key) {
            tracing::warn!(
                key = %dep.derived_key,
                title = %dep.title,
                "dep key does not match any .pw.toml stem; empack.yml may diverge from packwiz state"
            );
        }
    }
    if new_stems.len() != pending_deps.len() {
        tracing::warn!(
            expected = pending_deps.len(),
            actual = new_stems.len(),
            "pw.toml count mismatch"
        );
    }

    let override_total = resolved.manifest.overrides.len();
    let override_progress = session.display().progress().bar(override_total as u64);
    override_progress.set_message("Copying overrides");

    for override_entry in &resolved.manifest.overrides {
        let dest = sanitize_archive_path(&pack_dir, &override_entry.destination_path)?;
        extract_embedded_from_archive(
            &resolved.manifest.archive_path,
            &override_entry.source_path,
            &dest,
            session.filesystem(),
        )?;
        stats.overrides_copied += 1;
        override_progress.inc();
    }
    override_progress.finish(&format!("{} overrides copied", override_total));

    Ok(ImportResult {
        project_dir: config.target_dir,
        stats,
    })
}

/// Outcome of attempting to add a platform reference via packwiz.
enum AddRefResult {
    /// packwiz add succeeded; the .pw.toml was created.
    Added,
    /// No identifiers available; nothing to attempt.
    Skipped,
    /// packwiz add failed; the detail string contains the error output.
    Failed(String),
}

const MAX_ADD_RETRIES: u32 = 5;
#[cfg(feature = "test-utils")]
const RETRY_BASE_DELAY_SECS: u64 = 0;
#[cfg(not(feature = "test-utils"))]
const RETRY_BASE_DELAY_SECS: u64 = 5;

/// Wrap [`add_platform_ref`] with exponential backoff retries.
///
/// Retries up to [`MAX_ADD_RETRIES`] times on `AddRefResult::Failed`,
/// using 5s base with exponential growth (5s/10s/20s/40s/80s) to ride
/// out Modrinth 429 rate limits. packwiz makes 2 API calls per add even
/// with `--version-id`, so rate limit exhaustion is common in batch
/// imports. `Added` and `Skipped` return immediately.
async fn add_platform_ref_with_retry(
    pref: &PlatformRef,
    pack_dir: &Path,
    session: &dyn Session,
    datapack_folder: Option<&str>,
    no_refresh: bool,
) -> Result<AddRefResult> {
    for attempt in 0..=MAX_ADD_RETRIES {
        let result = add_platform_ref(pref, pack_dir, session, datapack_folder, no_refresh).await?;
        match &result {
            AddRefResult::Added | AddRefResult::Skipped => return Ok(result),
            AddRefResult::Failed(detail) => {
                if attempt < MAX_ADD_RETRIES {
                    let delay =
                        std::time::Duration::from_secs(RETRY_BASE_DELAY_SECS * (1 << attempt));
                    tracing::warn!(
                        attempt = attempt + 1,
                        max = MAX_ADD_RETRIES,
                        delay_secs = delay.as_secs(),
                        detail = %detail,
                        "packwiz add failed (likely 429); retrying"
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    return Ok(result);
                }
            }
        }
    }
    unreachable!()
}

fn side_from_env(env: &crate::empack::content::SideEnv) -> &'static str {
    use crate::empack::content::SideRequirement::*;
    match (&env.client, &env.server) {
        (Required, Unsupported) | (Required, Unknown) | (Optional, Unsupported) => "client",
        (Unsupported, Required) | (Unknown, Required) | (Unsupported, Optional) => "server",
        _ => "both",
    }
}

fn filename_from_path(dest: &str) -> String {
    std::path::Path::new(dest)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown.jar")
        .to_string()
}

async fn add_platform_ref(
    pref: &PlatformRef,
    pack_dir: &Path,
    session: &dyn Session,
    datapack_folder: Option<&str>,
    no_refresh: bool,
) -> Result<AddRefResult> {
    match pref.platform {
        ProjectPlatform::Modrinth => {
            if pref.project_id.is_empty() && pref.download_urls.is_empty() {
                return Ok(AddRefResult::Skipped);
            }

            // Fallback to url add for direct downloads without project/version IDs
            if pref.project_id.is_empty() && pref.file_id.is_none() {
                if let Some(url) = pref.download_urls.first() {
                    let name = std::path::Path::new(&pref.destination_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    let mut args: Vec<&str> = Vec::new();
                    if no_refresh {
                        args.push("--no-refresh");
                    }
                    args.extend(["url", "add", name, url, "-y"]);
                    let output =
                        session
                            .process()
                            .execute(session.packwiz_bin(), &args, pack_dir)?;
                    if output.success {
                        return Ok(AddRefResult::Added);
                    }
                    return Ok(AddRefResult::Failed(format!(
                        "packwiz url add failed for '{}': {}",
                        pref.destination_path,
                        output.error_output()
                    )));
                }
                return Ok(AddRefResult::Skipped);
            }

            let has_offline_data = !pref.download_urls.is_empty()
                && !pref.hashes.is_empty()
                && pref.resolved_name.is_some()
                && pref.file_id.is_some();

            let mut args = Vec::new();
            if no_refresh {
                args.push("--no-refresh".to_string());
                if has_offline_data {
                    args.push("--offline".to_string());
                }
            }
            args.push("modrinth".to_string());
            args.push("add".to_string());

            args.push("--project-id".to_string());
            args.push(pref.project_id.clone());

            if let Some(vid) = &pref.file_id {
                args.push("--version-id".to_string());
                args.push(vid.clone());
            }

            if has_offline_data {
                let name = pref.resolved_name.as_deref().unwrap_or("Unknown");
                args.push("--name".to_string());
                args.push(name.to_string());

                args.push("--filename".to_string());
                args.push(filename_from_path(&pref.destination_path));

                if let Some(url) = pref.download_urls.first() {
                    args.push("--url".to_string());
                    args.push(url.clone());
                }

                let (hash_format, hash_value) = pref
                    .hashes
                    .get("sha512")
                    .map(|h| ("sha512", h.as_str()))
                    .or_else(|| pref.hashes.get("sha1").map(|h| ("sha1", h.as_str())))
                    .unwrap_or(("sha512", ""));
                if !hash_value.is_empty() {
                    args.push("--hash".to_string());
                    args.push(hash_value.to_string());
                    args.push("--hash-format".to_string());
                    args.push(hash_format.to_string());
                }

                args.push("--side".to_string());
                args.push(side_from_env(&pref.env).to_string());

                if let Some(slug) = &pref.resolved_slug {
                    args.push("--slug".to_string());
                    args.push(slug.clone());
                }

                if let Some(pt) = &pref.resolved_type {
                    let pt_str = match pt {
                        crate::primitives::ProjectType::Mod => "mod",
                        crate::primitives::ProjectType::ResourcePack => "resourcepack",
                        crate::primitives::ProjectType::Shader => "shader",
                        crate::primitives::ProjectType::Datapack => "datapack",
                    };
                    args.push("--project-type".to_string());
                    args.push(pt_str.to_string());
                }
            }

            args.push("-y".to_string());

            let arg_refs = args.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            let output = execute_process_with_live_issues(
                session,
                session.packwiz_bin(),
                &arg_refs,
                pack_dir,
            )?;

            if output.success {
                return Ok(AddRefResult::Added);
            }

            Ok(AddRefResult::Failed(format!(
                "packwiz modrinth add failed for '{}': {}",
                pref.destination_path,
                output.error_output()
            )))
        }
        ProjectPlatform::CurseForge => {
            let mod_id = &pref.project_id;
            let file_id = pref
                .file_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("CurseForge ref missing file_id"))?;

            let has_cf_offline_data = !pref.hashes.is_empty() && pref.resolved_name.is_some();

            let mut args = Vec::new();
            if no_refresh {
                args.push("--no-refresh".to_string());
                if has_cf_offline_data {
                    args.push("--offline".to_string());
                }
            }
            args.extend([
                "curseforge".to_string(),
                "add".to_string(),
                "--addon-id".to_string(),
                mod_id.clone(),
                "--file-id".to_string(),
                file_id.to_string(),
            ]);

            if has_cf_offline_data {
                let name = pref.resolved_name.as_deref().unwrap_or("Unknown");
                args.push("--name".to_string());
                args.push(name.to_string());

                args.push("--filename".to_string());
                args.push(filename_from_path(&pref.destination_path));

                let (hash_format, hash_value) = pref
                    .hashes
                    .get("sha1")
                    .map(|h| ("sha1", h.as_str()))
                    .or_else(|| pref.hashes.get("md5").map(|h| ("md5", h.as_str())))
                    .unwrap_or(("sha1", ""));
                if !hash_value.is_empty() {
                    args.push("--hash".to_string());
                    args.push(hash_value.to_string());
                    args.push("--hash-format".to_string());
                    args.push(hash_format.to_string());
                }

                if let Some(slug) = &pref.resolved_slug {
                    args.push("--slug".to_string());
                    args.push(slug.clone());
                }

                if let Some(class_id) = pref.cf_class_id {
                    args.push("--class-id".to_string());
                    args.push(class_id.to_string());
                }
            }

            if pref.cf_class_id == Some(6945)
                && let Some(folder) = datapack_folder
            {
                args.extend(["--meta-folder".to_string(), folder.to_string()]);
            }

            args.push("-y".to_string());

            let arg_refs = args.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            let output = execute_process_with_live_issues(
                session,
                session.packwiz_bin(),
                &arg_refs,
                pack_dir,
            )?;

            if output.success {
                return Ok(AddRefResult::Added);
            }

            Ok(AddRefResult::Failed(format!(
                "packwiz curseforge add failed for '{}': {}",
                pref.destination_path,
                output.error_output()
            )))
        }
    }
}

/// Validate that a relative path from an archive does not escape the target directory.
fn sanitize_archive_path(base: &Path, relative: &str) -> Result<PathBuf> {
    // Canonicalize the base first so the join inherits the resolved prefix.
    // On macOS, /tmp is a symlink to /private/tmp; without this, the base
    // canonicalizes to /private/tmp/... but the joined path stays at /tmp/...
    // and the starts_with check fails.
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let joined = canonical_base.join(relative);
    let canonical_dest = joined.canonicalize().unwrap_or_else(|_| {
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

    let archive_bytes = fs
        .read_bytes(archive_path)
        .with_context(|| format!("opening archive: {}", archive_path.display()))?;
    let mut archive = open_zip_archive(archive_bytes)?;

    let mut entry = archive
        .by_name(source_path)
        .with_context(|| format!("entry '{}' not found in archive", source_path))?;

    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    fs.write_bytes(dest_path, &bytes)?;

    Ok(())
}

use crate::empack::config::format_empack_yml;

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
    if lower == "server.properties" || lower.starts_with("server-config/") {
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

fn collect_override_entries<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
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

fn prune_packwiz_override_metadata<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    entries: &mut Vec<OverrideEntry>,
) {
    let existing_destinations: std::collections::HashSet<String> = entries
        .iter()
        .map(|entry| entry.destination_path.replace('\\', "/"))
        .collect();

    entries.retain(|entry| {
        let Some(payload_path) = packwiz_override_payload_destination(archive, entry) else {
            return true;
        };
        !existing_destinations.contains(&payload_path)
    });
}

fn packwiz_override_payload_destination<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry: &OverrideEntry,
) -> Option<String> {
    let normalized_dest = entry.destination_path.replace('\\', "/");
    if !normalized_dest.ends_with(".pw.toml") || !normalized_dest.contains("/.index/") {
        return None;
    }

    let (folder, _) = normalized_dest.split_once("/.index/")?;
    let zip_entry = archive.by_name(&entry.source_path).ok()?;
    let content = read_zip_entry_to_string(zip_entry).ok()?;

    #[derive(Deserialize)]
    struct OverridePackwizMetadata {
        filename: Option<String>,
    }

    let metadata: OverridePackwizMetadata = toml::from_str(&content).ok()?;
    let filename = metadata.filename?;
    Some(format!("{folder}/{filename}"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_cf_loader(loaders: &[CfModLoader]) -> Result<(ModLoader, String)> {
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

fn open_zip_archive(data: Vec<u8>) -> Result<zip::ZipArchive<Cursor<Vec<u8>>>> {
    // Import tests and live code both enter through filesystem-provided bytes,
    // so we open archives from an in-memory cursor rather than a second file-backed path.
    zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| ImportError::ArchiveRead(e.to_string()).into())
}

fn mr_side_requirement(value: Option<&str>) -> SideRequirement {
    match value {
        Some("required") => SideRequirement::Required,
        Some("optional") => SideRequirement::Optional,
        Some("unsupported") => SideRequirement::Unsupported,
        _ => SideRequirement::Unknown,
    }
}

/// Derive a dependency key from the platform slug, display name, or destination path.
///
/// packwiz names `.pw.toml` files from the project slug (e.g., "jei" not
/// "Just Enough Items"). Prefers the API slug when available; falls back
/// to the display title (lowercased, spaces to hyphens) then the
/// destination filename.
fn derive_dep_key(
    slug: Option<&str>,
    resolved_name: Option<&str>,
    destination_path: &str,
) -> String {
    if let Some(s) = slug {
        return s.to_lowercase();
    }
    let name = resolved_name.map(|s| s.to_string()).unwrap_or_else(|| {
        std::path::Path::new(destination_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(destination_path)
            .to_string()
    });
    name.to_lowercase().replace(' ', "-")
}

/// Scan pack content directories for .pw.toml files and return their slugs.
#[instrument(skip_all, fields(dir_count = folders.len()))]
fn scan_pw_toml_stems(
    pack_dir: &Path,
    folders: &[&str],
    fs: &dyn crate::application::session::FileSystemProvider,
) -> std::collections::HashSet<String> {
    let mut slugs = std::collections::HashSet::new();
    for folder in folders {
        let dir = pack_dir.join(folder);
        if !fs.exists(&dir) {
            continue;
        }
        if let Ok(files) = fs.get_file_list(&dir) {
            for path in &files {
                if path.extension().and_then(|e| e.to_str()) == Some("toml")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    && stem.ends_with(".pw")
                {
                    slugs.insert(stem.strip_suffix(".pw").unwrap().to_lowercase());
                }
            }
        }
    }
    slugs
}

// ---------------------------------------------------------------------------
// Datapack strategy detection
// ---------------------------------------------------------------------------

/// Detect the appropriate datapack folder value from a modpack manifest.
///
/// Priority order:
/// 1. Paxi loader detected in overrides -> "config/paxi/datapacks"
/// 2. Open Loader detected in overrides -> "config/openloader/data"
/// 3. Root datapacks/ with .zip files in overrides -> "datapacks"
/// 4. Datapack-typed entries in files[] -> "datapacks"
/// 5. No datapack signals -> None
fn detect_datapack_folder(manifest: &ModpackManifest) -> Option<String> {
    let has_paxi = manifest
        .overrides
        .iter()
        .any(|o| o.destination_path.contains("config/paxi/datapacks"));

    if has_paxi {
        return Some("config/paxi/datapacks".to_string());
    }

    let has_openloader = manifest
        .overrides
        .iter()
        .any(|o| o.destination_path.contains("config/openloader/data"));

    if has_openloader {
        return Some("config/openloader/data".to_string());
    }

    let has_datapack_zips = manifest.overrides.iter().any(|o| {
        o.destination_path.starts_with("datapacks/") && o.destination_path.ends_with(".zip")
    });

    if has_datapack_zips {
        return Some("datapacks".to_string());
    }

    let has_datapack_refs = manifest.content.iter().any(|e| match e {
        ContentEntry::PlatformReferenced(p) => p.destination_path.starts_with("datapacks/"),
        _ => false,
    });

    if has_datapack_refs {
        return Some("datapacks".to_string());
    }

    let has_cf_datapack_class = manifest.content.iter().any(|e| match e {
        ContentEntry::PlatformReferenced(p) => p.cf_class_id == Some(6945),
        _ => false,
    });

    if has_cf_datapack_class {
        return Some("datapacks".to_string());
    }

    None
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
    ModrinthRemote {
        slug: String,
        version: Option<String>,
    },
    CurseForgeRemote {
        slug: String,
    },
}

#[cfg(test)]
mod tests {
    include!("import.test.rs");
}
