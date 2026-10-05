use std::fmt::Write;

/// A scalar QString in QSettings INI syntax, used by the launcher instance format.
/// Quotes preserve whitespace and suppress list separators. Leading @ and NUL use
/// QSettings' string representation rather than its QVariant type syntax.
pub(super) fn ini_value(value: &str) -> String {
    let value = if value.contains('\0') {
        format!("@String({value})")
    } else if value.starts_with('@') {
        format!("@{value}")
    } else {
        value.to_owned()
    };
    let mut result = String::from("\"");
    let mut hex_escape = false;
    for ch in value.chars() {
        if ch.is_ascii_control() || (hex_escape && ch.is_ascii_hexdigit()) {
            // QSettings consumes adjacent hex digits; escape the next such character
            // too so, for example, U+0001 followed by 'a' cannot become U+001A.
            write!(result, "\\x{:x}", ch as u32).expect("String write cannot fail");
            hex_escape = true;
        } else {
            if matches!(ch, '\\' | '"') {
                result.push('\\');
            }
            result.push(ch);
            hex_escape = false;
        }
    }
    result.push('"');
    result
}

/// Java properties value (also safe as a value fragment). ASCII serialization works
/// with both Properties.load(InputStream) and Properties.load(Reader). Unicode escapes
/// contain UTF-16 code units, including surrogate pairs for supplementary characters.
pub(super) fn properties_value(value: &str) -> String {
    let mut result = String::new();
    for unit in value.encode_utf16() {
        match unit {
            0x20 => result.push_str("\\ "),
            0x5c => result.push_str("\\\\"),
            0x21..=0x7e => result.push(char::from_u32(u32::from(unit)).unwrap()),
            _ => write!(result, "\\u{unit:04x}").expect("String write cannot fail"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_scalar_preserves_types_separators_controls_and_unicode() {
        assert_eq!(ini_value("@Variant(test)"), "\"@@Variant(test)\"");
        assert_eq!(ini_value(" a,;=\\\"é🦀 "), "\" a,;=\\\\\\\"é🦀 \"");
        assert_eq!(
            ini_value("\u{1}aF\n[General]"),
            "\"\\x1\\x61\\x46\\xa[General]\""
        );
        assert_eq!(ini_value("x\0a"), "\"@String(x\\x0\\x61)\"");
        assert_eq!(ini_value(""), "\"\"");
    }

    #[test]
    fn properties_value_preserves_unicode_and_literal_escape_sequences() {
        assert_eq!(
            properties_value(" a\\u0041\r\né🦀\t"),
            "\\ a\\\\u0041\\u000d\\u000a\\u00e9\\ud83e\\udd80\\u0009"
        );
        assert_eq!(properties_value("#!=:value"), "#!=:value");
        assert_eq!(properties_value(""), "");
    }
}
