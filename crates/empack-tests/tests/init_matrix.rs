use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

#[tokio::test]
async fn test_init_neoforge() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("neoforge-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("neoforge".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with NeoForge failed: {:?}", result);

    let project_dir = workdir.join("neoforge-pack");
    assert!(
        session.filesystem().exists(&project_dir),
        "neoforge-pack directory should exist"
    );

    let empack_yml = session
        .filesystem()
        .read_to_string(&project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: neoforge"),
        "empack.yml should have loader: neoforge, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.21.4")
            || empack_yml.contains("minecraft_version: \"1.21.4\""),
        "empack.yml should have minecraft_version 1.21.4, got: {}",
        empack_yml
    );

    let pack_toml = session
        .filesystem()
        .read_to_string(&project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("neoforge = "),
        "pack.toml [versions] should contain neoforge entry, got: {}",
        pack_toml
    );

    Ok(())
}

#[tokio::test]
async fn test_init_quilt() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("quilt-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("quilt".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with Quilt failed: {:?}", result);

    let project_dir = workdir.join("quilt-pack");
    assert!(
        session.filesystem().exists(&project_dir),
        "quilt-pack directory should exist"
    );

    let empack_yml = session
        .filesystem()
        .read_to_string(&project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: quilt"),
        "empack.yml should have loader: quilt, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.21.4")
            || empack_yml.contains("minecraft_version: \"1.21.4\""),
        "empack.yml should have minecraft_version 1.21.4, got: {}",
        empack_yml
    );

    let pack_toml = session
        .filesystem()
        .read_to_string(&project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("quilt = "),
        "pack.toml [versions] should contain quilt entry, got: {}",
        pack_toml
    );

    Ok(())
}

#[tokio::test]
async fn test_init_vanilla() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("vanilla-pack".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("none".to_string()),
            mc_version: Some("1.21.4".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Init with vanilla failed: {:?}", result);

    let project_dir = workdir.join("vanilla-pack");
    assert!(
        session.filesystem().exists(&project_dir),
        "vanilla-pack directory should exist"
    );

    let empack_yml = session
        .filesystem()
        .read_to_string(&project_dir.join("empack.yml"))?;
    assert!(
        !empack_yml.contains("loader:"),
        "vanilla empack.yml should not have a loader field, got: {}",
        empack_yml
    );

    Ok(())
}

#[tokio::test]
async fn test_init_fabric_older_mc() -> Result<()> {
    let session = MockSessionBuilder::new().with_yes_flag().build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;

    let result = execute_command_with_session(
        Commands::Init {
            name: Some("fabric-old-mc".to_string()),
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: Some("1.20.1".to_string()),
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "Init with Fabric 1.20.1 failed: {:?}",
        result
    );

    let project_dir = workdir.join("fabric-old-mc");
    assert!(
        session.filesystem().exists(&project_dir),
        "fabric-old-mc directory should exist"
    );

    let empack_yml = session
        .filesystem()
        .read_to_string(&project_dir.join("empack.yml"))?;
    assert!(
        empack_yml.contains("loader: fabric"),
        "empack.yml should have loader: fabric, got: {}",
        empack_yml
    );
    assert!(
        empack_yml.contains("minecraft_version: 1.20.1")
            || empack_yml.contains("minecraft_version: \"1.20.1\""),
        "empack.yml should have minecraft_version 1.20.1, got: {}",
        empack_yml
    );

    let pack_toml = session
        .filesystem()
        .read_to_string(&project_dir.join("pack").join("pack.toml"))?;
    assert!(
        pack_toml.contains("1.20.1"),
        "pack.toml should reference minecraft 1.20.1, got: {}",
        pack_toml
    );
    assert!(
        pack_toml.contains("fabric = "),
        "pack.toml [versions] should contain fabric entry, got: {}",
        pack_toml
    );

    Ok(())
}
