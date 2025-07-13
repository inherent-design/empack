use crate::primitives::*;
use std::sync::OnceLock;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Global logger instance - ensures single initialization
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Logger implementation using tracing with indicatif progress integration
#[derive(Debug)]
pub struct Logger {
    _guard: (), // Future: for async logging guards if needed
}

impl Logger {
    /// Initialize the global logger with terminal-aware configuration
    pub fn init(config: LoggerConfig) -> Result<&'static Self, LoggerError> {
        // Check if already initialized
        if GLOBAL_LOGGER.get().is_some() {
            return Err(LoggerError::AlreadyInitialized);
        }

        // Create indicatif layer for progress bars
        let indicatif_layer = IndicatifLayer::new();

        // Configure environment filter for log levels with empack-focused filtering
        let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            let level_str = match config.level {
                LogLevel::Error => "error",
                LogLevel::Warning => "warn",
                LogLevel::Info => "info",
                LogLevel::Debug => "debug",
                LogLevel::Trace => "trace",
            };

            // Filter: empack at level, external crates at warn
            let filter_str = format!("empack={},hyper_util=warn,reqwest=warn,h2=warn,tower=warn,tokio=warn,mio=warn,want=warn,{}",
                level_str, level_str);

            EnvFilter::new(filter_str)
        });

        // Configure terminal-aware formatting with output selection
        let fmt_layer = match (config.output, config.format) {
            (LogOutput::Stderr, LogFormat::Text) => fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_ansi(config.terminal_caps.color != TerminalColorCaps::None)
                .compact()
                .boxed(),
            (LogOutput::Stderr, LogFormat::Json) => fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_ansi(false)
                .json()
                .boxed(),
            (LogOutput::Stderr, LogFormat::Yaml) => fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_ansi(config.terminal_caps.color != TerminalColorCaps::None)
                .pretty()
                .boxed(),
            (LogOutput::Stdout, LogFormat::Text) => fmt::layer()
                .with_writer(indicatif_layer.get_stdout_writer())
                .with_ansi(config.terminal_caps.color != TerminalColorCaps::None)
                .compact()
                .boxed(),
            (LogOutput::Stdout, LogFormat::Json) => fmt::layer()
                .with_writer(indicatif_layer.get_stdout_writer())
                .with_ansi(false)
                .json()
                .boxed(),
            (LogOutput::Stdout, LogFormat::Yaml) => fmt::layer()
                .with_writer(indicatif_layer.get_stdout_writer())
                .with_ansi(config.terminal_caps.color != TerminalColorCaps::None)
                .pretty()
                .boxed(),
        };

        // Initialize tracing subscriber with layered configuration
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(indicatif_layer)
            .try_init()
            .map_err(|e| LoggerError::InitializationFailed {
                reason: e.to_string(),
            })?;

        // Create logger instance
        let logger = Logger { _guard: () };

        // Store in global static
        GLOBAL_LOGGER
            .set(logger)
            .map_err(|_| LoggerError::AlreadyInitialized)?;

        // Log initialization success
        tracing::info!(
            level = ?config.level,
            format = ?config.format,
            output = ?config.output,
            color_support = ?config.terminal_caps.color,
            "Logger initialized successfully"
        );

        Ok(GLOBAL_LOGGER.get().unwrap())
    }

    /// Get reference to the global logger instance
    pub fn global() -> Option<&'static Self> {
        GLOBAL_LOGGER.get()
    }

    /// Check if logger is initialized
    pub fn is_initialized() -> bool {
        GLOBAL_LOGGER.get().is_some()
    }

    // Convenience methods for structured logging
    // These delegate to tracing macros for consistency

    /// Log an error message with optional context
    pub fn error(&self, message: &str, context: Option<LogContext>) {
        if let Some(ctx) = context {
            tracing::error!(
                operation = %ctx.operation,
                current = ctx.current_item,
                total = ctx.total_items,
                "{}", message
            );
        } else {
            tracing::error!("{}", message);
        }
    }

    /// Log a warning message with optional context
    pub fn warn(&self, message: &str, context: Option<LogContext>) {
        if let Some(ctx) = context {
            tracing::warn!(
                operation = %ctx.operation,
                current = ctx.current_item,
                total = ctx.total_items,
                "{}", message
            );
        } else {
            tracing::warn!("{}", message);
        }
    }

    /// Log an info message with optional context
    pub fn info(&self, message: &str, context: Option<LogContext>) {
        if let Some(ctx) = context {
            tracing::info!(
                operation = %ctx.operation,
                current = ctx.current_item,
                total = ctx.total_items,
                "{}", message
            );
        } else {
            tracing::info!("{}", message);
        }
    }

    /// Log a debug message with optional context
    pub fn debug(&self, message: &str, context: Option<LogContext>) {
        if let Some(ctx) = context {
            tracing::debug!(
                operation = %ctx.operation,
                current = ctx.current_item,
                total = ctx.total_items,
                "{}", message
            );
        } else {
            tracing::debug!("{}", message);
        }
    }

    /// Log a trace message with optional context
    pub fn trace(&self, message: &str, context: Option<LogContext>) {
        if let Some(ctx) = context {
            tracing::trace!(
                operation = %ctx.operation,
                current = ctx.current_item,
                total = ctx.total_items,
                "{}", message
            );
        } else {
            tracing::trace!("{}", message);
        }
    }
}

// Convenience macros for easier logging throughout the application
// These can be used instead of the Logger methods

/// Create a span for operations that should show progress bars
#[macro_export]
macro_rules! progress_span {
    ($operation:expr) => {
        tracing::info_span!("progress", operation = $operation)
    };
    ($operation:expr, total = $total:expr) => {
        tracing::info_span!("progress", operation = $operation, total = $total)
    };
}

/// Quick logging macros that use the global logger if available, fall back to tracing macros
#[macro_export]
macro_rules! log_error {
    ($msg:expr) => {
        if let Some(logger) = $crate::logger::Logger::global() {
            logger.error($msg, None);
        } else {
            tracing::error!($msg);
        }
    };
    ($msg:expr, $ctx:expr) => {
        if let Some(logger) = $crate::logger::Logger::global() {
            logger.error($msg, Some($ctx));
        } else {
            tracing::error!($msg);
        }
    };
}

#[macro_export]
macro_rules! log_info {
    ($msg:expr) => {
        if let Some(logger) = $crate::logger::Logger::global() {
            logger.info($msg, None);
        } else {
            tracing::info!($msg);
        }
    };
    ($msg:expr, $ctx:expr) => {
        if let Some(logger) = $crate::logger::Logger::global() {
            logger.info($msg, Some($ctx));
        } else {
            tracing::info!($msg);
        }
    };
}

#[cfg(test)]
mod tests {
    include!("mod.test.rs");
}