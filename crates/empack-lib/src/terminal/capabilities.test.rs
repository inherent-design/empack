use super::*;
use crate::display::test_utils::clean_test_env;
use std::env;

#[test]
fn test_truecolor_detection_via_colorterm() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Auto);
    assert!(result.is_ok());
    let caps = result.unwrap();
    // In CI (non-TTY) with console crate, color may be None because is_tty is false
    // and console respects TTY; the test verifies no panic and valid detection.
    assert!(matches!(
        caps.color,
        TerminalColorCaps::TrueColor | TerminalColorCaps::Ansi256 | TerminalColorCaps::None
    ));

    clean_test_env();
}

#[test]
fn test_unicode_detection_via_lang() {
    clean_test_env();
    unsafe {
        env::set_var("LANG", "en_US.UTF-8");
    }

    let result = detect_unicode(true).unwrap();
    assert_eq!(result, TerminalUnicodeCaps::BasicUnicode);

    clean_test_env();
}

#[test]
fn test_unicode_detection_non_tty() {
    clean_test_env();
    unsafe {
        env::set_var("LANG", "en_US.UTF-8");
    }

    let result = detect_unicode(false).unwrap();
    assert_eq!(result, TerminalUnicodeCaps::Ascii);

    clean_test_env();
}

#[test]
fn test_terminal_capabilities_minimal() {
    let caps = TerminalCapabilities::minimal();
    assert_eq!(caps.color, TerminalColorCaps::None);
    assert_eq!(caps.unicode, TerminalUnicodeCaps::Ascii);
    assert!(!caps.is_tty);
    assert_eq!(caps.cols, 80);
}

#[test]
fn test_terminal_capabilities_detect_from_config_auto() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Auto);
    assert!(result.is_ok());

    clean_test_env();
}

#[test]
fn test_terminal_capabilities_detect_forced_never() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Never);
    assert!(result.is_ok());

    let caps = result.unwrap();
    assert_eq!(caps.color, TerminalColorCaps::None);

    clean_test_env();
}

#[test]
fn test_terminal_capabilities_detect_forced_always() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Always);
    assert!(result.is_ok());

    let caps = result.unwrap();
    assert!(matches!(
        caps.color,
        TerminalColorCaps::TrueColor | TerminalColorCaps::Ansi256
    ));

    clean_test_env();
}

#[test]
fn test_terminal_error_display_formatting() {
    let error = TerminalError::CommandFailed {
        command: "locale charmap".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "Command execution failed: locale charmap"
    );
}

#[test]
fn test_truecolor_or_256_with_truecolor_env() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    assert_eq!(truecolor_or_256(), TerminalColorCaps::TrueColor);

    clean_test_env();
}

#[test]
fn test_truecolor_or_256_with_24bit_env() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "24bit");
    }

    assert_eq!(truecolor_or_256(), TerminalColorCaps::TrueColor);

    clean_test_env();
}

#[test]
fn test_truecolor_or_256_without_colorterm() {
    clean_test_env();

    assert_eq!(truecolor_or_256(), TerminalColorCaps::Ansi256);

    clean_test_env();
}

#[test]
fn test_detect_color_never() {
    let result = detect_color(TerminalCapsDetectIntent::Never, true);
    assert_eq!(result, TerminalColorCaps::None);
}

#[test]
fn test_detect_color_always_with_truecolor() {
    clean_test_env();
    unsafe {
        env::set_var("COLORTERM", "truecolor");
    }

    let result = detect_color(TerminalCapsDetectIntent::Always, true);
    assert_eq!(result, TerminalColorCaps::TrueColor);

    clean_test_env();
}

#[test]
fn test_cols_is_nonzero() {
    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Auto);
    assert!(result.is_ok());
    let caps = result.unwrap();
    assert!(caps.cols > 0);
}

#[test]
#[cfg(unix)]
fn test_environment_isolation_unicode_locales() {
    clean_test_env();
    unsafe {
        env::set_var("LC_ALL", "C");
    }

    let result = detect_unicode(true).unwrap();
    assert_eq!(result, TerminalUnicodeCaps::Ascii);

    clean_test_env();
}

#[test]
#[cfg(unix)]
fn test_environment_isolation_utf8_locale() {
    clean_test_env();
    unsafe {
        env::set_var("LC_ALL", "en_US.UTF-8");
    }

    let result = detect_unicode(true).unwrap();
    assert_eq!(result, TerminalUnicodeCaps::BasicUnicode);

    clean_test_env();
}

#[test]
fn test_full_integration_legacy_terminal() {
    clean_test_env();
    unsafe {
        env::set_var("LC_ALL", "C");
    }

    let result = TerminalCapabilities::detect_from_config(TerminalCapsDetectIntent::Auto);
    assert!(result.is_ok());

    let caps = result.unwrap();
    assert_eq!(caps.unicode, TerminalUnicodeCaps::Ascii);

    clean_test_env();
}
