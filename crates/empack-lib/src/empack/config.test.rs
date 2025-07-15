use super::*;
use crate::empack::parsing::ModLoader;
use crate::primitives::ProjectType;
use crate::application::session_mocks::MockFileSystemProvider;
use crate::application::session::FileSystemProvider;
use std::collections::HashMap;
use std::path::PathBuf;

// Helper to create mock filesystem provider with test setup
fn create_mock_config_provider(workdir: PathBuf) -> MockFileSystemProvider {
    MockFileSystemProvider::new().with_current_dir(workdir)
}

fn with_empack_yml(provider: MockFileSystemProvider, workdir: &PathBuf, content: &str) -> MockFileSystemProvider {
    provider.with_file(workdir.join("empack.yml"), content.to_string())
}

fn with_pack_toml(provider: MockFileSystemProvider, workdir: &PathBuf, content: &str) -> MockFileSystemProvider {
    provider.with_file(workdir.join("pack").join("pack.toml"), content.to_string())
}

#[test]
fn test_load_empack_config_success() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
    - "sodium: \"Sodium|mod\""
  minecraft_version: "1.21"
  loader: fabric
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_empack_config();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.empack.dependencies.len(), 2);
    assert_eq!(config.empack.minecraft_version, Some("1.21".to_string()));
    assert_eq!(config.empack.loader, Some(ModLoader::Fabric));
    assert_eq!(config.empack.name, Some("Test Pack".to_string()));
    assert_eq!(config.empack.author, Some("Test Author".to_string()));
    assert_eq!(config.empack.version, Some("1.0.0".to_string()));
}

#[test]
fn test_load_empack_config_missing_file() {
    let workdir = PathBuf::from("/test/config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_empack_config();

    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::MissingField { field } => {
            assert_eq!(field, "empack.yml");
        }
        _ => panic!("Expected MissingField error"),
    }
}

#[test]
fn test_load_empack_config_invalid_yaml() {
    let workdir = PathBuf::from("/test/config");
    let invalid_yaml = r#"
empack:
  dependencies:
    - invalid yaml: [ unclosed bracket
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, invalid_yaml);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_empack_config();

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ConfigError::YamlError { .. }));
}

#[test]
fn test_load_pack_metadata_success() {
    let workdir = PathBuf::from("/test/config");
    let pack_content = r#"
name = "Test Modpack"
author = "Test Author"
version = "1.2.3"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_pack_metadata();

    assert!(result.is_ok());
    let metadata = result.unwrap();
    assert!(metadata.is_some());
    let metadata = metadata.unwrap();
    assert_eq!(metadata.name, "Test Modpack");
    assert_eq!(metadata.author, Some("Test Author".to_string()));
    assert_eq!(metadata.version, Some("1.2.3".to_string()));
    assert_eq!(metadata.versions.minecraft, "1.20.1");
    assert_eq!(metadata.versions.loader_versions.get("fabric"), Some(&"0.14.21".to_string()));
}

#[test]
fn test_load_pack_metadata_missing_file() {
    let workdir = PathBuf::from("/test/config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_pack_metadata();

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_load_pack_metadata_invalid_toml() {
    let workdir = PathBuf::from("/test/config");
    let invalid_toml = r#"
name = "Test"
invalid toml: [ unclosed bracket
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_pack_toml(provider, &workdir, invalid_toml);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_pack_metadata();

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ConfigError::TomlError { .. }));
}

#[test]
fn test_create_project_plan_empack_only() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
    - "sodium: \"Sodium|mod\""
  minecraft_version: "1.21"
  loader: fabric
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.name, "Test Pack");
    assert_eq!(plan.author, Some("Test Author".to_string()));
    assert_eq!(plan.version, Some("1.0.0".to_string()));
    assert_eq!(plan.minecraft_version, "1.21");
    assert_eq!(plan.loader, ModLoader::Fabric);
    assert_eq!(plan.dependencies.len(), 2);
}

#[test]
fn test_create_project_plan_pack_fallback() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
"#;
    let pack_content = r#"
name = "Fallback Pack"
author = "Fallback Author"
version = "2.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.name, "Fallback Pack");
    assert_eq!(plan.author, Some("Fallback Author".to_string()));
    assert_eq!(plan.version, Some("2.0.0".to_string()));
    assert_eq!(plan.minecraft_version, "1.20.1");
    assert_eq!(plan.loader, ModLoader::Fabric);
    assert_eq!(plan.dependencies.len(), 1);
}

#[test]
fn test_create_project_plan_empack_precedence() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
  name: "Empack Pack"
  author: "Empack Author"
  version: "3.0.0"
"#;
    let pack_content = r#"
name = "Pack Name"
author = "Pack Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    // empack.yml should take precedence
    assert_eq!(plan.name, "Empack Pack");
    assert_eq!(plan.author, Some("Empack Author".to_string()));
    assert_eq!(plan.version, Some("3.0.0".to_string()));
    assert_eq!(plan.minecraft_version, "1.21");
    assert_eq!(plan.loader, ModLoader::Fabric);
}

#[test]
fn test_create_project_plan_missing_minecraft_version() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::MissingField { field } => {
            assert!(field.contains("minecraft_version"));
        }
        _ => panic!("Expected MissingField error"),
    }
}

#[test]
fn test_create_project_plan_missing_loader() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::MissingField { field } => {
            assert!(field.contains("loader"));
        }
        _ => panic!("Expected MissingField error"),
    }
}

#[test]
fn test_infer_loader_from_metadata_fabric() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.loader, ModLoader::Fabric);
    assert_eq!(plan.loader_version, "0.14.21");
}

#[test]
fn test_infer_loader_from_metadata_forge() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "jei: \"Just Enough Items|mod\""
  minecraft_version: "1.21"
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
forge = "47.1.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.loader, ModLoader::Forge);
    assert_eq!(plan.loader_version, "47.1.0");
}

#[test]
fn test_infer_loader_from_metadata_quilt() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "quilted_fabric_api: \"Quilted Fabric API|mod\""
  minecraft_version: "1.21"
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
quilt = "0.21.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.loader, ModLoader::Quilt);
    assert_eq!(plan.loader_version, "0.21.0");
}

#[test]
fn test_infer_loader_from_metadata_neoforge() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "jei: \"Just Enough Items|mod\""
  minecraft_version: "1.21"
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
neoforge = "21.0.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.loader, ModLoader::NeoForge);
    assert_eq!(plan.loader_version, "21.0.0");
}

#[test]
fn test_infer_loader_from_metadata_unknown() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "some_mod: \"Some Mod|mod\""
  minecraft_version: "1.21"
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
unknown_loader = "1.0.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::ValidationError { reason } => {
            assert!(reason.contains("Cannot infer mod loader"));
        }
        _ => panic!("Expected ValidationError"),
    }
}

#[test]
fn test_parse_dependency_spec_basic() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 1);
    let dep = &plan.dependencies[0];
    assert_eq!(dep.key, "fabric_api");
    assert_eq!(dep.search_query, "Fabric API");
    assert_eq!(dep.project_type, ProjectType::Mod);
    assert_eq!(dep.minecraft_version, "1.21");
    assert_eq!(dep.loader, ModLoader::Fabric);
}

#[test]
fn test_parse_dependency_spec_with_type() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "xaeros_minimap: \"Xaero's Minimap|mod\""
    - "vanilla_tweaks: \"Vanilla Tweaks|datapack\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 2);
    
    let mod_dep = &plan.dependencies[0];
    assert_eq!(mod_dep.key, "xaeros_minimap");
    assert_eq!(mod_dep.project_type, ProjectType::Mod);
    
    let datapack_dep = &plan.dependencies[1];
    assert_eq!(datapack_dep.key, "vanilla_tweaks");
    assert_eq!(datapack_dep.project_type, ProjectType::Datapack);
}

#[test]
fn test_parse_dependency_spec_with_minecraft_version() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod|1.20.1\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 1);
    let dep = &plan.dependencies[0];
    assert_eq!(dep.minecraft_version, "1.20.1"); // Override from default
}

#[test]
fn test_parse_dependency_spec_with_loader() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod||fabric\""
    - "jei: \"Just Enough Items|mod||forge\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 2);
    
    let fabric_dep = &plan.dependencies[0];
    assert_eq!(fabric_dep.loader, ModLoader::Fabric);
    
    let forge_dep = &plan.dependencies[1];
    assert_eq!(forge_dep.loader, ModLoader::Forge);
}

#[test]
fn test_parse_dependency_spec_with_project_ids() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  project_ids:
    fabric_api: "P7dR8mSH"
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 1);
    let dep = &plan.dependencies[0];
    assert_eq!(dep.project_id, Some("P7dR8mSH".to_string()));
}

#[test]
fn test_parse_dependency_spec_with_version_overrides() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  version_overrides:
    fabric_api: "0.92.0"
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.dependencies.len(), 1);
    let dep = &plan.dependencies[0];
    assert!(dep.version_override.is_some());
    match &dep.version_override {
        Some(VersionOverride::Single(version)) => {
            assert_eq!(version, "0.92.0");
        }
        _ => panic!("Expected Single version override"),
    }
}

#[test]
fn test_parse_dependency_spec_invalid_format() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "invalid_spec_without_colon"
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidProjectSpec { spec } => {
            assert!(spec.contains("invalid_spec_without_colon"));
        }
        _ => panic!("Expected InvalidProjectSpec error"),
    }
}

#[test]
fn test_generate_default_empack_yml() {
    let workdir = PathBuf::from("/test/config");
    let pack_content = r#"
name = "Test Pack"
author = "Test Author"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.generate_default_empack_yml();

    assert!(result.is_ok());
    let yml_content = result.unwrap();
    println!("Generated YAML content:\n{}", yml_content);
    assert!(yml_content.contains("fabric_api"));
    assert!(yml_content.contains("sodium"));
    assert!(yml_content.contains("lithium"));
    assert!(yml_content.contains("minecraft_version: '1.21'") || yml_content.contains("minecraft_version: \"1.21\""));
    assert!(yml_content.contains("loader: fabric"));
    assert!(yml_content.contains("name: Test Pack") || yml_content.contains("name: \"Test Pack\""));
}

#[test]
fn test_generate_default_empack_yml_no_pack() {
    let workdir = PathBuf::from("/test/config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.generate_default_empack_yml();

    assert!(result.is_ok());
    let yml_content = result.unwrap();
    assert!(yml_content.contains("fabric_api"));
    assert!(yml_content.contains("sodium"));
    assert!(yml_content.contains("lithium"));
    // Should not contain specific versions when no pack.toml
    assert!(!yml_content.contains("minecraft_version"));
    assert!(!yml_content.contains("loader"));
}

#[test]
fn test_validate_consistency_matching() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.validate_consistency();

    assert!(result.is_ok());
    let issues = result.unwrap();
    assert!(issues.is_empty());
}

#[test]
fn test_validate_consistency_minecraft_mismatch() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.validate_consistency();

    assert!(result.is_ok());
    let issues = result.unwrap();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("Minecraft version mismatch"));
    assert!(issues[0].contains("1.21"));
    assert!(issues[0].contains("1.20.1"));
}

#[test]
fn test_validate_consistency_loader_mismatch() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    let pack_content = r#"
name = "Test Pack"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = ""

[versions]
minecraft = "1.21"
forge = "47.1.0"
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let provider = with_pack_toml(provider, &workdir, pack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.validate_consistency();

    assert!(result.is_ok());
    let issues = result.unwrap();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("Loader mismatch"));
    assert!(issues[0].contains("Fabric"));
    assert!(issues[0].contains("Forge"));
}

#[test]
fn test_validate_consistency_no_pack_toml() {
    let workdir = PathBuf::from("/test/config");
    let empack_content = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
  minecraft_version: "1.21"
  loader: fabric
"#;
    
    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.validate_consistency();

    assert!(result.is_ok());
    let issues = result.unwrap();
    assert!(issues.is_empty()); // No issues when no pack.toml
}