//! E2E tests for the client-full build target.

use anyhow::Result;
use empack_lib::application::Commands;
use empack_lib::application::cli::CliArchiveFormat;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

#[tokio::test]
async fn e2e_build_client_full_successfully() -> Result<()> {
    let project_name = "workflow-client-full";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        session.filesystem().exists(&client_full_dir),
        "Client-full build directory should exist"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("pack").join("pack.toml")),
        "Pack metadata should be copied into client-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("mods").join("both-installed.txt")),
        "Mock installer should leave a deterministic install marker"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client-full.zip"))),
        "Client-full archive should be created: {create_calls:?}"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Standalone client-full builds should not materialize the client target directory"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    assert!(
        packwiz_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "refresh")),
        "build should refresh pack metadata before client-full build: {packwiz_calls:?}"
    );

    let java_calls = session.process_provider.get_calls_for_command("java");
    assert!(
        java_calls
            .iter()
            .any(|call| call.args.iter().any(|a| a == "-s")
                && call.args.iter().any(|a| a == "both")
                && call.args.iter().any(|a| a == "--bootstrap-main-jar")
                && call.args.iter().any(|a| a.contains("pack.toml"))),
        "client-full build should invoke packwiz installer for both sides: {java_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_client_full_missing_installer() -> Result<()> {
    let project_name = "workflow-client-full-missing-installer";
    let workdir = mock_root().join("workdir");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when installer JAR is unavailable"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Mock HTTP client unavailable (test mode)")
            || error.contains("Failed to read file:"),
        "Missing installer should fail while resolving the bootstrap JAR, got: {error}"
    );
    assert!(
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join(format!("{project_name}-v1.0.0-client-full.zip"))
        ),
        "No client-full archive should be produced when the installer is missing"
    );
    assert!(
        !session.filesystem().exists(
            &workdir
                .join("dist")
                .join("client-full")
                .join("mods")
                .join("both-installed.txt")
        ),
        "The full installer step should not run when the installer bootstrap is missing"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Standalone client-full failures should not create the client target directory"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_client_full_with_pack_structure() -> Result<()> {
    let project_name = "workflow-client-full-structure";
    let workdir = mock_root().join("workdir");
    let mods_dir = workdir.join("pack").join("mods");

    let session = MockSessionBuilder::new()
        .with_empack_project(project_name, "1.21.1", "fabric")
        .with_pre_cached_jars()
        .with_file(
            mods_dir.join("example-mod.pw.toml"),
            "[download]\nurl = \"https://example.com/mod.jar\"\nhash = \"abc123\"\n".to_string(),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["client-full".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Client-full build failed: {result:?}");

    let client_full_dir = workdir.join("dist").join("client-full");
    assert!(
        session.filesystem().exists(&client_full_dir),
        "Client-full build directory should exist"
    );
    assert!(
        session.filesystem().exists(
            &client_full_dir
                .join("pack")
                .join("mods")
                .join("example-mod.pw.toml")
        ),
        "Existing pack structure should be copied into client-full output"
    );
    assert!(
        session
            .filesystem()
            .exists(&client_full_dir.join("mods").join("both-installed.txt")),
        "Installer marker should confirm the mocked full download step"
    );

    let create_calls = session.archive_provider.create_calls.lock().unwrap();
    assert!(
        create_calls.iter().any(|(_, dest)| dest
            .to_string_lossy()
            .contains(&format!("{project_name}-v1.0.0-client-full.zip"))),
        "Client-full archive should be created: {create_calls:?}"
    );
    assert!(
        !session
            .filesystem()
            .exists(&workdir.join("dist").join("client")),
        "Structured standalone client-full builds should not materialize the client target directory"
    );

    Ok(())
}
