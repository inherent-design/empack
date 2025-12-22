use empack_lib::application::config::AppConfig;
use empack_lib::primitives::{ConfigError, TerminalCapsDetectIntent};
use std::env;

#[test]
fn test_config_default_creation() {
    // Simple test for config creation without loading
    let config = AppConfig::default();

    // Just verify the config has reasonable defaults
    assert!(config.log_level <= 4);
    assert!(config.net_timeout > 0);
    assert_eq!(config.color, TerminalCapsDetectIntent::Auto);
}

#[test]
fn test_config_merging_integration() {
    let base_config = AppConfig::default();
    let override_config = AppConfig {
        log_level: 3,
        cpu_jobs: 8,
        color: TerminalCapsDetectIntent::Never,
        ..AppConfig::default()
    };

    let merged = base_config.merge_with(override_config);

    // Override values should be preserved
    assert_eq!(merged.log_level, 3);
    assert_eq!(merged.cpu_jobs, 8);
    assert_eq!(merged.color, TerminalCapsDetectIntent::Never);

    // Default values should remain for non-overridden fields
    assert_eq!(merged.net_timeout, 30); // default timeout
}
