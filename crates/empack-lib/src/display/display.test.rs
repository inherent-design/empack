use crate::display::Display;
use crate::display::DisplayProvider;
use crate::display::live::LiveDisplayProvider;
use crate::display::styling::StyleManager;
use crate::display::test_utils::create_test_capabilities;
use crate::primitives::{TerminalColorCaps, TerminalUnicodeCaps};
use crate::terminal::TerminalCapabilities;
use indicatif::MultiProgress;
use std::sync::Arc;

#[test]
fn test_style_manager_truecolor_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::TrueColor,
        unicode: TerminalUnicodeCaps::Ascii,
        is_tty: false,
        cols: 80,
    };
    let display = Display::new(caps);
    let prims = display.styling().primitives();
    assert!(
        !prims.red.is_empty(),
        "TrueColor should have red escape code"
    );
    assert!(!prims.reset.is_empty(), "TrueColor should have reset code");
}

#[test]
fn test_style_manager_no_color_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        is_tty: false,
        cols: 80,
    };
    let display = Display::new(caps);
    let prims = display.styling().primitives();
    assert!(prims.red.is_empty(), "No color should have empty red");
    assert!(prims.bold.is_empty(), "No color should have empty bold");
    assert!(prims.reset.is_empty(), "No color should have empty reset");
}

#[test]
fn test_style_manager_ansi256_palette() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::Ansi256,
        unicode: TerminalUnicodeCaps::BasicUnicode,
        is_tty: false,
        cols: 80,
    };
    let display = Display::new(caps);
    let prims = display.styling().primitives();
    assert!(!prims.red.is_empty(), "Ansi256 should have red escape code");
    assert!(
        prims.red.contains("38;5;"),
        "Ansi256 red should use 256-color escape"
    );
}

#[test]
fn test_display_default_is_minimal() {
    let display = Display::default();
    let caps = display.capabilities();
    assert_eq!(caps.color, TerminalColorCaps::None);
    assert!(!caps.is_tty);
    let prims = display.styling.primitives();
    assert!(prims.red.is_empty());
}

#[test]
fn test_style_manager_format_methods() {
    let caps = create_test_capabilities();
    let display = Display::new(caps);
    let styling = display.styling();

    let success = styling.format_success("done");
    assert!(
        success.contains("done"),
        "Success message should contain text"
    );

    let error = styling.format_error("failed");
    assert!(
        error.contains("failed"),
        "Error message should contain text"
    );

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
    let display = Display::new(caps);

    let _status = display.status();
    let _progress = display.progress();
    let _table = display.table();
}

#[test]
fn test_status_display_variants() {
    let caps = create_test_capabilities();
    let display = Display::new(caps);

    let status = display.status();
    status.checking("tool dependencies");
    status.success("packwiz", "v0.16.1");
    status.success("packwiz", "");
    status.error("packwiz", "not found");
    status.error("packwiz", "");
    status.warning("experimental feature enabled");
    status.info("using default configuration");
    status.message("plain message");
    status.emphasis("configuration complete");
    status.subtle("run empack --help");
    status.list(&["first", "second"]);
    status.complete("dependencies checked");
    status.tool_check("packwiz", true, "v0.16.1");
    status.tool_check("packwiz", false, "");
    status.section("Dependency Check");
    status.step(1, 3, "Loading configuration");
}

#[test]
fn test_progress_display_trackers_and_multi_progress() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::None,
        unicode: TerminalUnicodeCaps::Ascii,
        is_tty: false,
        cols: 80,
    };
    let display = Display::new(caps);

    let progress = display.progress();

    let bar = progress.bar(3);
    assert_eq!(bar.bar().length(), Some(3));
    bar.set_position(1);
    bar.inc();
    bar.inc_by(1);
    bar.set_message("downloading");
    bar.tick("mod.jar");
    bar.finish("downloaded");

    let spinner = progress.spinner("resolving");
    spinner.set_message("resolving dependencies");
    spinner.finish_clear();

    let multi = progress.multi();
    let multi_bar = multi.add_bar(2, "copying");
    multi_bar.inc();
    multi_bar.finish("copied");

    let multi_spinner = multi.add_spinner("waiting");
    multi_spinner.abandon("cancelled");
    multi.clear();
}

#[test]
fn test_progress_display_unicode_branch() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::TrueColor,
        unicode: TerminalUnicodeCaps::ExtendedUnicode,
        is_tty: false,
        cols: 80,
    };
    let display = Display::new(caps);

    let progress = display.progress();
    let bar = progress.bar(2);
    assert_eq!(bar.bar().length(), Some(2));
    bar.finish("done");

    let spinner = progress.spinner("unicode spinner");
    spinner.finish("done");

    let multi = progress.multi();
    let multi_bar = multi.add_bar(1, "unicode");
    multi_bar.finish("done");
}

#[test]
fn test_structured_display_rendering_paths() {
    let caps = create_test_capabilities();
    let display = Display::new(caps);

    let structured = display.table();
    structured.pairs(&[("Project", "my-modpack"), ("Minecraft", "1.21.6")]);
    structured.list(&["first", "second"]);
    structured.numbered_list(&["alpha", "beta"]);

    structured.table().render();

    structured
        .table()
        .header(&["Name", "Version", "Notes"])
        .row(&["packwiz", "v0.16.1", "stable"])
        .row(&["very-long-tool-name", "v1", "this row will be truncated"])
        .max_width(24)
        .render();
}

#[test]
fn test_live_display_provider_delegation() {
    let caps = create_test_capabilities();
    let display = Display::new(caps);

    let provider = LiveDisplayProvider::new().with_capabilities(display.capabilities().clone());
    let default_provider = LiveDisplayProvider::default();
    let shared = Arc::new(MultiProgress::new());
    let arc_provider = LiveDisplayProvider::new_with_arc(shared);

    provider.status().checking("live status");
    provider.status().success("tool", "ok");
    provider.status().warning("watch this");
    provider.progress().bar(1).finish("done");
    provider.progress().spinner("live spinner").finish_clear();
    provider
        .progress()
        .multi()
        .add_bar(1, "multi")
        .finish("done");
    provider
        .table()
        .table(&["Name"], &[vec!["packwiz"], vec!["empack"]]);
    provider.table().list(&["one", "two"]);
    provider.table().properties(&[("key", "value")]);

    default_provider.status().info("default");
    arc_provider.status().message("arc-backed");
}

#[test]
fn test_style_manager_symbols_and_formats() {
    let caps = TerminalCapabilities {
        color: TerminalColorCaps::TrueColor,
        unicode: TerminalUnicodeCaps::ExtendedUnicode,
        is_tty: false,
        cols: 80,
    };
    let styling = StyleManager::new(&caps);

    assert!(styling.style_success("done").contains("done"));
    assert!(styling.style_error("failed").contains("failed"));
    assert!(styling.style_warning("careful").contains("careful"));
    assert!(styling.style_info("note").contains("note"));
    assert!(styling.style_emphasis("bold").contains("bold"));
    assert!(styling.style_subtle("muted").contains("muted"));
    assert!(styling.format_success("done").contains("done"));
    assert!(styling.format_error("failed").contains("failed"));
    assert!(styling.format_warning("careful").contains("careful"));
    assert!(styling.format_info("note").contains("note"));
    assert!(styling.format_working("working").contains("working"));
    assert!(styling.success_symbol().contains("✓"));
    assert!(styling.error_symbol().contains("✗"));
    assert!(styling.warning_symbol().contains("⚠"));
    assert!(styling.info_symbol().contains("ℹ"));
    assert_eq!(styling.bullet(), "●");
    assert_eq!(styling.arrow(), "→");
}

#[test]
fn independent_displays_keep_their_selected_capabilities() {
    let plain = Display::new(TerminalCapabilities::minimal());
    let mut capabilities = TerminalCapabilities::minimal();
    capabilities.color = TerminalColorCaps::TrueColor;
    capabilities.unicode = TerminalUnicodeCaps::ExtendedUnicode;
    capabilities.cols = 132;
    let colored = Display::new(capabilities);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            assert!(!colored.styling.style_success("colored").is_empty());
            assert!(colored.styling.has_unicode());
        });
        scope.spawn(|| {
            assert_eq!(plain.styling.style_success("plain"), "plain");
            assert!(!plain.styling.has_unicode());
        });
    });
    assert_eq!(plain.capabilities.color, TerminalColorCaps::None);
    assert_eq!(colored.capabilities.color, TerminalColorCaps::TrueColor);
    assert!(plain.styling.primitives().red.is_empty());
    assert!(!colored.styling.primitives().red.is_empty());
    assert_eq!(colored.capabilities.cols, 132);
}
