use crate::display::Display;
use crate::display::test_utils::create_test_capabilities;
use crate::primitives::{TerminalColorCaps, TerminalGraphicsCaps, TerminalUnicodeCaps};
use crate::terminal::{TerminalCapabilities, TerminalDimensions, TerminalInteractivity};

#[test]
fn test_style_manager_truecolor_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::TrueColor,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    Display::init_or_get(caps);
    let prims = Display::styling().primitives();
    assert!(!prims.red.is_empty(), "TrueColor should have red escape code");
    assert!(!prims.reset.is_empty(), "TrueColor should have reset code");
}

#[test]
fn test_style_manager_no_color_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    Display::init_or_get(caps);
    let prims = Display::styling().primitives();
    assert!(prims.red.is_empty(), "No color should have empty red");
    assert!(prims.bold.is_empty(), "No color should have empty bold");
    assert!(prims.reset.is_empty(), "No color should have empty reset");
}

#[test]
fn test_style_manager_ansi256_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::Ansi256,
        unicode: TerminalUnicodeCaps::BasicUnicode,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    Display::init_or_get(caps);
    let prims = Display::styling().primitives();
    assert!(!prims.red.is_empty(), "Ansi256 should have red escape code");
    assert!(
        prims.red.contains("38;5;"),
        "Ansi256 red should use 256-color escape"
    );
}

#[test]
fn test_display_global_auto_init() {
    // Display::global() should auto-initialize with minimal capabilities
    let display = Display::global();
    let caps = Display::capabilities();
    assert_eq!(caps.color, TerminalColorCaps::None);
    assert!(!caps.is_tty);
    // Styling should still work (returns empty strings for no-color)
    let prims = display.styling.primitives();
    assert!(prims.red.is_empty());
}

#[test]
fn test_style_manager_format_methods() {
    let caps = create_test_capabilities();
    Display::init_or_get(caps);
    let styling = Display::styling();

    // With no-color caps, formatted messages should still produce output
    let success = styling.format_success("done");
    assert!(success.contains("done"), "Success message should contain text");

    let error = styling.format_error("failed");
    assert!(error.contains("failed"), "Error message should contain text");

    let warning = styling.format_warning("caution");
    assert!(
        warning.contains("caution"),
        "Warning message should contain text"
    );

    let info = styling.format_info("note");
    assert!(info.contains("note"), "Info message should contain text");
}

#[test]
fn test_display_status_progress_table_accessors() {
    let caps = create_test_capabilities();
    Display::init_or_get(caps);

    // These should not panic; they return display subsystem handles
    let _status = Display::status();
    let _progress = Display::progress();
    let _table = Display::table();
}
