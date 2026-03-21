use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);

/// RAII guard that shows the cursor on Drop.
///
/// Tracks hidden state via a global AtomicBool so panic hooks
/// and signal handlers can also restore visibility.
pub struct CursorGuard;

impl Default for CursorGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorGuard {
    pub fn new() -> Self {
        if io::stdout().is_terminal() {
            let _ = io::stdout().write_all(b"\x1b[?25l");
            let _ = io::stdout().flush();
        }
        CURSOR_HIDDEN.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        show_cursor();
    }
}

/// Show cursor if it was hidden via `CursorGuard`. Safe to call multiple times.
pub fn show_cursor() {
    if CURSOR_HIDDEN.swap(false, Ordering::SeqCst) && io::stdout().is_terminal() {
        let _ = io::stdout().write_all(b"\x1b[?25h");
        let _ = io::stdout().flush();
    }
}

/// Force-show cursor regardless of tracking state.
/// Used in panic hooks and signal handlers where the AtomicBool
/// may not reflect reality (e.g. recovery from a prior crash).
pub fn force_show_cursor() {
    CURSOR_HIDDEN.store(false, Ordering::SeqCst);
    if io::stdout().is_terminal() {
        let _ = io::stdout().write_all(b"\x1b[?25h");
        let _ = io::stdout().flush();
    }
}

/// Install panic hook that restores cursor before the default handler runs.
/// Call once at startup before any terminal interaction.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        force_show_cursor();
        default_hook(info);
    }));
}
