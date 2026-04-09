use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fixtures::WorkflowProjectFixture;

/// Resolve the empack binary path.
///
/// Checks in order:
/// - `EMPACK_E2E_BIN`
/// - debug/release builds for normal local runs
/// - llvm-cov instrumented builds only when coverage is active
/// - bare PATH fallback
pub fn empack_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("EMPACK_E2E_BIN") {
        return PathBuf::from(bin);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_root = manifest.join("../../target");
    let exe = if cfg!(windows) { ".exe" } else { "" };

    let coverage_active = std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some();

    for profile in &["debug", "release"] {
        let candidate = target_root.join(format!("{profile}/empack{exe}"));
        if candidate.exists() {
            return candidate;
        }
    }

    if coverage_active {
        for cov_dir in &["llvm-cov-target/debug", "llvm-cov-target/release"] {
            let candidate = target_root.join(format!("{cov_dir}/empack{exe}"));
            if candidate.exists() {
                return candidate;
            }
        }
    }

    PathBuf::from(format!("empack{exe}"))
}

pub fn has_packwiz() -> bool {
    empack_lib::platform::packwiz_bin::resolve_packwiz_binary().is_ok()
}

pub fn has_java() -> bool {
    Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

pub fn has_cf_key() -> bool {
    std::env::var("EMPACK_KEY_CURSEFORGE").is_ok()
}

#[cfg(windows)]
fn set_windows_env_if_missing(cmd: &mut Command, key: &str, fallback: &Path) {
    if let Some(value) = std::env::var_os(key) {
        cmd.env(key, value);
    } else {
        cmd.env(key, fallback);
    }
}

fn configure_command_env(cmd: &mut Command, workdir: &Path) {
    cmd.env("NO_COLOR", "1");
    let empack_cache_dir = workdir.join(".empack-cache");
    std::fs::create_dir_all(&empack_cache_dir).expect("create EMPACK_CACHE_DIR fallback");
    cmd.env("EMPACK_CACHE_DIR", &empack_cache_dir);

    #[cfg(windows)]
    {
        let local_app_data = workdir.join(".windows-localappdata");
        let roaming_app_data = workdir.join(".windows-appdata");
        let user_profile = workdir.join(".windows-userprofile");
        let temp_dir = workdir.join(".windows-temp");

        std::fs::create_dir_all(&local_app_data).expect("create LOCALAPPDATA fallback");
        std::fs::create_dir_all(&roaming_app_data).expect("create APPDATA fallback");
        std::fs::create_dir_all(&user_profile).expect("create USERPROFILE fallback");
        std::fs::create_dir_all(&temp_dir).expect("create TEMP fallback");

        set_windows_env_if_missing(cmd, "LOCALAPPDATA", &local_app_data);
        set_windows_env_if_missing(cmd, "LocalAppData", &local_app_data);
        set_windows_env_if_missing(cmd, "APPDATA", &roaming_app_data);
        set_windows_env_if_missing(cmd, "USERPROFILE", &user_profile);
        set_windows_env_if_missing(cmd, "TEMP", &temp_dir);
        set_windows_env_if_missing(cmd, "TMP", &temp_dir);
    }

    #[cfg(not(windows))]
    let _ = workdir;
}

pub fn configure_fake_packwiz(cmd: &mut Command, workdir: &Path) {
    let path = write_fake_packwiz_binary(workdir);
    cmd.env("EMPACK_PACKWIZ_BIN", path);
}

fn write_fake_packwiz_binary(workdir: &Path) -> PathBuf {
    #[cfg(windows)]
    let path = workdir.join("fake-packwiz.cmd");
    #[cfg(not(windows))]
    let path = workdir.join("fake-packwiz");

    #[cfg(windows)]
    let script = r#"@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "NAME="
set "AUTHOR="
set "VERSION="
set "MC="
set "LOADER="
set "LOADER_VERSION="

:loop
if "%~1"=="" goto done
if "%~1"=="--name" (
  set "NAME=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--author" (
  set "AUTHOR=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--version" (
  set "VERSION=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--mc-version" (
  set "MC=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--modloader" (
  set "LOADER=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--fabric-version" (
  set "LOADER_VERSION=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--forge-version" (
  set "LOADER_VERSION=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--neoforge-version" (
  set "LOADER_VERSION=%~2"
  shift
  shift
  goto loop
)
if "%~1"=="--quilt-version" (
  set "LOADER_VERSION=%~2"
  shift
  shift
  goto loop
)
shift
goto loop

:done
> pack.toml (
  echo name = "!NAME!"
  echo author = "!AUTHOR!"
  echo version = "!VERSION!"
  echo pack-format = "packwiz:1.1.0"
  echo.
  echo [index]
  echo file = "index.toml"
  echo hash-format = "sha256"
  echo hash = ""
  echo.
  echo [versions]
  echo minecraft = "!MC!"
)
if not "!LOADER!"=="" if not "!LOADER!"=="none" if not "!LOADER_VERSION!"=="" (
  >> pack.toml echo !LOADER! = "!LOADER_VERSION!"
)
type nul > index.toml
exit /b 0
"#;

    #[cfg(not(windows))]
    let script = r#"#!/bin/sh
set -eu
NAME=""
AUTHOR=""
VERSION=""
MC=""
LOADER=""
LOADER_VERSION=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --name)
      NAME="$2"
      shift 2
      ;;
    --author)
      AUTHOR="$2"
      shift 2
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    --mc-version)
      MC="$2"
      shift 2
      ;;
    --modloader)
      LOADER="$2"
      shift 2
      ;;
    --fabric-version|--forge-version|--neoforge-version|--quilt-version)
      LOADER_VERSION="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

cat > pack.toml <<EOF
name = "$NAME"
author = "$AUTHOR"
version = "$VERSION"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "$MC"
EOF

if [ -n "$LOADER" ] && [ "$LOADER" != "none" ] && [ -n "$LOADER_VERSION" ]; then
  printf '%s = "%s"\n' "$LOADER" "$LOADER_VERSION" >> pack.toml
fi

: > index.toml
"#;

    std::fs::write(&path, script)
        .unwrap_or_else(|e| panic!("failed to write fake packwiz at {}: {}", path.display(), e));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .expect("fake packwiz metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("set fake packwiz executable");
    }

    path
}

/// Return early from a test when packwiz is not in PATH.
#[macro_export]
macro_rules! skip_if_no_packwiz {
    () => {
        if !$crate::e2e::has_packwiz() {
            eprintln!("SKIP: packwiz not in PATH");
            return;
        }
    };
}

/// Return early from a test when Java is not in PATH.
#[macro_export]
macro_rules! skip_if_no_java {
    () => {
        $crate::skip_if_no_packwiz!();
        if !$crate::e2e::has_java() {
            eprintln!("SKIP: java not in PATH");
            return;
        }
    };
}

/// Return early from a test when the CurseForge API key is not set.
#[macro_export]
macro_rules! skip_if_no_cf_key {
    () => {
        $crate::skip_if_no_packwiz!();
        if !$crate::e2e::has_cf_key() {
            eprintln!("SKIP: EMPACK_KEY_CURSEFORGE not set");
            return;
        }
    };
}

/// Isolated test project backed by a temporary directory.
///
/// The TempDir is held for the lifetime of this struct; dropping it
/// cleans up all files. Use `dir()` for the working directory and
/// `cmd()` for a pre-configured empack Command.
pub struct TestProject {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Default for TestProject {
    fn default() -> Self {
        Self::new()
    }
}

impl TestProject {
    /// Create a new empty test project directory.
    pub fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let root = tmp.path().to_path_buf();
        Self { _tmp: tmp, root }
    }

    /// Create a test project with an initialized empack pack.
    pub fn initialized(name: &str, loader: &str, mc_version: &str) -> Self {
        let project = Self::new();
        init_project(&project.root, name, loader, mc_version);
        Self {
            root: project.root.join(name),
            _tmp: project._tmp,
        }
    }

    /// Create a test project from the shared workflow fixture without live init.
    pub fn workflow_fixture(name: &str, loader: &str, mc_version: &str) -> Self {
        let project = Self::new();
        let mut fixture = WorkflowProjectFixture::new(name.to_string());
        fixture.loader = loader.to_string();
        fixture.minecraft_version = mc_version.to_string();
        fixture
            .write_to(&project.root)
            .unwrap_or_else(|e| panic!("failed to write workflow fixture: {e}"));
        project
    }

    /// Working directory for this project.
    pub fn dir(&self) -> &Path {
        &self.root
    }

    /// Build an empack Command pre-configured with NO_COLOR and the
    /// project working directory.
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(empack_bin());
        cmd.current_dir(&self.root);
        configure_command_env(&mut cmd, &self.root);
        cmd
    }

    pub fn run_output_with_retry(&self, args: &[&str]) -> Output {
        run_empack_output_with_retry(&self.root, args)
    }

    /// Assert a file relative to the project root contains the expected string.
    pub fn assert_contains(&self, relative: &str, expected: &str) {
        assert_file_contains(&self.root.join(relative), expected);
    }

    /// Assert a file relative to the project root exists.
    pub fn assert_exists(&self, relative: &str) {
        assert_file_exists(&self.root.join(relative));
    }
}

/// Build an assert_cmd Command from the resolved empack binary.
///
/// Prefers the llvm-cov instrumented binary when available so E2E
/// tests contribute to coverage reports.
pub fn empack_assert_cmd() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(empack_bin());
    cmd.env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        let workdir = std::env::current_dir().expect("current dir");
        let local_app_data = workdir.join(".windows-localappdata");
        let roaming_app_data = workdir.join(".windows-appdata");
        let user_profile = workdir.join(".windows-userprofile");
        let temp_dir = workdir.join(".windows-temp");

        std::fs::create_dir_all(&local_app_data).expect("create LOCALAPPDATA fallback");
        std::fs::create_dir_all(&roaming_app_data).expect("create APPDATA fallback");
        std::fs::create_dir_all(&user_profile).expect("create USERPROFILE fallback");
        std::fs::create_dir_all(&temp_dir).expect("create TEMP fallback");

        if let Some(value) = std::env::var_os("LOCALAPPDATA") {
            cmd.env("LOCALAPPDATA", value);
        } else {
            cmd.env("LOCALAPPDATA", &local_app_data);
        }
        if let Some(value) = std::env::var_os("LocalAppData") {
            cmd.env("LocalAppData", value);
        } else {
            cmd.env("LocalAppData", &local_app_data);
        }
        if let Some(value) = std::env::var_os("APPDATA") {
            cmd.env("APPDATA", value);
        } else {
            cmd.env("APPDATA", &roaming_app_data);
        }
        if let Some(value) = std::env::var_os("USERPROFILE") {
            cmd.env("USERPROFILE", value);
        } else {
            cmd.env("USERPROFILE", &user_profile);
        }
        if let Some(value) = std::env::var_os("TEMP") {
            cmd.env("TEMP", value);
        } else {
            cmd.env("TEMP", &temp_dir);
        }
        if let Some(value) = std::env::var_os("TMP") {
            cmd.env("TMP", value);
        } else {
            cmd.env("TMP", &temp_dir);
        }
    }
    cmd
}

/// Build an empack Command pointed at a specific directory with NO_COLOR.
pub fn empack_cmd(workdir: &Path) -> Command {
    let mut cmd = Command::new(empack_bin());
    cmd.current_dir(workdir);
    configure_command_env(&mut cmd, workdir);
    cmd
}

fn format_output_for_debug(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn is_transient_external_timeout(output: &Output) -> bool {
    if output.status.success() {
        return false;
    }

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();

    let looks_like_timeout = combined.contains("i/o timeout")
        || combined.contains("tls handshake timeout")
        || combined.contains("operation timed out")
        || combined.contains("timed out");
    let looks_like_external_fetch = combined.contains("error loading versions")
        || combined.contains("dial tcp")
        || combined.contains("maven.neoforged.net")
        || combined.contains("maven.minecraftforge.net")
        || combined.contains("meta.fabricmc.net")
        || combined.contains("quiltmc")
        || combined.contains("modrinth")
        || combined.contains("curseforge");

    looks_like_timeout && looks_like_external_fetch
}

fn run_empack_output_with_retry(workdir: &Path, args: &[&str]) -> Output {
    const MAX_ATTEMPTS: usize = 2;

    for attempt in 1..=MAX_ATTEMPTS {
        let output = empack_cmd(workdir)
            .args(args)
            .output()
            .expect("failed to spawn empack command");

        if attempt == MAX_ATTEMPTS || !is_transient_external_timeout(&output) {
            return output;
        }

        eprintln!(
            "retrying transient empack external timeout (attempt {attempt}/{MAX_ATTEMPTS})\n{}",
            format_output_for_debug(&output)
        );
    }

    unreachable!("retry loop should always return an output")
}

/// Initialize an empack project non-interactively.
///
/// Panics if the init command fails.
pub fn init_project(parent: &Path, name: &str, loader: &str, mc_version: &str) -> PathBuf {
    let output = run_empack_output_with_retry(
        parent,
        &[
            "init",
            "--yes",
            "--modloader",
            loader,
            "--mc-version",
            mc_version,
            name,
        ],
    );
    assert!(
        output.status.success(),
        "empack init failed\n{}",
        format_output_for_debug(&output)
    );
    parent.join(name)
}

/// Assert a file exists and contains the expected substring.
pub fn assert_file_contains(path: &Path, expected: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
    assert!(
        content.contains(expected),
        "{} does not contain '{}'\ncontent:\n{}",
        path.display(),
        expected,
        content
    );
}

/// Assert a file exists at the given path.
pub fn assert_file_exists(path: &Path) {
    assert!(path.exists(), "expected file at {}", path.display());
}

/// Assert the basic project files created by init/import exist.
pub fn assert_project_initialized(project_root: &Path) {
    assert_file_exists(&project_root.join("empack.yml"));
    assert_file_exists(&project_root.join("pack").join("pack.toml"));
}

/// Read and parse `empack.yml` into the typed config model.
pub fn read_empack_config(project_root: &Path) -> empack_lib::empack::config::EmpackConfig {
    let path = project_root.join("empack.yml");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
    serde_saphyr::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e))
}

/// Read and parse `pack/pack.toml` into a TOML value.
pub fn read_pack_toml(project_root: &Path) -> toml::Value {
    let path = project_root.join("pack").join("pack.toml");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("failed to read {}", path.display()));
    toml::from_str::<toml::Value>(&content)
        .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e))
}

/// Assert the typed project config recorded the expected loader family.
pub fn assert_project_loader(project_root: &Path, expected_loader: &str) {
    let config = read_empack_config(project_root);
    let actual = config.empack.loader.as_ref().map(|loader| loader.as_str());
    assert_eq!(
        actual,
        Some(expected_loader),
        "empack.yml loader mismatch for {}",
        project_root.display()
    );
}

/// Assert the typed project config recorded the expected Minecraft version.
pub fn assert_project_minecraft_version(project_root: &Path, expected_version: &str) {
    let config = read_empack_config(project_root);
    assert_eq!(
        config.empack.minecraft_version.as_deref(),
        Some(expected_version),
        "empack.yml minecraft_version mismatch for {}",
        project_root.display()
    );
}

/// Assert the typed project config does not record a loader family.
pub fn assert_project_loader_absent(project_root: &Path) {
    let config = read_empack_config(project_root);
    assert_eq!(
        config.empack.loader.as_ref().map(|loader| loader.as_str()),
        Option::<&str>::None,
        "empack.yml should not record a loader for {}",
        project_root.display()
    );
}

/// Assert the typed project config recorded the expected datapack folder.
pub fn assert_project_datapack_folder(project_root: &Path, expected_folder: &str) {
    let config = read_empack_config(project_root);
    assert_eq!(
        config.empack.datapack_folder.as_deref(),
        Some(expected_folder),
        "empack.yml datapack_folder mismatch for {}",
        project_root.display()
    );
}

/// Assert `pack/pack.toml` contains the expected loader version entry.
pub fn assert_pack_loader_version(project_root: &Path, loader: &str, expected_version: &str) {
    let pack_toml = read_pack_toml(project_root);
    let versions = pack_toml
        .get("versions")
        .and_then(|value| value.as_table())
        .unwrap_or_else(|| {
            panic!(
                "pack.toml missing [versions] for {}",
                project_root.display()
            )
        });

    assert_eq!(
        versions.get(loader).and_then(|value| value.as_str()),
        Some(expected_version),
        "pack.toml {loader} version mismatch for {}",
        project_root.display()
    );
}

/// Assert `pack/pack.toml` contains the expected Minecraft version entry.
pub fn assert_pack_minecraft_version(project_root: &Path, expected_version: &str) {
    let pack_toml = read_pack_toml(project_root);
    let versions = pack_toml
        .get("versions")
        .and_then(|value| value.as_table())
        .unwrap_or_else(|| {
            panic!(
                "pack.toml missing [versions] for {}",
                project_root.display()
            )
        });

    assert_eq!(
        versions.get("minecraft").and_then(|value| value.as_str()),
        Some(expected_version),
        "pack.toml minecraft version mismatch for {}",
        project_root.display()
    );
}

/// Assert `pack/pack.toml` contains a loader version entry with the expected prefix.
pub fn assert_pack_loader_version_prefix(project_root: &Path, loader: &str, expected_prefix: &str) {
    let pack_toml = read_pack_toml(project_root);
    let versions = pack_toml
        .get("versions")
        .and_then(|value| value.as_table())
        .unwrap_or_else(|| {
            panic!(
                "pack.toml missing [versions] for {}",
                project_root.display()
            )
        });

    let actual = versions.get(loader).and_then(|value| value.as_str());
    assert!(
        actual.is_some_and(|value| value.starts_with(expected_prefix)),
        "pack.toml {loader} version should start with {expected_prefix:?} for {} but was {:?}",
        project_root.display(),
        actual
    );
}

/// Assert `pack/pack.toml [options]` contains the expected string value.
pub fn assert_pack_option_string(project_root: &Path, key: &str, expected_value: &str) {
    let pack_toml = read_pack_toml(project_root);
    let options = pack_toml
        .get("options")
        .and_then(|value| value.as_table())
        .unwrap_or_else(|| panic!("pack.toml missing [options] for {}", project_root.display()));

    assert_eq!(
        options.get(key).and_then(|value| value.as_str()),
        Some(expected_value),
        "pack.toml [options] {key} mismatch for {}",
        project_root.display()
    );
}

/// Assert a dist artifact with the given suffix exists and return its path.
pub fn assert_dist_artifact_suffix(project_root: &Path, suffix: &str) -> PathBuf {
    let dist_dir = project_root.join("dist");
    let entries = std::fs::read_dir(&dist_dir)
        .unwrap_or_else(|_| panic!("failed to read {}", dist_dir.display()));

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            return path;
        }
    }

    panic!(
        "expected dist artifact ending with '{}' under {}",
        suffix,
        dist_dir.display()
    );
}

/// Load pending restricted-build state through the library helper.
pub fn load_pending_restricted_build(
    project_root: &Path,
) -> anyhow::Result<Option<empack_lib::empack::restricted_build::PendingRestrictedBuild>> {
    let filesystem = empack_lib::application::session::LiveFileSystemProvider;
    empack_lib::empack::restricted_build::load_pending_build(&filesystem, project_root)
}

/// Assert pending restricted-build state exists and matches the expected targets and filenames.
pub fn assert_pending_restricted_build(
    project_root: &Path,
    expected_targets: &[&str],
    expected_filenames: &[&str],
) -> empack_lib::empack::restricted_build::PendingRestrictedBuild {
    let pending = load_pending_restricted_build(project_root)
        .unwrap_or_else(|e| panic!("failed to load pending restricted build: {e}"))
        .unwrap_or_else(|| {
            panic!(
                "expected pending restricted build under {}",
                project_root.display()
            )
        });

    let expected_targets: Vec<String> = expected_targets
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    assert_eq!(
        pending.targets,
        expected_targets,
        "pending restricted targets mismatch for {}",
        project_root.display()
    );

    let mut actual_filenames: Vec<String> = pending
        .entries
        .iter()
        .map(|entry| entry.filename.clone())
        .collect();
    actual_filenames.sort();

    let mut expected_filenames: Vec<String> = expected_filenames
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    expected_filenames.sort();

    assert_eq!(
        actual_filenames,
        expected_filenames,
        "pending restricted filenames mismatch for {}",
        project_root.display()
    );

    pending
}

/// Seed the packwiz installer bootstrap/runtime jars expected by full-build E2E.
pub fn seed_packwiz_installer_jars(project_root: &Path) {
    let jars_dir = project_root.join(".empack-cache").join("jars");
    std::fs::create_dir_all(&jars_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {}", jars_dir.display(), e));

    std::fs::write(
        jars_dir.join("packwiz-installer-bootstrap.jar"),
        b"bootstrap",
    )
    .unwrap_or_else(|e| {
        panic!(
            "failed to seed bootstrap jar in {}: {}",
            jars_dir.display(),
            e
        )
    });
    std::fs::write(jars_dir.join("packwiz-installer.jar"), b"installer").unwrap_or_else(|e| {
        panic!(
            "failed to seed installer jar in {}: {}",
            jars_dir.display(),
            e
        )
    });
}

/// Seed the loader-version cache used by version fetcher in subprocess E2E tests.
pub fn seed_loader_version_cache(
    project_root: &Path,
    loader: &str,
    mc_version: &str,
    versions: &[&str],
) {
    let cache_dir = project_root.join(".empack-cache");
    std::fs::create_dir_all(&cache_dir)
        .unwrap_or_else(|e| panic!("failed to create {}: {}", cache_dir.display(), e));

    let cached_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = cache_dir.join(format!("{loader}_loader_{mc_version}.json"));
    let content = serde_json::json!({
        "versions": versions,
        "cached_at": cached_at,
    });

    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&content).expect("cache json"),
    )
    .unwrap_or_else(|e| panic!("failed to write {}: {}", path.display(), e));
}

/// Count `.pw.toml` files recursively under `pack_root/mods`.
///
/// Returns 0 if the directory does not exist.
pub fn count_pw_toml_files(pack_root: &Path) -> usize {
    let mods_dir = pack_root.join("mods");
    std::fs::read_dir(mods_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
                .count()
        })
        .unwrap_or(0)
}

/// Create a minimal Modrinth mrpack archive for local import tests.
pub fn write_local_mrpack(
    archive_path: &Path,
    pack_name: &str,
    version_id: &str,
    minecraft_version: &str,
    loader_id: &str,
    loader_version: &str,
) -> anyhow::Result<()> {
    use empack_lib::empack::archive::{ArchiveFormat, create_archive};

    let source_dir = tempfile::TempDir::new()?;
    let mut dependencies = serde_json::Map::new();
    dependencies.insert(
        "minecraft".to_string(),
        serde_json::Value::String(minecraft_version.to_string()),
    );
    dependencies.insert(
        loader_id.to_string(),
        serde_json::Value::String(loader_version.to_string()),
    );

    let manifest = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "name": pack_name,
        "versionId": version_id,
        "summary": "Local test fixture",
        "files": [],
        "dependencies": dependencies,
    });

    std::fs::write(
        source_dir.path().join("modrinth.index.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    create_archive(source_dir.path(), archive_path, ArchiveFormat::Zip)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
