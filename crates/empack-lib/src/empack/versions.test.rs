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

#[test]
fn test_filter_neoforge_versions() {
    // Sample NeoForge versions (realistic subset from actual API)
    let all_versions = vec![
        "21.1.69".to_string(),
        "21.1.68".to_string(),
        "21.1.67-beta".to_string(),
        "21.0.167".to_string(),
        "21.0.166".to_string(),
        "21.0.165-beta".to_string(),
        "20.4.167".to_string(),
        "20.4.166".to_string(),
        "20.4.165-beta".to_string(),
        "20.2.93".to_string(),
        "20.2.92".to_string(),
        "20.2.91-beta".to_string(),
    ];

    // Test filtering for MC 1.21
    let filtered_21 = filter_neoforge_versions_by_minecraft(&all_versions, "1.21").unwrap();
    assert_eq!(filtered_21.len(), 2, "Should find 2 stable versions for MC 1.21");
    assert!(filtered_21.contains(&"21.0.167".to_string()));
    assert!(filtered_21.contains(&"21.0.166".to_string()));
    assert!(!filtered_21.contains(&"21.0.165-beta".to_string()), "Should exclude beta versions when stable exist");

    // Test filtering for MC 1.21.1
    let filtered_21_1 = filter_neoforge_versions_by_minecraft(&all_versions, "1.21.1").unwrap();
    assert_eq!(filtered_21_1.len(), 2, "Should find 2 stable versions for MC 1.21.1");
    assert!(filtered_21_1.contains(&"21.1.69".to_string()));
    assert!(filtered_21_1.contains(&"21.1.68".to_string()));
    assert!(!filtered_21_1.contains(&"21.1.67-beta".to_string()), "Should exclude beta versions when stable exist");

    // Test filtering for MC 1.20.4
    let filtered_20_4 = filter_neoforge_versions_by_minecraft(&all_versions, "1.20.4").unwrap();
    assert_eq!(filtered_20_4.len(), 2, "Should find 2 stable versions for MC 1.20.4");
    assert!(filtered_20_4.contains(&"20.4.167".to_string()));
    assert!(filtered_20_4.contains(&"20.4.166".to_string()));

    // Test filtering for MC 1.20.2
    let filtered_20_2 = filter_neoforge_versions_by_minecraft(&all_versions, "1.20.2").unwrap();
    assert_eq!(filtered_20_2.len(), 2, "Should find 2 stable versions for MC 1.20.2");
    assert!(filtered_20_2.contains(&"20.2.93".to_string()));
    assert!(filtered_20_2.contains(&"20.2.92".to_string()));

    // Test unsupported MC version (too old)
    let filtered_old = filter_neoforge_versions_by_minecraft(&all_versions, "1.19.4").unwrap();
    assert_eq!(filtered_old.len(), 0, "Should return empty for MC < 1.20.2");

    // Test MC 1.20.1 (NeoForge doesn't support - no 20.1.x versions exist)
    let filtered_20_1 = filter_neoforge_versions_by_minecraft(&all_versions, "1.20.1").unwrap();
    assert_eq!(filtered_20_1.len(), 0, "Should return empty for MC 1.20.1 (no 20.1.x versions exist)");

    // Test beta-only scenario
    let beta_only_versions = vec![
        "21.10.64-beta".to_string(),
        "21.10.63-beta".to_string(),
    ];
    let filtered_beta = filter_neoforge_versions_by_minecraft(&beta_only_versions, "1.21.10").unwrap();
    assert_eq!(filtered_beta.len(), 2, "Should include beta versions if no stable exist");
    assert!(filtered_beta.contains(&"21.10.64-beta".to_string()));

    // Test dynamic algorithm handles new MC versions not hardcoded
    let new_versions = vec![
        "21.15.5".to_string(),
        "21.15.4".to_string(),
        "21.15.3-beta".to_string(),
    ];
    let filtered_new = filter_neoforge_versions_by_minecraft(&new_versions, "1.21.15").unwrap();
    assert_eq!(filtered_new.len(), 2, "Should dynamically handle new MC versions");
    assert!(filtered_new.contains(&"21.15.5".to_string()));
    assert!(filtered_new.contains(&"21.15.4".to_string()));
    assert!(!filtered_new.contains(&"21.15.3-beta".to_string()), "Should exclude beta when stable exist");
}

#[test]
fn test_filter_forge_versions() {
    use std::collections::HashMap;

    // Sample promotions (realistic subset from actual promotions_slim.json)
    let mut promotions = HashMap::new();

    // MC 1.20.1 has both latest and recommended (different versions)
    promotions.insert("1.20.1-latest".to_string(), "47.4.13".to_string());
    promotions.insert("1.20.1-recommended".to_string(), "47.4.10".to_string());

    // MC 1.16.5 has both latest and recommended (different versions)
    promotions.insert("1.16.5-latest".to_string(), "36.2.42".to_string());
    promotions.insert("1.16.5-recommended".to_string(), "36.2.34".to_string());

    // MC 1.21.1 has both latest and recommended (different versions)
    promotions.insert("1.21.1-latest".to_string(), "52.1.8".to_string());
    promotions.insert("1.21.1-recommended".to_string(), "52.1.0".to_string());

    // MC 1.21 has only latest (no .0 suffix)
    promotions.insert("1.21-latest".to_string(), "51.0.33".to_string());

    // MC 1.8 has both latest and recommended (same version)
    promotions.insert("1.8-latest".to_string(), "11.14.4.1577".to_string());
    promotions.insert("1.8-recommended".to_string(), "11.14.4.1563".to_string());

    // Test filtering for MC 1.20.1
    let filtered_20_1 = filter_forge_versions_by_minecraft(&promotions, "1.20.1").unwrap();
    assert_eq!(filtered_20_1.len(), 2, "Should find 2 versions for MC 1.20.1 (latest and recommended)");
    assert!(filtered_20_1.contains(&"47.4.13".to_string()), "Should include latest");
    assert!(filtered_20_1.contains(&"47.4.10".to_string()), "Should include recommended");
    // Verify newest first (47.4.13 > 47.4.10)
    assert_eq!(filtered_20_1[0], "47.4.13", "Newest version should be first");
    assert_eq!(filtered_20_1[1], "47.4.10", "Older version should be second");

    // Test filtering for MC 1.16.5
    let filtered_16_5 = filter_forge_versions_by_minecraft(&promotions, "1.16.5").unwrap();
    assert_eq!(filtered_16_5.len(), 2, "Should find 2 versions for MC 1.16.5");
    assert!(filtered_16_5.contains(&"36.2.42".to_string()));
    assert!(filtered_16_5.contains(&"36.2.34".to_string()));
    // Verify newest first
    assert_eq!(filtered_16_5[0], "36.2.42");

    // Test filtering for MC 1.21 (no .0 suffix in promotions)
    let filtered_21 = filter_forge_versions_by_minecraft(&promotions, "1.21").unwrap();
    assert_eq!(filtered_21.len(), 1, "Should find 1 version for MC 1.21 (only latest exists)");
    assert!(filtered_21.contains(&"51.0.33".to_string()));

    // Test filtering for MC 1.21.1
    let filtered_21_1 = filter_forge_versions_by_minecraft(&promotions, "1.21.1").unwrap();
    assert_eq!(filtered_21_1.len(), 2, "Should find 2 versions for MC 1.21.1");
    assert_eq!(filtered_21_1[0], "52.1.8", "Newest first");
    assert_eq!(filtered_21_1[1], "52.1.0", "Older second");

    // Test filtering for MC 1.8 (different latest and recommended)
    let filtered_8 = filter_forge_versions_by_minecraft(&promotions, "1.8").unwrap();
    assert_eq!(filtered_8.len(), 2, "Should find 2 versions for MC 1.8");
    assert!(filtered_8.contains(&"11.14.4.1577".to_string()));
    assert!(filtered_8.contains(&"11.14.4.1563".to_string()));

    // Test unsupported MC version (not in promotions)
    let filtered_unsupported = filter_forge_versions_by_minecraft(&promotions, "1.20.5").unwrap();
    assert_eq!(filtered_unsupported.len(), 0, "Should return empty for MC version not in promotions");

    // Test deduplication (when latest and recommended are same)
    let mut dedup_promotions = HashMap::new();
    dedup_promotions.insert("1.19-latest".to_string(), "41.1.0".to_string());
    dedup_promotions.insert("1.19-recommended".to_string(), "41.1.0".to_string());

    let filtered_dedup = filter_forge_versions_by_minecraft(&dedup_promotions, "1.19").unwrap();
    assert_eq!(filtered_dedup.len(), 1, "Should deduplicate when latest and recommended are same");
    assert_eq!(filtered_dedup[0], "41.1.0");

    // Test version normalization (MC "1.21" should also check "1.21.0")
    let mut norm_promotions = HashMap::new();
    norm_promotions.insert("1.21.0-latest".to_string(), "51.0.33".to_string());

    let filtered_norm = filter_forge_versions_by_minecraft(&norm_promotions, "1.21").unwrap();
    assert_eq!(filtered_norm.len(), 1, "Should normalize MC 1.21 to 1.21.0");
    assert!(filtered_norm.contains(&"51.0.33".to_string()));
}
