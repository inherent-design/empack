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