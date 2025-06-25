use std::str::FromStr;

use clap::ValueEnum;

/// Available log output streams
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogOutput {
    /// STDERR
    Stderr,
    /// STDOUT
    Stdout,
}

/// Log levels for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warning = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

/// Output formats for structured logging
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogFormat {
    /// TEXT
    /// alias: text, txt, plain
    Text,

    /// JSON
    /// alias: json
    Json,

    /// YAML
    /// alias: yaml, yml
    Yaml,
}

/// Runtime color detection intent
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCapsDetectIntent {
    /// Let module detect
    /// alias: auto, automatic, detect, default
    Auto,

    /// Explicitly enable (useful in non-interactive)
    /// alias: always, force, on
    Always,

    /// Explicitly disable (also useful in non-interactive)
    /// alias: never, off
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalColorCaps {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalUnicodeCaps {
    Ascii,
    BasicUnicode,
    ExtendedUnicode,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerminalGraphicsCaps {
    None,
    Kitty,
}

impl LogLevel {
    /// Convert verbosity level from AppConfig to LogLevel
    pub fn from_verbosity(verbosity: u8) -> Self {
        match verbosity {
            0 => LogLevel::Error,
            1 => LogLevel::Warning,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            4.. => LogLevel::Trace,
        }
    }

    /// Check if this log level should be displayed given current verbosity
    pub fn should_log(&self, current_level: LogLevel) -> bool {
        *self <= current_level
    }
}

impl ValueEnum for LogLevel {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Error,
            Self::Warning,
            Self::Info,
            Self::Debug,
            Self::Trace,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Error => Some(
                clap::builder::PossibleValue::new("error")
                    .alias("err")
                    .alias("fatal")
                    .alias("critical"),
            ),
            Self::Warning => Some(clap::builder::PossibleValue::new("warn").alias("warning")),
            Self::Info => Some(clap::builder::PossibleValue::new("info").alias("information")),
            Self::Debug => Some(clap::builder::PossibleValue::new("debug").alias("debugging")),
            Self::Trace => Some(
                clap::builder::PossibleValue::new("trace")
                    .alias("tracing")
                    .alias("verbose"),
            ),
        }
    }
}

impl ValueEnum for LogFormat {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Text, Self::Json, Self::Yaml]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Text => Some(
                clap::builder::PossibleValue::new("text")
                    .alias("txt")
                    .alias("plain"),
            ),
            Self::Json => Some(clap::builder::PossibleValue::new("json")),
            Self::Yaml => Some(clap::builder::PossibleValue::new("yaml").alias("yml")),
        }
    }
}

impl ValueEnum for TerminalCapsDetectIntent {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Auto, Self::Always, Self::Never]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Auto => Some(
                clap::builder::PossibleValue::new("auto")
                    .alias("automatic")
                    .alias("detect")
                    .alias("default"),
            ),
            Self::Always => Some(
                clap::builder::PossibleValue::new("always")
                    .alias("force")
                    .alias("on"),
            ),
            Self::Never => Some(clap::builder::PossibleValue::new("never").alias("off")),
        }
    }
}

impl FromStr for LogLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(*variant);
            }
        }

        Err(anyhow::anyhow!("Invalid log level: {}", s))
    }
}

impl FromStr for LogFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(*variant);
            }
        }

        Err(anyhow::anyhow!("Invalid log output format: {}", s))
    }
}

impl FromStr for LogOutput {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(*variant);
            }
        }

        Err(anyhow::anyhow!("Invalid log output stream: {}", s))
    }
}

impl FromStr for TerminalCapsDetectIntent {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(*variant);
            }
        }

        Err(anyhow::anyhow!(
            "Invalid terminal capability detection intent: {}",
            s
        ))
    }
}

#[cfg(test)]
mod tests {
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
    fn test_fromstr_case_sensitivity() {
        // Note: clap PossibleValue.matches() is case-sensitive by default
        // Our implementation should accept exact matches only
        assert!("error".parse::<LogLevel>().is_ok());
        assert!("warn".parse::<LogLevel>().is_ok());
        assert!("info".parse::<LogLevel>().is_ok());

        // Test that case sensitivity is enforced
        assert!("ERROR".parse::<LogLevel>().is_err());
        assert!("Error".parse::<LogLevel>().is_err());
        assert!("eRrOr".parse::<LogLevel>().is_err());

        // Test with different enum types
        assert!("text".parse::<LogFormat>().is_ok());
        assert!("json".parse::<LogFormat>().is_ok());
        assert!("always".parse::<TerminalCapsDetectIntent>().is_ok());

        // Wrong case should fail
        assert!("TEXT".parse::<LogFormat>().is_err());
        assert!("Json".parse::<LogFormat>().is_err());
        assert!("ALWAYS".parse::<TerminalCapsDetectIntent>().is_err());
    }

    #[test]
    fn test_fromstr_invalid_inputs() {
        // Test that invalid inputs properly error
        assert!("invalid".parse::<LogLevel>().is_err());
        assert!("".parse::<LogLevel>().is_err());
        assert!("log_level".parse::<LogLevel>().is_err());
        assert!("errorr".parse::<LogLevel>().is_err()); // Extra letter

        assert!("plaintext".parse::<LogFormat>().is_err());
        assert!("xml".parse::<LogFormat>().is_err());

        assert!("sometimes".parse::<TerminalCapsDetectIntent>().is_err());
        assert!("maybe".parse::<TerminalCapsDetectIntent>().is_err());
    }

    #[test]
    fn test_serde_compatibility() {
        // Test that serde deserialization works with expected formats
        // This ensures config file parsing will work correctly
        // Note: Using simple direct construction tests since serde_json isn't a dep yet

        // Test that serde derives work by checking trait bounds
        fn assert_deserialize<T: for<'de> serde::Deserialize<'de>>() {}

        assert_deserialize::<LogFormat>();
        assert_deserialize::<LogOutput>();
        assert_deserialize::<TerminalCapsDetectIntent>();
        assert_deserialize::<TerminalColorCaps>();
        assert_deserialize::<TerminalUnicodeCaps>();
        assert_deserialize::<TerminalGraphicsCaps>();
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

        let graphics = TerminalGraphicsCaps::Kitty;
        assert_eq!(format!("{:?}", graphics), "Kitty");
    }
}
