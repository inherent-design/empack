use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Semver requirement for compatible packwiz-tx releases.
pub const PACKWIZ_TX_REQUIREMENT: &str = ">=0.2.0, <0.3.0";

/// Pinned version to download when no cached binary exists.
pub const PACKWIZ_TX_VERSION: &str = "v0.2.0";

/// GitHub repository for packwiz-tx releases.
const PACKWIZ_TX_REPO: &str = "mannie-exe/packwiz-tx";

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
    if std::process::Command::new(crate::empack::packwiz::PACKWIZ_BIN)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        tracing::debug!("found {} in PATH", crate::empack::packwiz::PACKWIZ_BIN);
        return Ok(PathBuf::from(crate::empack::packwiz::PACKWIZ_BIN));
    }

    // Tier 3: cached/downloaded managed binary
    let cache_dir = crate::platform::cache::cache_root()?
        .join("bin")
        .join(format!("packwiz-tx-{}", PACKWIZ_TX_VERSION));

    let bin_name = binary_name();
    let cached_bin = cache_dir.join(&bin_name);

    if cached_bin.exists() && is_executable(&cached_bin) {
        tracing::debug!(path = %cached_bin.display(), "using cached packwiz-tx binary");
        return Ok(cached_bin);
    }

    download_release(PACKWIZ_TX_VERSION, &cache_dir)
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

    let output_file = target_dir.join(&asset);
    let status = std::process::Command::new("curl")
        .args([
            "--proto", "=https",
            "--tlsv1.2",
            "-fSL",
            "--retry", "3",
            "-o", &output_file.to_string_lossy(),
            &url,
        ])
        .status()
        .with_context(|| format!("failed to run curl for {}", url))?;

    if !status.success() {
        anyhow::bail!("download failed: curl exited {} for {}", status, url);
    }

    let bytes = std::fs::read(&output_file)
        .with_context(|| format!("failed to read downloaded file: {}", output_file.display()))?;
    let _ = std::fs::remove_file(&output_file);

    extract_tarball(&bytes, target_dir)?;

    let bin_name = binary_name();
    let bin_path = target_dir.join(&bin_name);

    if !bin_path.exists() {
        anyhow::bail!(
            "binary '{}' not found in tarball after extraction",
            bin_name
        );
    }

    #[cfg(unix)]
    set_executable(&bin_path)?;

    tracing::info!(path = %bin_path.display(), "packwiz-tx cached");

    Ok(bin_path)
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

    for entry in archive.entries().context("failed to read tarball entries")? {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path().context("failed to read entry path")?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

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
}
