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

fn create_test_capabilities() -> TerminalCapabilities {
    TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    }
}

#[test]
fn test_interactive_prompt_yes_no() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    // Test confirm prompt creation (cannot test interaction in unit test)
    let _prompt = interactive.confirm("Proceed?");

    // Test with default
    let _prompt_with_default = interactive.confirm("Overwrite?").default(false);
}

#[test]
fn test_interactive_select_from_options() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    let options = vec!["Fabric", "Quilt", "NeoForge"];

    // Test select prompt creation
    let _prompt = interactive.select("Choose loader:").options(&options);

    // Test with default
    let _prompt_with_default = interactive
        .select("Choose loader:")
        .options(&options)
        .default(1);
}

#[test]
fn test_interactive_input_validation() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    // Test input prompt creation
    let _prompt = interactive.input("Project name:");

    // Test with default value
    let _prompt_with_default = interactive.input("Project name:").default("my-modpack");

    // Test with empty allowed
    let _prompt_allow_empty = interactive
        .input("Optional field:")
        .allow_empty(true);
}

#[test]
fn test_interactive_default_value() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    // Confirm with default
    let _confirm = interactive.confirm("Proceed?").default(true);

    // Input with default
    let _input = interactive.input("Name:").default("default-name");

    // Select with default
    let _select = interactive
        .select("Option:")
        .options(&["A", "B", "C"])
        .default(1);
}

#[test]
fn test_interactive_is_interactive_check() {
    // Test static method
    let is_interactive = InteractiveDisplay::is_interactive();

    // In test environment, this is typically false
    // but we just verify the method exists and returns a bool
    let _ = is_interactive;
}

#[test]
fn test_interactive_non_interactive_fallback() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    // Test non-interactive fallback methods
    let result = interactive.confirm_or_default("Proceed?", true);
    // In non-interactive environment, should return default
    assert_eq!(result, true);

    let result = interactive.confirm_or_default("Cancel?", false);
    assert_eq!(result, false);
}

#[test]
fn test_interactive_select_or_default_fallback() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    let options = vec!["Option A", "Option B", "Option C"];

    // In non-interactive environment, should return default option
    let result = interactive.select_or_default("Choose:", &options, 1);
    assert_eq!(result, "Option B");

    let result = interactive.select_or_default("Choose:", &options, 0);
    assert_eq!(result, "Option A");
}

#[test]
fn test_interactive_theme_selection() {
    let styling = create_test_styling();

    // Test with no color capability
    let caps_no_color = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    let _interactive_simple = InteractiveDisplay::new(&styling, &caps_no_color);

    // Test with color capability
    let caps_color = TerminalCapabilities {
        color: TerminalColorCaps::Ansi16,
        unicode: TerminalUnicodeCaps::ExtendedUnicode,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    let _interactive_colorful = InteractiveDisplay::new(&styling, &caps_color);
}

#[test]
fn test_interactive_empty_options() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    // Edge case: empty options list
    let _select = interactive.select("Choose:").options(&[]);
}

#[test]
fn test_interactive_select_or_default_out_of_bounds() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let interactive = InteractiveDisplay::new(&styling, &caps);

    let options = vec!["A", "B"];

    // Default index out of bounds - should handle gracefully
    let result = interactive.select_or_default("Choose:", &options, 10);
    assert_eq!(result, ""); // Falls back to empty string
}
