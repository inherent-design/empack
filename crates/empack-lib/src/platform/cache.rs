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
    if let Ok(dir) = std::env::var("EMPACK_CACHE_DIR")
        && !dir.is_empty()
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_uses_env_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        unsafe {
            std::env::set_var("EMPACK_CACHE_DIR", temp_dir.path());
        }

        let cache_root = cache_root().expect("cache root");
        assert_eq!(cache_root, temp_dir.path());

        unsafe {
            std::env::remove_var("EMPACK_CACHE_DIR");
        }
    }

    #[test]
    fn cache_root_ignores_empty_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        unsafe {
            std::env::set_var("EMPACK_CACHE_DIR", "");
        }

        let cache_root = cache_root().expect("cache root");
        assert!(!cache_root.as_os_str().is_empty());

        unsafe {
            std::env::remove_var("EMPACK_CACHE_DIR");
        }
    }
}
