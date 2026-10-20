//! Local authoring inspection and document-only provider pin edits.
use super::*;
use crate::application::cli::ManifestCommand;
use crate::engine::{
    api::SyncRequest,
    documents::{AuthoringInspection, DocumentCodec},
    resources::ResourceRequest,
};
use empack_core::model::DependencyKey;

pub async fn pin(session: &dyn Session, keys: Vec<String>, pinned: bool) -> Result<()> {
    let keys = NonEmpty::new(
        keys.iter()
            .map(|key| DependencyKey::parse(key))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    )?;
    dependencies::synchronize(session, SyncRequest::SelectionIntent { keys, pinned }).await
}
pub async fn inspect(session: &dyn Session, command: ManifestCommand) -> Result<()> {
    let (invocation, project) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    let report = initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 64 << 20,
                open_files: 8,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 64 << 20,
                ..Default::default()
            },
            move |cancel| {
                let snapshot = ProjectReader::new(RecoveryReader::new(state)).capture(
                    &project,
                    &[],
                    SnapshotLimits::default(),
                    &cancel,
                )?;
                Ok::<_, anyhow::Error>(
                    DocumentCodec.inspect_authoring(snapshot.intent(), snapshot.prior_lock()),
                )
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    let text = render(&report, command)?;
    session.display().status().message(&text);
    Ok(())
}
fn render(report: &AuthoringInspection, command: ManifestCommand) -> Result<String> {
    let caveat = "Local metadata only: no provider queries or file verification. Diff compares current constraints with locked selections; the lock does not retain earlier display metadata or build profiles.";
    let limitations = report
        .editing_limitations
        .iter()
        .map(|item| format!("{} {}: {}", item.document, item.path.join("."), item.reason))
        .collect::<Vec<_>>()
        .join("\n");
    let caveat = if limitations.is_empty() {
        caveat.to_owned()
    } else {
        format!("{caveat}\nEditing limitations (input remains readable):\n{limitations}")
    };
    match command {
        ManifestCommand::Check { json } => {
            if json {
                Ok(serde_json::to_string_pretty(report)?)
            } else {
                Ok(format!(
                    "{}: valid schema {} authoring; lock {}\n{}",
                    report.pack, report.author_schema, report.lock_status, caveat
                ))
            }
        }
        ManifestCommand::Explain { dependency, json } => {
            let row = report
                .dependencies
                .iter()
                .find(|row| row.key == dependency)
                .context("Unknown dependency key")?;
            if json {
                return Ok(serde_json::to_string_pretty(row)?);
            }
            let mut lines = vec![format!(
                "{} ({})",
                row.key,
                if row.explicit {
                    "authored root"
                } else {
                    "retained dependency"
                }
            )];
            if let Some(request) = &row.effective {
                lines.push(format!(
                    "Effective request:\n{}",
                    serde_saphyr::to_string(request)?
                ));
            }
            for (label, value) in [
                ("Canonical project", &row.canonical_project),
                ("Locked selection", &row.locked_selection),
            ] {
                if let Some(value) = value {
                    lines.push(format!("{label}: {}", serde_json::to_string(value)?));
                }
            }
            if let Some(policy) = row.evidence_policy {
                lines.push(format!(
                    "URL evidence: {policy}; retained observations remain binding"
                ));
            }
            if let Some(policy) = row.release_policy {
                lines.push(format!(
                    "Release policy: {policy} [{}]",
                    row.policy_origin.unwrap_or("default")
                ));
            }
            if let Some(reason) = &row.reason {
                lines.push(reason.clone());
            }
            lines.push(caveat);
            Ok(lines.join("\n"))
        }
        ManifestCommand::Diff { json } => {
            if json {
                Ok(serde_json::to_string_pretty(report)?)
            } else {
                let mut lines = vec![format!("Author intent versus lock: {}", report.lock_status)];
                if report.runtime_satisfied == Some(false) {
                    lines.push("Runtime request requires resolution".into());
                }
                for row in &report.dependencies {
                    if let Some(reason) = &row.reason {
                        lines.push(format!("{}: {reason}", row.key));
                    }
                }
                if let Some(reason) = &report.lock_reason {
                    lines.push(reason.clone());
                }
                lines.push(caveat);
                Ok(lines.join("\n"))
            }
        }
    }
}
