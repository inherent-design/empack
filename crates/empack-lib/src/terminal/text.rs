//! Render external text as data before adding empack-owned terminal styling.
use std::{borrow::Cow, fmt::Write};

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

/// JSON shown on stdout must remain valid data even when it contains terminal
/// controls. JSON escapes preserve decoded keys/values and survive human output
/// adapters without Rust's incompatible `\u{...}` spelling.
pub(crate) fn json_text<T: serde::Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let encoded = serde_json::to_string_pretty(value)?;
    if !encoded.chars().any(|ch| active(ch, false)) {
        return Ok(encoded);
    }
    let mut rendered = String::with_capacity(encoded.len());
    for ch in encoded.chars() {
        if active(ch, false) {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                write!(rendered, "\\u{unit:04x}").expect("writing to a String cannot fail");
            }
        } else {
            rendered.push(ch);
        }
    }
    Ok(rendered)
}

fn active(ch: char, single_line: bool) -> bool {
    (ch.is_control() && (single_line || !matches!(ch, '\n' | '\t')))
        || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn escaped(value: &str, single_line: bool) -> Cow<'_, str> {
    if !value.chars().any(|ch| active(ch, single_line)) {
        return Cow::Borrowed(value);
    }
    let mut rendered = String::with_capacity(value.len());
    for ch in value.chars() {
        if active(ch, single_line) {
            rendered.extend(ch.escape_default());
        } else {
            rendered.push(ch);
        }
    }
    Cow::Owned(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_preserves_unicode_values_and_literal_escapes_without_active_controls() {
        let data = serde_json::json!({
            "日本\u{202e}": ["café 🦀", "\u{009b}[2J", "\u{1b}]52;clipboard\u{7}", "line\nnext\ttab"],
            "literal": "\\u{202e} \\u202e",
        });
        let text = json_text(&data).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            data
        );
        assert!(!text.chars().any(|ch| active(ch, false)));
        assert!(text.contains("\\u202e"));
        assert!(text.contains("\\u009b"));
        assert!(text.contains("café 🦀"));
        assert_eq!(
            display_text(&text),
            text,
            "human adapters must not corrupt JSON"
        );
        assert!(!display_text("\u{202e}unsafe").contains('\u{202e}'));
    }
}
