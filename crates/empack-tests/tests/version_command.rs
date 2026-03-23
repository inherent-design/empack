use anyhow::Result;
use empack_lib::application::cli::Commands;
use empack_lib::application::commands::execute_command_with_session;
use empack_lib::application::config::AppConfig;
use empack_lib::application::session::{
    CommandSession, LiveConfigProvider, LiveFileSystemProvider, LiveNetworkProvider,
};
use empack_lib::application::session_mocks::{MockInteractiveProvider, MockProcessProvider};
use empack_lib::display::Display;
use empack_lib::terminal::TerminalCapabilities;
use tempfile::TempDir;

#[tokio::test]
async fn e2e_version_prints_successfully() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let workdir = temp_dir.path().to_path_buf();

    std::env::set_current_dir(&workdir)?;

    let app_config = AppConfig {
        workdir: Some(workdir.clone()),
        ..AppConfig::default()
    };

    let terminal_caps = TerminalCapabilities::detect_from_config(&app_config)?;
    Display::init_or_get(terminal_caps);

    let session = CommandSession::new_with_providers(
        LiveFileSystemProvider,
        LiveNetworkProvider::new(),
        MockProcessProvider::new(),
        LiveConfigProvider::new(app_config),
        MockInteractiveProvider::new(),
    );

    let result = execute_command_with_session(Commands::Version, &session).await;

    assert!(
        result.is_ok(),
        "Version command should succeed without a modpack directory: {:?}",
        result
    );

    // Verify the compiled-in version is a valid semver string.
    // handle_version formats this as "empack {version}" via session.display(),
    // which writes through LiveDisplayProvider (indicatif MultiProgress) and
    // isn't capturable in tests. Asserting the source value ensures the
    // command won't emit garbage if the Cargo.toml version is malformed.
    let pkg_version = env!("CARGO_PKG_VERSION");
    assert!(
        !pkg_version.is_empty(),
        "CARGO_PKG_VERSION should not be empty"
    );

    // Strip pre-release suffix (e.g. "0.0.0-alpha.1" -> "0.0.0") then validate major.minor.patch
    let base_version = pkg_version.split('-').next().unwrap();
    let parts: Vec<&str> = base_version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "Version '{}' should have exactly three semver components (major.minor.patch)",
        pkg_version
    );
    for (i, part) in parts.iter().enumerate() {
        assert!(
            part.parse::<u32>().is_ok(),
            "Version component {} ('{}') in '{}' should be a valid number",
            i,
            part,
            pkg_version
        );
    }

    Ok(())
}
