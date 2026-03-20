use semver::Version;
use serde_json;

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
fn test_parse_version() {
    assert_eq!(parse_version("1.20"), Some(Version::new(1, 20, 0)));
    assert_eq!(parse_version("1.20.4"), Some(Version::new(1, 20, 4)));
    assert_eq!(parse_version("not-a-version"), None);
    assert!(parse_version("1.20.1") < parse_version("1.20.2"));
}

#[test]
fn test_sort_versions_desc_multi_digit() {
    let mut versions = vec![
        "21.1.7".to_string(),
        "21.1.69".to_string(),
        "21.1.8".to_string(),
    ];

    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["21.1.69", "21.1.8", "21.1.7"]);
}

#[test]
fn test_sort_versions_desc_prerelease() {
    let mut versions = vec![
        "21.1.67-beta".to_string(),
        "21.1.67".to_string(),
        "21.1.69".to_string(),
    ];

    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["21.1.69", "21.1.67", "21.1.67-beta"]);
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
    // Sample maven-metadata.xml versions (realistic subset from actual API)
    let all_versions = vec![
        // MC 1.20.1 versions (multiple)
        "1.20.1-47.4.13".to_string(),
        "1.20.1-47.4.12".to_string(),
        "1.20.1-47.4.11".to_string(),
        "1.20.1-47.4.10".to_string(),
        "1.20.1-47.4.9".to_string(),
        // MC 1.16.5 versions (multiple)
        "1.16.5-36.2.42".to_string(),
        "1.16.5-36.2.41".to_string(),
        "1.16.5-36.2.40".to_string(),
        "1.16.5-36.2.34".to_string(),
        // MC 1.21.1 versions
        "1.21.1-52.1.8".to_string(),
        "1.21.1-52.1.7".to_string(),
        "1.21.1-52.1.0".to_string(),
        // MC 1.21 versions (no .0 suffix in some)
        "1.21-51.0.33".to_string(),
        "1.21-51.0.32".to_string(),
        "1.21-51.0.31".to_string(),
        // MC 1.8 versions (legacy)
        "1.8-11.14.4.1577".to_string(),
        "1.8-11.14.4.1563".to_string(),
        "1.8-11.14.4.1562".to_string(),
        // MC 1.16.4 versions (larger set)
        "1.16.4-35.1.37".to_string(),
        "1.16.4-35.1.36".to_string(),
        "1.16.4-35.1.35".to_string(),
        "1.16.4-35.0.1".to_string(),
        "1.16.4-35.0.0".to_string(),
    ];

    // Test filtering for MC 1.20.1 (should get all 5 versions)
    let filtered_20_1 = filter_forge_versions_by_minecraft(&all_versions, "1.20.1").unwrap();
    assert_eq!(filtered_20_1.len(), 5, "Should find all 5 versions for MC 1.20.1");
    assert!(filtered_20_1.contains(&"47.4.13".to_string()), "Should include latest");
    assert!(filtered_20_1.contains(&"47.4.10".to_string()), "Should include recommended");
    // Verify newest first (47.4.13 > 47.4.10)
    assert_eq!(filtered_20_1[0], "47.4.13", "Newest version should be first");
    assert_eq!(filtered_20_1[4], "47.4.9", "Oldest version should be last");

    // Test filtering for MC 1.16.5
    let filtered_16_5 = filter_forge_versions_by_minecraft(&all_versions, "1.16.5").unwrap();
    assert_eq!(filtered_16_5.len(), 4, "Should find 4 versions for MC 1.16.5");
    assert!(filtered_16_5.contains(&"36.2.42".to_string()));
    assert!(filtered_16_5.contains(&"36.2.34".to_string()));
    // Verify newest first
    assert_eq!(filtered_16_5[0], "36.2.42");

    // Test filtering for MC 1.21 (no .0 suffix in maven-metadata)
    let filtered_21 = filter_forge_versions_by_minecraft(&all_versions, "1.21").unwrap();
    assert_eq!(filtered_21.len(), 3, "Should find 3 versions for MC 1.21");
    assert!(filtered_21.contains(&"51.0.33".to_string()));
    assert_eq!(filtered_21[0], "51.0.33", "Newest first");

    // Test filtering for MC 1.21.1
    let filtered_21_1 = filter_forge_versions_by_minecraft(&all_versions, "1.21.1").unwrap();
    assert_eq!(filtered_21_1.len(), 3, "Should find 3 versions for MC 1.21.1");
    assert_eq!(filtered_21_1[0], "52.1.8", "Newest first");
    assert_eq!(filtered_21_1[2], "52.1.0", "Oldest last");

    // Test filtering for MC 1.8 (legacy versions)
    let filtered_8 = filter_forge_versions_by_minecraft(&all_versions, "1.8").unwrap();
    assert_eq!(filtered_8.len(), 3, "Should find 3 versions for MC 1.8");
    assert!(filtered_8.contains(&"11.14.4.1577".to_string()));
    assert!(filtered_8.contains(&"11.14.4.1563".to_string()));
    assert_eq!(filtered_8[0], "11.14.4.1577", "Newest first");

    // Test filtering for MC 1.16.4 (larger set - 5 versions)
    let filtered_16_4 = filter_forge_versions_by_minecraft(&all_versions, "1.16.4").unwrap();
    assert_eq!(filtered_16_4.len(), 5, "Should find 5 versions for MC 1.16.4");
    assert_eq!(filtered_16_4[0], "35.1.37", "Newest first");
    assert_eq!(filtered_16_4[4], "35.0.0", "Oldest last");

    // Test unsupported MC version (not in maven-metadata)
    let filtered_unsupported = filter_forge_versions_by_minecraft(&all_versions, "1.20.5").unwrap();
    assert_eq!(filtered_unsupported.len(), 0, "Should return empty for MC version not in maven-metadata");

    // Test deduplication (all_versions shouldn't have duplicates, but verify)
    let dedup_versions = vec![
        "1.19-41.1.0".to_string(),
        "1.19-41.1.0".to_string(), // Duplicate
    ];
    let filtered_dedup = filter_forge_versions_by_minecraft(&dedup_versions, "1.19").unwrap();
    assert_eq!(filtered_dedup.len(), 1, "Should deduplicate duplicate version entries");
    assert_eq!(filtered_dedup[0], "41.1.0");

    // Test version normalization (MC "1.21" should also match "1.21.0-" prefix)
    let norm_versions = vec![
        "1.21.0-51.0.33".to_string(),
        "1.21.0-51.0.32".to_string(),
    ];
    let filtered_norm = filter_forge_versions_by_minecraft(&norm_versions, "1.21").unwrap();
    assert_eq!(filtered_norm.len(), 2, "Should normalize MC 1.21 to also match 1.21.0- prefix");
    assert!(filtered_norm.contains(&"51.0.33".to_string()));
    assert!(filtered_norm.contains(&"51.0.32".to_string()));
}

#[test]
fn test_parse_forge_maven_metadata() {
    // Sample XML structure (minimal, realistic subset)
    let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.minecraftforge</groupId>
  <artifactId>forge</artifactId>
  <versioning>
    <latest>1.21.11-61.0.3</latest>
    <release>1.21.11-61.0.3</release>
    <versions>
      <version>1.21.11-61.0.3</version>
      <version>1.21.11-61.0.2</version>
      <version>1.21.11-61.0.1</version>
      <version>1.21.11-61.0.0</version>
      <version>1.20.1-47.4.13</version>
      <version>1.20.1-47.4.10</version>
    </versions>
    <lastUpdated>20251215014219</lastUpdated>
  </versioning>
</metadata>"#;

    let versions = parse_forge_maven_metadata(xml_content).unwrap();
    assert_eq!(versions.len(), 6, "Should parse 6 versions from XML");
    assert!(versions.contains(&"1.21.11-61.0.3".to_string()));
    assert!(versions.contains(&"1.20.1-47.4.13".to_string()));
    assert!(versions.contains(&"1.21.11-61.0.0".to_string()));
}

// NOTE: HTTP 400/404 handling for modloaders is tested via integration tests
// When API returns error for unsupported MC version:
// - Fabric HTTP 400 → fetch_fabric_loader_versions() returns Ok(vec![])
// - Quilt HTTP 404 → fetch_quilt_loader_versions() returns Ok(vec![])
// - NeoForge MC < 1.20.2 → returns Ok(vec![])
// - Forge unknown version → returns Ok(vec![])
// - Empty loader list is handled gracefully (no panic)
//
// Integration test: test_init_empty_loader_list_graceful_handling
// Location: crates/empack-tests/tests/init_error_recovery.rs
//
// Manual test: empack init with very old MC version (e.g., 1.7.10)
// Expected: Incompatible loaders not shown in selection dialog

#[test]
fn test_minecraft_versions_order_descending() {
    let manifest_json = r#"{
        "latest": {"release": "1.21.4", "snapshot": "25w14a"},
        "versions": [
            {"id": "1.7.10", "type": "release", "url": "https://example.com/1.7.10"},
            {"id": "1.8", "type": "release", "url": "https://example.com/1.8"},
            {"id": "1.20.1", "type": "release", "url": "https://example.com/1.20.1"},
            {"id": "1.21", "type": "release", "url": "https://example.com/1.21"},
            {"id": "1.21.1", "type": "release", "url": "https://example.com/1.21.1"},
            {"id": "1.21.4", "type": "release", "url": "https://example.com/1.21.4"},
            {"id": "25w14a", "type": "snapshot", "url": "https://example.com/25w14a"}
        ]
    }"#;

    let manifest: MinecraftVersionManifest = serde_json::from_str(manifest_json).unwrap();

    let mut versions: Vec<String> = manifest
        .versions
        .into_iter()
        .filter(|v| v.version_type == "release")
        .map(|v| v.id)
        .collect();

    sort_versions_desc(&mut versions);

    assert_eq!(
        versions[0], "1.21.4",
        "Last (newest) item becomes first after reverse"
    );
    assert_eq!(
        versions[versions.len() - 1], "1.7.10",
        "First (oldest) item becomes last after reverse"
    );

    for i in 0..versions.len() - 1 {
        let current = &versions[i];
        let next = &versions[i + 1];
        assert!(
            parse_version(current).unwrap() >= parse_version(next).unwrap(),
            "Version {} at index {} should be >= next version {} at index {} (not descending)",
            current, i, next, i + 1
        );
    }
}

#[test]
fn test_neoforge_versions_preserve_api_order_descending() {
    let all_versions = vec![
        "21.1.69".to_string(),
        "21.1.68".to_string(),
        "21.1.67-beta".to_string(),
        "21.0.167".to_string(),
        "21.0.166".to_string(),
        "21.0.165-beta".to_string(),
    ];

    let filtered = filter_neoforge_versions_by_minecraft(&all_versions, "1.21").unwrap();

    assert_eq!(filtered.len(), 2, "Should find 2 stable versions");
    assert_eq!(filtered[0], "21.0.167", "Newest NeoForge version should be first");
    assert_eq!(filtered[1], "21.0.166", "Second newest should be second");

    for i in 0..filtered.len() - 1 {
        let current = &filtered[i];
        let next = &filtered[i + 1];
        assert!(
            parse_version(current).unwrap() >= parse_version(next).unwrap(),
            "NeoForge version {} at index {} should be >= next version {} (not descending)",
            current, i, next
        );
    }
}

#[test]
fn test_fabric_versions_order_descending() {
    let api_versions = vec![
        FabricLoaderVersion {
            loader: FabricLoaderInfo {
                version: "0.15.0".to_string(),
                stable: true,
            },
        },
        FabricLoaderVersion {
            loader: FabricLoaderInfo {
                version: "0.15.1".to_string(),
                stable: true,
            },
        },
        FabricLoaderVersion {
            loader: FabricLoaderInfo {
                version: "0.16.0-beta".to_string(),
                stable: false,
            },
        },
        FabricLoaderVersion {
            loader: FabricLoaderInfo {
                version: "0.16.1-beta".to_string(),
                stable: false,
            },
        },
    ];

    let mut stable_versions: Vec<String> = api_versions
        .iter()
        .filter(|v| v.loader.stable)
        .map(|v| v.loader.version.clone())
        .collect();

    let mut beta_versions: Vec<String> = api_versions
        .iter()
        .filter(|v| !v.loader.stable)
        .map(|v| v.loader.version.clone())
        .collect();

    sort_versions_desc(&mut stable_versions);
    sort_versions_desc(&mut beta_versions);

    let stable_count = stable_versions.len();
    stable_versions.append(&mut beta_versions);

    assert_eq!(
        stable_versions[0], "0.15.1",
        "Newest stable Fabric version should be first"
    );
    assert_eq!(
        stable_versions[1], "0.15.0",
        "Second newest stable should be second"
    );
    assert_eq!(
        stable_versions[stable_count], "0.16.1-beta",
        "Beta versions start after stable versions"
    );

    let beta_start = stable_count;
    for i in 0..stable_versions.len() - 1 {
        let current = &stable_versions[i];
        let next = &stable_versions[i + 1];
        let crossing_boundary = (i < beta_start && i + 1 >= beta_start)
            || (i >= beta_start && i + 1 < beta_start);
        if crossing_boundary {
            continue;
        }
        assert!(
            parse_version(current).unwrap() >= parse_version(next).unwrap(),
            "Fabric version {} at index {} should be >= next version {} (not descending)",
            current, i, next
        );
    }
}
