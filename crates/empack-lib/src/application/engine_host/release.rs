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
    if let ReleaseCommand::Init { key_file } = &command {
        let file = absolute(&invocation, key_file);
        let setup = release::signing::PublisherSetup::prepare(&root, &file)?;
        session.display().status().info(&format!(
            "Publisher directory: {}; create private signing key at {}",
            root.display(),
            file.display()
        ));
        if !approve(session, "Publisher key setup")? {
            return Ok(());
        }
        let generated = initialize::discover(session, move |mut scope| async move {
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
                move |cancel| setup.create(&cancel),
            )?;
            scope.accept(work.wait().await?)?.transpose()
        })
        .await?;
        let (public, fingerprint) = &*generated;
        session.display().status().message(&format!(
            "Created publisher key\nPublic key: {public}\nFingerprint: {fingerprint}"
        ));
        session.display().status().info("Keep the private key outside every project, export and hosted directory. Share the public key independently; subscribers must explicitly enroll it.");
        let state = state_root(session.config().app_config(), &invocation).ok();
        if let Some(arguments) = root
            .to_str()
            .zip(file.to_str())
            .zip(state.as_deref().and_then(Path::to_str))
            .and_then(|((root, file), state)| {
                crate::application::shell::arguments(&[
                    "--workdir".into(),
                    root.into(),
                    "--state-dir".into(),
                    state.into(),
                    "release".into(),
                    "stage".into(),
                    "EXPORT".into(),
                    "--key-file".into(),
                    file.into(),
                ])
            })
        {
            session.display().status().message(&format!(
                "Next ({}): empack {arguments}",
                crate::application::shell::NAME
            ));
        } else {
            session.display().status().info("Next, stage an extracted native export with release stage. Use the original publisher, state and signing-key paths; a copyable command is unavailable for these paths.");
        }
        session.display().status().info("After staging, upload the release directory and assets first. Create and upload the channel file last.");
        return Ok(());
    }
    let (source, keys) = match &command {
        ReleaseCommand::Init { .. } => unreachable!(),
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
            ReleaseCommand::Init { .. } => unreachable!(),
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
                previous_keys,
                ..
            } => PublishChannelRequest {
                release,
                channel,
                base_url,
                sequence,
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
                "Verify the hosted release and assets before creating the new {channel} channel file"
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
                    "Created signed channel {channel} for verified release {} at {}; upload this file last",
                    receipt.release, receipt.envelope
                ),
                None => format!(
                    "Staged signed release {} at {}; upload this release directory and all assets before publishing a channel",
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
