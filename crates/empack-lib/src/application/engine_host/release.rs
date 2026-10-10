//! Publisher keys are host inputs, never project data or distribution members.
use super::*;
use crate::{
    application::cli::ReleaseCommand,
    engine::{
        api::{OperationPreview, StageReleaseRequest},
        release,
    },
};

pub(in crate::application) async fn dispatch(
    session: &dyn Session,
    command: ReleaseCommand,
) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let ReleaseCommand::Stage { source, keys } = command;
    let source = absolute(&invocation, &source);
    let files = keys
        .into_iter()
        .map(|key| absolute(&invocation, &key))
        .collect::<Vec<_>>();
    let output = root.clone();
    let input = source.clone();
    let keys = initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 1 << 20,
                open_files: 8,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 4096,
                ..Default::default()
            },
            move |cancel| release::signing::read_keys(&output, &input, &files, &cancel),
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) = cancellable(
            session,
            engine.prepare(
                root,
                StageReleaseRequest {
                    source,
                    keys: (*keys).clone(),
                },
            ),
        )
        .await?
        else {
            anyhow::bail!("Release staging requires input")
        };
        let OperationPreview::ReleasePublication(view) = prepared.view() else {
            anyhow::bail!("Unexpected release publication preview")
        };
        session.display().status().info(&format!(
            "Stage release {} for pack {} at {}",
            view.release, view.pack, view.envelope
        ));
        for key in &view.keys {
            session
                .display()
                .status()
                .info(&format!("Publisher key fingerprint: {key}"));
        }
        apply(session, &engine, prepared, "Immutable release", |receipt| {
            let ExecutionReceipt::ReleasePublication(receipt) = receipt else {
                anyhow::bail!("Unexpected release publication receipt")
            };
            Ok(format!(
                "Staged signed release {} at {}; channel unchanged",
                receipt.release, receipt.envelope
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
