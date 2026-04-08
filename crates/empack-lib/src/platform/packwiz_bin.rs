use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Semver requirement for compatible packwiz-tx releases.
pub const PACKWIZ_TX_REQUIREMENT: &str = ">=0.2.0, <0.3.0";

/// Pinned version to download when no cached binary exists.
pub const PACKWIZ_TX_VERSION: &str = "v0.2.0";

/// GitHub repository for packwiz-tx releases.
const PACKWIZ_TX_REPO: &str = "mannie-exe/packwiz-tx";
const INSTALL_LOCK_NAME: &str = ".install.lock";

/// Resolve the packwiz-tx binary path.
///
/// Resolution order:
/// 1. `EMPACK_PACKWIZ_BIN` env var (override for development/testing)
/// 2. Cached binary at `{cache_root}/bin/packwiz-tx-{version}/packwiz-tx`
///    (auto-downloaded from GitHub releases if missing)
///
/// Returns the absolute path to the binary. Errors if download fails
/// or the cached binary is not available.
pub fn resolve_packwiz_binary() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("EMPACK_PACKWIZ_BIN") {
        let path = PathBuf::from(&override_path);
        if path.exists() {
            tracing::debug!(path = %path.display(), "using EMPACK_PACKWIZ_BIN override");
            return Ok(path);
        }
        anyhow::bail!(
            "EMPACK_PACKWIZ_BIN is set to '{}' but the file does not exist",
            override_path
        );
    }

    // Tier 2: PATH lookup (user-installed or mise-managed)
    let path_bin = binary_name();
    if std::process::Command::new(&path_bin)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        tracing::debug!(binary = %path_bin, "found packwiz-tx in PATH");
        return Ok(PathBuf::from(path_bin));
    }

    // Tier 3: cached/downloaded managed binary
    let cache_dir = crate::platform::cache::cache_root()?
        .join("bin")
        .join(format!("packwiz-tx-{}", PACKWIZ_TX_VERSION));

    let bin_name = binary_name();
    let cached_bin = cache_dir.join(&bin_name);

    if cached_bin.exists() && is_executable(&cached_bin) {
        tracing::debug!(path = %cached_bin.display(), "using cached packwiz-tx binary");
        return prepare_managed_binary(&cached_bin);
    }

    let downloaded = with_install_lock(&cache_dir, || {
        if cached_bin.exists() && is_executable(&cached_bin) {
            return Ok(cached_bin.clone());
        }

        download_release(PACKWIZ_TX_VERSION, &cache_dir)
    })?;
    prepare_managed_binary(&downloaded)
}

/// Download the platform-specific packwiz-tx binary from a GitHub release.
///
/// Fetches the tarball, extracts the binary, sets executable permissions
/// on Unix, and returns the path to the cached binary.
fn download_release(version: &str, target_dir: &Path) -> Result<PathBuf> {
    let asset = release_asset_name(version);
    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        PACKWIZ_TX_REPO, version, asset
    );

    tracing::info!(url = %url, "downloading packwiz-tx");

    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create cache directory: {}", target_dir.display()))?;

    let scratch_dir = tempfile::tempdir_in(target_dir).with_context(|| {
        format!(
            "failed to create temporary download directory inside {}",
            target_dir.display()
        )
    })?;
    let output_file = scratch_dir.path().join(&asset);
    let status = std::process::Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--retry",
            "3",
            "-o",
            &output_file.to_string_lossy(),
            &url,
        ])
        .status()
        .with_context(|| format!("failed to run curl for {}", url))?;

    if !status.success() {
        anyhow::bail!("download failed: curl exited {} for {}", status, url);
    }

    let bytes = std::fs::read(&output_file)
        .with_context(|| format!("failed to read downloaded file: {}", output_file.display()))?;

    extract_tarball(&bytes, scratch_dir.path())?;

    let bin_name = binary_name();
    let extracted_bin = scratch_dir.path().join(&bin_name);

    if !extracted_bin.exists() {
        anyhow::bail!(
            "binary '{}' not found in tarball after extraction",
            bin_name
        );
    }

    #[cfg(unix)]
    set_executable(&extracted_bin)?;

    let bin_path = target_dir.join(&bin_name);
    install_binary(&extracted_bin, &bin_path)?;

    tracing::info!(path = %bin_path.display(), "packwiz-tx cached");

    Ok(bin_path)
}

fn with_install_lock<T, F>(target_dir: &Path, action: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    use std::io::ErrorKind;
    use std::time::{Duration, Instant};

    struct LockGuard {
        path: PathBuf,
    }

    impl Drop for LockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.path);
        }
    }

    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create cache directory: {}", target_dir.display()))?;

    let lock_path = target_dir.join(INSTALL_LOCK_NAME);
    let start = Instant::now();
    loop {
        match std::fs::create_dir(&lock_path) {
            Ok(()) => {
                let _guard = LockGuard {
                    path: lock_path.clone(),
                };
                return action();
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if start.elapsed() >= Duration::from_secs(30) {
                    anyhow::bail!(
                        "timed out waiting for packwiz-tx install lock at {}",
                        lock_path.display()
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to acquire packwiz-tx install lock at {}",
                        lock_path.display()
                    )
                });
            }
        }
    }
}

fn install_binary(source: &Path, target: &Path) -> Result<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            std::fs::copy(source, target).with_context(|| {
                format!(
                    "failed to install packwiz-tx from '{}' to '{}' after rename failed: {}",
                    source.display(),
                    target.display(),
                    rename_error
                )
            })?;
            #[cfg(unix)]
            set_executable(target)?;
            Ok(())
        }
    }
}

/// Verify that the managed binary is directly runnable from its cache location.
///
/// Some environments mount cache directories with `noexec`, which makes a
/// downloaded binary look valid on disk but fail with `EACCES` at spawn time.
/// In that case, stage a copy in the system temp directory and use that path.
fn prepare_managed_binary(path: &Path) -> Result<PathBuf> {
    validate_or_stage_binary_with_probe(path, probe_binary_runnable)
}

fn validate_or_stage_binary_with_probe<F>(path: &Path, probe: F) -> Result<PathBuf>
where
    F: Fn(&Path) -> Result<()>,
{
    match probe(path) {
        Ok(()) => Ok(path.to_path_buf()),
        Err(original_error) => {
            let staged = stage_binary_for_execution(path)?;
            if staged == path {
                return Err(original_error);
            }

            match probe(&staged) {
                Ok(()) => {
                    tracing::warn!(
                        original = %path.display(),
                        staged = %staged.display(),
                        error = %original_error,
                        "managed packwiz-tx was not runnable from cache; using staged copy"
                    );
                    Ok(staged)
                }
                Err(staged_error) => Err(staged_error).with_context(|| {
                    format!(
                        "managed packwiz-tx was not runnable from '{}' and staged copy '{}' also failed after initial probe error: {}",
                        path.display(),
                        staged.display(),
                        original_error
                    )
                }),
            }
        }
    }
}

fn probe_binary_runnable(path: &Path) -> Result<()> {
    std::process::Command::new(path)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|_| ())
        .with_context(|| format!("failed to execute managed packwiz-tx at {}", path.display()))
}

fn stage_binary_for_execution(path: &Path) -> Result<PathBuf> {
    let staging_dir = std::env::temp_dir()
        .join("empack-bin")
        .join(format!("packwiz-tx-{}", PACKWIZ_TX_VERSION));
    std::fs::create_dir_all(&staging_dir).with_context(|| {
        format!(
            "failed to create temporary staging directory: {}",
            staging_dir.display()
        )
    })?;

    let staged_path = staging_dir.join(binary_name());
    if staged_path != path {
        std::fs::copy(path, &staged_path).with_context(|| {
            format!(
                "failed to copy managed packwiz-tx from '{}' to '{}'",
                path.display(),
                staged_path.display()
            )
        })?;
        #[cfg(unix)]
        set_executable(&staged_path)?;
    }

    Ok(staged_path)
}

/// Extract a `.tar.gz` tarball into the target directory.
///
/// Overwrites existing files. Only extracts the `packwiz-tx` binary
/// (or `packwiz-tx.exe` on Windows) from the archive root.
fn extract_tarball(data: &[u8], target_dir: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let decoder = GzDecoder::new(data);
    let mut archive = Archive::new(decoder);

    let bin_name = binary_name();

    for entry in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path().context("failed to read entry path")?;

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name == bin_name {
            let dest = target_dir.join(&bin_name);
            entry
                .unpack(&dest)
                .with_context(|| format!("failed to extract {}", bin_name))?;
            return Ok(());
        }
    }

    anyhow::bail!("binary '{}' not found in tarball", bin_name);
}

/// Map the current platform to the goreleaser tarball asset name.
///
/// Goreleaser names assets as:
/// `packwiz-tx_{version_without_v}_{os}_{arch}.tar.gz`
fn release_asset_name(version: &str) -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        arch => arch,
    };
    format!(
        "packwiz-tx_{}_{}_{}.tar.gz",
        version.trim_start_matches('v'),
        os,
        arch
    )
}

/// Return the platform-appropriate binary filename.
fn binary_name() -> String {
    if cfg!(target_os = "windows") {
        "packwiz-tx.exe".to_string()
    } else {
        "packwiz-tx".to_string()
    }
}

/// Check whether a file has executable permission.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Check whether a file has executable permission (Windows: always true if exists).
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

/// Set the executable bit on a Unix file.
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("failed to set executable permission on {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use tempfile::TempDir;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        unsafe fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        unsafe fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match self.previous.as_ref() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn write_executable_script(path: &std::path::Path) {
        #[cfg(target_os = "windows")]
        {
            std::fs::copy(std::env::current_exe().expect("current exe"), path).expect("copy exe");
            return;
        }

        std::fs::write(path, b"#!/bin/sh\nexit 0\n").expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).expect("metadata").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("set executable");
        }
    }

    fn make_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut archive = tar::Builder::new(&mut encoder);
            for (name, contents) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_mode(0o755);
                header.set_size(contents.len() as u64);
                header.set_cksum();
                archive
                    .append_data(&mut header, *name, *contents)
                    .expect("append tar entry");
            }
            archive.finish().expect("finish tar archive");
        }
        encoder.finish().expect("finish gzip stream")
    }

    #[test]
    fn test_release_asset_name_strips_v_prefix() {
        let name = release_asset_name("v0.1.1");
        assert!(name.starts_with("packwiz-tx_0.1.1_"));
        assert!(name.ends_with(".tar.gz"));
    }

    #[test]
    fn test_release_asset_name_no_prefix() {
        let name = release_asset_name("0.1.1");
        assert!(name.starts_with("packwiz-tx_0.1.1_"));
    }

    #[test]
    fn test_binary_name_platform() {
        let name = binary_name();
        if cfg!(target_os = "windows") {
            assert_eq!(name, "packwiz-tx.exe");
        } else {
            assert_eq!(name, "packwiz-tx");
        }
    }

    #[test]
    fn validate_or_stage_binary_returns_original_when_probe_succeeds() {
        let temp = TempDir::new().expect("temp dir");
        let original = temp.path().join(binary_name());
        std::fs::write(&original, b"fake-binary").expect("write fake binary");

        let resolved =
            validate_or_stage_binary_with_probe(&original, |_| Ok(())).expect("resolution");

        assert_eq!(resolved, original);
    }

    #[test]
    fn validate_or_stage_binary_stages_copy_when_original_probe_fails() {
        let temp = TempDir::new().expect("temp dir");
        let original = temp.path().join(binary_name());
        std::fs::write(&original, b"fake-binary").expect("write fake binary");

        let original_for_probe = original.clone();
        let resolved = validate_or_stage_binary_with_probe(&original, |candidate| {
            if candidate == original_for_probe.as_path() {
                anyhow::bail!("permission denied")
            }
            Ok(())
        })
        .expect("staged resolution");

        assert_ne!(resolved, original);
        assert!(resolved.exists(), "staged copy should exist");
        assert_eq!(
            std::fs::read(&resolved).expect("read staged copy"),
            std::fs::read(&original).expect("read original"),
        );
    }

    #[test]
    fn resolve_packwiz_binary_uses_explicit_env_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let override_path = temp.path().join(binary_name());
        write_executable_script(&override_path);

        let _env = unsafe { EnvVarGuard::set("EMPACK_PACKWIZ_BIN", &override_path) };
        let resolved = resolve_packwiz_binary().expect("resolve override");

        assert_eq!(resolved, override_path);
    }

    #[test]
    fn resolve_packwiz_binary_errors_for_missing_env_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let missing = TempDir::new().expect("temp dir").path().join(binary_name());
        let _env = unsafe { EnvVarGuard::set("EMPACK_PACKWIZ_BIN", &missing) };

        let error = resolve_packwiz_binary().expect_err("missing override should fail");
        assert!(error.to_string().contains("EMPACK_PACKWIZ_BIN is set"));
        assert!(error.to_string().contains(&missing.display().to_string()));
    }

    #[test]
    fn resolve_packwiz_binary_uses_path_lookup_before_cache() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let packwiz_bin = temp.path().join(binary_name());
        write_executable_script(&packwiz_bin);
        let isolated_cache = TempDir::new().expect("cache dir");

        let _path = unsafe { EnvVarGuard::set("PATH", temp.path()) };
        let _cache = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", isolated_cache.path()) };
        let _override = unsafe { EnvVarGuard::remove("EMPACK_PACKWIZ_BIN") };

        let resolved = resolve_packwiz_binary().expect("resolve from path");
        assert_eq!(resolved, PathBuf::from(binary_name()));
    }

    #[test]
    fn resolve_packwiz_binary_uses_cached_binary_when_present() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let cache_dir = temp
            .path()
            .join("bin")
            .join(format!("packwiz-tx-{}", PACKWIZ_TX_VERSION));
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        let cached_bin = cache_dir.join(binary_name());
        write_executable_script(&cached_bin);

        let empty_path = TempDir::new().expect("empty path dir");
        let _path = unsafe { EnvVarGuard::set("PATH", empty_path.path()) };
        let _cache = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp.path()) };
        let _override = unsafe { EnvVarGuard::remove("EMPACK_PACKWIZ_BIN") };

        let resolved = resolve_packwiz_binary().expect("resolve cached binary");
        assert_eq!(resolved, cached_bin);
    }

    #[test]
    fn validate_or_stage_binary_reports_error_when_staged_copy_also_fails() {
        let temp = TempDir::new().expect("temp dir");
        let original = temp.path().join(binary_name());
        std::fs::write(&original, b"fake-binary").expect("write fake binary");

        let original_for_probe = original.clone();
        let error = validate_or_stage_binary_with_probe(&original, |candidate| {
            if candidate == original_for_probe.as_path() {
                anyhow::bail!("original probe failed")
            }
            anyhow::bail!("staged probe failed")
        })
        .expect_err("staged probe should fail");

        let message = error.to_string();
        assert!(message.contains("staged copy"));
        assert!(message.contains("original probe failed"));
    }

    #[test]
    fn probe_binary_runnable_reports_missing_binary_path() {
        let temp = TempDir::new().expect("temp dir");
        let missing = temp.path().join(binary_name());

        let error = probe_binary_runnable(&missing).expect_err("missing binary should fail");
        assert!(
            error
                .to_string()
                .contains("failed to execute managed packwiz-tx")
        );
        assert!(error.to_string().contains(&missing.display().to_string()));
    }

    #[test]
    fn extract_tarball_extracts_managed_binary() {
        let temp = TempDir::new().expect("temp dir");
        let archive = make_tarball(&[
            ("README.md", b"ignored"),
            (&binary_name(), b"#!/bin/sh\necho ok\n"),
        ]);

        extract_tarball(&archive, temp.path()).expect("extract tarball");

        let extracted = temp.path().join(binary_name());
        assert!(extracted.exists(), "managed binary should be extracted");
        assert_eq!(
            std::fs::read(&extracted).expect("read extracted binary"),
            b"#!/bin/sh\necho ok\n"
        );
    }

    #[test]
    fn extract_tarball_errors_when_binary_is_missing() {
        let temp = TempDir::new().expect("temp dir");
        let archive = make_tarball(&[("README.md", b"no binary here")]);

        let error = extract_tarball(&archive, temp.path()).expect_err("missing binary should fail");
        assert!(error.to_string().contains("binary"));
        assert!(error.to_string().contains("not found in tarball"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_packwiz_binary_downloads_and_extracts_cached_binary() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let binary = binary_name();
        let binary_contents = b"#!/bin/sh\necho downloaded\n";
        let archive = make_tarball(&[(binary.as_str(), binary_contents)]);
        let payload = temp.path().join("packwiz-tx.tar.gz");
        std::fs::write(&payload, archive).expect("write payload");

        let curl = temp.path().join("curl");
        std::fs::write(
            &curl,
            "#!/bin/sh\nout=\"\"\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then\n    shift\n    out=\"$1\"\n  fi\n  shift\ndone\n/bin/cp \"$FAKE_CURL_PAYLOAD\" \"$out\"\n",
        )
        .expect("write curl script");
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&curl).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&curl, perms).expect("set executable");

        let _path = unsafe { EnvVarGuard::set("PATH", temp.path()) };
        let _cache = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp.path()) };
        let _override = unsafe { EnvVarGuard::remove("EMPACK_PACKWIZ_BIN") };
        let _payload = unsafe { EnvVarGuard::set("FAKE_CURL_PAYLOAD", &payload) };

        let resolved = resolve_packwiz_binary().expect("download packwiz binary");
        let expected = temp
            .path()
            .join("bin")
            .join(format!("packwiz-tx-{}", PACKWIZ_TX_VERSION))
            .join(&binary);

        assert_eq!(resolved, expected);
        assert_eq!(
            std::fs::read(&resolved).expect("read extracted binary"),
            binary_contents
        );
    }
}
