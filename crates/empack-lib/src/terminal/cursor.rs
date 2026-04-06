use std::io::{self, IsTerminal, Write};

/// Force-show cursor regardless of tracking state.
/// Used in panic hooks and signal handlers where cursor state
/// may not be known (e.g. recovery from a prior crash).
pub fn force_show_cursor() {
    if io::stdout().is_terminal() {
        let _ = io::stdout().write_all(b"\x1b[?25h");
        let _ = io::stdout().flush();
    }
    if io::stderr().is_terminal() {
        let _ = io::stderr().write_all(b"\x1b[?25h");
        let _ = io::stderr().flush();
    }
}

/// Install panic hook that restores cursor before the default handler runs.
/// Call once at startup before any terminal interaction.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        force_show_cursor();
        // Best-effort telemetry flush; may fail if a lock is poisoned
        crate::logger::global_shutdown();
        default_hook(info);
    }));
}
