use empack_lib::application::{AppConfig, Commands, cli::CliConfig};
use empack_lib::platform::cache::cache_root;
use empack_lib::platform::{browser_open_command, config_dir, data_dir, home_dir};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    unsafe fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match self.previous.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn set(path: &Path) -> Self {
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

fn clear_cli_env() {
    unsafe {
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("FORCE_COLOR");
        std::env::remove_var("CI");
        std::env::remove_var("CLICOLOR");
        std::env::remove_var("CLICOLOR_FORCE");
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

fn has_path_component(path: &Path, expected: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new(expected))
}

#[test]
fn app_config_load_from_parses_cli_and_fills_workdir() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_cli_env();

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let _cwd = CurrentDirGuard::set(temp_dir.path());
    let expected_workdir = std::env::current_dir().expect("current dir");

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

    assert_eq!(config.color, empack_lib::TerminalCapsDetectIntent::Always);
    assert_eq!(config.log_level, 4);
    assert_eq!(config.net_timeout, 45);
    assert_eq!(config.cpu_jobs, 8);
    assert!(config.yes);
    assert!(config.dry_run);
    assert_eq!(config.workdir, Some(expected_workdir));
}

#[test]
fn app_config_load_from_reports_invalid_env_file() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_cli_env();

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let env_file = temp_dir.path().join(".env");
    std::fs::write(&env_file, "not valid dotenv syntax").expect("write env file");
    let _cwd = CurrentDirGuard::set(temp_dir.path());

    let result = AppConfig::load_from(["empack"]);
    assert!(matches!(
        result,
        Err(empack_lib::ConfigError::EnvFileError { .. })
    ));
}

#[test]
fn cli_config_load_from_parses_arguments_and_config() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_cli_env();

    let config = CliConfig::load_from([
        "empack",
        "--color",
        "always",
        "--log-level",
        "4",
        "init",
        "--force",
    ])
    .expect("parse cli config");

    assert_eq!(
        config.app_config.color,
        empack_lib::TerminalCapsDetectIntent::Always
    );
    assert_eq!(config.app_config.log_level, 4);
    assert!(matches!(config.command, Some(Commands::Init(_))));
}

#[test]
fn platform_helpers_cover_env_and_platform_paths() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::TempDir::new().expect("home");
    let userprofile = tempfile::TempDir::new().expect("userprofile");
    let _home = unsafe { EnvVarGuard::set("HOME", home.path()) };
    let _userprofile = unsafe { EnvVarGuard::set("USERPROFILE", userprofile.path()) };

    assert_eq!(home_dir(), PathBuf::from(home.path()));
    let (command, args) = browser_open_command();
    if cfg!(target_os = "macos") {
        assert_eq!(command, "open");
        assert!(args.is_empty());
    }
    assert!(config_dir().is_absolute());
    assert!(data_dir().is_absolute());
    assert!(has_path_component(&config_dir(), "empack"));
    assert!(has_path_component(&data_dir(), "empack"));

    let _ = (&_home, &_userprofile);
}

#[test]
fn cache_root_uses_env_override() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::TempDir::new().expect("temp dir");
    let _env = unsafe { EnvVarGuard::set("EMPACK_CACHE_DIR", temp.path()) };

    assert_eq!(cache_root().expect("cache root"), temp.path());
}
