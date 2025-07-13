use super::*;
use crate::testing::MockApiServer;

#[tokio::test]
async fn test_minecraft_version_fetching() {
    let mock_server = MockApiServer::new().await;
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    
    let latest = resolver.get_latest_minecraft_version().await;
    assert!(latest.is_ok());
    assert_eq!(latest.unwrap(), "1.21.6");
    
    let all_versions = resolver.get_all_minecraft_versions().await;
    assert!(all_versions.is_ok());
    let versions = all_versions.unwrap();
    assert!(!versions.is_empty());
    assert!(versions.contains(&"1.21.6".to_string()));
    assert!(versions.contains(&"1.21.1".to_string()));
}

#[tokio::test]
async fn test_stabilize_core_input_complete_config() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test with complete configuration
    let result = resolver.stabilize_core_input(
        Some(ModLoader::NeoForge),
        Some("1.21.1".to_string()),
        Some("21.1.186".to_string()),
    ).await;
    
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.modloader, ModLoader::NeoForge);
    assert_eq!(resolved.minecraft_version, "1.21.1");
    assert_eq!(resolved.modloader_version, Some("21.1.186".to_string()));
    assert!(resolved.compatibility_validated);
}

#[tokio::test]
async fn test_stabilize_core_input_auto_fill() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test auto-fill with only modloader specified
    let result = resolver.stabilize_core_input(
        Some(ModLoader::NeoForge),
        None,
        None,
    ).await;
    
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.modloader, ModLoader::NeoForge);
    assert!(!resolved.minecraft_version.is_empty());
    assert!(resolved.modloader_version.is_some());
    assert!(resolved.compatibility_validated);
}

#[tokio::test]
async fn test_stabilize_core_input_zero_config() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test zero-config mode (no inputs provided)
    let result = resolver.stabilize_core_input(None, None, None).await;
    
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.modloader, ModLoader::NeoForge); // Default
    assert!(!resolved.minecraft_version.is_empty());
    assert!(resolved.modloader_version.is_some());
    assert!(resolved.compatibility_validated);
}

#[tokio::test]
async fn test_neoforge_minecraft_version_mapping() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test V1's NeoForge version → Minecraft mapping heuristics
    let mc_21 = resolver.get_compatible_modloader_version_for_minecraft(
        ModLoader::NeoForge,
        "1.21.1"
    ).await;
    assert!(mc_21.is_ok());
    
    let mc_20 = resolver.get_compatible_modloader_version_for_minecraft(
        ModLoader::NeoForge,
        "1.20.1"
    ).await;
    assert!(mc_20.is_ok());
}

#[tokio::test]
async fn test_fabric_quilt_compatibility() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test Fabric compatibility (should work with any MC version)
    let fabric_result = resolver.get_compatible_modloader_version_for_minecraft(
        ModLoader::Fabric,
        "1.21.1"
    ).await;
    assert!(fabric_result.is_ok());
    
    // Test Quilt compatibility (should work with any MC version)
    let quilt_result = resolver.get_compatible_modloader_version_for_minecraft(
        ModLoader::Quilt,
        "1.21.1"
    ).await;
    assert!(quilt_result.is_ok());
}

#[tokio::test]
async fn test_vanilla_handling() {
    let mock_server = MockApiServer::new().await;
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    
    // Test vanilla modloader handling
    let result = resolver.stabilize_core_input(
        Some(ModLoader::Vanilla),
        Some("1.21.1".to_string()),
        None,
    ).await;
    
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.modloader, ModLoader::Vanilla);
    assert_eq!(resolved.minecraft_version, "1.21.1");
    assert!(resolved.modloader_version.is_none()); // Vanilla has no modloader version
}

#[tokio::test]
async fn test_minecraft_version_override() {
    let resolver = VersionResolver::new().await.unwrap();
    
    // Test providing custom Minecraft version with NeoForge
    let result = resolver.stabilize_core_input(
        Some(ModLoader::NeoForge),
        Some("1.20.1".to_string()),
        None,
    ).await;
    
    assert!(result.is_ok());
    let resolved = result.unwrap();
    assert_eq!(resolved.modloader, ModLoader::NeoForge);
    assert_eq!(resolved.minecraft_version, "1.20.1");
    assert!(resolved.modloader_version.is_some());
    
    // Should auto-select compatible NeoForge version for MC 1.20.1
    let neoforge_version = resolved.modloader_version.unwrap();
    assert!(!neoforge_version.is_empty());
}

#[tokio::test]
async fn test_all_modloader_defaults() {
    let mock_server = MockApiServer::new().await;
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    
    // Test all modloaders can get defaults
    for modloader in [ModLoader::NeoForge, ModLoader::Fabric, ModLoader::Quilt, ModLoader::Vanilla] {
        let result = resolver.get_recommended_defaults(Some(modloader)).await;
        assert!(result.is_ok(), "Failed to get defaults for {:?}", modloader);
        
        let resolved = result.unwrap();
        assert_eq!(resolved.modloader, modloader);
        assert!(!resolved.minecraft_version.is_empty());
        
        match modloader {
            ModLoader::Vanilla => assert!(resolved.modloader_version.is_none()),
            _ => assert!(resolved.modloader_version.is_some()),
        }
    }
}

#[tokio::test]
async fn test_fabric_version_fetching() {
    let mock_server = MockApiServer::new().await;
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    
    let latest = resolver.get_latest_fabric_version().await;
    assert!(latest.is_ok());
    assert_eq!(latest.unwrap(), "0.16.14");
    
    let stable = resolver.get_stable_fabric_version().await;
    assert!(stable.is_ok());
    assert_eq!(stable.unwrap(), "0.16.14");
}

#[tokio::test]
async fn test_recommended_defaults() {
    let mock_server = MockApiServer::new().await;
    let resolver = VersionResolver::new_with_mock_server(mock_server.url());
    
    let defaults = resolver.get_recommended_defaults(Some(ModLoader::NeoForge)).await;
    assert!(defaults.is_ok());
    
    let resolved = defaults.unwrap();
    assert_eq!(resolved.modloader, ModLoader::NeoForge);
    assert!(resolved.modloader_version.is_some());
    assert!(!resolved.minecraft_version.is_empty());
}

#[tokio::test]
async fn test_maven_version_parsing() {
    let resolver = VersionResolver::new().await.unwrap();
    
    let xml = r#"
<metadata>
  <versioning>
    <latest>21.1.14</latest>
    <release>21.1.14</release>
    <versions>
      <version>21.1.0</version>
      <version>21.1.1</version>
      <version>21.1.14</version>
      <version>21.2.0-beta1</version>
    </versions>
  </versioning>
</metadata>
        "#;
    
    let versions = resolver.parse_maven_versions(xml, "NeoForge").unwrap();
    assert!(!versions.is_empty());
    assert!(versions.iter().any(|v| v.version == "21.1.14"));
    assert!(versions.iter().any(|v| v.is_stable));
    assert!(versions.iter().any(|v| !v.is_stable));
}
