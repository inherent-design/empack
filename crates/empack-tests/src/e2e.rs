use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the empack binary path.
///
/// Checks in order: `EMPACK_E2E_BIN` env var, llvm-cov instrumented
/// binary, debug build, release build, bare PATH.
pub fn empack_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("EMPACK_E2E_BIN") {
        return PathBuf::from(bin);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_root = manifest.join("../../target");
    let exe = if cfg!(windows) { ".exe" } else { "" };

    for cov_dir in &["llvm-cov-target/debug", "llvm-cov-target/release"] {
        let candidate = target_root.join(format!("{cov_dir}/empack{exe}"));
        if candidate.exists() {
            return candidate;
        }
    }

    for profile in &["debug", "release"] {
        let candidate = target_root.join(format!("{profile}/empack{exe}"));
        if candidate.exists() {
            return candidate;
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

    std::fs::write(&path, script).unwrap_or_else(|e| {
        panic!("failed to write fake packwiz at {}: {}", path.display(), e)
    });

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

/// Initialize an empack project non-interactively.
///
/// Panics if the init command fails.
pub fn init_project(parent: &Path, name: &str, loader: &str, mc_version: &str) -> PathBuf {
    let status = empack_cmd(parent)
        .args([
            "init",
            "--yes",
            "--modloader",
            loader,
            "--mc-version",
            mc_version,
            name,
        ])
        .status()
        .expect("failed to spawn empack init");
    assert!(status.success(), "empack init exited with {}", status);
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
