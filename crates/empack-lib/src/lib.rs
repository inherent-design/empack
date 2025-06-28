//! # empack Library
//!
//! Minecraft modpack management library.
//!
//! ## Core Modules
//!
//! - [`primitives`] - Foundation types, errors, and shared coordination
//! - [`terminal`] - Cross-platform terminal capability detection
//! - [`logger`] - Structured logging with progress tracking
//! - [`networking`] - Async HTTP client with concurrency management
//! - [`platform`] - System resource detection and optimization
//! - [`empack`] - Domain-specific modpack management types
//! - [`application`] - CLI interface and configuration management
//!
//! ## Quick Start
//!
//! ```no_run
//! # tokio_test::block_on(async {
//! // Initialize and run empack
//! empack_lib::main().await.unwrap();
//! # })
//! ```

pub mod application;
pub mod empack;
pub mod logger;
pub mod networking;
pub mod platform;
pub mod primitives;
pub mod terminal;

// Re-export commonly used types for convenience
pub use application::{AppConfig, Cli, Commands};
pub use logger::Logger;
pub use networking::{NetworkingConfig, NetworkingManager};
pub use platform::SystemResources;
pub use primitives::{
    ConfigError, LogFormat, LogLevel, LogOutput, LoggerError, TerminalCapsDetectIntent,
    TerminalColorCaps,
};
pub use terminal::TerminalCapabilities;

// Private imports for the main function
use anyhow::Result;
use primitives::{LogContext, TerminalPrimitives, from_terminal_capabilities};
use std::io::Write;
use terminal::DimensionSource;

pub async fn main() -> Result<()> {
    // Load config
    let config = AppConfig::load()?;

    // Detect terminal caps
    let terminal_caps = TerminalCapabilities::detect_from_config(&config)?;

    // Init logger
    let logger_config = config.to_logger_config(&terminal_caps);
    let logger = Logger::init(logger_config)?;

    // Init global config
    AppConfig::init_global(config.clone())?;

    // Now use structured logging throughout
    logger.info("empack core system test starting", None);

    // Log configuration details
    logger.info(
        "Configuration loaded successfully",
        Some(LogContext::new("config_load")),
    );
    logger.debug(&format!("Config details: {:#?}", config), None);

    // Log terminal capabilities with context
    let terminal_context = LogContext::new("terminal_detection");
    logger.info("Terminal capabilities detected", Some(terminal_context));

    // Create terminal primitives based on detected capabilities
    let basic_caps = from_terminal_capabilities(&terminal_caps);
    let terminal_primitives = TerminalPrimitives::new(&basic_caps);

    // Ensure terminal state is completely clean before any output
    if terminal_caps.is_tty {
        print!("{}", terminal_primitives.reset);
        std::io::stdout().flush().ok();
    }

    // Print terminal capabilities in a clean, readable format
    println!("✅ Terminal caps detected:");
    println!("   • Color: {:?}", terminal_caps.color);
    println!("   • Unicode: {:?}", terminal_caps.unicode);
    println!("   • Graphics: {:?}", terminal_caps.graphics);
    println!(
        "   • Dimensions: {}x{} ({})",
        terminal_caps.dimensions.cols,
        terminal_caps.dimensions.rows,
        match terminal_caps.dimensions.detection_source {
            DimensionSource::Tiocgwinsz => "ioctl",
            DimensionSource::CsiQuery => "CSI query",
            DimensionSource::Environment => "env vars",
            DimensionSource::Default => "default",
        }
    );
    println!("   • TTY: {}", terminal_caps.is_tty);

    logger.info("Global config initialized", None);

    // Display hello world in highest detected format
    logger.info("Demonstrating terminal color capabilities", None);

    // Only use ANSI if we're in a proper TTY to avoid shell corruption
    if terminal_caps.is_tty && terminal_caps.color != TerminalColorCaps::None {
        match terminal_caps.color {
            TerminalColorCaps::TrueColor => {
                // 24-bit RGB color using terminal primitives
                println!(
                    "{}Hello from TrueColor! 🎨{}",
                    terminal_primitives.red, terminal_primitives.reset
                );
                println!(
                    "{}Terminal supports 24-bit RGB!{}",
                    terminal_primitives.green, terminal_primitives.reset
                );
                logger.debug("Using TrueColor (24-bit RGB) terminal output", None);
            }
            TerminalColorCaps::Ansi256 => {
                // 256 color palette using terminal primitives
                println!(
                    "{}Hello from 256-color! 🎭{}",
                    terminal_primitives.error, terminal_primitives.reset
                );
                println!(
                    "{}Terminal supports 8-bit color!{}",
                    terminal_primitives.success, terminal_primitives.reset
                );
                logger.debug("Using 256-color terminal output", None);
            }
            TerminalColorCaps::Ansi16 => {
                // Basic 16 colors using terminal primitives
                println!(
                    "{}Hello from Standard color! 🔴{}",
                    terminal_primitives.red, terminal_primitives.reset
                );
                println!(
                    "{}Terminal supports basic 16 colors!{}",
                    terminal_primitives.green, terminal_primitives.reset
                );
                logger.debug("Using 16-color terminal output", None);
            }
            TerminalColorCaps::None => unreachable!(), // Already checked above
        }
        // Ensure we're completely reset using primitives
        print!("{}", terminal_primitives.reset);
        std::io::stdout().flush().ok();
    } else {
        // Plain text fallback for non-TTY or no color support
        println!(
            "Hello from plain text! (TTY: {}, Color: {:?})",
            terminal_caps.is_tty, terminal_caps.color
        );
        logger.debug("Using plain text output (no TTY or color support)", None);
    }

    // Test platform detection
    logger.info("Testing platform detection capabilities", None);
    let resources = SystemResources::detect()?;
    println!("🖥️  Platform resources detected:");
    println!("   • CPU cores: {}", resources.cpu_cores);
    println!(
        "   • Memory pressure: {:.2}%",
        resources.memory_pressure * 100.0
    );
    println!(
        "   • Total memory: {:.1} GB",
        resources.total_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!(
        "   • Available memory: {:.1} GB",
        resources.available_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!(
        "   • Optimal jobs: {}",
        resources.calculate_optimal_jobs(None)
    );

    // Test networking with safe HTTP endpoint
    logger.info("Testing networking manager initialization", None);
    let networking_config = NetworkingConfig {
        max_jobs: Some(2), // Keep it light for testing
        timeout_seconds: 10,
        trace_requests: true,
    };

    let networking_manager = NetworkingManager::new(networking_config).await?;
    println!("🌐 Networking manager initialized:");
    println!(
        "   • Optimal concurrent jobs: {}",
        networking_manager.optimal_jobs()
    );

    // Test with safe HTTP endpoints (httpbin.org is designed for testing)
    logger.info("Testing HTTP client with safe endpoints", None);
    let test_urls = vec![
        "https://httpbin.org/uuid".to_string(),
        "https://httpbin.org/json".to_string(),
    ];

    // Simple resolver function for testing
    let test_resolver = |client: reqwest::Client, url: String| async move {
        let response = client
            .get(&url)
            .header("User-Agent", "empack/0.1.0-test")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(networking::NetworkingError::RequestFailed {
                source: response.error_for_status().unwrap_err(),
            });
        }

        let body = response.text().await?;

        Ok::<String, networking::NetworkingError>(format!("Success: {} chars", body.len()))
    };

    let results = networking_manager
        .resolve_mods(test_urls, test_resolver)
        .await?;

    println!("📡 HTTP test results:");
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(response) => println!("   • Test {}: {}", i + 1, response),
            Err(e) => println!("   • Test {}: Error - {}", i + 1, e),
        }
    }

    logger.info("Core system test completed successfully", None);
    logger.debug(
        "All systems validated: config → terminal → logger → platform → networking → main",
        None,
    );

    Ok(())
}
