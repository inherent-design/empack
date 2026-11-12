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
    match command {
        ManifestCommand::Check { json: true } | ManifestCommand::Diff { json: true } => {
            Ok(crate::terminal::json_text(report)?)
        }
        ManifestCommand::Explain { dependency, json } => {
            let row = report
                .dependencies
                .iter()
                .find(|row| row.key == dependency)
                .context("Dependency is not in the author files or lock")?;
            if json {
                Ok(crate::terminal::json_text(row)?)
            } else {
                text::explain(report, row)
            }
        }
        ManifestCommand::Check { json: false } => Ok(text::check(report)),
        ManifestCommand::Diff { json: false } => Ok(text::diff(report)),
    }
}

mod text;
