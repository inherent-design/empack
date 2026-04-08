use super::*;
use crate::primitives::TerminalCapsDetectIntent;
use std::path::PathBuf;

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
        log_level: 4,
        color: TerminalCapsDetectIntent::Always,
        cpu_jobs: 16,
        ..AppConfig::default()
    };

    let merged = base.merge_with(override_config);
    assert_eq!(merged.log_level, 4);
    assert_eq!(merged.color, TerminalCapsDetectIntent::Always);
    assert_eq!(merged.cpu_jobs, 16);
    assert_eq!(merged.net_timeout, 30);
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
    assert_eq!(
        config.workdir,
        Some(std::fs::canonicalize(temp_dir.path()).expect("canonical temp dir"))
    );
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
