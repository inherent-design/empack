//! Copyable arguments for the host's documented shell, never execution authority.
#[cfg(windows)]
pub(super) const NAME: &str = "PowerShell";
#[cfg(not(windows))]
pub(super) const NAME: &str = "Bash/zsh";

pub(super) fn quote(value: &str) -> Option<String> {
    // Rendering must not change the command's path or hide active characters.
    if crate::terminal::display_line(value) != value {
        return None;
    }
    #[cfg(windows)]
    {
        Some(format!("'{}'", value.replace('\'', "''")))
    }
    #[cfg(not(windows))]
    {
        Some(format!("'{}'", value.replace('\'', "'\"'\"'")))
    }
}

/// KEY=VALUE options split at the first '=' after shell parsing.
pub(super) fn association(key: &str, value: &str) -> Option<String> {
    if key.contains('=') {
        return None;
    }
    quote(&format!("{key}={value}"))
}

/// Copyable arguments for a known command; unsafe display text is never executable guidance.
pub(super) fn arguments(values: &[String]) -> Option<String> {
    values
        .iter()
        .map(|value| {
            if !value.is_empty()
                && value
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || b"-_".contains(&ch))
            {
                Some(value.clone())
            } else {
                quote(value)
            }
        })
        .collect::<Option<Vec<_>>>()
        .map(|args| args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_or_ambiguous_text_is_not_a_copyable_argument() {
        for value in ["line\nbreak", "tab\tvalue", "\u{1b}[2J", "rtl\u{202e}value"] {
            assert!(quote(value).is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    fn shell_interprets_quoted_arguments_as_literal_unicode_data() {
        for value in [
            "日本 Ω spaces",
            "-leading-option",
            "a' $(printf BAD) `printf BAD` ; #",
        ] {
            let output = std::process::Command::new("/bin/sh")
                .args([
                    "-c",
                    &format!("printf '%s' --preserve={}", quote(value).unwrap()),
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("--preserve={value}")
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn powershell_arguments_escape_literal_apostrophes() {
        assert_eq!(quote("name'日本"), Some("'name''日本'".into()));
    }
    #[test]
    fn association_hints_preserve_the_cli_separator_and_literal_value() {
        assert!(association("file=key", "PATH").is_none());
        assert!(association("file\u{202e}key", "PATH").is_none());
        assert_eq!(association("key", "a=b"), quote("key=a=b"));
    }
}
