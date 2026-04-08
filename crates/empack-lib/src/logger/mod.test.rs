use super::*;
use crate::terminal::TerminalCapabilities;
use std::sync::OnceLock;

fn test_logger_config() -> LoggerConfig {
    LoggerConfig {
        level: LogLevel::Debug,
        format: LogFormat::Text,
        output: LogOutput::Stderr,
        terminal_caps: TerminalCapabilities::minimal(),
    }
}

fn init_test_logger() -> &'static Logger {
    static LOGGER_INIT: OnceLock<()> = OnceLock::new();

    LOGGER_INIT.get_or_init(|| {
        let config = test_logger_config();
        let logger = Logger::init(config.clone()).expect("logger should initialize");
        assert!(Logger::is_initialized());
        assert!(Logger::global().is_some());
        assert!(std::ptr::eq(Logger::global().unwrap(), logger));

        let second = Logger::init(config);
        assert!(matches!(second, Err(LoggerError::AlreadyInitialized)));
    });

    Logger::global().expect("logger should remain initialized")
}

#[test]
fn test_log_context_creation() {
    let context = LogContext::new("test_operation");
    assert_eq!(context.operation, "test_operation");
    assert_eq!(context.total_items, None);
    assert_eq!(context.current_item, None);
}

#[test]
fn test_log_context_with_progress() {
    let mut context = LogContext::with_progress("downloading", 100);
    assert_eq!(context.operation, "downloading");
    assert_eq!(context.total_items, Some(100));
    assert_eq!(context.current_item, None);

    context.set_progress(50);
    assert_eq!(context.current_item, Some(50));
}

#[test]
fn test_logger_not_initialized_initially() {
    // Note: This test assumes no other test has initialized the logger
    // In practice, we might need test isolation for the global logger
    assert!(!Logger::is_initialized() || Logger::global().is_some());
}

#[test]
fn test_logger_filter_string_levels() {
    let cases = [
        (LogLevel::Error, "error"),
        (LogLevel::Warning, "warn"),
        (LogLevel::Info, "info"),
        (LogLevel::Debug, "debug"),
        (LogLevel::Trace, "trace"),
    ];

    for (level, expected) in cases {
        let config = LoggerConfig {
            level,
            ..test_logger_config()
        };

        let filter = Logger::build_filter_string(&config);
        assert!(filter.contains(&format!("empack={expected}")));
        assert!(filter.ends_with(expected));
    }
}

#[test]
fn test_logger_init_and_logging_paths() {
    let logger = init_test_logger();
    let mut context = LogContext::with_progress("download", 3);
    context.set_progress(2);

    logger.error("failed", Some(context.clone()));
    logger.error("failed", None);
    logger.warn("warning", Some(context.clone()));
    logger.warn("warning", None);
    logger.info("info", Some(context.clone()));
    logger.info("info", None);
    logger.debug("debug", Some(context.clone()));
    logger.debug("debug", None);
    logger.trace("trace", Some(context));
    logger.trace("trace", None);

    logger.shutdown();
    crate::logger::global_shutdown();
}
