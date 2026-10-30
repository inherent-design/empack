//! CLI recovery commands retain native selections without turning paths into shell code.
use super::session::Session;
use crate::engine::publication::{RecoveryKind, RecoveryLocation, RecoveryStatus};

#[cfg(windows)]
const SHELL: &str = "PowerShell";
#[cfg(not(windows))]
const SHELL: &str = "POSIX shell";

fn quote(value: &str) -> String {
    #[cfg(windows)]
    {
        format!("'{}'", value.replace('\'', "''"))
    }
    #[cfg(not(windows))]
    {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn command(location: &RecoveryLocation, action: &str, operation: Option<&str>) -> Option<String> {
    let project = location.project.to_str()?;
    let state = location.state.to_str()?;
    if !location.project.is_absolute()
        || !location.state.is_absolute()
        || project.chars().any(char::is_control)
        || state.chars().any(char::is_control)
        || operation.is_some_and(|value| value.chars().any(char::is_control))
    {
        return None;
    }
    let mut command = format!(
        "empack --workdir {} --state-dir {} recover {action}",
        quote(project),
        quote(state)
    );
    if let Some(operation) = operation {
        command.push_str(&format!(" --operation {}", quote(operation)));
    }
    Some(command)
}

pub(super) fn annotate(error: anyhow::Error) -> anyhow::Error {
    let Some(location) = error.downcast_ref::<RecoveryLocation>() else {
        return error;
    };
    let guidance = match command(location, "inspect", None) {
        Some(command) => format!("Recovery inspection ({SHELL}):\n  {command}\nOriginal failure"),
        None => "Inspect recovery using the original native --workdir and --state-dir selections; shell command unavailable for these native paths".into(),
    };
    error.context(guidance)
}

pub(super) fn show(session: &dyn Session, location: &RecoveryLocation, status: &RecoveryStatus) {
    let Some(finish) = command(location, "finish", Some(&status.operation)) else {
        session.display().status().info("Recovery commands require the original native --workdir and --state-dir selections; shell command unavailable for these native paths");
        return;
    };
    session
        .display()
        .status()
        .info(&format!("Next ({SHELL}): {finish}"));
    if status.kind == RecoveryKind::Publication && !status.restoring {
        let restore = command(location, "restore", Some(&status.operation)).unwrap();
        session
            .display()
            .status()
            .info(&format!("Alternative ({SHELL}): {restore}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn non_unicode_recovery_paths_are_not_lossily_rendered() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
        let location = RecoveryLocation {
            project: PathBuf::from(OsString::from_vec(b"/project/\xff".to_vec())),
            state: "/state".into(),
        };
        let error = annotate(location.attach(anyhow::anyhow!("original recovery cause")));
        let text = format!("{error:#}");
        assert!(
            text.contains("shell command unavailable for these native paths"),
            "{text}"
        );
        assert!(text.contains("original recovery cause"), "{text}");
        assert!(!text.contains('\u{fffd}'), "{text}");
    }

    #[test]
    fn shell_commands_do_not_emit_control_characters_or_ambiguous_relative_paths() {
        let base = std::env::temp_dir();
        for unsafe_path in ["project\nname", "project\u{1b}name", "project\0name"] {
            let location = RecoveryLocation {
                project: base.join(unsafe_path),
                state: base.join("state"),
            };
            assert!(command(&location, "inspect", None).is_none());
        }
        let relative = RecoveryLocation {
            project: "relative-project".into(),
            state: base.join("state"),
        };
        assert!(command(&relative, "inspect", None).is_none());
    }
}
