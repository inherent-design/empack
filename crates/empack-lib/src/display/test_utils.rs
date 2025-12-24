//! Test utilities for display module tests
//!
//! Provides shared helper functions for creating test fixtures and cleaning
//! test environment state. Only compiled in test builds.

use crate::display::styling::StyleManager;
use crate::primitives::{TerminalColorCaps, TerminalGraphicsCaps, TerminalUnicodeCaps};
use crate::terminal::{TerminalCapabilities, TerminalDimensions, TerminalInteractivity};

/// Create test styling with minimal capabilities
pub fn create_test_styling() -> StyleManager {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    StyleManager::new(&caps)
}

/// Create test terminal capabilities with minimal features
pub fn create_test_capabilities() -> TerminalCapabilities {
    TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    }
}

/// Clean environment variables that affect terminal/color detection
///
/// # Safety
///
/// Uses unsafe `env::remove_var()` as required by Rust's environment variable API.
/// Only safe to call in test contexts with proper test isolation.
pub fn clean_test_env() {
    unsafe {
        std::env::remove_var("TERM");
        std::env::remove_var("COLORTERM");
        std::env::remove_var("CLICOLOR");
        std::env::remove_var("CLICOLOR_FORCE");
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("FORCE_COLOR");
        std::env::remove_var("CI");
        std::env::remove_var("TERM_PROGRAM");
        std::env::remove_var("WT_SESSION");
        std::env::remove_var("TERMINAL_EMULATOR");
        std::env::remove_var("INSIDE_EMACS");
        std::env::remove_var("EMACS");
        std::env::remove_var("VTE_VERSION");
        std::env::remove_var("KONSOLE_VERSION");
        std::env::remove_var("ALACRITTY_SOCKET");
        std::env::remove_var("KITTY_WINDOW_ID");
        std::env::remove_var("ITERM_SESSION_ID");
    }
}
