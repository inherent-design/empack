//! Human failure presentation uses the same typed facts as preparation.
use crate::engine::instance::{InstanceAction, InstanceConflicts};
use crate::terminal::display_line;

/// Render a completed command error without dropping its outer failure context.
pub fn message(error: &anyhow::Error) -> String {
    let Some((index, conflicts)) = error.chain().enumerate().find_map(|(index, cause)| {
        cause
            .downcast_ref::<InstanceConflicts>()
            .map(|value| (index, value))
    }) else {
        return general(error);
    };
    let mut lines = Vec::new();
    for cause in error.chain().take(index) {
        lines.push(cause.to_string());
    }
    lines.push("Instance has unresolved file conflicts".into());
    let command = match conflicts.action {
        InstanceAction::Ensure => "ensure",
        InstanceAction::Apply if conflicts.authenticated_release => "update",
        InstanceAction::Apply => "install",
        InstanceAction::Repair => "repair",
        InstanceAction::ChangeChoices => "options",
        InstanceAction::Rollback => "rollback",
    };
    lines.push(format!(
        "For instance {command}, choose one option per file ({}):",
        super::shell::NAME
    ));
    for file in &conflicts.files {
        lines.push(String::new());
        lines.push(format!(
            "{}: {}",
            display_line(&file.path),
            file.explanation()
        ));
        if file.reason == empack_core::instance::FileConflict::WrongKind {
            lines
                .push("    Move the blocking directory to a safe location before retrying.".into());
            continue;
        }
        let Some(destination) = super::shell::quote(&file.destination) else {
            lines.push("    These paths cannot be shown as safe shell arguments; use the original path when choosing an explicit file decision.".into());
            continue;
        };
        if file.can_preserve() {
            lines.push(format!(
                "    --preserve={destination}  {}",
                if file.incoming_policy == Some(empack_core::instance::FilePolicy::Managed) {
                    "Keep your file as a local override"
                } else {
                    "Keep your file outside the release"
                }
            ));
        }
        if file.can_replace() {
            lines.push(format!(
                "    --replace={destination}  {}",
                if file.incoming_policy.is_none() {
                    "Remove your edited file"
                } else if file.incoming_policy == Some(empack_core::instance::FilePolicy::Seed) {
                    "Use the release default"
                } else if file.existing_file {
                    "Replace your file with the release version; empack will manage it"
                } else {
                    "Create the release file; empack will manage it"
                }
            ));
        }
        if file.can_merge()
            && let Some(argument) = super::shell::association(&file.destination, "FILE")
        {
            lines.push(format!(
                "    --merge={argument}  Use your prepared merged file as a local override"
            ));
        }
    }
    lines.join("\n")
}

fn general(error: &anyhow::Error) -> String {
    // Anyhow exposes typed context through downcast_ref, but its std::error
    // chain can contain opaque context wrappers. Never drop a frame based on
    // a failed downcast or its wording; each owner supplies readable Display.
    if error
        .downcast_ref::<crate::engine::diagnostics::Diagnostic>()
        .is_some()
        || error
            .downcast_ref::<crate::engine::publication::RecoveryLocation>()
            .is_some()
    {
        error
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("{error:#}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::instance::InstanceConflict;
    use empack_core::instance::{FileConflict, FilePolicy};

    #[test]
    fn typed_diagnostics_keep_the_cause_and_recovery_without_debug_names() {
        use crate::engine::diagnostics::{
            Diagnostic, DiagnosticCode, DiagnosticPhase, RecoveryClassification,
        };
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::RuntimeRecoveryRequired,
            DiagnosticPhase::Execution,
        );
        diagnostic.recovery = RecoveryClassification::Required;
        diagnostic.object = Some("runtime\u{202e}record".into());
        let error = anyhow::Error::new(crate::application::process_runtime::Interrupted)
            .context("Runtime retired, but ownership cleanup failed")
            .context(diagnostic.clone());
        let before = serde_json::to_value(&diagnostic).unwrap();
        let text = message(&error);
        assert!(
            text.contains("Runtime retired, but ownership cleanup failed\nOperation interrupted"),
            "{text}"
        );
        assert!(text.contains("Runtime ownership needs recovery (while running the operation)"));
        assert!(text.contains("Recovery is required before retrying."));
        assert!(!text.contains("RuntimeRecoveryRequired"));
        assert!(!text.contains('\u{202e}'));
        assert_eq!(
            crate::application::classify_error(&error),
            crate::application::EmpackExitCode::General
        );
        assert_eq!(
            serde_json::to_value(error.downcast_ref::<Diagnostic>().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn a_runtime_failure_does_not_claim_that_nothing_ran() {
        use crate::engine::diagnostics::{
            Diagnostic, DiagnosticCode, DiagnosticPhase, RecoveryClassification,
        };
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::OperationFailed, DiagnosticPhase::Execution);
        diagnostic.recovery = RecoveryClassification::NotPublished;
        let error = anyhow::anyhow!("terminal restoration failed")
            .context("Instance launch failed")
            .context(diagnostic);
        assert_eq!(
            message(&error),
            "Operation failed (while running the operation)\nInstance launch failed\nterminal restoration failed"
        );
    }

    #[test]
    fn recovery_instructions_keep_the_failure_and_exact_directory_selections() {
        let root = std::env::temp_dir();
        let location = crate::engine::publication::RecoveryLocation {
            project: root.join("pack 日本"),
            state: root.join("saved operations"),
        };
        let error = super::super::recovery_guidance::annotate(
            location.attach(anyhow::anyhow!("could not complete file changes")),
        );
        let text = message(&error);
        assert!(text.ends_with("could not complete file changes"), "{text}");
        assert!(text.contains("Recovery inspection ("));
        assert!(text.contains("recover inspect"));
        assert!(text.contains("pack 日本"));
        assert!(text.contains("saved operations"));
        assert_eq!(
            error
                .downcast_ref::<crate::engine::publication::RecoveryLocation>()
                .unwrap()
                .project,
            location.project
        );
    }

    fn report(reason: FileConflict, existing: bool) -> InstanceConflicts {
        InstanceConflicts {
            action: InstanceAction::Repair,
            authenticated_release: false,
            files: vec![InstanceConflict {
                path: ".minecraft/config/my settings.json".into(),
                destination: "config/my settings.json".into(),
                reason,
                existing_file: existing,
                incoming_policy: Some(FilePolicy::Managed),
            }],
        }
    }
    #[test]
    fn managed_edit_explains_choices_with_release_relative_arguments_and_context() {
        let error = anyhow::Error::new(report(FileConflict::ModifiedManagedFile, true))
            .context("Preparing the selected release");
        let text = message(&error);
        assert!(
            text.starts_with(
                "Preparing the selected release\nInstance has unresolved file conflicts"
            )
        );
        assert!(text.contains("For instance repair"));
        assert!(text.contains(
            ".minecraft/config/my settings.json: A pack-managed file has local changes."
        ));
        assert!(text.contains("--preserve='config/my settings.json'"));
        assert!(text.contains("--replace='config/my settings.json'"));
        assert!(text.contains("--merge='config/my settings.json=FILE'"));
        assert!(!text.contains("--preserve='.minecraft/"));
        assert!(!text.contains("ModifiedManagedFile"));
    }
    #[test]
    fn absent_former_seed_only_offers_creation_and_ownership_adoption() {
        let text = message(&report(FileConflict::SeedOwnership, false).into());
        assert!(text.contains("previously user-owned file"));
        assert!(text.contains("--replace="));
        assert!(text.contains("Create the release file; empack will manage it"));
        assert!(!text.contains("--preserve="));
        assert!(!text.contains("--merge="));
    }
    #[test]
    fn directory_removal_and_unrenderable_paths_do_not_get_unsafe_choices() {
        let text = message(&report(FileConflict::WrongKind, false).into());
        assert!(text.contains("Move the blocking directory to a safe location"));
        assert!(!text.contains("--replace="));
        let mut removed = report(FileConflict::ModifiedManagedFile, true);
        removed.files[0].incoming_policy = None;
        let text = message(&removed.into());
        assert!(text.contains("Keep your file outside the release"));
        assert!(text.contains("Remove your edited file"));
        assert!(!text.contains("--merge="));
        let mut unsafe_path = report(FileConflict::UnownedDestination, true);
        unsafe_path.files[0].destination = "config/\u{202e}hidden.json".into();
        let text = message(&unsafe_path.into());
        assert!(text.contains("cannot be shown as safe shell arguments"));
        assert!(!text.contains("--preserve="));
        assert!(!text.contains("--replace="));
        assert!(!text.contains("--merge="));
    }
    #[test]
    fn authenticated_application_names_update_without_suggesting_launch_flags() {
        let mut conflict = report(FileConflict::SeedOwnership, true);
        conflict.action = InstanceAction::Apply;
        conflict.authenticated_release = true;
        let text = message(&conflict.into());
        assert!(text.contains("For instance update"));
        assert!(!text.contains("instance launch --"));
    }
}
