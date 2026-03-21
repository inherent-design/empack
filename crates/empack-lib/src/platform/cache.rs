use std::path::PathBuf;

use anyhow::Result;
use directories::ProjectDirs;

/// Returns the platform-appropriate cache root for empack.
///
/// Resolution order:
/// 1. `EMPACK_CACHE_DIR` env var (for testing and custom deployments)
/// 2. `ProjectDirs` for platform-standard cache location
/// 3. Temp directory fallback when ProjectDirs is unavailable
pub fn cache_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("EMPACK_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }

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
