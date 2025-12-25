use super::*;

#[cfg(feature = "test-utils")]
use crate::application::session_mocks::{MockFileSystemProvider, MockNetworkProvider};

#[tokio::test]
#[cfg(feature = "test-utils")]
async fn test_version_fetcher_creation() {
    let network = MockNetworkProvider::new();
    let filesystem = MockFileSystemProvider::new();
    let fetcher = VersionFetcher::new(&network, &filesystem).unwrap();

    // Verify cache directory contains "empack" (platform-specific path format)
    // Linux: ~/.cache/empack
    // macOS: ~/Library/Caches/inherent.design.empack
    // Windows: %LOCALAPPDATA%\inherent.design\empack\cache
    assert!(
        fetcher.cache_dir.to_string_lossy().contains("empack"),
        "Cache directory should contain 'empack': {:?}",
        fetcher.cache_dir
    );
}

#[test]
fn test_cached_versions_expiry() {
    let versions = vec!["1.21.4".to_string(), "1.21.1".to_string()];
    let cached = CachedVersions::new(versions);

    // Should not be expired immediately
    assert!(!cached.is_expired(1));

    // Create an old cached version
    let old_cached = CachedVersions {
        versions: vec!["1.20.1".to_string()],
        cached_at: 0, // Unix epoch
    };

    // Should be expired
    assert!(old_cached.is_expired(1));
}

#[test]
fn test_fallback_versions() {
    let mc_versions = VersionFetcher::get_fallback_minecraft_versions();
    assert!(!mc_versions.is_empty());
    assert!(mc_versions.contains(&"1.21.4".to_string()));

    let fabric_versions = VersionFetcher::get_fallback_loader_versions("fabric", "1.21.4");
    assert!(!fabric_versions.is_empty());
    assert!(fabric_versions.contains(&"0.15.0".to_string()));
}

#[test]
fn test_modloader_enum() {
    assert_eq!(ModLoader::NeoForge.as_str(), "neoforge");
    assert_eq!(ModLoader::Fabric.as_str(), "fabric");
    assert_eq!(ModLoader::Forge.as_str(), "forge");
    assert_eq!(ModLoader::Quilt.as_str(), "quilt");
}

#[test]
fn test_version_compare() {
    // Test equal versions
    assert_eq!(version_compare("1.20.1", "1.20.1"), 0);

    // Test less than
    assert_eq!(version_compare("1.20.1", "1.20.2"), -1);
    assert_eq!(version_compare("1.19.4", "1.20.1"), -1);

    // Test greater than
    assert_eq!(version_compare("1.20.2", "1.20.1"), 1);
    assert_eq!(version_compare("1.21.1", "1.20.1"), 1);

    // Test different length versions
    assert_eq!(version_compare("1.20", "1.20.1"), -1);
    assert_eq!(version_compare("1.20.1", "1.20"), 1);
}
