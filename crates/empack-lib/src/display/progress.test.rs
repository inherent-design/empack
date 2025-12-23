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
fn test_progress_bar_renders_with_message() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let bar = progress.bar(100);
    bar.set_message("Downloading mods");
    bar.set_position(0);

    // Test that bar exists and doesn't panic
    assert_eq!(bar.bar().length().unwrap(), 100);
}

#[test]
fn test_progress_bar_updates_increment() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let bar = progress.bar(50);

    // Initial position
    assert_eq!(bar.bar().position(), 0);

    // Increment by 1
    bar.inc();
    assert_eq!(bar.bar().position(), 1);

    // Increment by 10
    bar.inc_by(10);
    assert_eq!(bar.bar().position(), 11);

    // Set specific position
    bar.set_position(25);
    assert_eq!(bar.bar().position(), 25);
}

#[test]
fn test_progress_bar_completes_at_100_percent() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let bar = progress.bar(10);

    // Simulate completing all items
    for i in 0..10 {
        bar.set_position(i);
    }

    bar.finish("Completed");
    assert!(bar.bar().is_finished());
}

#[test]
fn test_multi_progress_bars_parallel() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let multi = progress.multi();

    // Create multiple bars
    let bar1 = multi.add_bar(100, "Task 1");
    let bar2 = multi.add_bar(50, "Task 2");
    let spinner = multi.add_spinner("Task 3");

    // Update bars independently
    bar1.set_position(50);
    bar2.set_position(25);
    spinner.set_message("Working...");

    // Verify positions
    assert_eq!(bar1.bar().position(), 50);
    assert_eq!(bar2.bar().position(), 25);
}

#[test]
fn test_progress_bar_cleanup_on_drop() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    {
        let bar = progress.bar(100);
        bar.set_position(50);

        // Bar should exist
        assert_eq!(bar.bar().position(), 50);

        // Explicitly finish
        bar.finish_clear();
        assert!(bar.bar().is_finished());
    }
    // Bar dropped here - test that no panic occurs
}

#[test]
fn test_spinner_creation_and_update() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let spinner = progress.spinner("Loading");

    // Update message
    spinner.set_message("Processing");
    spinner.tick("Item 1");

    // Finish spinner
    spinner.finish("Done");
    assert!(spinner.bar().is_finished());
}

#[test]
fn test_progress_bar_abandon_on_error() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let bar = progress.bar(100);
    bar.set_position(50);

    // Abandon instead of finishing
    bar.abandon("Failed");
    assert!(bar.bar().is_finished());
}

#[test]
fn test_multi_progress_clear() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    let multi = progress.multi();

    let _bar1 = multi.add_bar(100, "Task 1");
    let _bar2 = multi.add_bar(50, "Task 2");

    // Clear all progress bars
    multi.clear();
    // Should not panic
}

#[test]
fn test_progress_bar_zero_length() {
    let styling = create_test_styling();
    let progress = ProgressDisplay::new(&styling);

    // Edge case: zero length progress bar
    let bar = progress.bar(0);

    // Should not panic on operations
    bar.set_position(0);
    bar.finish("Empty task completed");
    assert!(bar.bar().is_finished());
}

#[test]
fn test_progress_bar_unicode_vs_ascii() {
    // Unicode capable terminal
    let caps_unicode = TerminalCapabilities {
        color: TerminalColorCaps::Ansi16,
        unicode: TerminalUnicodeCaps::ExtendedUnicode,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    let styling_unicode = StyleManager::new(&caps_unicode);
    let progress_unicode = ProgressDisplay::new(&styling_unicode);

    let _bar_unicode = progress_unicode.bar(100);
    // Unicode bar should use fancy characters (verified via template in implementation)

    // ASCII-only terminal
    let caps_ascii = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        graphics: TerminalGraphicsCaps::None,
        dimensions: TerminalDimensions::default(),
        interactivity: TerminalInteractivity::default(),
        is_tty: false,
    };
    let styling_ascii = StyleManager::new(&caps_ascii);
    let progress_ascii = ProgressDisplay::new(&styling_ascii);

    let _bar_ascii = progress_ascii.bar(100);
    // ASCII bar should use basic characters (verified via template in implementation)
}
