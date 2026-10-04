//! Packwiz integration for metadata management and mod installation.
//!
//! Two abstractions:
//! - [`PackwizMetadata`]: convenience wrapper for `packwiz modrinth/curseforge add/remove/refresh`
//! - [`PackwizInstaller`]: wraps `java -jar packwiz-installer-bootstrap.jar` invocation
//!
//! Both use [`ProcessProvider`] for command execution and follow the session-based DI pattern.
//!
//! Commands currently use direct `ProcessProvider` for simple single-command
//! operations. The wrapper is reserved for cases requiring multi-step
//! validation or structured error handling.

use crate::application::session::{FileSystemProvider, ProcessProvider, Session};
use crate::empack::state::StateError;
use crate::empack::versions::{canonicalize_forge_loader_version, uses_legacy_forge_coordinate};
use crate::primitives::ProjectPlatform;

/// Binary name for packwiz CLI operations.
///
/// Uses `packwiz-tx` fork (mannie-exe/packwiz-tx) which adds `--no-refresh`
/// for batch operations. Override at runtime via `EMPACK_PACKWIZ_BIN`.
pub const PACKWIZ_BIN: &str = "packwiz-tx";
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::instrument;

/// Trait for packwiz CLI operations extracted from FileSystemProvider.
///
/// Separates packwiz-specific orchestration (init, refresh, list, JAR caching)
/// from pure filesystem I/O, following the Interface Segregation Principle.
pub trait PackwizOps {
    /// Run packwiz init command to scaffold a new pack
    #[allow(clippy::too_many_arguments)]
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError>;

    /// Run packwiz refresh command to sync index
    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError>;

    /// Get list of currently installed mods from packwiz
    fn get_installed_mods(&self, workdir: &Path) -> crate::Result<HashSet<String>>;

    /// Snapshot provider identity, version and installed filename for reconciliation.
    fn installed_snapshot(
        &self,
        workdir: &Path,
    ) -> crate::Result<Vec<super::installed::InstalledDependency>>;

    /// Verify the live postcondition, including requested pins, after backend success.
    fn verify_reconciled(
        &self,
        workdir: &Path,
        plan: &super::config::ProjectPlan,
    ) -> crate::Result<()> {
        let observed = self.installed_snapshot(workdir)?;
        let remaining = crate::application::sync::build_sync_plan(plan, &observed)?;
        anyhow::ensure!(
            remaining.actions.is_empty(),
            "Backend reported success but installed dependencies do not satisfy empack.yml; inspect pack metadata before syncing again"
        );
        Ok(())
    }

    /// Whether persisted imported side/optional intent matches installed metadata.
    fn requirements_satisfied(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<bool>;

    /// Apply imported side/optional intent after an installation or pin change.
    fn apply_requirements(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<()>;

    /// Refuse mrpack conversions that would turn optional restricted files into overrides.
    fn validate_optional_export(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<()>;

    /// Return the observed metadata key only after identity, type and pin agree.
    fn verify_added(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<Option<String>> {
        let identity = super::installed::DependencyIdentity {
            platform: record.platform,
            project_id: record.project_id.clone(),
            project_type: record.project_type,
        };
        let observed = self.installed_snapshot(workdir)?;
        let matches: Vec<_> = observed
            .iter()
            .filter(|entry| entry.identity.as_ref() == Some(&identity))
            .collect();
        anyhow::ensure!(
            matches.len() == 1
                && record
                    .version
                    .as_ref()
                    .is_none_or(|pin| matches[0].version.as_ref() == Some(pin)),
            "Backend reported success but installed metadata does not match requested identity or pin for '{}'; inspect pack metadata before retrying",
            record.title
        );
        Ok(Some(matches[0].key.clone()))
    }

    /// Validate the filesystem authority of the observed backend removal target.
    fn validate_removal_target(
        &self,
        _workdir: &Path,
        _target: &super::installed::InstalledDependency,
    ) -> crate::Result<()>;

    /// A successful backend exit must actually remove the selected installation.
    fn verify_removed(
        &self,
        workdir: &Path,
        target: &super::installed::InstalledDependency,
    ) -> crate::Result<()> {
        let observed = self.installed_snapshot(workdir)?;
        anyhow::ensure!(
            !observed.iter().any(|entry| entry.key == target.key
                || (target.identity.is_some() && entry.identity == target.identity)),
            "Backend reported success but '{}' is still installed; manifest intent was retained",
            target.key
        );
        Ok(())
    }

    /// Get the expected cache path for packwiz-installer-bootstrap.jar
    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf>;

    /// Get the expected cache path for packwiz-installer.jar
    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf>;
}

/// Live implementation that routes all packwiz commands through ProcessProvider
pub struct LivePackwizOps<'a> {
    process: &'a dyn ProcessProvider,
    filesystem: &'a dyn FileSystemProvider,
    /// Resolved packwiz-tx binary path (absolute path or bare name).
    packwiz_bin: &'a str,
    lazy_bin: Option<&'a std::sync::OnceLock<String>>,
}

impl<'a> LivePackwizOps<'a> {
    pub fn new(
        process: &'a dyn ProcessProvider,
        filesystem: &'a dyn FileSystemProvider,
        packwiz_bin: &'a str,
    ) -> Self {
        Self {
            process,
            filesystem,
            packwiz_bin,
            lazy_bin: None,
        }
    }
    pub fn new_lazy(
        process: &'a dyn ProcessProvider,
        filesystem: &'a dyn FileSystemProvider,
        bin: &'a std::sync::OnceLock<String>,
    ) -> Self {
        Self {
            process,
            filesystem,
            packwiz_bin: PACKWIZ_BIN,
            lazy_bin: Some(bin),
        }
    }

    fn binary(&self) -> &str {
        self.lazy_bin
            .map(|bin| {
                bin.get_or_init(crate::application::session::resolve_packwiz_bin_path)
                    .as_str()
            })
            .unwrap_or(self.packwiz_bin)
    }

    fn installed_paths(
        &self,
        workdir: &Path,
    ) -> crate::Result<std::collections::BTreeMap<String, PathBuf>> {
        let pack_dir = workdir.join("pack");
        let mut scan_dirs = HashSet::from([
            "mods".to_string(),
            "resourcepacks".to_string(),
            "shaderpacks".to_string(),
            "datapacks".to_string(),
        ]);
        if self.filesystem.exists(&workdir.join("empack.yml"))
            && let Some(folder) = self
                .filesystem
                .config_manager(workdir.to_path_buf())
                .load_empack_config()?
                .empack
                .datapack_folder
        {
            scan_dirs.insert(folder);
        }
        let pack_path = pack_dir.join("pack.toml");
        if self.filesystem.exists(&pack_path) {
            let metadata: toml::Value =
                toml::from_str(&self.filesystem.read_to_string(&pack_path)?)?;
            if let Some(folder) = metadata
                .get("options")
                .and_then(|o| o.get("datapack-folder"))
            {
                let folder = folder.as_str().ok_or_else(|| {
                    anyhow::anyhow!("pack.toml options.datapack-folder must be a string")
                })?;
                scan_dirs.insert(folder.to_string());
            }
        }

        let mut installed = std::collections::BTreeMap::new();
        for folder in &scan_dirs {
            let dir = pack_dir.join(folder);
            if !self.filesystem.exists(&dir) {
                continue;
            }
            let file_list = self.filesystem.get_file_list(&dir)?;
            for path in &file_list {
                if let Some(file_name) = path.file_name().and_then(|f| f.to_str())
                    && let Some(slug) = file_name.strip_suffix(".pw.toml")
                    && !slug.is_empty()
                    && let Some(previous) = installed.insert(slug.to_string(), path.clone())
                    && previous != *path
                {
                    anyhow::bail!("Duplicate installed dependency key: {slug}");
                }
            }
        }

        Ok(installed)
    }
}

impl PackwizOps for LivePackwizOps<'_> {
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError> {
        let pack_dir = workdir.join("pack");

        if !self.filesystem.exists(&pack_dir) {
            return Err(StateError::MissingFile {
                file: pack_dir.to_path_buf(),
            });
        }

        if uses_legacy_forge_init_workaround(modloader, mc_version, loader_version) {
            return self.run_legacy_forge_init(
                workdir,
                name,
                author,
                version,
                mc_version,
                loader_version,
            );
        }

        let mut args = vec![
            "init",
            "--name",
            name,
            "--author",
            author,
            "--version",
            version,
            "--mc-version",
            mc_version,
            "--modloader",
            modloader,
            "-y",
        ];

        match modloader {
            "neoforge" => {
                args.push("--neoforge-version");
                args.push(loader_version);
            }
            "fabric" => {
                args.push("--fabric-version");
                args.push(loader_version);
            }
            "quilt" => {
                args.push("--quilt-version");
                args.push(loader_version);
            }
            "forge" => {
                args.push("--forge-version");
                args.push(loader_version);
            }
            _ => {}
        }

        let output = self
            .process
            .execute(self.binary(), &args, &pack_dir)
            .map_err(|e| StateError::CommandFailed {
                command: format!("packwiz init failed: {}", e),
            })?;

        if !output.success {
            return Err(StateError::CommandFailed {
                command: format!("packwiz init returned non-zero: {}", output.error_output()),
            });
        }

        Ok(())
    }

    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError> {
        let pack_file = workdir.join("pack").join("pack.toml");

        let pack_file_str = pack_file.to_str().ok_or_else(|| StateError::IoError {
            source: anyhow::anyhow!("Invalid UTF-8 in pack.toml path"),
        })?;

        let output = self
            .process
            .execute(
                self.binary(),
                &["--pack-file", pack_file_str, "refresh"],
                workdir,
            )
            .map_err(|e| StateError::CommandFailed {
                command: format!("packwiz refresh failed: {}", e),
            })?;

        if !output.success {
            return Err(StateError::CommandFailed {
                command: format!(
                    "packwiz refresh returned non-zero: {}",
                    output.error_output()
                ),
            });
        }

        Ok(())
    }

    fn get_installed_mods(&self, workdir: &Path) -> crate::Result<HashSet<String>> {
        Ok(self.installed_paths(workdir)?.into_keys().collect())
    }

    fn installed_snapshot(
        &self,
        workdir: &Path,
    ) -> crate::Result<Vec<super::installed::InstalledDependency>> {
        self.installed_paths(workdir)?
            .into_iter()
            .map(|(key, path)| {
                let pack = workdir.join("pack");
                let project_type = if path.starts_with(pack.join("mods")) {
                    crate::primitives::ProjectType::Mod
                } else if path.starts_with(pack.join("resourcepacks")) {
                    crate::primitives::ProjectType::ResourcePack
                } else if path.starts_with(pack.join("shaderpacks")) {
                    crate::primitives::ProjectType::Shader
                } else {
                    crate::primitives::ProjectType::Datapack
                };
                let metadata = toml::from_str(&self.filesystem.read_to_string(&path)?)?;
                super::installed::InstalledDependency::from_metadata(key, project_type, &metadata)
            })
            .collect()
    }

    fn validate_removal_target(
        &self,
        workdir: &Path,
        target: &super::installed::InstalledDependency,
    ) -> crate::Result<()> {
        let paths = self.installed_paths(workdir)?;
        let path = paths
            .get(&target.key)
            .ok_or_else(|| anyhow::anyhow!("Removal target disappeared"))?;
        self.filesystem
            .validate_output_path(&workdir.join("pack"), path)?;
        anyhow::ensure!(
            self.filesystem.is_regular_file(path),
            "Removal metadata must be a regular file"
        );
        Ok(())
    }

    fn validate_optional_export(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<()> {
        let Some(env) = &record.environment else {
            return Ok(());
        };
        if !super::url_file::requirements(env)?.1 {
            return Ok(());
        }
        let (_, metadata) = self
            .requirement_metadata(workdir, record)?
            .ok_or_else(|| anyhow::anyhow!("Missing optional metadata for {}", record.title))?;
        let mode = metadata
            .get("download")
            .and_then(|d| d.get("mode"))
            .and_then(toml::Value::as_str)
            .unwrap_or("url");
        anyhow::ensure!(
            mode == "url",
            "Mrpack export cannot preserve optional restricted file '{}'; use a full distribution target or explicitly make it required",
            record.title
        );
        Ok(())
    }

    fn requirements_satisfied(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<bool> {
        let Some(env) = &record.environment else {
            return Ok(true);
        };
        super::url_file::requirements(env)?;
        let Some((_, metadata)) = self.requirement_metadata(workdir, record)? else {
            return Ok(false);
        };
        let mut desired = metadata.clone();
        super::url_file::apply_requirements(&mut desired, env)?;
        Ok(metadata == desired)
    }

    fn apply_requirements(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<()> {
        let Some(env) = &record.environment else {
            return Ok(());
        };
        let (path, mut metadata) =
            self.requirement_metadata(workdir, record)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "Installed metadata is missing for '{}' while preserving import requirements",
                    record.title
                )
            })?;
        super::url_file::apply_requirements(&mut metadata, env)?;
        self.filesystem
            .validate_output_path(&workdir.join("pack"), &path)?;
        self.filesystem
            .write_atomic(&path, &toml::to_string(&metadata)?)
    }

    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf> {
        let cache_dir = crate::platform::cache::jar_cache_dir()?;
        Ok(cache_dir.join("packwiz-installer-bootstrap.jar"))
    }

    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf> {
        let cache_dir = crate::platform::cache::jar_cache_dir()?;
        Ok(cache_dir.join("packwiz-installer.jar"))
    }
}

impl LivePackwizOps<'_> {
    fn requirement_metadata(
        &self,
        workdir: &Path,
        record: &super::config::DependencyRecord,
    ) -> crate::Result<Option<(PathBuf, toml::Value)>> {
        let identity = super::installed::DependencyIdentity {
            platform: record.platform,
            project_id: record.project_id.clone(),
            project_type: record.project_type,
        };
        let matches: Vec<_> = self
            .installed_snapshot(workdir)?
            .into_iter()
            .filter(|entry| entry.identity.as_ref() == Some(&identity))
            .collect();
        anyhow::ensure!(
            matches.len() <= 1,
            "Ambiguous installed identity for {}",
            record.title
        );
        let Some(target) = matches.first() else {
            return Ok(None);
        };
        let paths = self.installed_paths(workdir)?;
        let path = &paths[&target.key];
        let metadata = toml::from_str(&self.filesystem.read_to_string(path)?)?;
        Ok(Some((path.clone(), metadata)))
    }

    fn run_legacy_forge_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError> {
        let pack_dir = workdir.join("pack");
        let args = vec![
            "init",
            "--name",
            name,
            "--author",
            author,
            "--version",
            version,
            "--mc-version",
            mc_version,
            "--modloader",
            "none",
            "-y",
        ];

        let output = self
            .process
            .execute(self.binary(), &args, &pack_dir)
            .map_err(|e| StateError::CommandFailed {
                command: format!("packwiz init failed: {}", e),
            })?;

        if !output.success {
            return Err(StateError::CommandFailed {
                command: format!("packwiz init returned non-zero: {}", output.error_output()),
            });
        }

        let pack_toml_path = pack_dir.join("pack.toml");
        let normalized_loader_version =
            canonicalize_forge_loader_version(mc_version, loader_version);
        write_pack_toml_versions(
            &pack_toml_path,
            &[("forge", &normalized_loader_version)],
            self.filesystem,
        )
        .map_err(|e| StateError::IoError {
            source: anyhow::anyhow!("failed to patch legacy Forge pack.toml: {}", e),
        })?;

        self.run_packwiz_refresh(workdir)
    }
}

fn uses_legacy_forge_init_workaround(
    modloader: &str,
    mc_version: &str,
    loader_version: &str,
) -> bool {
    // packwiz-tx cannot currently initialize Forge 1.7.10 packs because the
    // Maven metadata switches at 10.13.2.1300 to `...-1.7.10` suffixed
    // versions while modpack manifests and packwiz init expect the raw
    // Forge loader version.
    modloader == "forge" && uses_legacy_forge_coordinate(mc_version, loader_version)
}

/// Check if packwiz is available in PATH and return version info.
///
/// Uses ProcessProvider::find_program for cross-platform program lookup.
pub fn check_packwiz_available(
    process: &dyn ProcessProvider,
    workdir: &Path,
) -> crate::Result<(bool, String)> {
    match process.find_program(PACKWIZ_BIN) {
        Some(path) => {
            let version = get_packwiz_version(process, &path, workdir)
                .unwrap_or_else(|| "unknown".to_string());
            Ok((true, version))
        }
        None => Ok((false, "not found".to_string())),
    }
}

/// Get packwiz version using go toolchain.
///
/// Takes the packwiz binary path directly and queries version via `go version -m`.
/// The `workdir` parameter is only needed as a required arg to `process.execute`;
/// `go version -m` reads the binary at an absolute path and ignores the working directory.
pub fn get_packwiz_version(
    process: &dyn ProcessProvider,
    packwiz_path: &str,
    workdir: &Path,
) -> Option<String> {
    let go_output = process
        .execute("go", &["version", "-m", packwiz_path], workdir)
        .ok()?;
    if !go_output.success {
        return None;
    }

    for line in go_output.stdout.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("mod") {
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            if fields.len() >= 3 {
                return Some(fields[2].to_string());
            }
        }
    }

    None
}

/// Mock implementation for unit testing.
///
/// Replicates the mock behavior previously in MockFileSystemProvider:
/// - `run_packwiz_init` creates pack/pack.toml and pack/index.toml in-memory
/// - `run_packwiz_refresh` verifies pack.toml exists
/// - `get_installed_mods` returns a configured mock mod set
/// - JAR cache paths return test-appropriate paths
#[cfg(feature = "test-utils")]
pub struct MockPackwizOps {
    pub installed_mods: HashSet<String>,
    pub filesystem: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>>,
    pub current_dir: PathBuf,
    pub fail_init: bool,
}

#[cfg(feature = "test-utils")]
impl MockPackwizOps {
    pub fn new() -> Self {
        Self {
            installed_mods: HashSet::new(),
            filesystem: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            ),
            current_dir: crate::application::session_mocks::mock_root().join("workdir"),
            fail_init: false,
        }
    }

    pub fn with_failing_init(mut self) -> Self {
        self.fail_init = true;
        self
    }

    pub fn with_installed_mods(mut self, mods: HashSet<String>) -> Self {
        self.installed_mods = mods;
        self
    }

    pub fn with_current_dir(mut self, dir: PathBuf) -> Self {
        self.current_dir = dir;
        self
    }

    /// Connect to a MockFileSystemProvider's in-memory files for init/refresh side effects
    pub fn with_filesystem(
        mut self,
        files: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>>,
    ) -> Self {
        self.filesystem = files;
        self
    }
}

#[cfg(feature = "test-utils")]
impl Default for MockPackwizOps {
    fn default() -> Self {
        Self::new()
    }
}

/// Default index.toml template for packwiz mock init
#[cfg(feature = "test-utils")]
const MOCK_DEFAULT_INDEX_TOML: &str = r#"hash-format = "sha256"

[[files]]
file = "pack.toml"
hash = ""
"#;

#[cfg(feature = "test-utils")]
impl PackwizOps for MockPackwizOps {
    fn run_packwiz_init(
        &self,
        workdir: &Path,
        name: &str,
        author: &str,
        version: &str,
        modloader: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> Result<(), StateError> {
        if self.fail_init {
            return Err(StateError::IoError {
                source: anyhow::anyhow!("mock packwiz init failure"),
            });
        }
        let pack_dir = workdir.join("pack");
        let pack_file = pack_dir.join("pack.toml");
        let loader_line = if modloader == "none" {
            String::new()
        } else {
            format!("{} = \"{}\"\n", modloader, loader_version)
        };
        let default_pack_toml = format!(
            r#"name = "{}"
author = "{}"
version = "{}"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "{}"
{}"#,
            name, author, version, mc_version, loader_line
        );
        self.filesystem
            .lock()
            .unwrap()
            .insert(pack_file, default_pack_toml);

        let index_file = pack_dir.join("index.toml");
        self.filesystem
            .lock()
            .unwrap()
            .insert(index_file, MOCK_DEFAULT_INDEX_TOML.to_string());

        Ok(())
    }

    fn run_packwiz_refresh(&self, workdir: &Path) -> Result<(), StateError> {
        let pack_file = workdir.join("pack").join("pack.toml");
        if !self.filesystem.lock().unwrap().contains_key(&pack_file) {
            return Err(StateError::MissingFile {
                file: pack_file.to_path_buf(),
            });
        }
        Ok(())
    }

    fn get_installed_mods(&self, _workdir: &Path) -> crate::Result<HashSet<String>> {
        Ok(self.installed_mods.clone())
    }

    fn installed_snapshot(
        &self,
        workdir: &Path,
    ) -> crate::Result<Vec<super::installed::InstalledDependency>> {
        // Legacy mock names model satisfied declarations. Live safety regressions
        // use LivePackwizOps and real metadata rather than this convenience fixture.
        let files = self.filesystem.lock().unwrap();
        let config = files
            .get(&workdir.join("empack.yml"))
            .and_then(|text| serde_saphyr::from_str::<super::config::EmpackConfig>(text).ok());
        Ok(self
            .installed_mods
            .iter()
            .map(|key| {
                if let Some(metadata) =
                    files.get(&workdir.join("pack/mods").join(format!("{key}.pw.toml")))
                    && let Ok(metadata) = toml::from_str(metadata)
                    && let Ok(observed) = super::installed::InstalledDependency::from_metadata(
                        key.clone(),
                        crate::primitives::ProjectType::Mod,
                        &metadata,
                    )
                {
                    return observed;
                }
                let record = config
                    .as_ref()
                    .and_then(|c| c.empack.dependencies.get(key))
                    .and_then(|entry| match entry {
                        super::config::DependencyEntry::Resolved(record) => Some(record),
                        _ => None,
                    });
                super::installed::InstalledDependency {
                    key: key.clone(),
                    identity: record.map(|r| super::installed::DependencyIdentity {
                        platform: r.platform,
                        project_id: r.project_id.clone(),
                        project_type: r.project_type,
                    }),
                    version: record.and_then(|r| r.version.clone()),
                }
            })
            .collect())
    }

    fn validate_optional_export(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<()> {
        Ok(())
    }

    fn requirements_satisfied(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<bool> {
        Ok(true)
    }
    fn apply_requirements(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<()> {
        Ok(())
    }
    fn validate_removal_target(
        &self,
        _workdir: &Path,
        _target: &super::installed::InstalledDependency,
    ) -> crate::Result<()> {
        Ok(())
    }

    fn verify_added(
        &self,
        _workdir: &Path,
        _record: &super::config::DependencyRecord,
    ) -> crate::Result<Option<String>> {
        Ok(None)
    }

    fn verify_removed(
        &self,
        _workdir: &Path,
        _target: &super::installed::InstalledDependency,
    ) -> crate::Result<()> {
        // Call-recording mock; real filesystem smoke tests verify this postcondition.
        Ok(())
    }

    fn verify_reconciled(
        &self,
        _workdir: &Path,
        _plan: &super::config::ProjectPlan,
    ) -> crate::Result<()> {
        // This mock records process calls. Live provider tests verify postconditions.
        Ok(())
    }

    fn bootstrap_jar_cache_path(&self) -> crate::Result<PathBuf> {
        Ok(self
            .current_dir
            .join("cache")
            .join("packwiz-installer-bootstrap.jar"))
    }

    fn installer_jar_cache_path(&self) -> crate::Result<PathBuf> {
        Ok(self.current_dir.join("cache").join("packwiz-installer.jar"))
    }
}

/// Write `[options]` section to an existing pack.toml file.
///
/// Reads the file, parses as TOML, merges `datapack-folder` and
/// `acceptable-game-versions` into the `[options]` table, preserves
/// all other content, and writes back. Keys use kebab-case per
/// packwiz convention.
///
/// Passing `None` for a parameter leaves any existing value for that
/// key untouched. To clear a key, remove it from the TOML manually.
#[instrument(skip_all)]
pub fn write_pack_toml_options(
    pack_toml_path: &Path,
    datapack_folder: Option<&str>,
    acceptable_game_versions: Option<&[String]>,
    fs: &dyn FileSystemProvider,
) -> Result<(), PackwizError> {
    if datapack_folder.is_none() && acceptable_game_versions.is_none() {
        return Ok(());
    }

    let content = fs
        .read_to_string(pack_toml_path)
        .map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::other(e),
        })?;

    let mut table: toml::Table = toml::from_str(&content)
        .map_err(|e| PackwizError::PackFormatError(format!("failed to parse pack.toml: {e}")))?;

    let options = table
        .entry("options")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

    let options_table = options
        .as_table_mut()
        .ok_or_else(|| PackwizError::PackFormatError("[options] is not a table".into()))?;

    if let Some(folder) = datapack_folder {
        options_table.insert(
            "datapack-folder".to_string(),
            toml::Value::String(folder.to_string()),
        );
    }

    if let Some(versions) = acceptable_game_versions {
        let arr: Vec<toml::Value> = versions
            .iter()
            .map(|v| toml::Value::String(v.clone()))
            .collect();
        options_table.insert(
            "acceptable-game-versions".to_string(),
            toml::Value::Array(arr),
        );
    }

    let output = toml::to_string(&table).map_err(|e| {
        PackwizError::PackFormatError(format!("failed to serialize pack.toml: {e}"))
    })?;

    fs.write_file(pack_toml_path, &output)
        .map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::other(e),
        })?;

    Ok(())
}

fn write_pack_toml_versions(
    pack_toml_path: &Path,
    version_entries: &[(&str, &str)],
    fs: &dyn FileSystemProvider,
) -> Result<(), PackwizError> {
    if version_entries.is_empty() {
        return Ok(());
    }

    let content = fs
        .read_to_string(pack_toml_path)
        .map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::other(e),
        })?;

    let mut table: toml::Table = toml::from_str(&content)
        .map_err(|e| PackwizError::PackFormatError(format!("failed to parse pack.toml: {e}")))?;

    let versions = table
        .entry("versions")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

    let versions_table = versions
        .as_table_mut()
        .ok_or_else(|| PackwizError::PackFormatError("[versions] is not a table".into()))?;

    for (key, value) in version_entries {
        versions_table.insert(
            (*key).to_string(),
            toml::Value::String((*value).to_string()),
        );
    }

    let output = toml::to_string(&table).map_err(|e| {
        PackwizError::PackFormatError(format!("failed to serialize pack.toml: {e}"))
    })?;

    fs.write_file(pack_toml_path, &output)
        .map_err(|e| PackwizError::ProcessFailed {
            source: std::io::Error::other(e),
        })?;

    Ok(())
}

/// Errors from packwiz operations
#[derive(Debug, Error)]
pub enum PackwizError {
    #[error("Packwiz not available: {0}")]
    NotAvailable(String),

    #[error("Command failed: {command}\n{stderr}")]
    CommandFailed { command: String, stderr: String },

    #[error("Pack format error: {0}")]
    PackFormatError(String),

    #[error("Hash mismatch: {0}")]
    HashMismatchError(String),

    #[error("Invalid path: {reason}")]
    InvalidPath { reason: String },

    #[error("Process execution failed: {source}")]
    ProcessFailed {
        #[from]
        source: std::io::Error,
    },
}

/// Packwiz CLI wrapper for metadata management (.pw.toml files)
///
/// Wraps commands: `packwiz modrinth add`, `packwiz curseforge add`,
/// `packwiz remove`, `packwiz refresh`, `packwiz modrinth export`
pub struct PackwizMetadata<'a> {
    process_provider: &'a dyn ProcessProvider,
    pack_dir: PathBuf,
    packwiz_path: Option<PathBuf>,
}

impl<'a> PackwizMetadata<'a> {
    /// Create a new PackwizMetadata instance from a session
    ///
    /// Extracts workdir from session config and constructs pack_dir path.
    /// Uses standalone construction pattern (not factory) to avoid lifetime issues.
    pub fn new(session: &'a dyn Session) -> Result<Self, PackwizError> {
        let workdir = match session.config().app_config().workdir.as_ref().cloned() {
            Some(w) => w,
            None => session
                .filesystem()
                .current_dir()
                .map_err(|e| PackwizError::InvalidPath {
                    reason: format!("Failed to get current directory: {e}"),
                })?,
        };

        let pack_dir = workdir.join("pack");
        let packwiz_path = if session.packwiz_bin() == PACKWIZ_BIN {
            None
        } else {
            Some(PathBuf::from(session.packwiz_bin()))
        };

        Ok(Self {
            process_provider: session.process(),
            pack_dir,
            packwiz_path,
        })
    }

    /// Resolve the packwiz-tx binary path (cached after first call).
    ///
    /// Resolution order:
    /// 1. `EMPACK_PACKWIZ_BIN` env var override
    /// 2. PATH lookup via `find_program(PACKWIZ_BIN)`
    /// 3. Cached/downloaded managed binary via `resolve_packwiz_binary()`
    fn ensure_packwiz(&mut self) -> Result<(), PackwizError> {
        if self.packwiz_path.is_some() {
            return Ok(());
        }

        if let Ok(env_path) = std::env::var("EMPACK_PACKWIZ_BIN") {
            let path = std::path::PathBuf::from(&env_path);
            if path.exists() {
                tracing::debug!(path = %path.display(), "using EMPACK_PACKWIZ_BIN override");
                self.packwiz_path = Some(path);
                return Ok(());
            }
        }

        if self.process_provider.find_program(PACKWIZ_BIN).is_some() {
            self.packwiz_path = Some(std::path::PathBuf::from(PACKWIZ_BIN));
            return Ok(());
        }

        match crate::platform::packwiz_bin::resolve_packwiz_binary() {
            Ok(path) => {
                self.packwiz_path = Some(path);
                Ok(())
            }
            Err(e) => Err(PackwizError::NotAvailable(format!(
                "packwiz-tx not in PATH and managed download failed: {e}"
            ))),
        }
    }

    /// Return the resolved binary path as a string slice for process execution.
    fn packwiz_bin(&self) -> &str {
        self.packwiz_path
            .as_ref()
            .and_then(|p| p.to_str())
            .unwrap_or(PACKWIZ_BIN)
    }

    /// Add a project from Modrinth
    ///
    /// Executes: `packwiz modrinth add --project-id <id> -y`
    /// Creates: pack/mods/<mod-name>.pw.toml
    /// Updates: pack/index.toml
    #[instrument(skip_all, fields(project_id, platform = ?platform))]
    pub fn add_mod(
        &mut self,
        project_id: &str,
        platform: ProjectPlatform,
    ) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let (platform_cmd, id_flag) = match platform {
            ProjectPlatform::Modrinth => ("modrinth", "--project-id"),
            ProjectPlatform::CurseForge => ("curseforge", "--addon-id"),
        };

        let output = self
            .process_provider
            .execute(
                self.packwiz_bin(),
                &[
                    "--pack-file",
                    pack_toml_str,
                    platform_cmd,
                    "add",
                    id_flag,
                    project_id,
                    "-y",
                ],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz {} add {} {}", platform_cmd, id_flag, project_id),
                stderr: output.error_output().to_string(),
            });
        }

        Ok(())
    }

    /// Remove a project by name
    ///
    /// Executes: `packwiz remove <name> -y`
    /// Deletes: pack/mods/<name>.pw.toml
    /// Updates: pack/index.toml
    pub fn remove_mod(&mut self, mod_name: &str) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                self.packwiz_bin(),
                &["--pack-file", pack_toml_str, "remove", mod_name, "-y"],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz remove {}", mod_name),
                stderr: output.error_output().to_string(),
            });
        }

        Ok(())
    }

    /// Refresh index file
    ///
    /// Executes: `packwiz refresh`
    /// Updates: pack/index.toml with latest hashes
    pub fn refresh_index(&mut self) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                self.packwiz_bin(),
                &["--pack-file", pack_toml_str, "refresh"],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            let detail = output.error_output().to_string();

            if detail.contains("Hash mismatch") {
                return Err(PackwizError::HashMismatchError(detail));
            }

            if detail.contains("pack format") && detail.contains("not supported") {
                return Err(PackwizError::PackFormatError(detail));
            }

            return Err(PackwizError::CommandFailed {
                command: "packwiz refresh".to_string(),
                stderr: output.error_output().to_string(),
            });
        }

        Ok(())
    }

    /// Export to Modrinth pack format
    ///
    /// Executes: `packwiz modrinth export -o <output>`
    /// Creates: .mrpack ZIP archive
    pub fn export_mrpack(&mut self, output_path: &Path) -> Result<(), PackwizError> {
        self.ensure_packwiz()?;

        let pack_toml = self.pack_dir.join("pack.toml");
        let pack_toml_str = pack_toml
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output_str = output_path
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "output path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                self.packwiz_bin(),
                &[
                    "--pack-file",
                    pack_toml_str,
                    "modrinth",
                    "export",
                    "-o",
                    output_str,
                ],
                &self.pack_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz modrinth export -o {}", output_path.display()),
                stderr: output.error_output().to_string(),
            });
        }

        Ok(())
    }
}

/// A CurseForge mod that packwiz-installer identified as restricted.
#[derive(Debug, Clone)]
pub struct RestrictedModInfo {
    /// Mod display name.
    pub name: String,
    /// Browser-open URL for the restricted file download.
    pub url: String,
    /// Absolute path where the file should be saved.
    pub dest_path: String,
}

pub(crate) fn restricted_destination_filename(dest_path: &str) -> Option<String> {
    Path::new(dest_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

pub(crate) fn restricted_curseforge_file_id(url: &str) -> Option<u64> {
    let path = url.split('?').next().unwrap_or(url);
    let segments: Vec<_> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    segments.windows(2).find_map(|pair| match pair {
        ["files", file_id] | ["download", file_id] => file_id.parse::<u64>().ok(),
        _ => None,
    })
}

pub(crate) fn normalize_curseforge_manual_download_url(url: &str) -> String {
    let Some(file_id) = restricted_curseforge_file_id(url) else {
        return url.to_string();
    };

    let files_segment = format!("/files/{file_id}");
    let download_segment = format!("/download/{file_id}");

    if url.contains(&download_segment) {
        return url.to_string();
    }

    if url.contains(&files_segment) {
        return url.replacen(&files_segment, &download_segment, 1);
    }

    url.to_string()
}

/// Result of running packwiz-installer.
#[derive(Debug)]
pub enum InstallResult {
    /// All mods installed successfully.
    Success,
    /// Some mods are restricted and require manual download.
    RestrictedMods(Vec<RestrictedModInfo>),
}

/// Parse packwiz-installer CLI output for restricted mod messages.
///
/// packwiz-installer CLIHandler prints:
///   stdout: `"Failed to download modpack, the following errors were encountered:\n"`
///   stdout: `"SomeMod.jar: "` (no newline)
///   stderr: `"java.lang.Exception: This mod is excluded from the CurseForge API...\nPlease go to {url} and save this file to {path}\n\tat ..."`
///
/// When empack combines stdout + stderr, the result is:
///   `"...SomeMod.jar: \njava.lang.Exception: This mod is excluded...\nPlease go to ..."`
///
/// The mod name is on the line BEFORE the "excluded" line (stdout portion).
/// The URL and path are on lines after the "excluded" line (stderr stack trace).
fn parse_installer_restricted_output(output: &str) -> Vec<RestrictedModInfo> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let lines: Vec<&str> = output.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if !line.contains("excluded from the CurseForge API") {
            continue;
        }

        // Scan ahead for the URL line in the stack trace output.
        let mut url = String::new();
        let mut dest = String::new();
        for next_line in lines.iter().skip(i + 1).take(5) {
            if let Some(rest) = next_line.strip_prefix("Please go to ")
                && let Some((u, p)) = rest.split_once(" and save this file to ")
            {
                url = u.trim().to_string();
                dest = p.trim().to_string();
                break;
            }
        }

        if !url.is_empty() {
            url = normalize_curseforge_manual_download_url(&url);
        }

        if !url.is_empty() && seen.insert((url.clone(), dest.clone())) {
            let name = lines[..i]
                .iter()
                .rev()
                .map(|line| line.trim())
                .find(|line| {
                    !line.is_empty()
                        && !line.starts_with("at ")
                        && !line.starts_with('\t')
                        && !line.starts_with("java.lang.")
                        && !line.starts_with("Please go to ")
                        && !line.starts_with("Failed to download modpack")
                })
                .map(|line| line.trim_end_matches(':').trim().to_string())
                .filter(|line| !line.is_empty())
                .or_else(|| restricted_destination_filename(&dest))
                .unwrap_or_else(|| "Unknown".to_string());

            results.push(RestrictedModInfo {
                name,
                url,
                dest_path: dest,
            });
        }
    }

    results
}

fn strip_ansi_control_sequences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                    chars.next();
                    let mut saw_escape = false;
                    for next in chars.by_ref() {
                        if saw_escape {
                            if next == '\\' {
                                break;
                            }
                            saw_escape = next == '\u{1b}';
                            continue;
                        }

                        if next == '\u{0007}' {
                            break;
                        }

                        saw_escape = next == '\u{1b}';
                    }
                }
                Some(next) if ('@'..='~').contains(&next) => {
                    chars.next();
                }
                _ => {}
            }
            continue;
        }

        result.push(ch);
    }

    result
}

fn join_reported_import_path(import_dir: &str, filename: &str) -> String {
    let import_dir = import_dir.trim();
    let filename = filename.trim();

    if import_dir.is_empty() {
        return filename.to_string();
    }

    if import_dir.ends_with(['/', '\\']) {
        return format!("{import_dir}{filename}");
    }

    let separator = match (import_dir.rfind('/'), import_dir.rfind('\\')) {
        (Some(slash), Some(backslash)) => {
            if backslash > slash {
                '\\'
            } else {
                '/'
            }
        }
        (Some(_), None) => '/',
        (None, Some(_)) => '\\',
        (None, None) => std::path::MAIN_SEPARATOR,
    };

    format!("{import_dir}{separator}{filename}")
}

pub(crate) fn parse_export_restricted_output(output: &str) -> Vec<RestrictedModInfo> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    let sanitized = strip_ansi_control_sequences(output).replace('\r', "\n");

    let Some((before_import_dir, import_suffix)) = sanitized
        .split_once("Once you have done so, place these files in ")
        .or_else(|| sanitized.split_once("place these files in "))
    else {
        return Vec::new();
    };

    let Some((import_dir, _)) = import_suffix.split_once(" and re-run this command") else {
        return Vec::new();
    };

    let Some(items_start) = before_import_dir
        .rsplit_once("must be manually downloaded:")
        .map(|(_, tail)| tail)
        .or_else(|| {
            before_import_dir
                .rsplit_once("manual downloads:")
                .map(|(_, tail)| tail)
        })
    else {
        return Vec::new();
    };

    let mut remaining = items_start.trim();
    while !remaining.is_empty() {
        let Some(from_idx) = remaining.find(") from ") else {
            break;
        };

        let name_and_filename = remaining[..=from_idx].trim();
        let Some(open_idx) = name_and_filename.rfind(" (") else {
            break;
        };
        let Some(filename) = name_and_filename[open_idx + 2..].strip_suffix(')') else {
            break;
        };

        let url_start = from_idx + ") from ".len();
        let after_from = &remaining[url_start..];
        let url_end = after_from
            .find(char::is_whitespace)
            .unwrap_or(after_from.len());
        let url = normalize_curseforge_manual_download_url(after_from[..url_end].trim());
        let name = name_and_filename[..open_idx].trim().to_string();
        let dest_path = join_reported_import_path(import_dir, filename);
        if !url.is_empty() && seen.insert((url.clone(), dest_path.clone())) {
            results.push(RestrictedModInfo {
                name,
                url,
                dest_path,
            });
        }

        remaining = after_from[url_end..].trim_start();
    }

    results
}

/// Packwiz-installer wrapper for build-time JAR downloads
///
/// Wraps: `java -jar packwiz-installer-bootstrap.jar --bootstrap-main-jar packwiz-installer.jar -g -s <side> <pack_toml_path>`
pub struct PackwizInstaller<'a> {
    filesystem: &'a dyn FileSystemProvider,
    process_provider: &'a dyn ProcessProvider,
    bootstrap_jar_path: PathBuf,
    installer_jar_path: PathBuf,
}

impl<'a> PackwizInstaller<'a> {
    /// Create a new PackwizInstaller instance
    ///
    /// Requires explicit bootstrap and installer JAR paths (caller is responsible for download/caching)
    pub fn new(
        session: &'a dyn Session,
        bootstrap_jar_path: PathBuf,
        installer_jar_path: PathBuf,
    ) -> Self {
        Self {
            filesystem: session.filesystem(),
            process_provider: session.process(),
            bootstrap_jar_path,
            installer_jar_path,
        }
    }

    /// Install projects for specified side
    ///
    /// Executes: `java -jar packwiz-installer-bootstrap.jar --bootstrap-main-jar packwiz-installer.jar -g -s <side> <pack_toml_path>`
    /// Downloads: Mod JARs from URLs in .pw.toml files
    /// Verifies: SHA-512 hashes
    /// Side: "both" (client+server), "client" (client-only), "server" (server-only)
    pub fn install_mods(
        &self,
        side: &str,
        working_dir: &Path,
    ) -> Result<InstallResult, PackwizError> {
        if !["both", "client", "server"].contains(&side) {
            return Err(PackwizError::CommandFailed {
                command: format!("install_mods({})", side),
                stderr: format!(
                    "Invalid side: {}. Must be 'both', 'client', or 'server'",
                    side
                ),
            });
        }

        let bootstrap_str =
            self.bootstrap_jar_path
                .to_str()
                .ok_or_else(|| PackwizError::InvalidPath {
                    reason: "Bootstrap JAR path contains invalid UTF-8".to_string(),
                })?;

        let installer_str =
            self.installer_jar_path
                .to_str()
                .ok_or_else(|| PackwizError::InvalidPath {
                    reason: "Installer JAR path contains invalid UTF-8".to_string(),
                })?;

        let pack_toml_path = working_dir.join("pack").join("pack.toml");
        let pack_toml_str = pack_toml_path
            .to_str()
            .ok_or_else(|| PackwizError::InvalidPath {
                reason: "pack.toml path contains invalid UTF-8".to_string(),
            })?;

        let output = self
            .process_provider
            .execute(
                "java",
                &[
                    "-jar",
                    bootstrap_str,
                    "--bootstrap-main-jar",
                    installer_str,
                    "-g",
                    "-s",
                    side,
                    pack_toml_str,
                ],
                working_dir,
            )
            .map_err(|e| PackwizError::ProcessFailed {
                source: std::io::Error::other(e),
            })?;

        if !output.success {
            let combined = format!("{}\n{}", output.stdout, output.stderr);
            let restricted = parse_installer_restricted_output(&combined);
            if !restricted.is_empty() {
                return Ok(InstallResult::RestrictedMods(restricted));
            }
            return Err(PackwizError::CommandFailed {
                command: format!("packwiz-installer-bootstrap (side={})", side),
                stderr: output.error_output().to_string(),
            });
        }

        Ok(InstallResult::Success)
    }

    /// Check if packwiz-installer-bootstrap.jar is available
    pub fn check_installer_available(&self) -> Result<bool, PackwizError> {
        Ok(self.filesystem.metadata_exists(&self.bootstrap_jar_path))
    }
}

#[cfg(test)]
mod tests {
    include!("packwiz.test.rs");
}
