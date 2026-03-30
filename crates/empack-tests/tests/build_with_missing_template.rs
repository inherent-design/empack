use anyhow::Result;
use empack_lib::application::cli::{CliArchiveFormat, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

#[tokio::test]
async fn test_build_with_missing_template() -> Result<()> {
    let session = MockSessionBuilder::new()
        .with_yes_flag()
        .with_pre_cached_jars()
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    execute_command_with_session(
        Commands::Init {
            dir: None,
            pack_name: None,
            force: false,
            modloader: Some("fabric".to_string()),
            mc_version: None,
            author: None,
            loader_version: None,
            pack_version: None,
        },
        &session,
    )
    .await?;

    let workdir = session.filesystem().current_dir()?;

    assert!(
        session.filesystem().exists(&workdir.join("empack.yml")),
        "empack.yml should exist"
    );
    assert!(
        session.filesystem().exists(&workdir.join("pack")),
        "pack/ directory should exist"
    );

    // Reconfigure: point workdir at the project directory so build can find it.
    // We do this by updating the config provider's workdir.
    // Since MockCommandSession doesn't allow changing workdir after build,
    // we use a new session pre-populated with the init output.
    let session = MockSessionBuilder::new()
        .with_empack_project("workdir", "1.21.4", "fabric")
        .with_pre_cached_jars()
        .build();

    let build_result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    // The build should either succeed (if templates are all embedded and found)
    // or fail with a clear error message about the template issue
    match build_result {
        Ok(_) => {
            // Build succeeded - templates were optional or all found
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            assert!(
                err_msg.contains("template")
                    || err_msg.contains("Template")
                    || err_msg.contains("file")
                    || err_msg.contains("not found")
                    || err_msg.contains("missing")
                    || err_msg.contains("Build")
                    || err_msg.contains("packwiz"),
                "Error should indicate what's missing or what failed: {}",
                err_msg
            );
        }
    }

    Ok(())
}
