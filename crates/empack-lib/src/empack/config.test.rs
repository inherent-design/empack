use super::*;
use crate::application::session::FileSystemProvider;
use crate::application::session_mocks::{mock_root, MockFileSystemProvider};
use crate::empack::parsing::ModLoader;
use crate::primitives::{ProjectPlatform, ProjectType};
use std::path::{Path, PathBuf};

// Helper to create mock filesystem provider with test setup
fn create_mock_config_provider(workdir: PathBuf) -> MockFileSystemProvider {
    MockFileSystemProvider::new().with_current_dir(workdir)
}

fn with_empack_yml(
    provider: MockFileSystemProvider,
    workdir: &Path,
    content: &str,
) -> MockFileSystemProvider {
    provider.with_file(workdir.join("empack.yml"), content.to_string())
}

fn with_pack_toml(
    provider: MockFileSystemProvider,
    workdir: &Path,
    content: &str,
) -> MockFileSystemProvider {
    provider.with_file(workdir.join("pack").join("pack.toml"), content.to_string())
}

#[test]
fn test_load_empack_config_success() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
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
    let workdir = mock_root().join("config");
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
    let workdir = mock_root().join("config");
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
    let workdir = mock_root().join("config");
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
    assert_eq!(
        metadata.versions.loader_versions.get("fabric"),
        Some(&"0.14.21".to_string())
    );
}

#[test]
fn test_load_pack_metadata_missing_file() {
    let workdir = mock_root().join("config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.load_pack_metadata();

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn test_load_pack_metadata_invalid_toml() {
    let workdir = mock_root().join("config");
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    jei:
      status: resolved
      title: Just Enough Items
      platform: curseforge
      project_id: "238222"
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    quilted_fabric_api:
      status: resolved
      title: Quilted Fabric API
      platform: modrinth
      project_id: qvIfYCYJ
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    jei:
      status: resolved
      title: Just Enough Items
      platform: curseforge
      project_id: "238222"
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    some_mod:
      status: resolved
      title: Some Mod
      platform: modrinth
      project_id: test-id
      type: mod
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
fn test_build_project_spec_from_record() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    assert_eq!(dep.project_id, "P7dR8mSH");
    assert_eq!(dep.project_platform, ProjectPlatform::Modrinth);
}

#[test]
fn test_build_project_spec_with_types() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    xaeros_minimap:
      status: resolved
      title: "Xaero's Minimap"
      platform: modrinth
      project_id: test-id-minimap
      type: mod
    vanilla_tweaks:
      status: resolved
      title: Vanilla Tweaks
      platform: modrinth
      project_id: test-id-tweaks
      type: datapack
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

    let mod_dep = plan.dependencies.iter().find(|d| d.key == "xaeros_minimap").unwrap();
    assert_eq!(mod_dep.project_type, ProjectType::Mod);

    let datapack_dep = plan.dependencies.iter().find(|d| d.key == "vanilla_tweaks").unwrap();
    assert_eq!(datapack_dep.project_type, ProjectType::Datapack);
}

#[test]
fn test_build_project_spec_with_resolved_ids() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    assert_eq!(dep.project_id, "P7dR8mSH");
    assert_eq!(dep.project_platform, ProjectPlatform::Modrinth);
}

#[test]
fn test_build_project_spec_with_curseforge_platform() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    jei:
      status: resolved
      title: Just Enough Items
      platform: curseforge
      project_id: "238222"
      type: mod
  minecraft_version: "1.21"
  loader: forge
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    let dep = &plan.dependencies[0];
    assert_eq!(dep.project_id, "238222");
    assert_eq!(dep.project_platform, ProjectPlatform::CurseForge);
}

#[test]
fn test_build_project_spec_with_version_pin() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
      version: "0.92.0"
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
    assert_eq!(dep.version_pin, Some("0.92.0".to_string()));
}

#[test]
fn test_search_entries_excluded_from_project_plan() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    unresolved_mod:
      title: Unresolved Mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.create_project_plan();

    assert!(result.is_ok());
    let plan = result.unwrap();
    // Only the resolved entry should appear in dependencies
    assert_eq!(plan.dependencies.len(), 1);
    assert_eq!(plan.dependencies[0].key, "fabric_api");
}

#[test]
fn test_generate_default_empack_yml() {
    let workdir = mock_root().join("config");
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
    assert!(yml_content.contains("fabric_api") || yml_content.contains("fabric-api"));
    assert!(yml_content.contains("sodium"));
    assert!(yml_content.contains("lithium"));
    assert!(yml_content.contains("loader: fabric") || yml_content.contains("loader: Fabric"));
}

#[test]
fn test_generate_default_empack_yml_no_pack() {
    let workdir = mock_root().join("config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir);
    let result = config_manager.generate_default_empack_yml();

    assert!(result.is_ok());
    let yml_content = result.unwrap();
    // No pack.toml means no loader detected, so no default deps
    assert!(!yml_content.contains("sodium"));
    assert!(!yml_content.contains("lithium"));
    assert!(!yml_content.contains("fabric-api"));
    // Should not contain specific versions when no pack.toml
    assert!(!yml_content.contains("minecraft_version"));
    assert!(!yml_content.contains("loader"));
}

#[test]
fn test_validate_consistency_matching() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
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

#[test]
fn test_add_dependency_basic() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Add a new dependency
    let result = config_manager.add_dependency(
        "appleskin",
        DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "AppleSkin".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "snDcZxV8".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        },
    );

    assert!(result.is_ok());

    // Verify the dependency was added
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 2);
    assert!(config.empack.dependencies.contains_key("appleskin"));

    match &config.empack.dependencies["appleskin"] {
        DependencyEntry::Resolved(record) => {
            assert_eq!(record.project_id, "snDcZxV8");
            assert_eq!(record.platform, ProjectPlatform::Modrinth);
        }
        _ => panic!("Expected Resolved entry"),
    }
}

#[test]
fn test_add_dependency_duplicate() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Add the same dependency twice (upsert)
    let record = DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Fabric API".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "P7dR8mSH".to_string(),
        project_type: ProjectType::Mod,
        version: None,
    };
    let result1 = config_manager.add_dependency("fabric_api", record.clone());
    assert!(result1.is_ok());

    let result2 = config_manager.add_dependency("fabric_api", record);
    assert!(result2.is_ok());

    // Should still only have one copy
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
}

#[test]
fn test_add_dependency_upsert_overwrites_existing() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    let result = config_manager.add_dependency(
        "fabric_api",
        DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Fabric API Renamed".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "P7dR8mSH".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        },
    );

    assert!(result.is_ok());

    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    // Upsert overwrites the existing entry
    match &config.empack.dependencies["fabric_api"] {
        DependencyEntry::Resolved(record) => {
            assert_eq!(record.title, "Fabric API Renamed");
            assert_eq!(record.project_id, "P7dR8mSH");
            assert_eq!(record.platform, ProjectPlatform::Modrinth);
        }
        _ => panic!("Expected Resolved entry"),
    }
}

#[test]
fn test_add_dependency_no_existing_file() {
    let workdir = mock_root().join("config");
    let provider = create_mock_config_provider(workdir.clone());
    let config_manager = provider.config_manager(workdir.clone());

    // Add dependency when no empack.yml exists
    let result = config_manager.add_dependency(
        "appleskin",
        DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "AppleSkin".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "snDcZxV8".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        },
    );

    assert!(result.is_ok());

    // Verify the dependency was added
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    assert!(config.empack.dependencies.contains_key("appleskin"));
}

#[test]
fn test_remove_dependency_basic() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    appleskin:
      status: resolved
      title: AppleSkin
      platform: modrinth
      project_id: snDcZxV8
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove the dependency
    let result = config_manager.remove_dependency("appleskin");

    assert!(result.is_ok());

    // Verify the dependency was removed
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    assert!(!config.empack.dependencies.contains_key("appleskin"));
}

#[test]
fn test_remove_dependency_by_slug() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    appleskin:
      status: resolved
      title: AppleSkin
      platform: modrinth
      project_id: snDcZxV8
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove using exact slug
    let result = config_manager.remove_dependency("appleskin");

    assert!(result.is_ok());

    // Verify the dependency was removed
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    assert!(!config.empack.dependencies.contains_key("appleskin"));
}

#[test]
fn test_remove_dependency_with_slug_key() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    xaeros_minimap:
      status: resolved
      title: "Xaero's Minimap"
      platform: modrinth
      project_id: test-id-minimap
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove using exact slug key
    let result = config_manager.remove_dependency("xaeros_minimap");

    assert!(result.is_ok());

    // Verify the dependency was removed
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 0);
}

#[test]
fn test_remove_nonexistent_dependency() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove a dependency that doesn't exist
    let result = config_manager.remove_dependency("nonexistent");

    assert!(result.is_ok()); // Should not error, just do nothing

    // Original dependency should still be there
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
}

// ─── DependencyEntry serde round-trip tests ───────────────────────────────

#[test]
fn test_serde_round_trip_resolved_modrinth_mod_no_version() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Sodium".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "AANobbMI".to_string(),
        project_type: ProjectType::Mod,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_curseforge_with_version() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Just Enough Items".to_string(),
        platform: ProjectPlatform::CurseForge,
        project_id: "238222".to_string(),
        project_type: ProjectType::Mod,
        version: Some("4.7.0".to_string()),
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Just Enough Items".to_string(),
            platform: ProjectPlatform::CurseForge,
            project_id: "238222".to_string(),
            project_type: ProjectType::Mod,
            version: Some("4.7.0".to_string()),
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_project_type_datapack() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Vanilla Tweaks".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "test-dp".to_string(),
        project_type: ProjectType::Datapack,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Vanilla Tweaks".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "test-dp".to_string(),
            project_type: ProjectType::Datapack,
            version: None,
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_project_type_resourcepack() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Faithful".to_string(),
        platform: ProjectPlatform::CurseForge,
        project_id: "test-rp".to_string(),
        project_type: ProjectType::ResourcePack,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Faithful".to_string(),
            platform: ProjectPlatform::CurseForge,
            project_id: "test-rp".to_string(),
            project_type: ProjectType::ResourcePack,
            version: None,
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_project_type_shader() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Complementary Shaders".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "test-sh".to_string(),
        project_type: ProjectType::Shader,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Complementary Shaders".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "test-sh".to_string(),
            project_type: ProjectType::Shader,
            version: None,
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_unicode_title() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Xaero's Minimap".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "1bokaNcj".to_string(),
        project_type: ProjectType::Mod,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Xaero's Minimap".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "1bokaNcj".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
}

#[test]
fn test_serde_round_trip_resolved_special_chars_project_id() {
    let entry = DependencyEntry::Resolved(DependencyRecord {
        status: DependencyStatus::Resolved,
        title: "Test Mod".to_string(),
        platform: ProjectPlatform::Modrinth,
        project_id: "A-B_c.123".to_string(),
        project_type: ProjectType::Mod,
        version: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Test Mod".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "A-B_c.123".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
}

// ─── DependencySearch serde round-trip tests ──────────────────────────────

#[test]
fn test_serde_round_trip_search_title_only() {
    let entry = DependencyEntry::Search(DependencySearch {
        title: "Some Mod".to_string(),
        project_type: None,
        platform: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Search(DependencySearch {
            title: "Some Mod".to_string(),
            project_type: None,
            platform: None,
        })
    );
}

#[test]
fn test_serde_round_trip_search_with_type_hint() {
    let entry = DependencyEntry::Search(DependencySearch {
        title: "BSL Shaders".to_string(),
        project_type: Some(ProjectType::Shader),
        platform: None,
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Search(DependencySearch {
            title: "BSL Shaders".to_string(),
            project_type: Some(ProjectType::Shader),
            platform: None,
        })
    );
}

#[test]
fn test_serde_round_trip_search_with_platform_hint() {
    let entry = DependencyEntry::Search(DependencySearch {
        title: "Optifine".to_string(),
        project_type: None,
        platform: Some(ProjectPlatform::Modrinth),
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Search(DependencySearch {
            title: "Optifine".to_string(),
            project_type: None,
            platform: Some(ProjectPlatform::Modrinth),
        })
    );
}

#[test]
fn test_serde_round_trip_search_with_both_hints() {
    let entry = DependencyEntry::Search(DependencySearch {
        title: "Iris Shaders".to_string(),
        project_type: Some(ProjectType::Shader),
        platform: Some(ProjectPlatform::CurseForge),
    });

    let yaml = serde_saphyr::to_string(&entry).unwrap();
    let deserialized: DependencyEntry = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(
        deserialized,
        DependencyEntry::Search(DependencySearch {
            title: "Iris Shaders".to_string(),
            project_type: Some(ProjectType::Shader),
            platform: Some(ProjectPlatform::CurseForge),
        })
    );
}

// ─── Untagged discrimination tests ────────────────────────────────────────

#[test]
fn test_untagged_yaml_with_status_parses_as_resolved() {
    let yaml = r#"
status: resolved
title: Sodium
platform: modrinth
project_id: AANobbMI
type: mod
"#;

    let entry: DependencyEntry = serde_saphyr::from_str(yaml).unwrap();

    assert_eq!(
        entry,
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
}

#[test]
fn test_untagged_yaml_without_status_parses_as_search() {
    let yaml = r#"
title: Unresolved Mod
"#;

    let entry: DependencyEntry = serde_saphyr::from_str(yaml).unwrap();

    assert_eq!(
        entry,
        DependencyEntry::Search(DependencySearch {
            title: "Unresolved Mod".to_string(),
            project_type: None,
            platform: None,
        })
    );
}

#[test]
fn test_untagged_yaml_resolved_missing_platform_fails() {
    // status: resolved but missing required platform field —
    // should fail Resolved and also fail Search (which requires no status).
    // With untagged, serde tries Resolved first (fails: missing platform),
    // then Search (succeeds: title is present, extra fields ignored).
    let yaml = r#"
status: resolved
title: Broken Mod
project_id: test-id
type: mod
"#;

    let result: Result<DependencyEntry, _> = serde_saphyr::from_str(yaml);
    // serde untagged: Resolved fails (missing platform), Search succeeds
    // because DependencySearch only requires `title` and ignores unknown fields
    match result {
        Ok(DependencyEntry::Search(s)) => {
            assert_eq!(s.title, "Broken Mod");
        }
        Ok(DependencyEntry::Resolved(_)) => {
            panic!("Should not parse as Resolved without platform field");
        }
        Err(e) => {
            // Also acceptable if serde rejects it entirely
            assert!(
                e.to_string().contains("data did not match any variant"),
                "Unexpected error: {e}"
            );
        }
    }
}

// ─── EmpackConfig edge case tests ─────────────────────────────────────────

#[test]
fn test_empack_config_empty_dependencies() {
    let yaml = r#"
empack:
  dependencies: {}
"#;

    let config: EmpackConfig = serde_saphyr::from_str(yaml).unwrap();
    assert!(config.empack.dependencies.is_empty());
    assert_eq!(config.empack.minecraft_version, None);
    assert_eq!(config.empack.loader, None);
    assert_eq!(config.empack.name, None);
    assert_eq!(config.empack.author, None);
    assert_eq!(config.empack.version, None);
}

#[test]
fn test_empack_config_mixed_resolved_and_search() {
    let yaml = r#"
empack:
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
    unknown_mod:
      title: Unknown Mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let config: EmpackConfig = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(config.empack.dependencies.len(), 2);

    assert_eq!(
        config.empack.dependencies["sodium"],
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );

    assert_eq!(
        config.empack.dependencies["unknown_mod"],
        DependencyEntry::Search(DependencySearch {
            title: "Unknown Mod".to_string(),
            project_type: None,
            platform: None,
        })
    );
}

#[test]
fn test_empack_config_minimal_required_fields() {
    // EmpackConfig only requires the `empack` key; everything inside is optional/default
    let yaml = r#"
empack: {}
"#;

    let config: EmpackConfig = serde_saphyr::from_str(yaml).unwrap();
    assert!(config.empack.dependencies.is_empty());
    assert_eq!(config.empack.minecraft_version, None);
    assert_eq!(config.empack.loader, None);
    assert_eq!(config.empack.name, None);
}

#[test]
fn test_empack_config_round_trip_multiple_dependencies() {
    let config = EmpackConfig {
        empack: EmpackProjectConfig {
            dependencies: {
                let mut deps = BTreeMap::new();
                deps.insert(
                    "sodium".to_string(),
                    DependencyEntry::Resolved(DependencyRecord {
                        status: DependencyStatus::Resolved,
                        title: "Sodium".to_string(),
                        platform: ProjectPlatform::Modrinth,
                        project_id: "AANobbMI".to_string(),
                        project_type: ProjectType::Mod,
                        version: None,
                    }),
                );
                deps.insert(
                    "iris".to_string(),
                    DependencyEntry::Search(DependencySearch {
                        title: "Iris Shaders".to_string(),
                        project_type: Some(ProjectType::Mod),
                        platform: None,
                    }),
                );
                deps.insert(
                    "complementary".to_string(),
                    DependencyEntry::Resolved(DependencyRecord {
                        status: DependencyStatus::Resolved,
                        title: "Complementary Shaders".to_string(),
                        platform: ProjectPlatform::CurseForge,
                        project_id: "431203".to_string(),
                        project_type: ProjectType::Shader,
                        version: Some("4.7.2".to_string()),
                    }),
                );
                deps
            },
            minecraft_version: Some("1.21".to_string()),
            loader: Some(ModLoader::Fabric),
            loader_version: None,
            name: Some("Test Pack".to_string()),
            author: Some("Tester".to_string()),
            version: Some("1.0.0".to_string()),
        },
    };

    let yaml = serde_saphyr::to_string(&config).unwrap();
    let deserialized: EmpackConfig = serde_saphyr::from_str(&yaml).unwrap();

    assert_eq!(deserialized.empack.dependencies.len(), 3);
    assert_eq!(
        deserialized.empack.dependencies["sodium"],
        config.empack.dependencies["sodium"]
    );
    assert_eq!(
        deserialized.empack.dependencies["iris"],
        config.empack.dependencies["iris"]
    );
    assert_eq!(
        deserialized.empack.dependencies["complementary"],
        config.empack.dependencies["complementary"]
    );
    assert_eq!(deserialized.empack.minecraft_version, Some("1.21".to_string()));
    assert_eq!(deserialized.empack.loader, Some(ModLoader::Fabric));
    assert_eq!(deserialized.empack.name, Some("Test Pack".to_string()));
    assert_eq!(deserialized.empack.author, Some("Tester".to_string()));
    assert_eq!(deserialized.empack.version, Some("1.0.0".to_string()));
}

// ─── remove_dependency additional tests ───────────────────────────────────

#[test]
fn test_remove_dependency_by_case_insensitive_title() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove by title (case-insensitive) — "SODIUM" should match title "Sodium"
    let result = config_manager.remove_dependency("SODIUM");
    assert!(result.is_ok());

    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    assert!(config.empack.dependencies.contains_key("fabric_api"));
    assert!(!config.empack.dependencies.contains_key("sodium"));
}

#[test]
fn test_remove_dependency_ambiguous_title_returns_error() {
    let workdir = mock_root().join("config");
    // Two different slugs but same title
    let empack_content = r#"
empack:
  dependencies:
    sodium_modrinth:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
    sodium_curseforge:
      status: resolved
      title: Sodium
      platform: curseforge
      project_id: "394468"
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // "Sodium" matches two entries by title — should be ambiguous
    let result = config_manager.remove_dependency("Sodium");
    assert!(result.is_err());

    match result.unwrap_err() {
        ConfigError::AmbiguousRemoval { query, matches } => {
            assert_eq!(query, "Sodium");
            assert_eq!(matches.len(), 2);
            assert!(matches.contains(&"sodium_modrinth".to_string()));
            assert!(matches.contains(&"sodium_curseforge".to_string()));
        }
        other => panic!("Expected AmbiguousRemoval, got: {other:?}"),
    }

    // Map should be unchanged
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 2);
}

#[test]
fn test_remove_dependency_not_found_by_slug_or_title() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Neither slug nor title matches
    let result = config_manager.remove_dependency("totally_absent");
    // Current implementation returns Ok(()) for not-found
    assert!(result.is_ok());

    // Map should be unchanged
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 1);
    assert!(config.empack.dependencies.contains_key("fabric_api"));
}

#[test]
fn test_remove_dependency_then_verify_remaining() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    sodium:
      status: resolved
      title: Sodium
      platform: modrinth
      project_id: AANobbMI
      type: mod
    lithium:
      status: resolved
      title: Lithium
      platform: modrinth
      project_id: gvQqBUqZ
      type: mod
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Remove lithium by slug
    let result = config_manager.remove_dependency("lithium");
    assert!(result.is_ok());

    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.dependencies.len(), 2);
    assert!(config.empack.dependencies.contains_key("sodium"));
    assert!(config.empack.dependencies.contains_key("fabric_api"));
    assert!(!config.empack.dependencies.contains_key("lithium"));

    // Verify remaining entries have correct data
    assert_eq!(
        config.empack.dependencies["sodium"],
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
    assert_eq!(
        config.empack.dependencies["fabric_api"],
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Fabric API".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "P7dR8mSH".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        })
    );
}

// ─── loader_version round-trip tests ──────────────────────────────────────

#[test]
fn test_loader_version_preserved_after_add_dependency() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
  loader_version: "0.16.14"
  name: "Test Pack"
  author: "Test Author"
  version: "1.0.0"
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Verify loader_version was parsed
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.loader_version, Some("0.16.14".to_string()));

    // Add a new dependency (triggers round-trip: deserialize -> modify -> serialize)
    let result = config_manager.add_dependency(
        "sodium",
        DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        },
    );
    assert!(result.is_ok());

    // Reload and verify loader_version survived the round-trip
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.loader_version, Some("0.16.14".to_string()));
    assert_eq!(config.empack.dependencies.len(), 2);
}

#[test]
fn test_loader_version_absent_stays_absent_after_add_dependency() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
"#;

    let provider = create_mock_config_provider(workdir.clone());
    let provider = with_empack_yml(provider, &workdir, empack_content);
    let config_manager = provider.config_manager(workdir.clone());

    // Add a dependency
    let result = config_manager.add_dependency(
        "sodium",
        DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Sodium".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        },
    );
    assert!(result.is_ok());

    // loader_version should remain None (not injected by round-trip)
    let config = config_manager.load_empack_config().unwrap();
    assert_eq!(config.empack.loader_version, None);
}

#[test]
fn test_create_project_plan_uses_empack_loader_version() {
    let workdir = mock_root().join("config");
    let empack_content = r#"
empack:
  dependencies:
    fabric_api:
      status: resolved
      title: Fabric API
      platform: modrinth
      project_id: P7dR8mSH
      type: mod
  minecraft_version: "1.21"
  loader: fabric
  loader_version: "0.16.14"
"#;
    // pack.toml has a different loader version
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
    let plan = config_manager.create_project_plan().unwrap();

    // empack.yml loader_version should take precedence over pack.toml
    assert_eq!(plan.loader_version, "0.16.14");
}

// ─── BTreeMap deterministic ordering tests ────────────────────────────────

#[test]
fn test_btreemap_dependencies_serialize_in_alphabetical_order() {
    let mut deps = BTreeMap::new();
    // Insert in non-alphabetical order
    deps.insert(
        "zebra".to_string(),
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Zebra Mod".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "zebra-id".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        }),
    );
    deps.insert(
        "alpha".to_string(),
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Alpha Mod".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "alpha-id".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        }),
    );
    deps.insert(
        "middle".to_string(),
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Middle Mod".to_string(),
            platform: ProjectPlatform::Modrinth,
            project_id: "middle-id".to_string(),
            project_type: ProjectType::Mod,
            version: None,
        }),
    );

    let config = EmpackConfig {
        empack: EmpackProjectConfig {
            dependencies: deps,
            minecraft_version: Some("1.21".to_string()),
            loader: Some(ModLoader::Fabric),
            loader_version: None,
            name: None,
            author: None,
            version: None,
        },
    };

    let yaml = serde_saphyr::to_string(&config).unwrap();

    // Keys must appear in alphabetical order: alpha < middle < zebra
    let alpha_pos = yaml.find("alpha:").expect("alpha key missing");
    let middle_pos = yaml.find("middle:").expect("middle key missing");
    let zebra_pos = yaml.find("zebra:").expect("zebra key missing");

    assert!(
        alpha_pos < middle_pos,
        "alpha ({alpha_pos}) should come before middle ({middle_pos}) in YAML output"
    );
    assert!(
        middle_pos < zebra_pos,
        "middle ({middle_pos}) should come before zebra ({zebra_pos}) in YAML output"
    );
}
