use super::*;
use crate::empack::parsing::{ResourcePackResolution, ShaderLoader, ModLoader};

#[test]
fn test_resolution_parsing() {
    assert_eq!(
        ResourcePackResolution::parse("64").unwrap(),
        ResourcePackResolution::X64
    );
    assert_eq!(
        ResourcePackResolution::parse("128").unwrap(),
        ResourcePackResolution::X128
    );
    assert_eq!(
        ResourcePackResolution::parse("256").unwrap(),
        ResourcePackResolution::X256
    );
    assert!(ResourcePackResolution::parse("invalid").is_err());
}

#[test]
fn test_shader_loader_parsing() {
    assert_eq!(ShaderLoader::parse("iris").unwrap(), ShaderLoader::Iris);
    assert_eq!(ShaderLoader::parse("IRIS").unwrap(), ShaderLoader::Iris);
    assert!(ShaderLoader::parse("invalid").is_err());
}

#[test]
fn test_mod_loader_parsing() {
    assert_eq!(ModLoader::parse("fabric").unwrap(), ModLoader::Fabric);
    assert_eq!(ModLoader::parse("NEOFORGE").unwrap(), ModLoader::NeoForge);
    assert_eq!(ModLoader::parse("quilt").unwrap(), ModLoader::Quilt);
    assert!(ModLoader::parse("invalid").is_err());
}

#[test]
fn test_mod_loader_serialization() {
    // Test that ModLoader serializes to lowercase
    let fabric_yaml = serde_yaml::to_string(&ModLoader::Fabric).unwrap();
    assert_eq!(fabric_yaml.trim(), "fabric");
    
    let neoforge_yaml = serde_yaml::to_string(&ModLoader::NeoForge).unwrap();
    assert_eq!(neoforge_yaml.trim(), "neoforge");
    
    let quilt_yaml = serde_yaml::to_string(&ModLoader::Quilt).unwrap();
    assert_eq!(quilt_yaml.trim(), "quilt");
    
    let forge_yaml = serde_yaml::to_string(&ModLoader::Forge).unwrap();
    assert_eq!(forge_yaml.trim(), "forge");
    
    // Test deserialization from lowercase
    let fabric_from_yaml: ModLoader = serde_yaml::from_str("fabric").unwrap();
    assert_eq!(fabric_from_yaml, ModLoader::Fabric);
    
    let neoforge_from_yaml: ModLoader = serde_yaml::from_str("neoforge").unwrap();
    assert_eq!(neoforge_from_yaml, ModLoader::NeoForge);
}