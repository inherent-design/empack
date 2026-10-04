use crate::application::session::{FileMetadata, FileSystemProvider};
use crate::empack::archive::ArchiveFormat;
use crate::empack::packwiz::{
    RestrictedModInfo, restricted_curseforge_file_id, restricted_destination_filename,
};
use crate::primitives::BuildTarget;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const PENDING_RESTRICTED_BUILD_FILE: &str = ".empack-build-continue.json";
const PENDING_RESTRICTED_BUILD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedBuildFingerprint {
    pub empack_yml_sha256: String,
    pub pack_toml_sha256: String,
    pub index_toml_sha256: String,
    #[serde(default)]
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedContent {
    pub algorithm: String,
    pub digest: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedBuildEntry {
    #[serde(default)]
    pub expected_content: Option<ExpectedContent>,
    pub name: String,
    pub url: String,
    pub filename: String,
    pub dest_path: String,
}

impl PendingRestrictedBuildEntry {
    pub fn cache_filename(&self) -> String {
        match &self.expected_content {
            Some(content) => format!("{}-{}", content.algorithm, content.digest),
            None => self.filename.clone(),
        }
    }
    pub fn curseforge_file_id(&self) -> Option<u64> {
        restricted_curseforge_file_id(&self.url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRestrictedCandidateSnapshot {
    pub path: String,
    pub len: u64,
    pub modified_unix_ms: Option<u64>,
    pub created_unix_ms: Option<u64>,
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
    #[serde(default)]
    pub candidate_baseline: Vec<PendingRestrictedCandidateSnapshot>,
    pub entries: Vec<PendingRestrictedBuildEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheEntryStatus {
    Missing,
    Current,
    PreexistingUnchanged,
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
    let project_hash = crate::platform::cache::project_cache_key(workdir);
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
        content_sha256: Some(project_content_fingerprint(provider, workdir)?),
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

    let metadata = installed_download_metadata(provider, workdir)?;
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

            let expected_content = expected_content_for(
                &metadata,
                &filename,
                restricted_curseforge_file_id(&restricted.url),
            )?;
            Ok(PendingRestrictedBuildEntry {
                expected_content,
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
        candidate_baseline: Vec::new(),
        entries,
    };

    persist_pending_build(provider, workdir, &pending)?;
    Ok(pending)
}

pub fn persist_pending_build(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
) -> Result<()> {
    let serialized = serde_json::to_string_pretty(pending)?;
    provider.write_atomic(&pending_state_path(workdir), &serialized)
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

fn validate_pending_paths(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
) -> Result<()> {
    let cache_root = crate::platform::cache::cache_root()?;
    let cache_dir = restricted_cache_dir(workdir)?;
    if pending.restricted_cache_path() != cache_dir {
        anyhow::bail!("Restricted cache path differs from the configured project cache");
    }
    provider.validate_output_path(&cache_root, &cache_dir)?;
    let targets = crate::empack::builds::plan_build_targets(&pending.target_list()?);
    let metadata = installed_download_metadata(provider, workdir)?;
    let mut identities = BTreeMap::new();
    for entry in &pending.entries {
        crate::empack::paths::validate_filename(&entry.filename)?;
        if let Some(content) = &entry.expected_content {
            let length = match content.algorithm.as_str() {
                "sha1" => 40,
                "sha256" => 64,
                "sha512" => 128,
                _ => anyhow::bail!("Unsupported restricted content digest"),
            };
            anyhow::ensure!(
                content.digest.len() == length
                    && content.digest.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid restricted content digest"
            );
        }
        anyhow::ensure!(
            entry.expected_content
                == expected_content_for(
                    &metadata,
                    &entry.filename,
                    restricted_curseforge_file_id(&entry.url)
                )?,
            "Saved restricted identity differs from installed metadata for {}",
            entry.filename
        );
        let cache_name = entry.cache_filename();
        if let Some(previous) = identities.insert(cache_name.clone(), &entry.url) {
            anyhow::ensure!(
                previous == &entry.url || entry.expected_content.is_some(),
                "Conflicting restricted requests use the same filename without verified identity: {}",
                entry.filename
            );
        }
        provider.validate_output_path(&cache_root, &cache_dir.join(cache_name))?;
        let dest = Path::new(&entry.dest_path);
        if dest.file_name() != Some(std::ffi::OsStr::new(&entry.filename)) {
            anyhow::bail!("Restricted destination filename does not match its cache entry");
        }
        let mut allowed = false;
        for target in &targets {
            if matches!(target, BuildTarget::ClientFull | BuildTarget::ServerFull) {
                let root = crate::empack::state::artifact_root(workdir).join(target.to_string());
                if dest.starts_with(&root) {
                    provider.validate_output_path(workdir, dest)?;
                    allowed = true;
                }
            }
        }
        if targets.contains(&BuildTarget::Mrpack) {
            let root = crate::platform::cache::packwiz_download_cache_dir(workdir)?.join("import");
            if dest.parent() == Some(root.as_path()) {
                provider.validate_output_path(&cache_root, dest)?;
                allowed = true;
            }
        }
        if !allowed {
            anyhow::bail!(
                "Restricted destination is outside the selected build roots: {}",
                dest.display()
            );
        }
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

    validate_pending_paths(provider, workdir, pending)?;

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
    validate_pending_paths(provider, workdir, pending)?;
    let cache_dir = pending.restricted_cache_path();
    provider.create_dir_all(&cache_dir)?;
    let search_dirs = ordered_search_dirs(&cache_dir, search_dirs);
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for directory in &search_dirs {
        if !provider.is_directory(directory) {
            continue;
        }
        // Automatic discovery is best-effort. Explicit associations still report IO errors.
        let Ok(files) = provider.get_file_list(directory) else {
            continue;
        };
        let mut files: Vec<_> = files.into_iter().collect();
        files.sort();
        candidates.extend(files.into_iter().filter(|path| seen.insert(path.clone())));
    }
    let mut observations = CandidateObservations::default();
    for entry in &pending.entries {
        if cached_entry_is_current(provider, pending, entry) {
            continue;
        }
        let Some(expected) = &entry.expected_content else {
            continue;
        };
        let cache_path = cache_dir.join(entry.cache_filename());
        for candidate in &candidates {
            if candidate == &cache_path || !observations.matches(provider, candidate, expected) {
                continue;
            }
            import_candidate_into_cache(provider, candidate, &cache_path)?;
            if cached_entry_is_current(provider, pending, entry) {
                break;
            }
        }
    }
    Ok(())
}

/// Associate a user-selected file with a named request. Validate all mappings before writes.
pub fn associate_downloads(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
    mappings: &[String],
    dry_run: bool,
) -> Result<()> {
    validate_pending_paths(provider, workdir, pending)?;
    let mut associations = Vec::new();
    let mut seen = HashSet::new();
    for mapping in mappings {
        let (filename, path) = mapping
            .split_once('=')
            .context("Use --associate-download FILENAME=PATH")?;
        let entries: Vec<_> = pending
            .entries
            .iter()
            .filter(|e| e.filename == filename)
            .collect();
        anyhow::ensure!(!entries.is_empty(), "No pending download named {filename}");
        let identity = entries[0].cache_filename();
        anyhow::ensure!(
            entries.iter().all(|e| e.cache_filename() == identity),
            "Ambiguous pending filename {filename}; use verified cache content instead"
        );
        anyhow::ensure!(
            seen.insert(identity.clone()),
            "Duplicate association for {filename}"
        );
        let source = workdir.join(path);
        anyhow::ensure!(
            provider.exists(&source) && !provider.is_directory(&source),
            "Download file does not exist: {}",
            source.display()
        );
        if let Some(expected) = &entries[0].expected_content {
            anyhow::ensure!(
                content_matches(provider, &source, expected)?,
                "Download does not match expected content for {filename}"
            );
        }
        associations.push((source, pending.restricted_cache_path().join(identity)));
    }
    if !dry_run {
        for (source, destination) in associations {
            import_candidate_into_cache(provider, &source, &destination)?;
        }
    }
    Ok(())
}

pub fn missing_cached_entries(
    provider: &dyn FileSystemProvider,
    pending: &PendingRestrictedBuild,
) -> Vec<PendingRestrictedBuildEntry> {
    pending
        .entries
        .iter()
        .filter(|entry| !cached_entry_is_current(provider, pending, entry))
        .cloned()
        .collect()
}

pub fn stage_cached_entries_to_destinations(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    pending: &PendingRestrictedBuild,
) -> Result<Vec<PendingRestrictedBuildEntry>> {
    validate_pending_paths(provider, workdir, pending)?;
    let cache_dir = pending.restricted_cache_path();
    let mut missing = Vec::new();

    for entry in &pending.entries {
        let cache_path = cache_dir.join(entry.cache_filename());
        if !cached_entry_is_current(provider, pending, entry) {
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

fn cached_entry_is_current(
    provider: &dyn FileSystemProvider,
    pending: &PendingRestrictedBuild,
    entry: &PendingRestrictedBuildEntry,
) -> bool {
    let path = pending.restricted_cache_path().join(entry.cache_filename());
    match &entry.expected_content {
        Some(expected) => content_matches(provider, &path, expected).unwrap_or(false),
        None => matches!(
            cache_entry_status(provider, &path, &pending.candidate_baseline),
            CacheEntryStatus::Current
        ),
    }
}

/// Observations last for one discovery pass; later passes see file changes.
#[derive(Default)]
struct CandidateObservations {
    metadata: BTreeMap<PathBuf, Option<FileMetadata>>,
    digests: BTreeMap<(PathBuf, String), Option<String>>,
}

impl CandidateObservations {
    fn matches(
        &mut self,
        provider: &dyn FileSystemProvider,
        path: &Path,
        expected: &ExpectedContent,
    ) -> bool {
        let Some(metadata) = self
            .metadata
            .entry(path.to_path_buf())
            .or_insert_with(|| provider.file_metadata(path).ok())
        else {
            return false;
        };
        if metadata.is_directory || expected.size.is_some_and(|size| metadata.len != size) {
            return false;
        }
        self.digests
            .entry((path.to_path_buf(), expected.algorithm.clone()))
            .or_insert_with(|| content_digest(provider, path, &expected.algorithm).ok())
            .as_ref()
            .is_some_and(|digest| digest.eq_ignore_ascii_case(&expected.digest))
    }
}

fn content_digest(
    provider: &dyn FileSystemProvider,
    path: &Path,
    algorithm: &str,
) -> Result<String> {
    match algorithm {
        "sha1" => stream_digest::<sha1::Sha1>(provider, path),
        "sha256" => stream_digest::<Sha256>(provider, path),
        "sha512" => stream_digest::<sha2::Sha512>(provider, path),
        _ => anyhow::bail!("Unsupported restricted content digest"),
    }
}

fn content_matches(
    provider: &dyn FileSystemProvider,
    path: &Path,
    expected: &ExpectedContent,
) -> Result<bool> {
    if !provider.exists(path) || provider.is_directory(path) {
        return Ok(false);
    }
    if let Some(size) = expected.size
        && provider.file_metadata(path)?.len != size
    {
        return Ok(false);
    }
    let digest = content_digest(provider, path, &expected.algorithm)?;
    Ok(digest.eq_ignore_ascii_case(&expected.digest))
}

fn stream_digest<D: Digest + Default>(
    provider: &dyn FileSystemProvider,
    path: &Path,
) -> Result<String> {
    let mut reader = provider.open_reader(path)?;
    let mut digest = D::default();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn project_files(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    if !provider.exists(directory) {
        return Ok(());
    }
    provider.validate_output_path(workdir, directory)?;
    for path in provider.get_file_list(directory)? {
        provider.validate_output_path(workdir, &path)?;
        if provider.is_directory(&path) {
            project_files(provider, workdir, &path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn project_content_fingerprint(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
) -> Result<String> {
    let mut files = Vec::new();
    for directory in ["pack", "overrides", "templates"] {
        project_files(provider, workdir, &workdir.join(directory), &mut files)?;
    }
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        let relative = path.strip_prefix(workdir)?.as_os_str().as_encoded_bytes();
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative);
        digest.update(stream_digest::<Sha256>(provider, &path)?.as_bytes());
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn installed_download_metadata(
    provider: &dyn FileSystemProvider,
    workdir: &Path,
) -> Result<Vec<toml::Value>> {
    let mut paths = Vec::new();
    project_files(provider, workdir, &workdir.join("pack"), &mut paths)?;
    paths
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(".pw.toml"))
        .map(|path| Ok(toml::from_str(&provider.read_to_string(&path)?)?))
        .collect()
}

fn expected_content_for(
    metadata: &[toml::Value],
    filename: &str,
    file_id: Option<u64>,
) -> Result<Option<ExpectedContent>> {
    let mut expected = None;
    for metadata in metadata {
        if metadata.get("filename").and_then(|v| v.as_str()) != Some(filename) {
            continue;
        }
        if let Some(file_id) = file_id {
            let installed = metadata
                .get("update")
                .and_then(|v| v.get("curseforge"))
                .and_then(|v| v.get("file-id"));
            let installed = installed.and_then(|v| {
                v.as_integer()
                    .and_then(|n| u64::try_from(n).ok())
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            });
            if installed != Some(file_id) {
                continue;
            }
        }
        let Some(download) = metadata.get("download") else {
            continue;
        };
        let (Some(algorithm), Some(hash)) = (
            download.get("hash-format").and_then(|v| v.as_str()),
            download.get("hash").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if !matches!(algorithm, "sha1" | "sha256" | "sha512") {
            continue;
        }
        let content = ExpectedContent {
            algorithm: algorithm.to_string(),
            digest: hash.to_ascii_lowercase(),
            size: download
                .get("size")
                .and_then(|v| v.as_integer())
                .and_then(|n| u64::try_from(n).ok()),
        };
        if let Some(previous) = &expected {
            anyhow::ensure!(
                previous == &content,
                "Ambiguous restricted file identity for {filename}"
            );
        }
        expected = Some(content);
    }
    Ok(expected)
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

fn cache_entry_status(
    provider: &dyn FileSystemProvider,
    cache_path: &Path,
    baseline: &[PendingRestrictedCandidateSnapshot],
) -> CacheEntryStatus {
    if !provider.exists(cache_path) {
        return CacheEntryStatus::Missing;
    }

    if baseline.is_empty() {
        return CacheEntryStatus::Current;
    }

    let metadata = match provider.file_metadata(cache_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::debug!(
                path = %cache_path.display(),
                error = %error,
                "treating restricted cache entry with unreadable metadata as missing"
            );
            return CacheEntryStatus::Missing;
        }
    };

    if snapshot_changed(cache_path, &metadata, baseline) {
        CacheEntryStatus::Current
    } else {
        CacheEntryStatus::PreexistingUnchanged
    }
}

pub fn capture_candidate_baseline(
    provider: &dyn FileSystemProvider,
    search_dirs: &[PathBuf],
) -> Result<Vec<PendingRestrictedCandidateSnapshot>> {
    let mut snapshots = Vec::new();
    let mut seen_paths = HashSet::new();

    for dir in search_dirs {
        for candidate in provider
            .get_file_list(dir)
            .with_context(|| format!("Failed to scan {}", dir.display()))?
        {
            let candidate_path = candidate.to_string_lossy().into_owned();
            if !seen_paths.insert(candidate_path.clone()) {
                continue;
            }

            let metadata = match provider.file_metadata(&candidate) {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::debug!(
                        path = %candidate.display(),
                        error = %error,
                        "skipping baseline candidate with unreadable metadata"
                    );
                    continue;
                }
            };
            if metadata.is_directory {
                continue;
            }

            snapshots.push(PendingRestrictedCandidateSnapshot {
                path: candidate_path,
                len: metadata.len,
                modified_unix_ms: metadata.modified_unix_ms,
                created_unix_ms: metadata.created_unix_ms,
            });
        }
    }

    snapshots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(snapshots)
}

fn snapshot_changed(
    candidate_path: &Path,
    metadata: &FileMetadata,
    baseline: &[PendingRestrictedCandidateSnapshot],
) -> bool {
    let candidate_path = candidate_path.to_string_lossy();
    let Some(snapshot) = baseline
        .iter()
        .find(|snapshot| snapshot.path == candidate_path)
    else {
        return true;
    };

    snapshot.len != metadata.len
        || snapshot.modified_unix_ms != metadata.modified_unix_ms
        || snapshot.created_unix_ms != metadata.created_unix_ms
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
