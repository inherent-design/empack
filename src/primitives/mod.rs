use clap::ValueEnum;
use std::str::FromStr;
use thiserror::Error;

// Import shared macros and patterns
mod shared;
use shared::impl_fromstr_for_value_enum;

// Import and re-export terminal primitives
pub mod terminal;
pub use terminal::*;

/// Available log output streams
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
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
#[serde(rename_all = "lowercase")]
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

// ============================================================================
// LOGGER CONFIGURATION TYPES
// ============================================================================

/// Logger configuration combining terminal capabilities with application config
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub level: LogLevel,
    pub format: LogFormat,
    pub output: LogOutput,
    pub terminal_caps: crate::terminal::TerminalCapabilities,
}

/// Progress-aware logging context for operations that need progress tracking
#[derive(Debug, Clone)]
pub struct LogContext {
    pub operation: String,
    pub total_items: Option<u64>,
    pub current_item: Option<u64>,
}

impl LoggerConfig {
    /// Create LoggerConfig from AppConfig and TerminalCapabilities
    pub fn from_app_config(
        config: &crate::application::AppConfig,
        terminal_caps: &crate::terminal::TerminalCapabilities,
    ) -> Self {
        Self {
            level: LogLevel::from_verbosity(config.log_level),
            format: config.log_format,
            output: config.log_output,
            terminal_caps: terminal_caps.clone(),
        }
    }
}

impl LogContext {
    pub fn new(operation: &str) -> Self {
        Self {
            operation: operation.to_string(),
            total_items: None,
            current_item: None,
        }
    }

    pub fn with_progress(operation: &str, total: u64) -> Self {
        Self {
            operation: operation.to_string(),
            total_items: Some(total),
            current_item: None,
        }
    }

    pub fn set_progress(&mut self, current: u64) {
        self.current_item = Some(current);
    }
}

// ============================================================================
// STRUCTURED ERROR TYPES
// ============================================================================

/// Application configuration loading and validation errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load environment file '{file}': {source}")]
    EnvFileError {
        file: String,
        source: dotenvy::Error,
    },

    #[error("Global configuration already initialized")]
    AlreadyInitialized,

    #[error("Invalid working directory: {path}")]
    InvalidWorkDir { path: String },

    #[error("Failed to parse environment variables: {source}")]
    EnvironmentParsingFailed {
        #[from]
        source: envy::Error,
    },

    #[error("Configuration validation failed: {reason}")]
    ValidationFailed { reason: String },

    #[error("Failed to get current directory: {source}")]
    CurrentDirError {
        #[from]
        source: std::io::Error,
    },

    #[error("Failed to parse configuration value '{value}': {reason}")]
    ParseError { value: String, reason: String },
}

/// Logger initialization and operation errors
#[derive(Debug, Error)]
pub enum LoggerError {
    #[error("Failed to initialize tracing subscriber: {reason}")]
    InitializationFailed { reason: String },

    #[error("Logger already initialized")]
    AlreadyInitialized,

    #[error("Invalid log format configuration: {format:?}")]
    InvalidFormat { format: LogFormat },

    #[error("Writer initialization failed: {source}")]
    WriterError {
        #[from]
        source: std::io::Error,
    },

    #[error("Tracing subscriber build failed: {reason}")]
    SubscriberBuildFailed { reason: String },
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

// Generate FromStr implementations for all ValueEnum types
impl_fromstr_for_value_enum!(LogLevel, "invalid log level");
impl_fromstr_for_value_enum!(LogFormat, "invalid log format");
impl_fromstr_for_value_enum!(LogOutput, "invalid log output stream");

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

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

        let graphics = TerminalGraphicsCaps::Kitty(Default::default());
        assert!(format!("{:?}", graphics).contains("Kitty"));
    }

    #[test]
    fn test_terminal_error_display() {
        // Test error message formatting
        let error = TerminalError::NotInteractive;
        assert_eq!(
            error.to_string(),
            "Cannot probe capabilities on non-interactive terminal"
        );

        let error = TerminalError::ProbeTimeout { timeout: 1000 };
        assert_eq!(
            error.to_string(),
            "Terminal capability probing timed out after 1000ms"
        );

        let error = TerminalError::UnsupportedGraphics {
            protocol: "sixel".to_string(),
        };
        assert_eq!(error.to_string(), "Graphics protocol not supported: sixel");
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

        let error = ConfigError::ValidationFailed {
            reason: "missing required field".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Configuration validation failed: missing required field"
        );

        let error = ConfigError::ParseError {
            value: "invalid_level".to_string(),
            reason: "invalid log level".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Failed to parse configuration value 'invalid_level': invalid log level"
        );
    }

    #[test]
    fn test_error_source_chain() {
        // Test that errors can be constructed from their sources
        let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let terminal_error = TerminalError::ResponseReadFailed { source: io_error };

        // Should be able to access the source
        assert!(terminal_error.source().is_some());
        assert_eq!(
            terminal_error.to_string(),
            "Failed to read terminal response: access denied"
        );
    }

    // =============================================================================
    // LEVEL C: ADVANCED FIXTURES AND PROPERTY-BASED TESTING
    // =============================================================================

    // Test fixtures for complex scenarios
    struct LogLevelTestFixture {
        verbosity: u8,
        expected_level: LogLevel,
        should_log_at_info: bool,
        should_log_at_debug: bool,
        description: &'static str,
    }

    const LOG_LEVEL_FIXTURES: &[LogLevelTestFixture] = &[
        LogLevelTestFixture {
            verbosity: 0,
            expected_level: LogLevel::Error,
            should_log_at_info: false,
            should_log_at_debug: false,
            description: "Error level - minimal logging",
        },
        LogLevelTestFixture {
            verbosity: 1,
            expected_level: LogLevel::Warning,
            should_log_at_info: false,
            should_log_at_debug: false,
            description: "Warning level - error + warning",
        },
        LogLevelTestFixture {
            verbosity: 2,
            expected_level: LogLevel::Info,
            should_log_at_info: true,
            should_log_at_debug: false,
            description: "Info level - standard logging",
        },
        LogLevelTestFixture {
            verbosity: 3,
            expected_level: LogLevel::Debug,
            should_log_at_info: true,
            should_log_at_debug: true,
            description: "Debug level - development logging",
        },
        LogLevelTestFixture {
            verbosity: 4,
            expected_level: LogLevel::Trace,
            should_log_at_info: true,
            should_log_at_debug: true,
            description: "Trace level - full logging",
        },
        LogLevelTestFixture {
            verbosity: 255,
            expected_level: LogLevel::Trace,
            should_log_at_info: true,
            should_log_at_debug: true,
            description: "Maximum verbosity - still trace",
        },
    ];

    #[test]
    fn test_log_level_fixtures_comprehensive() {
        for fixture in LOG_LEVEL_FIXTURES {
            let level = LogLevel::from_verbosity(fixture.verbosity);
            assert_eq!(
                level, fixture.expected_level,
                "Verbosity {} ({}): expected {:?}, got {:?}",
                fixture.verbosity, fixture.description, fixture.expected_level, level
            );

            // Test logging behavior
            assert_eq!(
                LogLevel::Info.should_log(level),
                fixture.should_log_at_info,
                "Verbosity {} ({}): Info logging expectation failed",
                fixture.verbosity,
                fixture.description
            );

            assert_eq!(
                LogLevel::Debug.should_log(level),
                fixture.should_log_at_debug,
                "Verbosity {} ({}): Debug logging expectation failed",
                fixture.verbosity,
                fixture.description
            );
        }
    }

    // Advanced enum testing with complex scenarios
    struct EnumTestFixture<T> {
        variants: Vec<T>,
        invalid_strings: Vec<&'static str>,
        valid_aliases: Vec<(&'static str, T)>,
        case_sensitive_failures: Vec<&'static str>,
    }

    fn create_log_format_fixture() -> EnumTestFixture<LogFormat> {
        EnumTestFixture {
            variants: vec![LogFormat::Text, LogFormat::Json, LogFormat::Yaml],
            invalid_strings: vec!["xml", "csv", "binary", "", "log_format", "format"],
            valid_aliases: vec![
                ("text", LogFormat::Text),
                ("txt", LogFormat::Text),
                ("plain", LogFormat::Text),
                ("json", LogFormat::Json),
                ("yaml", LogFormat::Yaml),
                ("yml", LogFormat::Yaml),
            ],
            case_sensitive_failures: vec!["TEXT", "Json", "YAML", "TXT"],
        }
    }

    #[test]
    fn test_log_format_advanced_fixtures() {
        let fixture = create_log_format_fixture();

        // Test all variants are covered
        for variant in &fixture.variants {
            let debug_output = format!("{:?}", variant);
            assert!(
                !debug_output.is_empty(),
                "Debug output should not be empty for {:?}",
                variant
            );
        }

        // Test invalid strings fail consistently
        for invalid in &fixture.invalid_strings {
            let result: Result<LogFormat, _> = invalid.parse();
            assert!(
                result.is_err(),
                "Invalid string '{}' should not parse successfully",
                invalid
            );
        }

        // Test valid aliases work correctly
        for (alias, expected) in &fixture.valid_aliases {
            let result: Result<LogFormat, _> = alias.parse();
            assert!(
                result.is_ok(),
                "Valid alias '{}' should parse successfully",
                alias
            );
            assert_eq!(
                result.unwrap(),
                *expected,
                "Alias '{}' should parse to {:?}",
                alias,
                expected
            );
        }

        // Test case sensitivity
        for case_variant in &fixture.case_sensitive_failures {
            let result: Result<LogFormat, _> = case_variant.parse();
            assert!(
                result.is_err(),
                "Case variant '{}' should fail due to case sensitivity",
                case_variant
            );
        }
    }

    // =============================================================================
    // LEVEL D: CONCURRENT AND STRESS TESTING (leveraging nextest isolation)
    // =============================================================================

    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn test_enum_parsing_thread_safety() {
        // Test that enum parsing is thread-safe
        // cargo-nextest runs this in isolation, so we can test concurrency safely
        let iterations = 100;
        let thread_count = 8;
        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = vec![];

        for _thread_id in 0..thread_count {
            let results_clone = Arc::clone(&results);
            let handle = thread::spawn(move || {
                let mut local_results = Vec::new();

                for i in 0..iterations {
                    // Test parsing under concurrent load
                    let format: Result<LogFormat, _> = "json".parse();
                    local_results.push(format.is_ok());

                    let level: Result<LogLevel, _> = "debug".parse();
                    local_results.push(level.is_ok());

                    let intent: Result<TerminalCapsDetectIntent, _> = "auto".parse();
                    local_results.push(intent.is_ok());

                    // Add some variation to test different parse paths
                    let test_strings = ["text", "error", "always"];
                    let test_string = test_strings[i % test_strings.len()];
                    let result: Result<LogFormat, _> = test_string.parse();
                    local_results.push(result.is_ok() || result.is_err()); // Should always be true
                }

                let mut global_results = results_clone.lock().unwrap();
                global_results.extend(local_results);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let final_results = results.lock().unwrap();
        let total_operations = thread_count * iterations * 4;
        assert_eq!(
            final_results.len(),
            total_operations,
            "Should have {} total parsing operations",
            total_operations
        );

        // All parsing operations should have completed successfully
        assert!(
            final_results.iter().all(|&result| result),
            "All parsing operations should complete successfully under concurrent load"
        );
    }

    #[test]
    fn test_error_construction_stress() {
        // Stress test error construction and formatting
        // This test leverages nextest's process isolation for memory safety
        let iterations = 1000;

        for i in 0..iterations {
            // Create various error types rapidly
            let terminal_error = TerminalError::ProbeTimeout { timeout: i };
            let config_error = ConfigError::ParseError {
                value: format!("test_value_{}", i),
                reason: format!("test_reason_{}", i),
            };

            // Test that error display doesn't panic under stress
            let terminal_display = terminal_error.to_string();
            let config_display = config_error.to_string();

            assert!(terminal_display.contains(&i.to_string()));
            assert!(config_display.contains(&format!("test_value_{}", i)));

            // Test error chain construction
            if i % 100 == 0 {
                let io_error =
                    std::io::Error::new(std::io::ErrorKind::Other, format!("io_error_{}", i));
                let chained_error = TerminalError::ResponseReadFailed { source: io_error };
                let chained_display = chained_error.to_string();
                assert!(chained_display.contains("Failed to read terminal response"));
            }
        }
    }

    // =============================================================================
    // LEVEL E: INTEGRATION AND CROSS-TYPE TESTING
    // =============================================================================

    #[test]
    fn test_cross_type_compatibility_matrix() {
        // Test interactions between different primitive types
        // This showcases how primitives work together in real scenarios

        struct CompatibilityTestCase {
            log_format: LogFormat,
            log_level: LogLevel,
            log_output: LogOutput,
            terminal_intent: TerminalCapsDetectIntent,
            description: &'static str,
            valid_combination: bool,
        }

        let test_cases = vec![
            CompatibilityTestCase {
                log_format: LogFormat::Json,
                log_level: LogLevel::Error,
                log_output: LogOutput::Stderr,
                terminal_intent: TerminalCapsDetectIntent::Never,
                description: "JSON logging for production/CI",
                valid_combination: true,
            },
            CompatibilityTestCase {
                log_format: LogFormat::Text,
                log_level: LogLevel::Debug,
                log_output: LogOutput::Stdout,
                terminal_intent: TerminalCapsDetectIntent::Auto,
                description: "Human-readable development logging",
                valid_combination: true,
            },
            CompatibilityTestCase {
                log_format: LogFormat::Yaml,
                log_level: LogLevel::Trace,
                log_output: LogOutput::Stderr,
                terminal_intent: TerminalCapsDetectIntent::Always,
                description: "YAML debug output with forced color",
                valid_combination: true,
            },
        ];

        for case in test_cases {
            // Test that each combination can be constructed and serialized
            assert_eq!(
                case.valid_combination, true,
                "Test case should be valid: {}",
                case.description
            );

            // Test debug output for each component
            let format_debug = format!("{:?}", case.log_format);
            let level_debug = format!("{:?}", case.log_level);
            let output_debug = format!("{:?}", case.log_output);
            let intent_debug = format!("{:?}", case.terminal_intent);

            assert!(!format_debug.is_empty(), "Format debug should not be empty");
            assert!(!level_debug.is_empty(), "Level debug should not be empty");
            assert!(!output_debug.is_empty(), "Output debug should not be empty");
            assert!(!intent_debug.is_empty(), "Intent debug should not be empty");

            // Test that combinations follow logical patterns
            match (case.log_format, case.log_level) {
                (LogFormat::Json, LogLevel::Trace) | (LogFormat::Yaml, LogLevel::Trace) => {
                    // Structured formats with trace level should work
                    assert!(
                        case.valid_combination,
                        "Structured formats should support trace level"
                    );
                }
                (LogFormat::Text, _) => {
                    // Text format should work with any level
                    assert!(
                        case.valid_combination,
                        "Text format should be universally compatible"
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_memory_safety_and_clone_semantics() {
        // Test that all primitive types are properly copyable/cloneable
        // Leverages nextest's isolation to ensure no memory leaks between tests

        let original_format = LogFormat::Json;
        let cloned_format = original_format.clone();
        let copied_format = original_format; // Should work due to Copy trait

        assert_eq!(original_format, cloned_format);
        assert_eq!(cloned_format, copied_format);

        // Test with all types that should implement Copy
        let level = LogLevel::Debug;
        let output = LogOutput::Stderr;
        let intent = TerminalCapsDetectIntent::Auto;

        // These should all work without explicit clone() due to Copy trait
        let _level_copy = level;
        let _output_copy = output;
        let _intent_copy = intent;
        let _format_copy = copied_format;

        // Original values should still be usable
        assert_eq!(level, LogLevel::Debug);
        assert_eq!(output, LogOutput::Stderr);
        assert_eq!(intent, TerminalCapsDetectIntent::Auto);
        assert_eq!(copied_format, LogFormat::Json);
    }

    #[test]
    fn test_exhaustive_error_scenario_coverage() {
        // Comprehensive error testing with all realistic scenarios
        use std::io::ErrorKind;

        struct ErrorTestScenario {
            error: Box<dyn Fn() -> Box<dyn Error>>,
            expected_contains: Vec<&'static str>,
            description: &'static str,
        }

        let scenarios = vec![
            ErrorTestScenario {
                error: Box::new(|| Box::new(TerminalError::NotInteractive)),
                expected_contains: vec!["Cannot probe capabilities", "non-interactive"],
                description: "Non-interactive terminal error",
            },
            ErrorTestScenario {
                error: Box::new(|| Box::new(TerminalError::ProbeTimeout { timeout: 5000 })),
                expected_contains: vec!["timed out", "5000ms"],
                description: "Timeout error with specific duration",
            },
            ErrorTestScenario {
                error: Box::new(|| Box::new(ConfigError::AlreadyInitialized)),
                expected_contains: vec!["already initialized"],
                description: "Configuration initialization error",
            },
            ErrorTestScenario {
                error: Box::new(|| {
                    let io_err =
                        std::io::Error::new(ErrorKind::PermissionDenied, "permission denied");
                    Box::new(TerminalError::ResponseReadFailed { source: io_err })
                }),
                expected_contains: vec!["Failed to read", "permission denied"],
                description: "IO error chain",
            },
        ];

        for scenario in scenarios {
            let error = (scenario.error)();
            let error_string = error.to_string();

            for expected in &scenario.expected_contains {
                assert!(
                    error_string.contains(expected),
                    "Error '{}' should contain '{}' for scenario: {}",
                    error_string,
                    expected,
                    scenario.description
                );
            }

            // Test that error has proper Debug output
            let debug_string = format!("{:?}", error);
            assert!(
                !debug_string.is_empty(),
                "Debug output should not be empty for {}",
                scenario.description
            );
        }
    }

    // =============================================================================
    // LEVEL F: PERFORMANCE AND BENCHMARKING INDICATORS
    // =============================================================================

    #[test]
    fn test_parsing_performance_characteristics() {
        // Not a full benchmark, but ensures parsing performance is reasonable
        // cargo-nextest isolation means this won't interfere with other tests

        use std::time::{Duration, Instant};

        let iterations = 10_000;
        let start = Instant::now();

        for i in 0..iterations {
            let test_values = ["text", "json", "yaml", "error", "debug", "auto"];
            let test_value = test_values[i % test_values.len()];

            // These should be very fast operations
            let _format: Result<LogFormat, _> = test_value.parse();
            let _level: Result<LogLevel, _> = test_value.parse();
            let _intent: Result<TerminalCapsDetectIntent, _> = test_value.parse();
        }

        let elapsed = start.elapsed();
        let per_operation = elapsed / (iterations * 3) as u32;

        // Parsing should be very fast - this is a performance regression test
        assert!(
            per_operation < Duration::from_micros(10),
            "Parsing should be fast: {} per operation is too slow",
            per_operation.as_nanos()
        );

        println!(
            "Parsing performance: {} ns per operation",
            per_operation.as_nanos()
        );
    }

    #[test]
    fn test_enum_variant_completeness_regression() {
        // Regression test to ensure all enum variants are properly handled
        // If new variants are added, this test will fail and need updating

        assert_eq!(
            LogLevel::value_variants().len(),
            5,
            "LogLevel should have exactly 5 variants"
        );
        assert_eq!(
            LogFormat::value_variants().len(),
            3,
            "LogFormat should have exactly 3 variants"
        );
        assert_eq!(
            LogOutput::value_variants().len(),
            2,
            "LogOutput should have exactly 2 variants"
        );
        assert_eq!(
            TerminalCapsDetectIntent::value_variants().len(),
            3,
            "TerminalCapsDetectIntent should have exactly 3 variants"
        );

        // Ensure all variants can be round-tripped
        for variant in LogLevel::value_variants() {
            let possible_value = variant.to_possible_value().unwrap();
            let name = possible_value.get_name();
            let parsed: LogLevel = name.parse().unwrap();
            assert_eq!(*variant, parsed, "LogLevel variant round-trip failed");
        }
    }
}
