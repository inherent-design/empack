use super::*;
use tempfile::TempDir;

fn create_test_config() -> (TempDir, ConfigManager) {
    let temp_dir = TempDir::new().unwrap();
    let config_manager = ConfigManager::new(temp_dir.path().to_path_buf());

    // Create pack directory
    std::fs::create_dir_all(temp_dir.path().join("pack")).unwrap();

    (temp_dir, config_manager)
}

fn write_test_pack_toml(path: &std::path::Path) {
    let pack_toml = r#"
name = "Test Modpack"
pack-format = "packwiz:1.1.0"

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;
    std::fs::write(path.join("pack").join("pack.toml"), pack_toml).unwrap();
}

fn write_test_empack_yml(path: &std::path::Path) {
    let empack_yml = r#"
empack:
  dependencies:
    - "fabric_api: \"Fabric API|mod\""
    - "sodium: \"Sodium|mod|1.20.1|fabric\""
    - "lithium: \"Lithium|mod\""
  project_ids:
    fabric_api: "P7dR8mSH"
  version_overrides:
    sodium:
      - "mc1.20.1-0.5.0"
"#;
    std::fs::write(path.join("empack.yml"), empack_yml).unwrap();
}

#[test]
fn test_load_pack_metadata() {
    let (temp_dir, config_manager) = create_test_config();
    write_test_pack_toml(temp_dir.path());

    let pack_metadata = config_manager.load_pack_metadata().unwrap().unwrap();
    assert_eq!(pack_metadata.name, "Test Modpack");
    assert_eq!(pack_metadata.versions.minecraft, "1.20.1");
    assert_eq!(
        pack_metadata.versions.loader_versions.get("fabric"),
        Some(&"0.14.21".to_string())
    );
}

#[test]
fn test_load_empack_config() {
    let (temp_dir, config_manager) = create_test_config();
    write_test_empack_yml(temp_dir.path());

    let empack_config = config_manager.load_empack_config().unwrap();
    assert_eq!(empack_config.empack.dependencies.len(), 3);
    assert_eq!(
        empack_config.empack.project_ids.get("fabric_api"),
        Some(&"P7dR8mSH".to_string())
    );
}

#[test]
fn test_create_project_plan() {
    let (temp_dir, config_manager) = create_test_config();
    write_test_pack_toml(temp_dir.path());
    write_test_empack_yml(temp_dir.path());

    let project_plan = config_manager.create_project_plan().unwrap();
    assert_eq!(project_plan.name, "Test Modpack");
    assert_eq!(project_plan.minecraft_version, "1.20.1");
    assert_eq!(project_plan.loader, ModLoader::Fabric);
    assert_eq!(project_plan.dependencies.len(), 3);

    // Check first dependency
    let fabric_api = &project_plan.dependencies[0];
    assert_eq!(fabric_api.key, "fabric_api");
    assert_eq!(fabric_api.search_query, "Fabric API");
    assert_eq!(fabric_api.project_type, ProjectType::Mod);
    assert_eq!(fabric_api.project_id, Some("P7dR8mSH".to_string()));
}

#[test]
fn test_parse_dependency_spec() {
    let (temp_dir, config_manager) = create_test_config();

    let empack_config = EmpackProjectConfig {
        dependencies: vec![],
        project_ids: HashMap::new(),
        version_overrides: HashMap::new(),
        minecraft_version: None,
        loader: None,
        name: None,
        author: None,
        version: None,
    };

    let spec = config_manager
        .parse_dependency_spec(
            "sodium: \"Sodium|mod|1.20.1|fabric\"",
            "1.19.4",
            &ModLoader::Quilt,
            &empack_config,
        )
        .unwrap();

    assert_eq!(spec.key, "sodium");
    assert_eq!(spec.search_query, "Sodium");
    assert_eq!(spec.project_type, ProjectType::Mod);
    assert_eq!(spec.minecraft_version, "1.20.1");
    assert_eq!(spec.loader, ModLoader::Fabric);
}

#[test]
fn test_infer_loader_from_metadata() {
    let pack_metadata = PackMetadata {
        name: "Test".to_string(),
        author: None,
        version: None,
        versions: PackVersions {
            minecraft: "1.20.1".to_string(),
            loader_versions: {
                let mut map = HashMap::new();
                map.insert("fabric".to_string(), "0.14.21".to_string());
                map
            },
        },
    };

    let (_, config_manager) = create_test_config();
    let loader = config_manager
        .infer_loader_from_metadata(&pack_metadata)
        .unwrap();
    assert_eq!(loader, ModLoader::Fabric);
}
