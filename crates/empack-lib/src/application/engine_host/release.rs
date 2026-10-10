//! Publisher keys are host inputs, never project data or distribution members.
use super::*;
use crate::{
    application::cli::ReleaseCommand,
    engine::{
        api::{OperationPreview, PublishChannelRequest, Request, StageReleaseRequest},
        release,
    },
};

pub(in crate::application) async fn dispatch(
    session: &dyn Session,
    command: ReleaseCommand,
) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let (source, keys) = match &command {
        ReleaseCommand::Stage { source, keys } => (absolute(&invocation, source), keys.clone()),
        ReleaseCommand::PublishChannel { keys, .. } => (root.clone(), keys.clone()),
    };
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
        let request: Request = match command {
            ReleaseCommand::Stage { .. } => StageReleaseRequest {
                source,
                keys: (*keys).clone(),
            }
            .into(),
            ReleaseCommand::PublishChannel {
                release,
                channel,
                base_url,
                sequence,
                expires,
                previous_keys,
                ..
            } => PublishChannelRequest {
                release,
                channel,
                base_url,
                sequence,
                expires,
                keys: (*keys).clone(),
                previous_keys: release::signing::public_keys(&previous_keys)?,
            }
            .into(),
        };
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Release publication requires input")
        };
        let OperationPreview::ReleasePublication(view) = prepared.view() else {
            anyhow::bail!("Unexpected release publication preview")
        };
        session.display().status().info(&format!(
            "Release {} for pack {} at {}",
            view.release, view.pack, view.envelope
        ));
        for key in &view.keys {
            session
                .display()
                .status()
                .info(&format!("Publisher key fingerprint: {key}"));
        }
        let label = if let Some(channel) = &view.channel {
            session.display().status().info(&format!(
                "Verify hosted release and assets before advancing channel {channel}"
            ));
            "Channel publication"
        } else {
            "Immutable release"
        };
        apply(session, &engine, prepared, label, |receipt| {
            let ExecutionReceipt::ReleasePublication(receipt) = receipt else {
                anyhow::bail!("Unexpected release publication receipt")
            };
            Ok(match &receipt.channel {
                Some(channel) => format!(
                    "Published channel {channel} for verified release {} at {}",
                    receipt.release, receipt.envelope
                ),
                None => format!(
                    "Staged signed release {} at {}; channel unchanged",
                    receipt.release, receipt.envelope
                ),
            })
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
