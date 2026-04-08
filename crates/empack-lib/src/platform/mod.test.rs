use super::*;
use std::ffi::OsString;
use std::path::PathBuf;

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

    unsafe fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
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

#[test]
fn test_system_resources_detection() {
    let resources = SystemResources::detect();

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let resources = resources.expect("Should detect system resources");
        assert!(resources.cpu_cores > 0, "Should detect at least 1 CPU core");
        assert!(resources.total_memory > 0, "Should detect total memory");
        assert!(
            resources.memory_pressure >= 0.0 && resources.memory_pressure <= 1.0,
            "Memory pressure should be between 0.0 and 1.0"
        );
    }
}

#[test]
fn test_system_resources_cache_reuses_singleton() {
    let first = system_resources().expect("system resources") as *const SystemResources;
    let second = system_resources().expect("system resources") as *const SystemResources;
    assert_eq!(first, second);
}

#[test]
fn test_optimal_jobs_calculation() {
    let resources = SystemResources {
        cpu_cores: 8,
        memory_pressure: 0.3,
        total_memory: 16_000_000_000,
        available_memory: 11_200_000_000,
    };

    let jobs = resources.calculate_optimal_jobs(None);
    assert!(jobs >= 1, "Should calculate at least 1 job");
    assert!(jobs <= resources.cpu_cores, "Should not exceed CPU cores");

    let limited_jobs = resources.calculate_optimal_jobs(Some(4));
    assert!(limited_jobs <= 4, "Should respect max jobs limit");
}

#[test]
fn test_memory_pressure_edge_cases() {
    let high_pressure = SystemResources {
        cpu_cores: 4,
        memory_pressure: 0.9,
        total_memory: 8_000_000_000,
        available_memory: 800_000_000,
    };

    let jobs = high_pressure.calculate_optimal_jobs(None);
    assert!(
        jobs >= 1,
        "Should still allow at least 1 job under high pressure"
    );

    let zero_pressure = SystemResources {
        cpu_cores: 8,
        memory_pressure: 0.0,
        total_memory: 16_000_000_000,
        available_memory: 16_000_000_000,
    };

    let jobs = zero_pressure.calculate_optimal_jobs(None);
    assert!(jobs >= 1, "Should handle zero memory pressure gracefully");
}

#[test]
fn test_optimal_jobs_moderate_pressure_branch() {
    let resources = SystemResources {
        cpu_cores: 12,
        memory_pressure: 0.5,
        total_memory: 24_000_000_000,
        available_memory: 12_000_000_000,
    };

    let jobs = resources.calculate_optimal_jobs(None);
    assert!(jobs >= 1);
    assert!(jobs <= resources.cpu_cores);
}

#[test]
fn test_browser_open_command_matches_platform() {
    let (command, args) = browser_open_command();

    if cfg!(target_os = "macos") {
        assert_eq!(command, "open");
        assert!(args.is_empty());
    } else if cfg!(target_os = "windows") {
        assert_eq!(command, "cmd");
        assert_eq!(args, vec!["/c", "start", ""]);
    } else {
        assert_eq!(command, "xdg-open");
        assert!(args.is_empty());
    }
}

#[test]
fn test_home_dir_prefers_home_and_supports_fallbacks() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let home_fixture = tempfile::TempDir::new().expect("home dir");
    let userprofile_fixture = tempfile::TempDir::new().expect("userprofile dir");
    let home_path = home_fixture.path().to_path_buf();
    let userprofile_path = userprofile_fixture.path().to_path_buf();

    {
        let _home = unsafe { EnvVarGuard::set("HOME", &home_path) };
        let _userprofile = unsafe { EnvVarGuard::set("USERPROFILE", &userprofile_path) };
        assert_eq!(crate::platform::home_dir(), home_path);
        let _ = (&_home, &_userprofile);
    }

    {
        let _home_missing = unsafe { EnvVarGuard::remove("HOME") };
        let _userprofile = unsafe { EnvVarGuard::set("USERPROFILE", &userprofile_path) };
        assert_eq!(crate::platform::home_dir(), userprofile_path);
        let _ = (&_home_missing, &_userprofile);
    }

    {
        let _home_missing = unsafe { EnvVarGuard::remove("HOME") };
        let _userprofile_missing = unsafe { EnvVarGuard::remove("USERPROFILE") };
        assert_eq!(crate::platform::home_dir(), PathBuf::from("."));
        let _ = (&_home_missing, &_userprofile_missing);
    }
}

#[test]
fn test_config_and_data_dirs_follow_home_directory() {
    let _guard = crate::test_support::env_lock().lock().unwrap();
    let home_fixture = tempfile::TempDir::new().expect("home dir");
    let _home = unsafe { EnvVarGuard::set("HOME", home_fixture.path()) };
    let _userprofile = unsafe { EnvVarGuard::remove("USERPROFILE") };

    let config = config_dir();
    let data = data_dir();

    assert!(config.starts_with(home_fixture.path()));
    assert!(data.starts_with(home_fixture.path()));
}
