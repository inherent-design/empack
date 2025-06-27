mod primitives;
mod terminal;
mod logger;
mod application;
mod empack;
mod networking;
mod platform;

use primitives::{TerminalColorCaps, LoggerConfig, LogContext};
use application::AppConfig;
use terminal::{TerminalCapabilities, DimensionSource};
use logger::Logger;
use anyhow::Result;
use std::io::Write;

fn main() -> Result<()> {
    // 1. Load application configuration
    let config = AppConfig::load()?;
    
    // 2. Detect terminal capabilities 
    let terminal_caps = TerminalCapabilities::detect_from_config(&config)?;
    
    // 3. Initialize logger with terminal-aware configuration
    let logger_config = LoggerConfig::from_app_config(&config, &terminal_caps);
    let logger = Logger::init(logger_config)?;
    
    // 4. Initialize global configuration
    AppConfig::init_global(config.clone())?;
    
    // Now use structured logging throughout
    logger.info("🚀 Empack Core System Test starting", None);
    
    // Log configuration details
    logger.info("📄 Configuration loaded successfully", Some(LogContext::new("config_load")));
    logger.debug(&format!("Config details: {:#?}", config), None);
    
    // Log terminal capabilities with context
    let terminal_context = LogContext::new("terminal_detection");
    logger.info("🎨 Terminal capabilities detected", Some(terminal_context));
    
    // Ensure terminal state is completely clean before any output
    if terminal_caps.is_tty {
        print!("\x1b[0m\x1b[?25h\x1b[49m\x1b[39m");
        std::io::stdout().flush().ok();
    }
    
    // Print terminal capabilities in a clean, readable format
    println!("✅ Terminal caps detected:");
    println!("   • Color: {:?}", terminal_caps.color);
    println!("   • Unicode: {:?}", terminal_caps.unicode);
    println!("   • Graphics: {:?}", terminal_caps.graphics);
    println!("   • Dimensions: {}x{} ({})", 
             terminal_caps.dimensions.cols, 
             terminal_caps.dimensions.rows,
             match terminal_caps.dimensions.detection_source {
                 DimensionSource::Tiocgwinsz => "ioctl",
                 DimensionSource::CsiQuery => "CSI query", 
                 DimensionSource::Environment => "env vars",
                 DimensionSource::Default => "default",
             });
    println!("   • TTY: {}", terminal_caps.is_tty);
    
    logger.info("✅ Global config initialized", None);
    
    // Display hello world in highest detected format
    logger.info("🌈 Demonstrating terminal color capabilities", None);
    
    // Only use ANSI if we're in a proper TTY to avoid shell corruption
    if terminal_caps.is_tty && terminal_caps.color != TerminalColorCaps::None {
        match terminal_caps.color {
            TerminalColorCaps::TrueColor => {
                // 24-bit RGB color
                print!("\x1b[38;2;255;100;50mHello from TrueColor! 🎨\x1b[0m\n");
                print!("\x1b[38;2;50;255;100mTerminal supports 24-bit RGB!\x1b[0m\n");
                logger.debug("Using TrueColor (24-bit RGB) terminal output", None);
            },
            TerminalColorCaps::Ansi256 => {
                // 256 color palette
                print!("\x1b[38;5;196mHello from 256-color! 🎭\x1b[0m\n");
                print!("\x1b[38;5;46mTerminal supports 8-bit color!\x1b[0m\n");
                logger.debug("Using 256-color terminal output", None);
            },
            TerminalColorCaps::Ansi16 => {
                // Basic 16 colors
                print!("\x1b[31mHello from Standard color! 🔴\x1b[0m\n");
                print!("\x1b[32mTerminal supports basic 16 colors!\x1b[0m\n");
                logger.debug("Using 16-color terminal output", None);
            },
            TerminalColorCaps::None => unreachable!(), // Already checked above
        }
        // Ensure we're completely reset
        print!("\x1b[0m");
        std::io::stdout().flush().ok();
    } else {
        // Plain text fallback for non-TTY or no color support
        println!("Hello from plain text! (TTY: {}, Color: {:?})", 
                terminal_caps.is_tty, terminal_caps.color);
        logger.debug("Using plain text output (no TTY or color support)", None);
    }
    
    logger.info("✨ Core system test completed successfully!", None);
    logger.debug("All systems validated: config → terminal → logger → main", None);
    
    Ok(())
}