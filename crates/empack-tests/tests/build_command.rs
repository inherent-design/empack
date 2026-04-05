//! E2E tests for the build command

use anyhow::Result;
use empack_lib::application::cli::{CliArchiveFormat, Commands};
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::session::ProcessOutput;
use empack_lib::application::session_mocks::mock_root;
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use empack_tests::MockSessionBuilder;

#[tokio::test]
async fn e2e_build_mrpack_successfully() -> Result<()> {
    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-pack", "1.21.1", "fabric")
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let workdir = session.filesystem().current_dir()?;
    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-pack-v1.0.0.mrpack");

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(result.is_ok(), "Build command failed: {:?}", result);

    assert!(
        session.filesystem().exists(&mrpack_path),
        "mrpack build should create an artifact in dist/"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    let pack_file_str = pack_file.to_string_lossy();
    let mrpack_path_str = mrpack_path.to_string_lossy();
    assert!(
        packwiz_calls
            .iter()
            .any(|call| { call.args == vec!["--pack-file", pack_file_str.as_ref(), "refresh"] }),
        "build should refresh the pack before exporting: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file",
                    pack_file_str.as_ref(),
                    "mr",
                    "export",
                    "-o",
                    mrpack_path_str.as_ref(),
                ]
        }),
        "build should export the mrpack artifact through packwiz: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_clean_recreates_mrpack_and_preserves_configuration() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let stale_server_dir = workdir.join("dist").join("server");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-clean-v1.0.0.mrpack");
    let sentinel = workdir.join("sentinel.txt");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-clean", "1.21.1", "fabric")
        .with_file(
            stale_server_dir.join("stale.txt"),
            "stale build output".to_string(),
        )
        .with_file(mrpack_path.clone(), "stale mrpack artifact".to_string())
        .with_file(sentinel.clone(), "preserve me".to_string())
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let pack_file = workdir.join("pack").join("pack.toml");
    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: true,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_ok(),
        "clean-before-build should rebuild the mrpack artifact: {result:?}"
    );
    assert!(
        session.filesystem().exists(&mrpack_path),
        "clean-before-build should recreate the mrpack artifact in dist/"
    );
    let rebuilt_content = session.filesystem().read_to_string(&mrpack_path)?;
    assert_ne!(
        rebuilt_content, "stale mrpack artifact",
        "clean-before-build should replace stale artifact contents with the rebuilt artifact"
    );
    assert!(
        !session.filesystem().exists(&stale_server_dir),
        "clean-before-build should remove stale sibling build directories under dist/"
    );
    assert!(
        session.filesystem().exists(&workdir.join("empack.yml")),
        "clean-before-build should preserve empack.yml"
    );
    assert!(
        session.filesystem().exists(&pack_file),
        "clean-before-build should preserve pack.toml"
    );
    assert!(
        session
            .filesystem()
            .exists(&workdir.join("pack").join("index.toml")),
        "clean-before-build should preserve index.toml"
    );
    assert!(
        session.filesystem().exists(&sentinel),
        "clean-before-build should not remove unrelated project files outside dist/"
    );

    let packwiz_calls = session.process_provider.get_calls_for_command("packwiz");
    let pack_file_str = pack_file.to_string_lossy();
    let mrpack_path_str = mrpack_path.to_string_lossy();
    assert!(
        packwiz_calls
            .iter()
            .any(|call| { call.args == vec!["--pack-file", pack_file_str.as_ref(), "refresh"] }),
        "clean-before-build should refresh the pack after cleaning: {packwiz_calls:?}"
    );
    assert!(
        packwiz_calls.iter().any(|call| {
            call.args
                == vec![
                    "--pack-file",
                    pack_file_str.as_ref(),
                    "mr",
                    "export",
                    "-o",
                    mrpack_path_str.as_ref(),
                ]
        }),
        "clean-before-build should export the rebuilt mrpack artifact: {packwiz_calls:?}"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_packwiz_refresh_fails() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let pack_file = workdir.join("pack").join("pack.toml");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-refresh-fails", "1.21.1", "fabric")
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.to_string_lossy().to_string(),
                "refresh".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: "Error: pack.toml is corrupted".to_string(),
                success: false,
            }),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail fast when packwiz refresh returns a non-zero exit code"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to execute build pipeline"),
        "Refresh failure should propagate a clear packwiz error, got: {error}"
    );
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-refresh-fails-v1.0.0.mrpack");
    assert!(
        !session.filesystem().exists(&mrpack_path),
        "No mrpack artifact should be produced after a failed refresh"
    );

    Ok(())
}

#[tokio::test]
async fn e2e_build_packwiz_export_fails() -> Result<()> {
    let workdir = mock_root().join("workdir");
    let pack_file = workdir.join("pack").join("pack.toml");
    let mrpack_path = workdir
        .join("dist")
        .join("workflow-build-export-fails-v1.0.0.mrpack");

    let session = MockSessionBuilder::new()
        .with_empack_project("workflow-build-export-fails", "1.21.1", "fabric")
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.to_string_lossy().to_string(),
                "refresh".to_string(),
            ],
            Ok(ProcessOutput {
                stdout: "Refreshed packwiz index".to_string(),
                stderr: String::new(),
                success: true,
            }),
        )
        .with_packwiz_result(
            vec![
                "--pack-file".to_string(),
                pack_file.to_string_lossy().to_string(),
                "mr".to_string(),
                "export".to_string(),
                "-o".to_string(),
                mrpack_path.to_string_lossy().to_string(),
            ],
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: "Error: mrpack export failed".to_string(),
                success: false,
            }),
        )
        .build();

    Display::init_or_get(TerminalCapabilities::minimal());

    let result = execute_command_with_session(
        Commands::Build {
            targets: vec!["mrpack".to_string()],
            clean: false,
            format: CliArchiveFormat::Zip,
            downloads_dir: None,
        },
        &session,
    )
    .await;

    assert!(
        result.is_err(),
        "Build should fail when packwiz export returns a non-zero exit code"
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to execute build pipeline")
            || error.contains("Build failed for target Mrpack"),
        "Export failure should surface a clear build error, got: {error}"
    );
    assert!(
        !session.filesystem().exists(&mrpack_path),
        "Export failure should not leave a partial mrpack artifact"
    );

    Ok(())
}
