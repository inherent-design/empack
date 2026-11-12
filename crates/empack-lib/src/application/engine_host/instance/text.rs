//! Instance presentation describes the captured selection, not a new ownership policy.
use crate::engine::{
    instance::{
        InstanceLayout, InstanceRecord, InstanceSide, SelectedRelease,
        subscription::SubscriptionRecord,
    },
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
        "Installed: {} {} ({})\nGame directory: {}/\nRelease: {}",
        display_line(&record.pack),
        display_line(&release.document().version),
        side(record.side),
        record.layout.directory(),
        display_line(&record.release)
    )
}

pub(super) fn selected(release: &SelectedRelease) -> String {
    let source = match release {
        SelectedRelease::Snapshot(_) => "selected snapshot",
        SelectedRelease::Publisher(_) | SelectedRelease::Subscribed(_) => {
            "signed publisher release"
        }
    };
    format!(
        "Selected version: {} ({source})\nRelease: {}",
        display_line(&release.release().document().version),
        release.release().id()
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

pub(super) fn plan(release: &str, changed: usize, downloads: usize) -> String {
    let mut lines = vec![format!("Release: {}", display_line(release))];
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
            plan("release", 0, 0),
            "Release: release\nNo file changes needed."
        );
        assert_eq!(
            plan("release", 2, 1),
            "Release: release\nFile changes: 2\nDownloads: 1"
        );
    }
}
