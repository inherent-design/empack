//! Integration tests for `empack init` workflow
//!
//! Tests the complete initialization workflow across different scenarios:
//! - Zero-config init with defaults
//! - Explicit configuration via flags
//! - Directory creation from name argument
//! - Error handling for existing projects

use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

/// Test: empack init -y (zero-config with API-driven defaults)
///
/// Workflow:
/// 1. Run `empack init -y` in empty mock directory
/// 2. Verify empack.yml created with reasonable defaults
/// 3. Verify pack/ directory created
/// 4. Verify pack.toml exists (via MockPackwizOps init side effect)
/// 5. Verify project structure matches expected layout
#[tokio::test]
async fn test_init_zero_config() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            dir: None,
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init command failed: {:?}", result);

    assert!(
        session.filesystem().exists(&workdir.join("empack.yml")),
        "empack.yml should be created in the working directory"
    );

    let pack_dir = workdir.join("pack");
    assert!(
        session.filesystem().exists(&pack_dir),
        "pack/ directory should be created"
    );

    let pack_toml_path = pack_dir.join("pack.toml");
    assert!(
        session.filesystem().exists(&pack_toml_path),
        "pack.toml should exist after packwiz init"
    );

    Ok(())
}

/// Test: empack init with explicit CLI configuration
///
/// Workflow:
/// 1. Run `empack init matrix-fabric --pack-name "Matrix Fabric" --modloader fabric --mc-version 1.21.1`
/// 2. Verify the generated empack.yml reflects the explicit inputs
/// 3. Verify pack.toml created via MockPackwizOps with correct content
#[tokio::test]
async fn test_init_with_explicit_flags() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            dir: Some("matrix-fabric".to_string()),
            pack_name: Some("Matrix Fabric".to_string()),
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.1".to_string()),
            author: Some("Workflow Test".to_string()),
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with name failed: {:?}", result.err());

    let project_dir = workdir.join("matrix-fabric");
    assert!(
        session.filesystem().exists(&project_dir),
        "matrix-fabric directory should be created"
    );

    let empack_yml_path = project_dir.join("empack.yml");
    assert!(
        session.filesystem().exists(&empack_yml_path),
        "empack.yml should be created inside matrix-fabric/"
    );

    let empack_yml = session.filesystem().read_to_string(&empack_yml_path)?;
    assert!(
        empack_yml.contains("name: Matrix Fabric"),
        "empack.yml should persist the explicit pack name"
    );
    assert!(
        empack_yml.contains("author: Workflow Test"),
        "empack.yml should persist the explicit author"
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.21.1"),
        "empack.yml should persist the explicit Minecraft version"
    );
    assert!(
        empack_yml.contains("loader: fabric"),
        "empack.yml should persist the explicit loader"
    );

    let pack_dir = project_dir.join("pack");
    assert!(
        session.filesystem().exists(&pack_dir),
        "pack/ directory should be created inside matrix-fabric/"
    );

    let pack_toml_path = pack_dir.join("pack.toml");
    assert!(
        session.filesystem().exists(&pack_toml_path),
        "pack.toml should exist after init"
    );
    let pack_toml = session.filesystem().read_to_string(&pack_toml_path)?;
    assert!(
        pack_toml.contains("name = \"Matrix Fabric\""),
        "pack.toml should contain the explicit pack name: {pack_toml}"
    );
    assert!(
        pack_toml.contains("minecraft = \"1.21.1\""),
        "pack.toml should contain the explicit Minecraft version: {pack_toml}"
    );
    assert!(
        pack_toml.contains("fabric = "),
        "pack.toml should contain fabric loader entry: {pack_toml}"
    );

    Ok(())
}

/// Test: empack init my-pack (creates directory and initializes inside)
///
/// Workflow:
/// 1. Run `empack init my-pack -y` where my-pack doesn't exist
/// 2. Verify my-pack/ directory was created
/// 3. Verify empack.yml exists inside my-pack/
/// 4. Verify pack/ directory created inside my-pack/
#[tokio::test]
async fn test_init_creates_directory_from_name() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            dir: Some("my-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with name argument failed: {:?}",
        result.err()
    );

    let project_dir = workdir.join("my-pack");
    assert!(
        session.filesystem().exists(&project_dir),
        "my-pack directory should be created"
    );
    assert!(
        session.filesystem().is_directory(&project_dir),
        "my-pack should be a directory"
    );

    assert!(
        session.filesystem().exists(&project_dir.join("empack.yml")),
        "empack.yml should exist inside my-pack/"
    );

    assert!(
        session.filesystem().exists(&project_dir.join("pack")),
        "pack/ directory should exist inside my-pack/"
    );

    Ok(())
}

/// Test: empack init in directory with existing empack.yml (error handling)
///
/// Workflow:
/// 1. Pre-populate empack.yml in mock filesystem
/// 2. Run `empack init -y` (should detect existing project)
/// 3. Verify appropriate error (existing project without --force)
/// 4. Verify original empack.yml is preserved
#[tokio::test]
async fn test_init_existing_project_error() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let existing_yml = r#"empack:
  dependencies: []
  minecraft_version: "1.21.1"
  loader: fabric
  name: "existing-pack"
  author: "Existing Author"
  version: "1.0.0"
"#;
    session
        .filesystem()
        .write_file(&workdir.join("empack.yml"), existing_yml)?;

    let result = execute_command_with_session(
        Commands::Init {
            dir: None,
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Existing project without --force should return Err when refusing to overwrite"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("already contains a modpack project"),
        "Error should mention existing project: {}",
        err_msg
    );

    let empack_yml_content = session
        .filesystem()
        .read_to_string(&workdir.join("empack.yml"))?;
    assert!(
        empack_yml_content.contains("existing-pack"),
        "Original empack.yml should be preserved (not overwritten)"
    );
    assert!(
        empack_yml_content.contains("Existing Author"),
        "Original author should be preserved"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls.is_empty(),
        "Existing project detection should refuse early instead of invoking packwiz"
    );
    assert!(
        !session.filesystem().exists(&workdir.join("pack")),
        "Refused init should not create pack metadata in an existing project"
    );

    Ok(())
}

/// Test: empack init scaffolds template files and directory structure
///
/// Workflow:
/// 1. Run `empack init my-templates --modloader fabric --mc-version 1.21.1`
/// 2. Verify .gitignore exists in project root
/// 3. Verify .packwizignore exists in pack/ directory
/// 4. Verify templates/server/ directory exists
/// 5. Verify templates/client/ directory exists
/// 6. Verify dist/ build output directories exist
#[tokio::test]
async fn test_init_scaffolds_template_files() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            dir: Some("my-templates".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.21.1".to_string()),
            author: Some("Template Test".to_string()),
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with template scaffolding failed: {:?}",
        result.err()
    );

    let project_dir = workdir.join("my-templates");

    // Verify .gitignore exists
    assert!(
        session.filesystem().exists(&project_dir.join(".gitignore")),
        ".gitignore should be created after init"
    );

    // Verify .packwizignore exists in pack/
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join("pack/.packwizignore")),
        ".packwizignore should be created in pack/ after init"
    );

    // Verify templates/server/ directory exists
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join("templates/server")),
        "templates/server/ directory should be created after init"
    );

    // Verify templates/client/ directory exists
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join("templates/client")),
        "templates/client/ directory should be created after init"
    );

    // Verify dist/ build output directories exist
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join("dist/server")),
        "dist/server/ directory should be created after init"
    );
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join("dist/client")),
        "dist/client/ directory should be created after init"
    );

    // Verify .github/workflows/validate.yml exists
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join(".github/workflows/validate.yml")),
        ".github/workflows/validate.yml should be created after init"
    );

    // Verify .github/workflows/release.yml exists
    assert!(
        session
            .filesystem()
            .exists(&project_dir.join(".github/workflows/release.yml")),
        ".github/workflows/release.yml should be created after init"
    );

    Ok(())
}
