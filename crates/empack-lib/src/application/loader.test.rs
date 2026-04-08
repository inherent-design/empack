use super::*;
use crate::primitives::TerminalCapsDetectIntent;
use std::path::{Path, PathBuf};

fn clear_cli_env() {
    unsafe {
        std::env::remove_var("EMPACK_WORKDIR");
        std::env::remove_var("EMPACK_CPU_JOBS");
        std::env::remove_var("EMPACK_NET_TIMEOUT");
        std::env::remove_var("EMPACK_ID_MODRINTH");
        std::env::remove_var("EMPACK_KEY_MODRINTH");
        std::env::remove_var("EMPACK_KEY_CURSEFORGE");
        std::env::remove_var("EMPACK_LOG_LEVEL");
        std::env::remove_var("EMPACK_LOG_FORMAT");
        std::env::remove_var("EMPACK_LOG_OUTPUT");
        std::env::remove_var("EMPACK_COLOR");
        std::env::remove_var("EMPACK_YES");
        std::env::remove_var("EMPACK_DRY_RUN");
        std::env::remove_var("EMPACK_MODLOADER");
        std::env::remove_var("EMPACK_MC_VERSION");
        std::env::remove_var("EMPACK_AUTHOR");
        std::env::remove_var("EMPACK_NAME");
        std::env::remove_var("EMPACK_LOADER_VERSION");
        std::env::remove_var("EMPACK_PACK_VERSION");
        std::env::remove_var("EMPACK_DATAPACK_FOLDER");
        std::env::remove_var("EMPACK_GAME_VERSIONS");
    }
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(path).expect("set current dir");
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).expect("restore current dir");
    }
}

fn assert_same_existing_path(actual: Option<PathBuf>, expected: &Path) {
    let actual = actual.expect("path should be present");
    let actual = std::fs::canonicalize(actual).expect("canonical actual path");
    let expected = std::fs::canonicalize(expected).expect("canonical expected path");
    assert_eq!(actual, expected);
}

#[test]
fn test_config_loading_defaults() {
    let config = AppConfig::default();
    assert_eq!(config.log_level, 0);
    assert_eq!(config.net_timeout, 30);
    assert_eq!(config.color, TerminalCapsDetectIntent::Auto);
}

#[test]
fn test_config_merging() {
    let base = AppConfig::default();
    let override_config = AppConfig {
        workdir: Some(PathBuf::from("/tmp/override-workdir")),
        modrinth_api_client_id: Some("modrinth-id".to_string()),
        modrinth_api_client_key: Some("modrinth-key".to_string()),
        curseforge_api_client_key: Some("curseforge-key".to_string()),
        log_level: 4,
        net_timeout: 45,
        color: TerminalCapsDetectIntent::Always,
        cpu_jobs: 16,
        yes: true,
        dry_run: true,
        log_format: crate::primitives::LogFormat::Yaml,
        log_output: crate::primitives::LogOutput::Stdout,
    };

    let merged = base.merge_with(override_config);
    assert_eq!(
        merged.workdir,
        Some(PathBuf::from("/tmp/override-workdir"))
    );
    assert_eq!(
        merged.modrinth_api_client_id,
        Some("modrinth-id".to_string())
    );
    assert_eq!(
        merged.modrinth_api_client_key,
        Some("modrinth-key".to_string())
    );
    assert_eq!(
        merged.curseforge_api_client_key,
        Some("curseforge-key".to_string())
    );
    assert_eq!(merged.log_level, 4);
    assert_eq!(merged.net_timeout, 45);
    assert_eq!(merged.color, TerminalCapsDetectIntent::Always);
    assert_eq!(merged.cpu_jobs, 16);
    assert!(merged.yes);
    assert!(merged.dry_run);
    assert_eq!(merged.log_format, crate::primitives::LogFormat::Yaml);
    assert_eq!(merged.log_output, crate::primitives::LogOutput::Stdout);
}

#[test]
fn test_to_logger_config_uses_app_settings() {
    let app_config = AppConfig {
        log_level: 3,
        log_format: crate::primitives::LogFormat::Json,
        log_output: crate::primitives::LogOutput::Stdout,
        ..AppConfig::default()
    };
    let terminal_caps = crate::terminal::TerminalCapabilities::minimal();

    let logger_config = app_config.to_logger_config(&terminal_caps);

    assert_eq!(logger_config.level, crate::primitives::LogLevel::Debug);
    assert_eq!(logger_config.format, crate::primitives::LogFormat::Json);
    assert_eq!(logger_config.output, crate::primitives::LogOutput::Stdout);
    assert_eq!(logger_config.terminal_caps.color, terminal_caps.color);
    assert_eq!(logger_config.terminal_caps.unicode, terminal_caps.unicode);
    assert_eq!(logger_config.terminal_caps.is_tty, terminal_caps.is_tty);
    assert_eq!(logger_config.terminal_caps.cols, terminal_caps.cols);
}

#[test]
fn test_load_from_parses_cli_and_fills_workdir() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    crate::display::test_utils::clean_test_env();
    clear_cli_env();

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let _cwd = CurrentDirGuard::set(temp_dir.path());

    let config = AppConfig::load_from([
        "empack",
        "--color",
        "always",
        "--log-level",
        "4",
        "--net-timeout",
        "45",
        "-j",
        "8",
        "--yes",
        "--dry-run",
    ])
    .expect("load config");

    assert_eq!(config.color, TerminalCapsDetectIntent::Always);
    assert_eq!(config.log_level, 4);
    assert_eq!(config.net_timeout, 45);
    assert_eq!(config.cpu_jobs, 8);
    assert!(config.yes);
    assert!(config.dry_run);
    assert_same_existing_path(config.workdir, temp_dir.path());
}

#[test]
fn test_load_from_reads_env_local_and_applies_cli_overrides() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    crate::display::test_utils::clean_test_env();
    clear_cli_env();

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let env_workdir = temp_dir.path().join("env-workdir");
    std::fs::create_dir_all(&env_workdir).expect("create env workdir");
    let env_local = temp_dir.path().join(".env.local");
    std::fs::write(
        &env_local,
        format!(
            "EMPACK_WORKDIR={}\nEMPACK_CPU_JOBS=12\nEMPACK_NET_TIMEOUT=37\nEMPACK_ID_MODRINTH=modrinth-id\nEMPACK_KEY_MODRINTH=modrinth-key\nEMPACK_KEY_CURSEFORGE=curseforge-key\nEMPACK_LOG_LEVEL=2\nEMPACK_LOG_FORMAT=json\nEMPACK_LOG_OUTPUT=stderr\nEMPACK_COLOR=never\nEMPACK_YES=true\nEMPACK_DRY_RUN=true\n",
            env_workdir.display().to_string().replace('\\', "/"),
        ),
    )
    .expect("write env local");

    let _cwd = CurrentDirGuard::set(temp_dir.path());

    let config = AppConfig::load_from([
        "empack",
        "--color",
        "always",
        "--log-level",
        "4",
        "--log-format",
        "yaml",
        "--log-output",
        "stdout",
        "--net-timeout",
        "45",
        "-j",
        "8",
    ])
    .expect("load config from env local");

    assert_same_existing_path(config.workdir, &env_workdir);
    assert_eq!(config.cpu_jobs, 8);
    assert_eq!(config.net_timeout, 45);
    assert_eq!(config.modrinth_api_client_id, Some("modrinth-id".to_string()));
    assert_eq!(config.modrinth_api_client_key, Some("modrinth-key".to_string()));
    assert_eq!(
        config.curseforge_api_client_key,
        Some("curseforge-key".to_string())
    );
    assert_eq!(config.log_level, 4);
    assert_eq!(config.log_format, crate::primitives::LogFormat::Yaml);
    assert_eq!(config.log_output, crate::primitives::LogOutput::Stdout);
    assert_eq!(config.color, TerminalCapsDetectIntent::Always);
    assert!(config.yes);
    assert!(config.dry_run);
}

#[test]
fn test_load_from_reports_invalid_env_file() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    crate::display::test_utils::clean_test_env();
    clear_cli_env();

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let env_file = temp_dir.path().join(".env");
    std::fs::write(&env_file, "not valid dotenv syntax").expect("write invalid env file");

    let _cwd = CurrentDirGuard::set(temp_dir.path());

    let result = AppConfig::load_from(["empack"]);
    assert!(matches!(result, Err(crate::primitives::ConfigError::EnvFileError { .. })));
}

#[test]
fn test_init_global_and_global_round_trip() {
    let config = AppConfig::default();
    AppConfig::init_global(config.clone()).expect("init global");
    assert_eq!(AppConfig::global().log_level, config.log_level);
    assert!(matches!(
        AppConfig::init_global(AppConfig::default()),
        Err(crate::primitives::ConfigError::AlreadyInitialized)
    ));
}
