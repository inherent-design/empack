use crate::application::session::{FileMetadata, FileSystemProvider};
use crate::empack::archive::ArchiveFormat;
use crate::empack::packwiz::{
    RestrictedModInfo, restricted_curseforge_file_id, restricted_destination_filename,
};
use crate::primitives::BuildTarget;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const PENDING_RESTRICTED_BUILD_FILE: &str = ".empack-build-continue.json";
const PENDING_RESTRICTED_BUILD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedBuildFingerprint {
    pub empack_yml_sha256: String,
    pub pack_toml_sha256: String,
    pub index_toml_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedBuildEntry {
    pub name: String,
    pub url: String,
    pub filename: String,
    pub dest_path: String,
}

impl PendingRestrictedBuildEntry {
    pub fn curseforge_file_id(&self) -> Option<u64> {
        restricted_curseforge_file_id(&self.url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedBuild {
    pub schema_version: u32,
    pub targets: Vec<String>,
    pub archive_format: String,
    pub project_fingerprint: PendingRestrictedBuildFingerprint,
    pub restricted_cache_dir: String,
    #[serde(default)]
    pub recorded_at_unix_ms: Option<u64>,
    pub entries: Vec<PendingRestrictedBuildEntry>,
}

impl PendingRestrictedBuild {
    pub fn target_list(&self) -> Result<Vec<BuildTarget>> {
        self.targets
            .iter()
            .map(|target| {
                target
                    .parse::<BuildTarget>()
                    .map_err(|e| anyhow!("Invalid persisted build target '{target}': {e}"))
            })
            .collect()
    }

    pub fn archive_format_value(&self) -> Result<ArchiveFormat> {
        parse_archive_format(&self.archive_format)
    }

    pub fn restricted_cache_path(&self) -> PathBuf {
        PathBuf::from(&self.restricted_cache_dir)
    }
}

pub fn pending_state_path(workdir: &Path) -> PathBuf {
    workdir.join(PENDING_RESTRICTED_BUILD_FILE)
}

pub fn restricted_cache_dir(workdir: &Path) -> Result<PathBuf> {
    let cache_root = crate::platform::cache::restricted_builds_cache_dir()?;
    let project_hash = hex_sha256(workdir.to_string_lossy().as_bytes());
    Ok(cache_root.join(project_hash))
}

pub fn compute_project_fingerprint(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
) -> Result<PendingRestrictedBuildFingerprint> {
    Ok(PendingRestrictedBuildFingerprint {
        empack_yml_sha256: file_sha256(provider, &workdir.join("empack.yml"))?,
        pack_toml_sha256: file_sha256(provider, &workdir.join("pack").join("pack.toml"))?,
        index_toml_sha256: file_sha256(provider, &workdir.join("pack").join("index.toml"))?,
    })
}

pub fn save_pending_build(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    targets: &[BuildTarget],
    archive_format: ArchiveFormat,
    restricted_mods: &[RestrictedModInfo],
) -> Result<PendingRestrictedBuild> {
    let restricted_cache_dir = restricted_cache_dir(workdir)?;
    provider.create_dir_all(&restricted_cache_dir)?;

    let entries = restricted_mods
        .iter()
        .map(|restricted| {
            let filename =
                restricted_destination_filename(&restricted.dest_path).ok_or_else(|| {
                    anyhow!(
                        "Restricted mod '{}' is missing a destination filename",
                        restricted.name
                    )
                })?;

            Ok(PendingRestrictedBuildEntry {
                name: restricted.name.clone(),
                url: restricted.url.clone(),
                filename,
                dest_path: restricted.dest_path.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let pending = PendingRestrictedBuild {
        schema_version: PENDING_RESTRICTED_BUILD_SCHEMA_VERSION,
        targets: targets.iter().map(ToString::to_string).collect(),
        archive_format: archive_format_name(archive_format).to_string(),
        project_fingerprint: compute_project_fingerprint(provider, workdir)?,
        restricted_cache_dir: restricted_cache_dir.to_string_lossy().to_string(),
        recorded_at_unix_ms: Some(current_unix_ms()),
        entries,
    };

    let serialized = serde_json::to_string_pretty(&pending)?;
    provider.write_file(&pending_state_path(workdir), &serialized)?;

    Ok(pending)
}

pub fn load_pending_build(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
) -> Result<Option<PendingRestrictedBuild>> {
    let state_path = pending_state_path(workdir);
    if !provider.exists(&state_path) {
        return Ok(None);
    }

    let contents = provider
        .read_to_string(&state_path)
        .with_context(|| format!("Failed to read {}", state_path.display()))?;
    let pending: PendingRestrictedBuild = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", state_path.display()))?;
    Ok(Some(pending))
}

pub fn clear_pending_build(provider: &dyn FileSystemProvider, workdir: &Path) -> Result<()> {
    let state_path = pending_state_path(workdir);
    if provider.exists(&state_path) {
        provider.remove_file(&state_path)?;
    }
    Ok(())
}

pub fn validate_pending_build(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
) -> Result<Option<String>> {
    if pending.schema_version != PENDING_RESTRICTED_BUILD_SCHEMA_VERSION {
        return Ok(Some(format!(
            "unsupported pending restricted build schema version {}",
            pending.schema_version
        )));
    }

    let current = compute_project_fingerprint(provider, workdir)?;
    if current != pending.project_fingerprint {
        return Ok(Some(
            "project files changed since the restricted build was recorded".to_string(),
        ));
    }

    for target in pending.target_list()? {
        if matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull) {
            let target_dir = crate::empack::state::artifact_root(workdir).join(target.to_string());
            let requires_existing_dir = pending
                .entries
                .iter()
                .any(|entry| Path::new(&entry.dest_path).starts_with(&target_dir));
            if requires_existing_dir && !provider.is_directory(&target_dir) {
                return Ok(Some(format!(
                    "required build directory is missing: {}",
                    target_dir.display()
                )));
            }
        }
    }

    Ok(None)
}

pub fn import_matching_downloads_into_cache(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
    search_dirs: &[PathBuf],
) -> Result<()> {
    let cache_dir = pending.restricted_cache_path();
    provider.create_dir_all(&cache_dir)?;
    let search_dirs = ordered_search_dirs(&cache_dir, search_dirs);
    let recent_cutoff_ms = pending_recent_cutoff_ms(provider, workdir, pending);
    let mut used_fallback_hashes = HashSet::new();

    let entries_by_filename: BTreeMap<_, _> = pending
        .entries
        .iter()
        .map(|entry| (entry.filename.clone(), entry))
        .collect();

    for (filename, entry) in entries_by_filename {
        let cache_path = cache_dir.join(&filename);
        if provider.exists(&cache_path) {
            continue;
        }

        if let Some(candidate) =
            find_exact_candidate(provider, &cache_path, &filename, &search_dirs)
        {
            import_candidate_into_cache(provider, &candidate, &cache_path)?;
            continue;
        }

        tracing::debug!(
            filename = %filename,
            cache_path = %cache_path.display(),
            "restricted download exact filename match not found; trying recent-file fallback"
        );

        let Some(recent_cutoff_ms) = recent_cutoff_ms else {
            tracing::debug!(
                filename = %filename,
                "restricted download fallback disabled because no pending-build timestamp is available"
            );
            continue;
        };

        if let Some((hash, candidate)) = find_recent_candidate(
            provider,
            entry,
            &cache_path,
            &search_dirs,
            recent_cutoff_ms,
            &used_fallback_hashes,
        )? {
            import_candidate_into_cache(provider, &candidate, &cache_path)?;
            used_fallback_hashes.insert(hash);
        }
    }

    Ok(())
}

pub fn missing_cached_entries(
    provider: &dyn FileSystemProvider,
    pending: &PendingRestrictedBuild,
) -> Vec<PendingRestrictedBuildEntry> {
    let cache_dir = pending.restricted_cache_path();
    pending
        .entries
        .iter()
        .filter(|entry| !provider.exists(&cache_dir.join(&entry.filename)))
        .cloned()
        .collect()
}

pub fn stage_cached_entries_to_destinations(
    provider: &dyn FileSystemProvider,
    pending: &PendingRestrictedBuild,
) -> Result<Vec<PendingRestrictedBuildEntry>> {
    let cache_dir = pending.restricted_cache_path();
    let mut missing = Vec::new();

    for entry in &pending.entries {
        let cache_path = cache_dir.join(&entry.filename);
        if !provider.exists(&cache_path) {
            missing.push(entry.clone());
            continue;
        }

        let dest_path = Path::new(&entry.dest_path);
        if let Some(parent) = dest_path.parent() {
            provider.create_dir_all(parent)?;
        }

        let bytes = provider
            .read_bytes(&cache_path)
            .with_context(|| format!("Failed to read cached file {}", cache_path.display()))?;
        provider
            .write_bytes(dest_path, &bytes)
            .with_context(|| format!("Failed to restore {}", dest_path.display()))?;
    }

    Ok(missing)
}

fn file_sha256(provider: &dyn FileSystemProvider, path: &Path) -> Result<String> {
    let bytes = provider
        .read_bytes(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(hex_sha256(&bytes))
}

fn ordered_search_dirs(cache_dir: &Path, search_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut ordered = vec![cache_dir.to_path_buf()];
    for dir in search_dirs {
        if !ordered.contains(dir) {
            ordered.push(dir.clone());
        }
    }
    ordered
}

fn find_exact_candidate(
    provider: &dyn FileSystemProvider,
    cache_path: &Path,
    filename: &str,
    search_dirs: &[PathBuf],
) -> Option<PathBuf> {
    for dir in search_dirs {
        let candidate = dir.join(filename);
        if candidate != cache_path && provider.exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn find_recent_candidate(
    provider: &dyn FileSystemProvider,
    entry: &PendingRestrictedBuildEntry,
    cache_path: &Path,
    search_dirs: &[PathBuf],
    recent_cutoff_ms: u64,
    used_fallback_hashes: &HashSet<String>,
) -> Result<Option<(String, PathBuf)>> {
    let expected_extension = lowercase_extension(Path::new(&entry.filename));
    let mut candidates_by_hash = HashMap::new();

    for dir in search_dirs {
        for candidate in provider
            .get_file_list(dir)
            .with_context(|| format!("Failed to scan {}", dir.display()))?
        {
            if candidate == cache_path {
                continue;
            }

            let metadata = match provider.file_metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::debug!(
                        path = %candidate.display(),
                        error = %error,
                        "skipping restricted download candidate with unreadable metadata"
                    );
                    continue;
                }
            };
            if metadata.is_directory {
                continue;
            }

            if lowercase_extension(&candidate) != expected_extension {
                continue;
            }

            let Some(candidate_time_ms) = best_file_time_ms(&metadata) else {
                tracing::debug!(
                    path = %candidate.display(),
                    "skipping restricted download candidate without usable timestamp metadata"
                );
                continue;
            };

            if candidate_time_ms < recent_cutoff_ms {
                continue;
            }

            let hash = match candidate_sha256(provider, &candidate) {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::debug!(
                        path = %candidate.display(),
                        error = %error,
                        "skipping restricted download candidate that could not be hashed"
                    );
                    continue;
                }
            };
            if used_fallback_hashes.contains(&hash) {
                continue;
            }
            candidates_by_hash.entry(hash).or_insert(candidate);
        }
    }

    tracing::debug!(
        filename = %entry.filename,
        candidate_count = candidates_by_hash.len(),
        recent_cutoff_ms,
        "restricted download recent-file fallback evaluated"
    );

    match candidates_by_hash.len() {
        0 => Ok(None),
        1 => {
            let (hash, candidate) = candidates_by_hash
                .into_iter()
                .next()
                .expect("single candidate");
            tracing::debug!(
                filename = %entry.filename,
                candidate = %candidate.display(),
                "restricted download fallback selected a unique recent candidate"
            );
            Ok(Some((hash, candidate)))
        }
        _ => {
            tracing::debug!(
                filename = %entry.filename,
                candidate_count = candidates_by_hash.len(),
                "restricted download fallback skipped due to ambiguous recent candidates"
            );
            Ok(None)
        }
    }
}

fn import_candidate_into_cache(
    provider: &dyn FileSystemProvider,
    candidate: &Path,
    cache_path: &Path,
) -> Result<()> {
    let bytes = provider
        .read_bytes(candidate)
        .with_context(|| format!("Failed to read {}", candidate.display()))?;
    provider
        .write_bytes(cache_path, &bytes)
        .with_context(|| format!("Failed to cache {}", cache_path.display()))
}

fn pending_recent_cutoff_ms(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
) -> Option<u64> {
    pending
        .recorded_at_unix_ms
        .or_else(|| {
            provider
                .file_metadata(&pending_state_path(workdir))
                .ok()
                .and_then(|metadata| best_file_time_ms(&metadata))
        })
        .map(|recorded_at| recorded_at.saturating_sub(10_000))
}

fn best_file_time_ms(metadata: &FileMetadata) -> Option<u64> {
    metadata.created_unix_ms.or(metadata.modified_unix_ms)
}

fn lowercase_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

fn candidate_sha256(provider: &dyn FileSystemProvider, path: &Path) -> Result<String> {
    let bytes = provider
        .read_bytes(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn archive_format_name(archive_format: ArchiveFormat) -> &'static str {
    match archive_format {
        ArchiveFormat::Zip => "zip",
        ArchiveFormat::TarGz => "tar.gz",
        ArchiveFormat::SevenZ => "7z",
    }
}

fn parse_archive_format(value: &str) -> Result<ArchiveFormat> {
    match value {
        "zip" => Ok(ArchiveFormat::Zip),
        "tar.gz" => Ok(ArchiveFormat::TarGz),
        "7z" => Ok(ArchiveFormat::SevenZ),
        _ => Err(anyhow!("Invalid persisted archive format '{value}'")),
    }
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    include!("restricted_build.test.rs");
}
