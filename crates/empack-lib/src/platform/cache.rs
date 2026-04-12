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

pub fn bin_cache_dir() -> Result<PathBuf> {
    Ok(cache_root()?.join("bin"))
}

pub fn jar_cache_dir() -> Result<PathBuf> {
    Ok(cache_root()?.join("jars"))
}

pub fn restricted_builds_cache_dir() -> Result<PathBuf> {
    Ok(cache_root()?.join("restricted-builds"))
}

pub fn versions_cache_dir() -> Result<PathBuf> {
    Ok(cache_root()?.join("versions"))
}

pub fn legacy_versions_cache_file(filename: &str) -> Result<PathBuf> {
    Ok(cache_root()?.join(filename))
}

pub fn http_cache_dir() -> Result<PathBuf> {
    Ok(cache_root()?.join("http"))
}

pub fn staged_bin_dir() -> PathBuf {
    std::env::temp_dir().join("empack-bin")
}

pub fn legacy_http_cache_dir() -> PathBuf {
    std::env::temp_dir().join("empack").join("http_cache")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

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
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn cache_root_uses_env_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp_dir.path()) };

        let cache_root = cache_root().expect("cache root");
        assert_eq!(cache_root, temp_dir.path());
    }

    #[test]
    fn cache_root_ignores_empty_override() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", "") };

        let cache_root = cache_root().expect("cache root");
        assert!(!cache_root.as_os_str().is_empty());
    }

    #[test]
    fn typed_cache_dirs_are_derived_from_cache_root() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let _cache_dir = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp_dir.path()) };

        assert_eq!(
            bin_cache_dir().expect("bin cache"),
            temp_dir.path().join("bin")
        );
        assert_eq!(
            jar_cache_dir().expect("jar cache"),
            temp_dir.path().join("jars")
        );
        assert_eq!(
            restricted_builds_cache_dir().expect("restricted cache"),
            temp_dir.path().join("restricted-builds")
        );
        assert_eq!(
            versions_cache_dir().expect("versions cache"),
            temp_dir.path().join("versions")
        );
        assert_eq!(
            http_cache_dir().expect("http cache"),
            temp_dir.path().join("http")
        );
        assert_eq!(
            legacy_versions_cache_file("minecraft_versions.json").expect("legacy file"),
            temp_dir.path().join("minecraft_versions.json")
        );
    }
}
