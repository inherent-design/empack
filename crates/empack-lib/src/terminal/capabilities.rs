use crate::primitives::*;
use std::io::IsTerminal;

/// Runtime terminal capabilities detected at session startup.
///
/// Fields drive styling (color, unicode), progress bar rendering (is_tty),
/// and table column layout (cols).
#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub color: TerminalColorCaps,
    pub unicode: TerminalUnicodeCaps,
    pub is_tty: bool,
    pub cols: u16,
}

impl TerminalCapabilities {
    /// Create minimal capabilities for non-interactive/fallback contexts.
    /// Used for display instances when the host supplies no terminal policy.
    pub fn minimal() -> Self {
        Self {
            color: TerminalColorCaps::None,
            unicode: TerminalUnicodeCaps::Ascii,
            is_tty: false,
            cols: 80,
        }
    }

    /// Detect stderr capabilities from environment, respecting the color intent.
    ///
    /// Delegates to the `console` crate for color and dimension detection.
    /// Unicode support is inferred from locale environment variables.
    pub fn detect_from_config(
        color_intent: TerminalCapsDetectIntent,
    ) -> Result<Self, TerminalError> {
        Self::detect(
            color_intent,
            std::io::stderr().is_terminal(),
            console::colors_enabled_stderr(),
            console::Term::stderr(),
        )
    }

    /// Detect result-output capabilities independently of stderr and prompt input.
    pub fn detect_stdout_from_config(
        color_intent: TerminalCapsDetectIntent,
    ) -> Result<Self, TerminalError> {
        Self::detect(
            color_intent,
            std::io::stdout().is_terminal(),
            console::colors_enabled(),
            console::Term::stdout(),
        )
    }

    fn detect(
        color_intent: TerminalCapsDetectIntent,
        is_tty: bool,
        colors_enabled: bool,
        terminal: console::Term,
    ) -> Result<Self, TerminalError> {
        Ok(Self {
            color: detect_color(color_intent, colors_enabled),
            unicode: detect_unicode(is_tty)?,
            is_tty,
            cols: terminal.size().1,
        })
    }
}

/// Detect color capability level using `console` crate and COLORTERM env var.
fn detect_color(intent: TerminalCapsDetectIntent, colors_enabled: bool) -> TerminalColorCaps {
    match intent {
        TerminalCapsDetectIntent::Never => TerminalColorCaps::None,
        TerminalCapsDetectIntent::Always => truecolor_or_256(),
        TerminalCapsDetectIntent::Auto => {
            if colors_enabled {
                truecolor_or_256()
            } else {
                TerminalColorCaps::None
            }
        }
    }
}

/// Check COLORTERM for truecolor support; fall back to Ansi256.
fn truecolor_or_256() -> TerminalColorCaps {
    if std::env::var("COLORTERM")
        .ok()
        .is_some_and(|v| v == "truecolor" || v == "24bit")
    {
        TerminalColorCaps::TrueColor
    } else {
        TerminalColorCaps::Ansi256
    }
}

/// Detect unicode capability from locale environment variables.
///
/// Priority: LC_ALL > LC_CTYPE > LANG. Falls back to `locale charmap` on Unix.
fn detect_unicode(is_tty: bool) -> Result<TerminalUnicodeCaps, TerminalError> {
    if !is_tty {
        return Ok(TerminalUnicodeCaps::Ascii);
    }

    let locale_var = std::env::var("LC_ALL")
        .ok()
        .or_else(|| std::env::var("LC_CTYPE").ok())
        .or_else(|| std::env::var("LANG").ok());

    if let Some(locale) = locale_var
        && locale.to_lowercase().contains("utf")
    {
        return Ok(TerminalUnicodeCaps::BasicUnicode);
    }

    #[cfg(unix)]
    {
        if let Ok(charset) = get_unix_charset()
            && charset.to_lowercase().contains("utf")
        {
            return Ok(TerminalUnicodeCaps::BasicUnicode);
        }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::{GetACP, GetOEMCP};
        use windows_sys::Win32::System::Console::{GetConsoleCP, GetConsoleOutputCP};

        let unicode = unsafe {
            let acp = GetACP();
            let oemp = GetOEMCP();
            let console_cp = GetConsoleCP();
            let console_output_cp = GetConsoleOutputCP();

            acp == 65001 || oemp == 65001 || console_cp == 65001 || console_output_cp == 65001
        };

        return Ok(if unicode {
            TerminalUnicodeCaps::BasicUnicode
        } else {
            TerminalUnicodeCaps::Ascii
        });
    }

    #[cfg(not(windows))]
    {
        Ok(TerminalUnicodeCaps::Ascii)
    }
}

#[cfg(unix)]
fn get_unix_charset() -> Result<String, TerminalError> {
    use std::process::Command;

    let output = Command::new("locale")
        .arg("charmap")
        .output()
        .map_err(|_| TerminalError::CommandFailed {
            command: "locale charmap".to_string(),
        })?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| TerminalError::InvalidUtf8Response { source: e })
            .map(|s| s.trim().to_string())
    } else {
        Err(TerminalError::CommandFailed {
            command: "locale charmap".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    include!("capabilities.test.rs");
}
