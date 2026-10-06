use super::*;
use crate::application::session::LiveFileSystemProvider;
use tempfile::TempDir;

#[test]
fn test_template_engine_creation() {
    let engine = TemplateEngine::new();
    let templates = engine.template_names();

    assert!(templates.contains(&"gitignore".to_string()));
    assert!(templates.contains(&"instance.cfg".to_string()));
    assert!(templates.contains(&"install_pack.sh".to_string()));
    assert!(templates.contains(&"validate.yml".to_string()));
    assert!(templates.contains(&"release.yml".to_string()));
}

#[test]
fn test_template_variable_substitution() {
    let mut engine = TemplateEngine::new();
    engine.set_pack_variables("Test Pack", "TestAuthor", "1.21.1", "1.0.0");

    let result = engine.render_template("instance.cfg").unwrap();
    assert!(result.contains("name=\"Test Pack\""));
    assert!(result.contains("ExportAuthor=\"TestAuthor\""));
}

#[test]
fn test_template_installer_directory_creation() {
    let temp_dir = TempDir::new().unwrap();
    let fs = LiveFileSystemProvider;
    let installer = TemplateInstaller::new(&fs);

    installer.create_directory_structure(temp_dir.path()).unwrap();

    assert!(temp_dir.path().join("dist").join("client").exists());
    assert!(temp_dir.path().join("dist").join("server").exists());
    assert!(temp_dir.path().join("templates").join("common").exists());
    assert!(temp_dir.path().join("templates").join("client").exists());
    assert!(temp_dir.path().join("templates").join("server").exists());
    assert!(temp_dir.path().join(".github").join("workflows").exists());
    assert!(temp_dir.path().join("pack").exists());
}

#[test]
fn test_template_installer_full_install() {
    let temp_dir = TempDir::new().unwrap();
    let fs = LiveFileSystemProvider;
    let mut installer = TemplateInstaller::new(&fs);
    installer.configure("Test Pack", "TestAuthor", "1.21.1", "1.0.0");

    installer.install_all(temp_dir.path()).unwrap();

    // Verify key files were created
    assert!(temp_dir.path().join(".gitignore").exists());
    assert!(temp_dir.path().join("pack").join(".packwizignore").exists());
    assert!(temp_dir.path().join(".github").join("workflows").join("validate.yml").exists());
    assert!(temp_dir.path().join("templates").join("client").join("instance.cfg.template").exists());
    assert!(temp_dir.path().join("templates").join("server").join("install_pack.sh.template").exists());

    // Verify content substitution
    let gitignore_content = std::fs::read_to_string(temp_dir.path().join(".gitignore")).unwrap();
    assert!(gitignore_content.contains("dist/"));

    let instance_content = std::fs::read_to_string(temp_dir.path().join("templates").join("client").join("instance.cfg.template")).unwrap();
    assert!(instance_content.contains("name={{ini_quote NAME}}"));
    assert!(instance_content.contains("ExportAuthor={{ini_quote AUTHOR}}"));
}

#[test]
fn test_render_string_with_variables() {
    let mut engine = TemplateEngine::new();
    engine.set_pack_variables("MyPack", "Author1", "1.21.1", "2.0.0");

    let result = engine
        .render_string("Server: {{NAME}} v{{VERSION}} for MC {{MC_VERSION}}")
        .unwrap();
    assert_eq!(result, "Server: MyPack v2.0.0 for MC 1.21.1");
}

#[test]
fn test_render_string_missing_variable_passthrough() {
    let engine = TemplateEngine::new();
    // strict_mode is false, so missing vars render as empty
    let result = engine.render_string("Hello {{MISSING}}").unwrap();
    assert_eq!(result, "Hello ");
}

#[test]
fn test_pack_toml_parsing_with_modloader_data() {
    let mut engine = TemplateEngine::new();
    let fs = LiveFileSystemProvider;

    let sample_pack_toml = r#"
name = "test-modpack"
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
    std::fs::write(&pack_path, sample_pack_toml).unwrap();

    // Test the pack.toml loading functionality
    engine.load_from_pack_toml(&pack_path, &fs).unwrap();

    // Verify template variables
    let variables = engine.variables();
    assert_eq!(variables.get("NAME").unwrap(), "test-modpack");
    assert_eq!(variables.get("AUTHOR").unwrap(), "mannie-exe");
    assert_eq!(variables.get("VERSION").unwrap(), "0.4.5-alpha");
    assert_eq!(variables.get("MC_VERSION").unwrap(), "1.21.1");
    assert_eq!(variables.get("MODLOADER_NAME").unwrap(), "fabric");
    assert_eq!(variables.get("MODLOADER_VERSION").unwrap(), "0.16.14");
}

#[test]
fn test_pack_toml_parsing_with_quilt_loader_data() {
    let mut engine = TemplateEngine::new();
    let fs = LiveFileSystemProvider;

    let sample_pack_toml = r#"
name = "quilt-pack"
author = "quilt-author"
version = "1.2.3"
pack-format = "packwiz:1.1.0"

[versions]
quilt = "0.21.0"
minecraft = "1.21.1"
    "#;

    let temp_dir = TempDir::new().unwrap();
    let pack_path = temp_dir.path().join("pack.toml");
    std::fs::write(&pack_path, sample_pack_toml).unwrap();

    engine.load_from_pack_toml(&pack_path, &fs).unwrap();

    let variables = engine.variables();
    assert_eq!(variables.get("NAME").map(String::as_str), Some("quilt-pack"));
    assert_eq!(variables.get("MODLOADER_NAME").map(String::as_str), Some("quilt"));
    assert_eq!(variables.get("MODLOADER_VERSION").map(String::as_str), Some("0.21.0"));
}

#[test]
fn test_pack_toml_parsing_with_forge_loader_data() {
    let mut engine = TemplateEngine::new();
    let fs = LiveFileSystemProvider;

    let sample_pack_toml = r#"
name = "forge-pack"
author = "forge-author"
version = "4.5.6"
pack-format = "packwiz:1.1.0"

[versions]
forge = "47.1.0"
minecraft = "1.20.1"
    "#;

    let temp_dir = TempDir::new().unwrap();
    let pack_path = temp_dir.path().join("pack.toml");
    std::fs::write(&pack_path, sample_pack_toml).unwrap();

    engine.load_from_pack_toml(&pack_path, &fs).unwrap();

    let variables = engine.variables();
    assert_eq!(variables.get("NAME").map(String::as_str), Some("forge-pack"));
    assert_eq!(variables.get("MODLOADER_NAME").map(String::as_str), Some("forge"));
    assert_eq!(variables.get("MODLOADER_VERSION").map(String::as_str), Some("47.1.0"));
}

#[test]
fn test_pack_toml_parsing_without_loader_data() {
    let mut engine = TemplateEngine::new();
    let fs = LiveFileSystemProvider;

    let sample_pack_toml = r#"
name = "vanilla-pack"
version = "0.1.0"

[versions]
minecraft = "1.21.1"
    "#;

    let temp_dir = TempDir::new().unwrap();
    let pack_path = temp_dir.path().join("pack.toml");
    std::fs::write(&pack_path, sample_pack_toml).unwrap();

    engine.load_from_pack_toml(&pack_path, &fs).unwrap();

    let variables = engine.variables();
    assert_eq!(variables.get("NAME").map(String::as_str), Some("vanilla-pack"));
    assert_eq!(variables.get("MC_VERSION").map(String::as_str), Some("1.21.1"));
    assert!(!variables.contains_key("MODLOADER_NAME"));
    assert!(!variables.contains_key("MODLOADER_VERSION"));
}

#[test]
fn test_build_time_template_rendering() {
    let temp_dir = TempDir::new().unwrap();
    let fs = LiveFileSystemProvider;
    let mut installer = TemplateInstaller::new(&fs);

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
        temp_dir.path().join("templates").join("server").join("install_pack.sh.template")
    ).unwrap();

    let rendered = installer.engine.render_string(&install_script).unwrap();
    assert!(rendered.contains("PACK_NAME='MyModpack'"));
    assert!(rendered.contains("PACK_VERSION='2.1.0'"));
}

#[test]
fn test_installer_with_modloader_variables() {
    let temp_dir = TempDir::new().unwrap();
    let fs = LiveFileSystemProvider;
    let mut installer = TemplateInstaller::new(&fs);
    installer.configure("MyPack", "TestAuthor", "1.21.1", "1.0.0");
    installer
        .engine_mut()
        .set_modloader_variables("fabric", "0.16.14");

    installer.install_all(temp_dir.path()).unwrap();

    // Verify .gitignore exists
    assert!(temp_dir.path().join(".gitignore").exists());
    // Verify .packwizignore exists in pack/
    assert!(temp_dir.path().join("pack/.packwizignore").exists());
    // Verify .github/workflows/validate.yml exists
    assert!(temp_dir
        .path()
        .join(".github/workflows/validate.yml")
        .exists());
    // Verify templates/server/ directory exists with files
    assert!(temp_dir
        .path()
        .join("templates/server/install_pack.sh.template")
        .exists());
    assert!(temp_dir
        .path()
        .join("templates/server/server.properties.template")
        .exists());
    // Verify templates/client/ directory exists with files
    assert!(temp_dir
        .path()
        .join("templates/common")
        .exists());
    assert!(temp_dir
        .path()
        .join("templates/client/instance.cfg.template")
        .exists());

    // Verify server template has variable substitution
    let install_sh = std::fs::read_to_string(
        temp_dir
            .path()
            .join("templates/server/install_pack.sh.template"),
    )
    .unwrap();
    assert!(
        install_sh.contains("{{shell_quote NAME}}"),
        "install_pack.sh should retain build-time placeholders"
    );
}

#[test]
fn installed_templates_retain_build_time_placeholders() {
    let dir = TempDir::new().unwrap();
    let fs = LiveFileSystemProvider;
    let mut installer = TemplateInstaller::new(&fs);
    installer.configure("Old Name", "Author", "1.21.1", "old");
    installer.install_client_templates(dir.path()).unwrap();
    installer.install_server_templates(dir.path()).unwrap();
    let template = std::fs::read_to_string(dir.path().join("templates/client/instance.cfg.template")).unwrap();
    assert!(template.contains("{{ini_quote NAME}}"));
    installer.configure("New Name", "Author", "1.21.1", "new");
    assert!(installer.engine.render_string(&template).unwrap().contains("New Name"));
}

#[cfg(unix)]
#[test]
fn generated_installer_treats_shell_metadata_as_data() {
    let dir = TempDir::new().unwrap();
    let mut engine = TemplateEngine::new();
    let name = "$(touch injected); `touch injected2` ' quoted\n$(touch injected3)";
    engine.set_pack_variables(name, "Author", "1.21.1", "$(touch injected4)");
    let script = engine.render_template("install_pack.sh").unwrap();
    std::fs::write(dir.path().join("install.sh"), script).unwrap();
    let output = std::process::Command::new("bash").arg("install.sh").arg("/usr/bin/true")
        .current_dir(dir.path()).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains(name));
    for file in ["injected", "injected2", "injected3", "injected4"] {
        assert!(!dir.path().join(file).exists(), "shell executed metadata: {file}");
    }
}

#[test]
fn generated_configuration_keeps_metadata_on_one_physical_line() {
    let mut engine = TemplateEngine::new();
    engine.set_pack_variables("name\nPreLaunchCommand=unexpected\r\n[Other]", "author\nonline-mode=false", "1.21.1", "1\rserver-port=1");
    let client = engine.render_template("instance.cfg").unwrap();
    assert_eq!(client.lines().filter(|line| line.starts_with("PreLaunchCommand=")).count(), 1);
    assert!(!client.lines().any(|line| line == "[Other]"));
    let server = engine.render_template("server.properties").unwrap();
    assert!(!server.lines().any(|line| line == "online-mode=false" || line == "server-port=1"));
}

#[test]
fn full_client_default_has_no_bootstrap_command() {
    let mut engine = TemplateEngine::new();
    engine.set_pack_variables("Full", "Author", "1.21.1", "1");
    assert!(engine.render_template("instance.cfg").unwrap().contains("packwiz-installer-bootstrap.jar"));
    engine.set_variable("BOOTSTRAP", "");
    let full = engine.render_template("instance.cfg").unwrap();
    assert!(full.contains("InstanceType=OneSix"));
    assert!(full.lines().any(|line| line == "PreLaunchCommand="));
    assert!(!full.contains("packwiz-installer-bootstrap.jar"));
}
