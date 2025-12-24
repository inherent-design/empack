use super::*;
use crate::display::test_utils::clean_test_env;
use std::env;

#[test]
fn test_no_color_environment_variable() {
    clean_test_env();
    unsafe {
        env::set_var("NO_COLOR", "1");
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
    assert_eq!(color, TerminalCapsDetectIntent::Never);

    clean_test_env();
}

#[test]
fn test_force_color_environment_variable() {
    clean_test_env();
    unsafe {
        env::set_var("FORCE_COLOR", "1");
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
    assert_eq!(color, TerminalCapsDetectIntent::Always);

    clean_test_env();
}

#[test]
fn test_environment_variable_precedence() {
    clean_test_env();

    // Set all environment variables that affect color
    unsafe {
        env::set_var("CLICOLOR", "0"); // Should disable color
        env::set_var("NO_COLOR", "1"); // Should override CLICOLOR and disable color
        env::set_var("FORCE_COLOR", "1"); // Should override everything and enable color
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);

    // FORCE_COLOR=1 should win, enabling color despite NO_COLOR and CLICOLOR
    assert_eq!(color, TerminalCapsDetectIntent::Always);

    clean_test_env();
}

#[test]
fn test_ci_environment_variable() {
    clean_test_env();
    unsafe {
        env::set_var("CI", "true");
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
    assert_eq!(color, TerminalCapsDetectIntent::Never);

    clean_test_env();
}

#[test]
fn test_empty_no_color_is_ignored() {
    clean_test_env();
    unsafe {
        env::set_var("NO_COLOR", "");
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
    assert_eq!(color, TerminalCapsDetectIntent::Auto);

    clean_test_env();
}

#[test]
fn test_invalid_force_color_values_ignored() {
    clean_test_env();
    unsafe {
        env::set_var("FORCE_COLOR", "invalid");
    }

    let env_config = EnvironmentConfig::load().unwrap();
    let color = env_config.apply_color_config(TerminalCapsDetectIntent::Auto);
    assert_eq!(color, TerminalCapsDetectIntent::Auto);

    clean_test_env();
}
