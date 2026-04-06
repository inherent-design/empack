use crate::primitives::*;
use std::sync::OnceLock;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(feature = "telemetry")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "telemetry")]
use opentelemetry_sdk::trace::SdkTracerProvider;

static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Global structured logging with optional telemetry layers.
///
/// When the `telemetry` feature is enabled and `EMPACK_PROFILE` is set,
/// additional tracing layers are composed into the subscriber stack:
/// - `chrome`: writes Perfetto-compatible `trace-*.json` files
/// - `otlp`: exports spans via OTLP HTTP/protobuf to a collector
/// - `all`: enables both layers simultaneously
pub struct Logger {
    #[cfg(feature = "telemetry")]
    _chrome_guard: std::sync::Mutex<Option<tracing_chrome::FlushGuard>>,
    #[cfg(feature = "telemetry")]
    _tracer_provider: Option<SdkTracerProvider>,
}

impl std::fmt::Debug for Logger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("Logger");
        #[cfg(feature = "telemetry")]
        {
            dbg.field("chrome", &self._chrome_guard.lock().ok().map(|g| g.is_some()));
            dbg.field("otlp", &self._tracer_provider.is_some());
        }
        dbg.finish()
    }
}

impl Logger {
    /// Initialize the global logger with the given configuration.
    ///
    /// Composes: `registry + env_filter + fmt_layer + indicatif_layer`
    /// and, when the `telemetry` feature is enabled, optional Chrome and
    /// OTLP layers selected by the `EMPACK_PROFILE` env var.
    pub fn init(config: LoggerConfig) -> Result<&'static Self, LoggerError> {
        if GLOBAL_LOGGER.get().is_some() {
            return Err(LoggerError::AlreadyInitialized);
        }

        let indicatif_layer = IndicatifLayer::new();

        let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            let level_str = match config.level {
                LogLevel::Error => "error",
                LogLevel::Warning => "warn",
                LogLevel::Info => "info",
                LogLevel::Debug => "debug",
                LogLevel::Trace => "trace",
            };

            let filter_str = format!("empack={},hyper_util=warn,reqwest=warn,h2=warn,tower=warn,tokio=warn,mio=warn,want=warn,{}",
                level_str, level_str);

            EnvFilter::new(filter_str)
        });

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

        #[cfg(feature = "telemetry")]
        let profile = std::env::var("EMPACK_PROFILE").ok();

        #[cfg(feature = "telemetry")]
        let (chrome_layer, chrome_guard) = if profile
            .as_deref()
            .is_some_and(|p| p == "chrome" || p == "all")
        {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .include_args(true)
                .include_locations(true)
                .build();
            (Some(layer), Some(guard))
        } else {
            (None, None)
        };

        #[cfg(feature = "telemetry")]
        let (otel_layer, tracer_provider) = if profile
            .as_deref()
            .is_some_and(|p| p == "otlp" || p == "all")
        {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .build()
                .map_err(|e| LoggerError::InitializationFailed {
                    reason: format!("OTLP exporter: {e}"),
                })?;
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .build();
            let tracer = provider.tracer("empack");
            let layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
            (Some(layer), Some(provider))
        } else {
            (None, None)
        };

        // Per-layer filtering when telemetry is enabled: fmt and indicatif
        // layers get the user's configured filter; telemetry layers (chrome,
        // otlp) run unfiltered so #[instrument] spans at INFO level are always
        // captured. Two separate EnvFilter instances because EnvFilter is not
        // Clone. The indicatif layer MUST be filtered identically to fmt;
        // otherwise it receives span events for progress bars that fmt never
        // created, causing a panic in pb_manager.
        #[cfg(feature = "telemetry")]
        {
            let indicatif_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                let level_str = match config.level {
                    LogLevel::Error => "error",
                    LogLevel::Warning => "warn",
                    LogLevel::Info => "info",
                    LogLevel::Debug => "debug",
                    LogLevel::Trace => "trace",
                };
                let filter_str = format!("empack={},hyper_util=warn,reqwest=warn,h2=warn,tower=warn,tokio=warn,mio=warn,want=warn,{}",
                    level_str, level_str);
                EnvFilter::new(filter_str)
            });

            tracing_subscriber::registry()
                .with(fmt_layer.with_filter(env_filter))
                .with(indicatif_layer.with_filter(indicatif_filter))
                .with(chrome_layer)
                .with(otel_layer)
                .try_init()
                .map_err(|e| LoggerError::InitializationFailed {
                    reason: e.to_string(),
                })?;
        }
        #[cfg(not(feature = "telemetry"))]
        {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(indicatif_layer)
                .try_init()
                .map_err(|e| LoggerError::InitializationFailed {
                    reason: e.to_string(),
                })?;
        }

        let logger = Logger {
            #[cfg(feature = "telemetry")]
            _chrome_guard: std::sync::Mutex::new(chrome_guard),
            #[cfg(feature = "telemetry")]
            _tracer_provider: tracer_provider,
        };

        GLOBAL_LOGGER
            .set(logger)
            .map_err(|_| LoggerError::AlreadyInitialized)?;

        tracing::info!(
            level = ?config.level,
            format = ?config.format,
            output = ?config.output,
            color_support = ?config.terminal_caps.color,
            "Logger initialized successfully"
        );

        Ok(GLOBAL_LOGGER.get().unwrap())
    }

    /// Retrieve the global logger instance, if initialized.
    pub fn global() -> Option<&'static Self> {
        GLOBAL_LOGGER.get()
    }

    /// Returns `true` if the global logger has been initialized.
    pub fn is_initialized() -> bool {
        GLOBAL_LOGGER.get().is_some()
    }

    /// Flush all telemetry providers.
    ///
    /// Takes the Chrome `FlushGuard` and drops it (signals write thread to
    /// flush and join). Calls `shutdown_with_timeout` on the OTLP
    /// `SdkTracerProvider` (drop alone does NOT flush the batch exporter).
    pub fn shutdown(&self) {
        #[cfg(feature = "telemetry")]
        {
            if let Ok(mut guard) = self._chrome_guard.lock() {
                drop(guard.take());
            }
            if let Some(ref provider) = self._tracer_provider {
                let _ = provider.shutdown_with_timeout(std::time::Duration::from_secs(2));
            }
        }
    }

    /// Log an error message with optional context.
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

/// Flush telemetry providers on the global logger, if initialized.
///
/// Safe to call from signal handlers and panic hooks. No-op when the
/// logger has not been initialized or when the `telemetry` feature is
/// disabled.
pub fn global_shutdown() {
    if let Some(logger) = Logger::global() {
        logger.shutdown();
    }
}

/// Create a span for operations that should show progress bars.
#[macro_export]
macro_rules! progress_span {
    ($operation:expr) => {
        tracing::info_span!("progress", operation = $operation)
    };
    ($operation:expr, total = $total:expr) => {
        tracing::info_span!("progress", operation = $operation, total = $total)
    };
}

/// Quick logging macros that use the global logger if available, falling back to tracing macros.
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
