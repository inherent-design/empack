//! Render external text as data before adding empack-owned terminal styling.
use std::borrow::Cow;

/// Escape active terminal and directional controls, preserving ordinary Unicode
/// and the line breaks/tabs used by complete human diagnostics. Machine output
/// and inherited child streams retain their separate contracts.
pub fn display_text(value: &str) -> Cow<'_, str> {
    escaped(value, false)
}

/// Table cells and progress labels cannot introduce additional layout lines.
pub(crate) fn display_line(value: &str) -> Cow<'_, str> {
    escaped(value, true)
}

fn escaped(value: &str, single_line: bool) -> Cow<'_, str> {
    let active = |ch: char| {
        (ch.is_control() && (single_line || !matches!(ch, '\n' | '\t')))
            || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    };
    if !value.chars().any(active) {
        return Cow::Borrowed(value);
    }
    let mut rendered = String::with_capacity(value.len());
    for ch in value.chars() {
        if active(ch) {
            rendered.extend(ch.escape_default());
        } else {
            rendered.push(ch);
        }
    }
    Cow::Owned(rendered)
}
