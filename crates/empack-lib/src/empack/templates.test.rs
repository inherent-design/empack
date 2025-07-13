use super::*;
use tempfile::TempDir;

#[test]
fn test_template_engine_creation() {
    let engine = TemplateEngine::new();
    let templates = engine.template_names();
    
    assert!(templates.contains(&"gitignore".to_string()));
    assert!(templates.contains(&"instance.cfg".to_string()));
    assert!(templates.contains(&"install_pack.sh".to_string()));
}

#[test]
fn test_template_variable_substitution() {
    let mut engine = TemplateEngine::new();
    engine.set_pack_variables("Test Pack", "TestAuthor", "1.21.1", "1.0.0");
    
    let result = engine.render_template("instance.cfg").unwrap();
    assert!(result.contains("name=Test Pack"));
    assert!(result.contains("ExportAuthor=TestAuthor"));
}

#[test]
fn test_template_installer_directory_creation() {
    let temp_dir = TempDir::new().unwrap();
    let installer = TemplateInstaller::new();
    
    installer.create_directory_structure(temp_dir.path()).unwrap();
    
    assert!(temp_dir.path().join("dist/client").exists());
    assert!(temp_dir.path().join("dist/server").exists());
    assert!(temp_dir.path().join("templates/client").exists());
    assert!(temp_dir.path().join("templates/server").exists());
    assert!(temp_dir.path().join(".github/workflows").exists());
    assert!(temp_dir.path().join("pack").exists());
    assert!(temp_dir.path().join("installer").exists());
}

#[test]
fn test_template_installer_full_install() {
    let temp_dir = TempDir::new().unwrap();
    let mut installer = TemplateInstaller::new();
    installer.configure("Test Pack", "TestAuthor", "1.21.1", "1.0.0");
    
    installer.install_all(temp_dir.path()).unwrap();
    
    // Verify key files were created
    assert!(temp_dir.path().join(".gitignore").exists());
    assert!(temp_dir.path().join("pack/.packwizignore").exists());
    assert!(temp_dir.path().join(".github/workflows/validate.yml").exists());
    assert!(temp_dir.path().join("templates/client/instance.cfg.template").exists());
    assert!(temp_dir.path().join("templates/server/install_pack.sh.template").exists());
    
    // Verify content substitution
    let gitignore_content = std::fs::read_to_string(temp_dir.path().join(".gitignore")).unwrap();
    assert!(gitignore_content.contains("dist/"));
    
    let instance_content = std::fs::read_to_string(temp_dir.path().join("templates/client/instance.cfg.template")).unwrap();
    assert!(instance_content.contains("name=Test Pack"));
    assert!(instance_content.contains("ExportAuthor=TestAuthor"));
}

#[test]
fn test_pack_toml_parsing_with_layer1_data() {
    let mut engine = TemplateEngine::new();
    
    // Create mock layer_1 pack.toml content
    let layer1_pack_toml = r#"
name = "layer_1"
author = "mannie-exe"
version = "0.4.5-alpha"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = "2df956639ac1847dd449288cf475401f88d8bdb65b08798e0b580b2fc565c09f"

[versions]
fabric = "0.16.14"
minecraft = "1.21.1"

[options]
acceptable-game-versions = ["1.21.1"]
datapack-folder = "config/openloader/data"
    "#;
    
    // Write to temp file and test parsing
    let temp_dir = TempDir::new().unwrap();
    let pack_path = temp_dir.path().join("pack.toml");
    std::fs::write(&pack_path, layer1_pack_toml).unwrap();
    
    // Test the pack.toml loading functionality
    engine.load_from_pack_toml(&pack_path).unwrap();
    
    // Verify V1-compatible variables were extracted correctly
    let variables = engine.variables();
    assert_eq!(variables.get("NAME").unwrap(), "layer_1");
    assert_eq!(variables.get("AUTHOR").unwrap(), "mannie-exe");
    assert_eq!(variables.get("VERSION").unwrap(), "0.4.5-alpha");
    assert_eq!(variables.get("MC_VERSION").unwrap(), "1.21.1");
    assert_eq!(variables.get("MODLOADER_NAME").unwrap(), "fabric");
    assert_eq!(variables.get("MODLOADER_VERSION").unwrap(), "0.16.14");
}

#[test]
fn test_build_time_template_rendering() {
    let temp_dir = TempDir::new().unwrap();
    let mut installer = TemplateInstaller::new();
    
    // Create mock pack.toml for build-time rendering
    let pack_toml = r#"
name = "MyModpack"
author = "PackMaker"
version = "2.1.0"

[versions]
neoforge = "21.1.186"
minecraft = "1.21.1"
    "#;
    
    let pack_path = temp_dir.path().join("pack.toml");
    std::fs::write(&pack_path, pack_toml).unwrap();
    
    // Configure from pack.toml (build-time use case)
    installer.configure_from_pack_toml(&pack_path).unwrap();
    
    // Install templates and verify build-time variable substitution
    installer.install_server_templates(temp_dir.path()).unwrap();
    
    let install_script = std::fs::read_to_string(
        temp_dir.path().join("templates/server/install_pack.sh.template")
    ).unwrap();
    
    assert!(install_script.contains("# MyModpack v2.1.0 Server Installer"));
    assert!(install_script.contains("Installing MyModpack v2.1.0 server pack"));
}
