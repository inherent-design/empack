use std::path::PathBuf;

use anyhow::Result;
use directories::ProjectDirs;

/// Returns the platform-appropriate cache root for empack.
///
/// Primary: Uses `ProjectDirs` for platform-standard cache location.
/// Fallback: Uses temp directory with a warning when ProjectDirs is unavailable
/// (e.g., in containers without home directories).
pub fn cache_root() -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("design", "inherent", "empack") {
        Ok(proj_dirs.cache_dir().to_path_buf())
    } else {
        let fallback = std::env::temp_dir().join("empack-cache");
        tracing::warn!(
            "ProjectDirs unavailable, falling back to temp directory for cache: {}",
            fallback.display()
        );
        Ok(fallback)
    }
}
