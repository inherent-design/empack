use super::*;
use crate::primitives::TerminalCapsDetectIntent;

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
