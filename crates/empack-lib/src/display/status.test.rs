use super::*;
use crate::display::styling::StyleManager;
use crate::primitives::{TerminalColorCaps, TerminalUnicodeCaps, TerminalGraphicsCaps};
use crate::terminal::{TerminalCapabilities, TerminalDimensions, TerminalInteractivity};

fn create_test_styling() -> StyleManager {
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

#[test]
fn test_status_message_formatting() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Test various message types (verify no panics)
    status.checking("dependencies");
    status.success("packwiz", "v0.16.1");
    status.error("tool", "not found");
    status.warning("experimental feature");
    status.info("using default config");
    status.message("Plain message");
    status.emphasis("Important message");
    status.subtle("Secondary info");

    // No assertions needed - just verify no panics occur
}

#[test]
fn test_status_levels_info_warn_error() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Test all status levels
    status.info("Information message");
    status.warning("Warning message");
    status.error("Error item", "error details");
    status.success("Success item", "success details");

    // Verify empty details handling
    status.success("Success without details", "");
    status.error("Error without details", "");
}

#[test]
fn test_status_with_long_messages_truncation() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Very long message
    let long_message = "a".repeat(500);

    // Should not panic with extremely long messages
    status.info(&long_message);
    status.warning(&long_message);
    status.message(&long_message);
    status.emphasis(&long_message);
    status.subtle(&long_message);
}

#[test]
fn test_status_list_rendering() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    let items = vec!["Item 1", "Item 2", "Item 3"];

    // Should render bulleted list
    status.list(&items);

    // Empty list should not panic
    status.list(&[]);
}

#[test]
fn test_status_tool_check_pattern() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Available tool
    status.tool_check("packwiz", true, "v0.16.1");

    // Unavailable tool
    status.tool_check("missing-tool", false, "");
}

#[test]
fn test_status_section_and_step() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Section header
    status.section("Dependencies");

    // Multi-step progress
    status.step(1, 5, "Loading configuration");
    status.step(2, 5, "Checking tools");
    status.step(3, 5, "Resolving dependencies");
    status.step(4, 5, "Downloading mods");
    status.step(5, 5, "Building modpack");
}

#[test]
fn test_status_complete_pattern() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    status.checking("dependencies");
    // ... work happens
    status.complete("Dependencies checked");
}

#[test]
fn test_status_empty_strings() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Edge case: empty strings should not panic
    status.message("");
    status.info("");
    status.warning("");
    status.emphasis("");
    status.subtle("");
    status.success("", "");
    status.error("", "");
}

#[test]
fn test_status_special_characters() {
    let styling = create_test_styling();
    let status = StatusDisplay::new(&styling);

    // Messages with special characters
    status.info("Message with\nnewlines\nhere");
    status.warning("Tab\tseparated\tvalues");
    status.message("Unicode: 🚀 ✓ ✗");
    status.emphasis("Special chars: @#$%^&*()");
}
