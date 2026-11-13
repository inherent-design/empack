//! Instance presentation describes the captured selection, not a new ownership policy.
use crate::engine::{
    api::InstancePreview,
    instance::{InstanceLayout, InstanceRecord, InstanceSide, subscription::SubscriptionRecord},
    release::DecodedRelease,
};
use crate::terminal::display_line;

fn side(value: InstanceSide) -> &'static str {
    match value {
        InstanceSide::Client => "client",
        InstanceSide::Server => "server",
    }
}

pub(super) fn first(side_value: InstanceSide, layout: InstanceLayout) -> String {
    format!(
        "First installation: {} ({}/)",
        side(side_value),
        layout.directory()
    )
}

pub(super) fn installed(record: &InstanceRecord, release: &DecodedRelease) -> String {
    format!(
        "Installed: {} {} ({})\nGame directory: {}/",
        display_line(&record.pack),
        display_line(&release.document().version),
        side(record.side),
        record.layout.directory(),
    )
}

fn selected(record: &InstanceRecord, version: &str, show_identity: bool) -> String {
    let mut lines = vec![format!("Selected version: {}", display_line(version))];
    if show_identity {
        lines.push(format!("Release: {}", display_line(&record.release)));
    }
    lines.join("\n")
}

pub(super) fn completed(record: &InstanceRecord, version: &str) -> String {
    format!(
        "Instance files ready: {} {} ({}/)",
        display_line(&record.pack),
        display_line(version),
        record.layout.directory()
    )
}

pub(super) fn inspect(record: &InstanceRecord, version: &str) -> String {
    let mut lines = vec![
        format!(
            "Pack: {} {}",
            display_line(&record.pack),
            display_line(version)
        ),
        format!("Environment: {}", side(record.side)),
        format!("Game directory: {}/", record.layout.directory()),
        format!("Release: {}", display_line(&record.release)),
    ];
    if !record.choices.is_empty() {
        lines.push("Options:".into());
        for choice in &record.choices {
            lines.push(format!(
                "    {}={}",
                display_line(&choice.key),
                display_line(&choice.value)
            ));
        }
    }
    if !record.local_overrides.is_empty() {
        lines.push("Local overrides:".into());
        for local in &record.local_overrides {
            lines.push(format!(
                "    {} (SHA-256 {})",
                display_line(&local.destination),
                display_line(&local.accepted.sha256)
            ));
        }
    }
    for release in &record.history {
        lines.push(format!("Retained release: {}", display_line(release)));
    }
    lines.join("\n")
}

pub(super) fn plan(view: &InstancePreview, show_identity: bool) -> String {
    selection_plan(
        &view.record,
        &view.release_version,
        show_identity,
        view.files.changes().len(),
        view.downloads.len(),
    )
}
fn selection_plan(
    record: &InstanceRecord,
    version: &str,
    show_identity: bool,
    changed: usize,
    downloads: usize,
) -> String {
    format!(
        "{}\n{}",
        selected(record, version, show_identity),
        changes(changed, downloads)
    )
}
pub(super) fn changes(changed: usize, downloads: usize) -> String {
    let mut lines = Vec::new();
    if changed == 0 {
        lines.push("No file changes needed.".into());
    } else {
        lines.push(format!("File changes: {changed}"));
    }
    if downloads > 0 {
        lines.push(format!("Downloads: {downloads}"));
    }
    lines.join("\n")
}

pub(super) fn publisher(
    record: &SubscriptionRecord,
    origin: &str,
    fingerprints: &[String],
    observing: bool,
) -> String {
    let mut lines = vec![
        format!(
            "Publisher: {} / {}",
            display_line(&record.pack),
            display_line(&record.channel)
        ),
        format!("Host: {}", display_line(origin)),
        format!("Trusted keys: {}", record.keys.len()),
    ];
    if !observing && let Some(floor) = &record.floor {
        lines.push(format!("Update number: {}", floor.sequence));
    }
    if record.keys.is_empty() {
        lines.push("Publisher updates are disabled; saved update history is kept.".into());
    }
    for fingerprint in fingerprints {
        lines.push(format!("Key fingerprint: {}", display_line(fingerprint)));
    }
    lines.join("\n")
}

pub(super) fn offer_preview(release: &str, sequence: u64, dry_run: bool) -> String {
    format!(
        "Offered release: {}\nUpdate number: {sequence}\nOffer verified; installation plan is not prepared because this offer has not been saved.\n{}",
        display_line(release),
        if dry_run {
            "Run without --dry-run and with --apply to accept the offer and prepare installation."
        } else {
            "Call with --apply to accept the offer and prepare installation."
        },
    )
}

pub(super) fn offer(release: &str, sequence: u64) -> String {
    format!(
        "Offered release: {}\nUpdate number: {sequence}\nApproval saves this update offer, even if installation is declined or fails. It does not install files.",
        display_line(release)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::instance::{ChoiceSelection, FileIdentity, LocalOverride};
    fn record() -> InstanceRecord {
        InstanceRecord {
            schema: 1,
            root: "fixture".into(),
            pack: "Workshop 日本".into(),
            release: "a".repeat(64),
            side: InstanceSide::Client,
            layout: InstanceLayout::Prism,
            choices: vec![],
            history: vec![],
            local_overrides: vec![],
            initialized_worlds: vec![],
            asset_bases: vec![],
        }
    }
    #[test]
    fn inspection_keeps_choices_local_files_and_exact_release_history() {
        let mut record = record();
        record.choices.push(ChoiceSelection {
            key: "extras".into(),
            value: "enabled".into(),
        });
        let identity = FileIdentity {
            sha256: "b".repeat(64),
            bytes: 7,
            readonly: false,
            executable: false,
        };
        record.local_overrides.push(LocalOverride {
            destination: "config/local.cfg".into(),
            original: identity.clone(),
            accepted: identity,
        });
        record.history.push("c".repeat(64));
        assert_eq!(
            inspect(&record, "2"),
            format!(
                "Pack: Workshop 日本 2\nEnvironment: client\nGame directory: .minecraft/\nRelease: {}\nOptions:\n    extras=enabled\nLocal overrides:\n    config/local.cfg (SHA-256 {})\nRetained release: {}",
                "a".repeat(64),
                "b".repeat(64),
                "c".repeat(64)
            )
        );
        assert_eq!(
            first(InstanceSide::Server, InstanceLayout::Game),
            "First installation: server (game/)"
        );
        assert_eq!(
            selected(&record, "2", true),
            format!("Selected version: 2\nRelease: {}", record.release)
        );
        assert_eq!(selected(&record, "2", false), "Selected version: 2");
        assert_eq!(
            completed(&record, "2"),
            "Instance files ready: Workshop 日本 2 (.minecraft/)"
        );
    }
    #[test]
    fn external_labels_cannot_insert_report_lines_or_hide_identity() {
        let mut record = record();
        record.pack = "Pack\nFAKE\u{202e}".into();
        let text = inspect(&record, "v\t2");
        assert!(
            text.starts_with("Pack: Pack\\nFAKE\\u{202e} v\\t2"),
            "{text}"
        );
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains(&format!("Release: {}", record.release)));
        assert!(!text.contains("Options:"));
    }
    #[test]
    fn publisher_preview_distinguishes_offer_from_installation() {
        let record = SubscriptionRecord {
            schema: 1,
            root: "fixture".into(),
            pack: "fixture".into(),
            channel: "stable".into(),
            url: "https://publisher.test/path?private=omit".into(),
            keys: vec!["key".into()],
            floor: None,
            observed: None,
        };
        assert_eq!(
            publisher(
                &record,
                "https://publisher.test",
                &["fingerprint".into()],
                false
            ),
            "Publisher: fixture / stable\nHost: https://publisher.test\nTrusted keys: 1\nKey fingerprint: fingerprint"
        );
        assert_eq!(
            offer("release", 7),
            "Offered release: release\nUpdate number: 7\nApproval saves this update offer, even if installation is declined or fails. It does not install files."
        );
        let mut revoked = record;
        revoked.keys.clear();
        assert!(
            publisher(&revoked, "https://publisher.test", &[], false)
                .contains("Publisher updates are disabled; saved update history is kept.")
        );
    }
    #[test]
    fn a_verified_unchanged_plan_does_not_claim_a_download_or_install() {
        assert_eq!(
            selection_plan(&record(), "2", true, 0, 0),
            format!(
                "Selected version: 2\nRelease: {}\nNo file changes needed.",
                "a".repeat(64)
            )
        );
        assert_eq!(
            selection_plan(&record(), "2", false, 2, 1),
            "Selected version: 2\nFile changes: 2\nDownloads: 1"
        );
    }
}
