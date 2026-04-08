use super::*;

// =============================================================================
// LEVEL A: EXHAUSTIVE DATA TYPE TESTING
// =============================================================================

macro_rules! test_enum_completeness {
    ($enum_type:ty, $test_name:ident) => {
        #[test]
        fn $test_name() {
            // Test all variants are constructible and round-trip correctly
            for variant in <$enum_type>::value_variants() {
                // Test Debug doesn't panic and produces output
                let debug_str = format!("{:?}", variant);
                assert!(!debug_str.is_empty(), "Debug output should not be empty");

                // Test clap PossibleValue generation
                let possible_value = variant.to_possible_value();
                assert!(
                    possible_value.is_some(),
                    "PossibleValue should exist for all variants"
                );

                // Test round-trip through primary name
                let possible_val = possible_value.unwrap();
                let primary_name = possible_val.get_name();
                let parsed: Result<$enum_type, _> = primary_name.parse();
                assert!(
                    parsed.is_ok(),
                    "Primary name '{}' should parse correctly",
                    primary_name
                );
                assert_eq!(
                    parsed.unwrap(),
                    *variant,
                    "Round-trip should preserve variant"
                );
            }
        }
    };
}

macro_rules! test_fromstr_aliases {
    ($enum_type:ty, $test_name:ident, $expected_mappings:expr) => {
        #[test]
        fn $test_name() {
            let mappings: &[(&str, $enum_type)] = &$expected_mappings;

            for (input, expected) in mappings {
                let parsed: Result<$enum_type, _> = input.parse();
                assert!(
                    parsed.is_ok(),
                    "Failed to parse '{}' for {}",
                    input,
                    stringify!($enum_type)
                );
                assert_eq!(
                    parsed.unwrap(),
                    *expected,
                    "Wrong variant for input '{}', expected {:?}",
                    input,
                    expected
                );
            }
        }
    };
}

// Generate exhaustive tests for all ValueEnum types
test_enum_completeness!(LogLevel, test_log_level_completeness);
test_enum_completeness!(LogFormat, test_log_format_completeness);
test_enum_completeness!(LogOutput, test_log_output_completeness);
test_enum_completeness!(
    TerminalCapsDetectIntent,
    test_tty_caps_detect_intent_completeness
);

// Test all documented aliases work correctly
test_fromstr_aliases!(
    LogLevel,
    test_log_level_aliases,
    [
        ("error", LogLevel::Error),
        ("err", LogLevel::Error),
        ("fatal", LogLevel::Error),
        ("critical", LogLevel::Error),
        ("warn", LogLevel::Warning),
        ("warning", LogLevel::Warning),
        ("info", LogLevel::Info),
        ("information", LogLevel::Info),
        ("debug", LogLevel::Debug),
        ("debugging", LogLevel::Debug),
        ("trace", LogLevel::Trace),
        ("tracing", LogLevel::Trace),
        ("verbose", LogLevel::Trace),
    ]
);

test_fromstr_aliases!(
    LogFormat,
    test_log_format_aliases,
    [
        ("text", LogFormat::Text),
        ("txt", LogFormat::Text),
        ("plain", LogFormat::Text),
        ("json", LogFormat::Json),
        ("yaml", LogFormat::Yaml),
        ("yml", LogFormat::Yaml),
    ]
);

test_fromstr_aliases!(
    LogOutput,
    test_log_output_aliases,
    [("stderr", LogOutput::Stderr), ("stdout", LogOutput::Stdout),]
);

test_fromstr_aliases!(
    TerminalCapsDetectIntent,
    test_tty_caps_detect_intent_aliases,
    [
        ("auto", TerminalCapsDetectIntent::Auto),
        ("automatic", TerminalCapsDetectIntent::Auto),
        ("detect", TerminalCapsDetectIntent::Auto),
        ("default", TerminalCapsDetectIntent::Auto),
        ("always", TerminalCapsDetectIntent::Always),
        ("force", TerminalCapsDetectIntent::Always),
        ("on", TerminalCapsDetectIntent::Always),
        ("never", TerminalCapsDetectIntent::Never),
        ("off", TerminalCapsDetectIntent::Never),
    ]
);

// =============================================================================
// LEVEL B: COMPLEX IMPLEMENTATION BEHAVIORS/VALIDATIONS
// =============================================================================

#[test]
fn test_log_level_from_verbosity_boundary_conditions() {
    // Test expected mappings
    assert_eq!(LogLevel::from_verbosity(0), LogLevel::Error);
    assert_eq!(LogLevel::from_verbosity(1), LogLevel::Warning);
    assert_eq!(LogLevel::from_verbosity(2), LogLevel::Info);
    assert_eq!(LogLevel::from_verbosity(3), LogLevel::Debug);
    assert_eq!(LogLevel::from_verbosity(4), LogLevel::Trace);

    // Test overflow behavior (4.. pattern should map to Trace)
    assert_eq!(LogLevel::from_verbosity(5), LogLevel::Trace);
    assert_eq!(LogLevel::from_verbosity(100), LogLevel::Trace);
    assert_eq!(LogLevel::from_verbosity(u8::MAX), LogLevel::Trace);
}

#[test]
fn test_log_level_should_log_matrix() {
    // Test complete matrix of message level vs current level
    let levels = [
        LogLevel::Error,
        LogLevel::Warning,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    for (i, message_level) in levels.iter().enumerate() {
        for (j, current_level) in levels.iter().enumerate() {
            let should_display = message_level.should_log(*current_level);
            // A message should display if its level <= current level (same or higher verbosity)
            // Error (0) shows at all levels, Trace (4) only shows at Trace level
            let expected = i <= j;
            assert_eq!(
                should_display, expected,
                "message_level: {:?} ({}), current_level: {:?} ({}), should_log: {}, expected: {}",
                message_level, i, current_level, j, should_display, expected
            );
        }
    }
}

#[test]
fn test_log_level_ordering() {
    // Test that LogLevel ordering matches expected severity
    assert!(LogLevel::Error < LogLevel::Warning);
    assert!(LogLevel::Warning < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Debug);
    assert!(LogLevel::Debug < LogLevel::Trace);

    // Test transitivity
    assert!(LogLevel::Error < LogLevel::Trace);
}

#[test]
fn test_data_types_are_copy() {
    // Ensure all primitive types implement Copy for performance
    fn assert_copy<T: Copy>() {}

    assert_copy::<LogLevel>();
    assert_copy::<LogFormat>();
    assert_copy::<LogOutput>();
    assert_copy::<TerminalCapsDetectIntent>();
    assert_copy::<TerminalColorCaps>();
    assert_copy::<TerminalUnicodeCaps>();
    assert_copy::<TerminalGraphicsCaps>();
}

#[test]
fn test_terminal_capability_types_basic() {
    // Basic smoke test for terminal capability enums
    // (More comprehensive tests will be in terminal.rs)

    let color = TerminalColorCaps::TrueColor;
    assert_eq!(format!("{:?}", color), "TrueColor");

    let unicode = TerminalUnicodeCaps::ExtendedUnicode;
    assert_eq!(format!("{:?}", unicode), "ExtendedUnicode");

    let graphics = TerminalGraphicsCaps::None;
    assert_eq!(format!("{:?}", graphics), "None");
}

#[test]
fn test_config_error_display() {
    // Test error message formatting
    let error = ConfigError::AlreadyInitialized;
    assert_eq!(
        error.to_string(),
        "Global configuration already initialized"
    );

    let error = ConfigError::InvalidWorkDir {
        path: "/invalid/path".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "Invalid working directory: /invalid/path"
    );
}

fn basic_caps(color: TerminalColorCaps, unicode: TerminalUnicodeCaps) -> BasicTerminalCapabilities {
    BasicTerminalCapabilities {
        color,
        unicode,
        graphics: TerminalGraphicsCaps::None,
    }
}

#[test]
fn test_terminal_primitives_truecolor_extended_unicode() {
    let primitives = TerminalPrimitives::new(&basic_caps(
        TerminalColorCaps::TrueColor,
        TerminalUnicodeCaps::ExtendedUnicode,
    ));

    assert!(primitives.red.contains("38;2;"));
    assert!(primitives.bg_blue.contains("48;2;"));
    assert_eq!(primitives.checkmark, "✓");
    assert_eq!(primitives.cross, "✗");
    assert_eq!(primitives.warning_symbol, "⚠");
    assert_eq!(primitives.info_symbol, "ℹ");
    assert_eq!(primitives.bullet, "●");
    assert_eq!(primitives.arrow, "→");
}

#[test]
fn test_terminal_primitives_ansi256_ascii() {
    let primitives = TerminalPrimitives::new(&basic_caps(
        TerminalColorCaps::Ansi256,
        TerminalUnicodeCaps::Ascii,
    ));

    assert!(primitives.red.contains("38;5;"));
    assert!(primitives.bg_green.contains("48;5;"));
    assert_eq!(primitives.checkmark, "+");
    assert_eq!(primitives.cross, "x");
    assert_eq!(primitives.warning_symbol, "!");
    assert_eq!(primitives.info_symbol, "i");
    assert_eq!(primitives.bullet, "*");
    assert_eq!(primitives.arrow, "->");
}

#[test]
fn test_terminal_primitives_ansi16_and_no_color() {
    let ansi16 = TerminalPrimitives::new(&basic_caps(
        TerminalColorCaps::Ansi16,
        TerminalUnicodeCaps::BasicUnicode,
    ));
    assert_eq!(ansi16.red, "\x1b[0;91m");
    assert_eq!(ansi16.strikethrough, "");
    assert_eq!(ansi16.checkmark, "+");
    assert_eq!(ansi16.arrow, "->");

    let no_color = TerminalPrimitives::new(&basic_caps(
        TerminalColorCaps::None,
        TerminalUnicodeCaps::Ascii,
    ));
    assert!(no_color.red.is_empty());
    assert!(no_color.bold.is_empty());
    assert!(no_color.reset.is_empty());
    assert_eq!(no_color.checkmark, "+");
    assert_eq!(no_color.bullet, "*");
}

#[test]
fn test_terminal_capability_bridge_to_basic() {
    let caps = crate::terminal::TerminalCapabilities {
        color: TerminalColorCaps::Ansi256,
        unicode: TerminalUnicodeCaps::BasicUnicode,
        is_tty: true,
        cols: 120,
    };

    let basic = from_terminal_capabilities(&caps);
    assert_eq!(basic.color, TerminalColorCaps::Ansi256);
    assert_eq!(basic.unicode, TerminalUnicodeCaps::BasicUnicode);
    assert_eq!(basic.graphics, TerminalGraphicsCaps::None);
}
