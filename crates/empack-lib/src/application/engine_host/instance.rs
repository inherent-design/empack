//! Explicit snapshot installation, with no implicit subscription or author resolution.
use super::*;
use crate::{
    application::cli::InstanceCommand,
    engine::{
        api::{InstallInstanceRequest, OperationPreview},
        instance::{ChoiceSelection, InstanceLayout, InstanceSide, SelectedRelease},
        release::{DecodedRelease, trust::SelectedSnapshot},
    },
};
use std::collections::BTreeMap;

async fn install(session: &dyn Session, command: InstanceCommand) -> Result<()> {
    let prepare_current = matches!(command, InstanceCommand::Prepare { .. });
    let (InstanceCommand::Install {
        release,
        sha256,
        side,
        layout,
        choices,
        files,
    }
    | InstanceCommand::Prepare {
        release,
        sha256,
        side,
        layout,
        choices,
        files,
    }) = command
    else {
        anyhow::bail!("Expected a snapshot installation request");
    };
    let (invocation, root) = project_path(session)?;
    let selected = absolute(&invocation, &release);
    let assets = selected
        .parent()
        .context("Release source has no directory")?
        .to_owned();
    let mut local_files = BTreeMap::new();
    for input in files {
        let (key, path) = input
            .split_once('=')
            .context("File association must be KEY=PATH")?;
        ensure!(
            !key.is_empty() && !path.is_empty(),
            "File association must name a key and path"
        );
        ensure!(
            local_files
                .insert(key.to_owned(), absolute(&invocation, Path::new(path)))
                .is_none(),
            "Duplicate file association"
        );
    }
    let choices = choices
        .into_iter()
        .map(|input| {
            let (key, value) = input.rsplit_once('=').context("Choice must be KEY=VALUE")?;
            Ok(ChoiceSelection {
                key: key.into(),
                value: value.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let decoded = initialize::discover(session, move |mut scope| async move {
        let mut resources = operation_resources().capture;
        resources.scratch_bytes = 0; // Bounded document reads create no temporary content.
        let work =
            scope.spawn_blocking(resources, operation_resources().prepared, move |cancel| {
                DecodedRelease::read(&selected, &cancel)
            })?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    // Development builds implement this protocol version without pretending to be a tagged binary.
    let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
        "0.6.0-beta"
    } else {
        env!("CARGO_PKG_VERSION")
    };
    let snapshot =
        SelectedSnapshot::select(decoded.bytes(), &sha256, &semver::Version::parse(version)?)?;
    let request = InstallInstanceRequest {
        action: if prepare_current {
            crate::engine::instance::InstanceAction::Prepare
        } else {
            crate::engine::instance::InstanceAction::Apply
        },
        layout: layout
            .map(|layout| match layout.as_str() {
                "game" => Ok(InstanceLayout::Game),
                "prism" => Ok(InstanceLayout::Prism),
                _ => anyhow::bail!("Instance layout must be game or prism"),
            })
            .transpose()?,
        release: SelectedRelease::Snapshot(snapshot),
        side: match side.as_str() {
            "server" => InstanceSide::Server,
            "client" => InstanceSide::Client,
            _ => anyhow::bail!("Instance side must be client or server"),
        },
        choices,
        supplied: BTreeMap::new(),
        local_files,
        assets: Some(assets),
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Instance content requires input");
        };
        let OperationPreview::Instance(view) = prepared.view() else {
            anyhow::bail!("Unexpected instance preview");
        };
        session.display().status().info(&format!(
            "Release {}: {} exact file changes, {} downloads",
            view.record.release,
            view.files.changes().len(),
            view.downloads.len()
        ));
        apply(session, &engine, prepared, "Instance content", |receipt| {
            let ExecutionReceipt::Instance(receipt) = receipt else {
                anyhow::bail!("Unexpected instance receipt");
            };
            Ok(format!(
                "Applied release {} to {}/",
                receipt.record.release,
                receipt.record.layout.directory()
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}

pub(in crate::application) async fn dispatch(
    session: &dyn Session,
    command: InstanceCommand,
) -> Result<()> {
    match command {
        command @ (InstanceCommand::Subscribe { .. }
        | InstanceCommand::Trust { .. }
        | InstanceCommand::ObserveChannel { .. }) => subscription(session, command).await,
        command @ (InstanceCommand::Install { .. } | InstanceCommand::Prepare { .. }) => {
            install(session, command).await
        }
        InstanceCommand::Inspect => {
            let (invocation, root) = project_path(session)?;
            let state = state_root(session.config().app_config(), &invocation)?;
            let inspected = inspect(session, root, None, state).await?;
            let (record, _release) = &*inspected;
            session.display().status().info(&format!(
                "Pack {} ({:?}), release {}",
                record.pack, record.side, record.release
            ));
            for choice in &record.choices {
                session
                    .display()
                    .status()
                    .info(&format!("{}={}", choice.key, choice.value));
            }
            for release in &record.history {
                session
                    .display()
                    .status()
                    .info(&format!("Retained release: {release}"));
            }
            Ok(())
        }
        InstanceCommand::Repair { assets, files } => {
            maintain(session, None, assets, files, Vec::new()).await
        }
        InstanceCommand::Rollback {
            release,
            assets,
            files,
            choices,
        } => maintain(session, Some(release), assets, files, choices).await,
    }
}
async fn inspect(
    session: &dyn Session,
    root: PathBuf,
    release: Option<String>,
    state: PathBuf,
) -> Result<
    crate::engine::runtime::RetainedOutput<(
        crate::engine::instance::InstanceRecord,
        DecodedRelease,
    )>,
> {
    initialize::discover(session, move |mut scope| async move {
        let mut resources = operation_resources().capture;
        resources.scratch_bytes = 0;
        let work =
            scope.spawn_blocking(resources, operation_resources().prepared, move |cancel| {
                crate::engine::instance::inspect(
                    &root,
                    release.as_deref(),
                    crate::engine::publication::RecoveryReader::new(state),
                    SnapshotLimits::default(),
                    &cancel,
                )
            })?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await
}
async fn maintain(
    session: &dyn Session,
    release: Option<String>,
    assets: Option<PathBuf>,
    files: Vec<String>,
    choices: Vec<String>,
) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    let action = if release.is_some() {
        crate::engine::instance::InstanceAction::Rollback
    } else {
        crate::engine::instance::InstanceAction::Repair
    };
    let inspected = inspect(session, root.clone(), release, state).await?;
    let (record, decoded) = &*inspected;
    let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
        "0.6.0-beta"
    } else {
        env!("CARGO_PKG_VERSION")
    };
    let selected = SelectedSnapshot::select(
        decoded.bytes(),
        decoded.id(),
        &semver::Version::parse(version)?,
    )?;
    let mut local_files = BTreeMap::new();
    for input in files {
        let (key, path) = input
            .split_once('=')
            .context("File association must be KEY=PATH")?;
        ensure!(
            !path.is_empty() && decoded.document().files.iter().any(|file| file.key == key),
            "File association must name an exact release file and host path"
        );
        ensure!(
            local_files
                .insert(key.to_owned(), absolute(&invocation, Path::new(path)))
                .is_none(),
            "Duplicate file association"
        );
    }
    let choices = choices
        .into_iter()
        .map(|input| {
            let (key, value) = input.rsplit_once('=').context("Choice must be KEY=VALUE")?;
            Ok(ChoiceSelection {
                key: key.into(),
                value: value.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let request = InstallInstanceRequest {
        action,
        layout: None,
        release: SelectedRelease::Snapshot(selected),
        side: record.side,
        choices,
        supplied: BTreeMap::new(),
        local_files,
        assets: assets.map(|path| absolute(&invocation, &path)),
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Instance maintenance requires input");
        };
        let OperationPreview::Instance(view) = prepared.view() else {
            anyhow::bail!("Unexpected instance preview");
        };
        session.display().status().info(&format!(
            "Release {}: {} exact file changes, {} downloads",
            view.record.release,
            view.files.changes().len(),
            view.downloads.len()
        ));
        apply(
            session,
            &engine,
            prepared,
            "Instance maintenance",
            |receipt| {
                let ExecutionReceipt::Instance(receipt) = receipt else {
                    anyhow::bail!("Unexpected instance receipt");
                };
                Ok(format!(
                    "Applied release {} to {}/",
                    receipt.record.release,
                    receipt.record.layout.directory()
                ))
            },
        )
        .await
    }
    .await;
    engine.shutdown().await;
    result
}

async fn subscription(session: &dyn Session, command: InstanceCommand) -> Result<()> {
    use crate::engine::api::SubscriptionRequest;
    use ed25519_dalek::VerifyingKey;
    let keys = |values: Vec<String>| -> Result<Vec<VerifyingKey>> {
        values
            .into_iter()
            .map(|value| {
                ensure!(
                    value.len() == 64
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                    "Publisher public keys require 64 lowercase hexadecimal characters"
                );
                let mut bytes = [0; 32];
                for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
                    bytes[index] = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
                }
                Ok(VerifyingKey::from_bytes(&bytes)?)
            })
            .collect()
    };
    let (invocation, root) = project_path(session)?;
    let request = match command {
        InstanceCommand::Subscribe {
            pack,
            channel,
            url,
            keys: values,
        } => SubscriptionRequest::Enroll {
            pack,
            channel,
            url,
            keys: keys(values)?,
        },
        InstanceCommand::Trust { keys: values, .. } => SubscriptionRequest::ReplaceKeys {
            keys: keys(values)?,
        },
        InstanceCommand::ObserveChannel { envelope } => {
            let path = absolute(&invocation, &envelope);
            let bytes = initialize::discover(session, move |mut scope| async move {
                let work = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        memory_bytes: 1 << 20,
                        open_files: 8,
                        ..Default::default()
                    },
                    ResourceRequest {
                        memory_bytes: 128 << 10,
                        ..Default::default()
                    },
                    move |cancel| {
                        crate::engine::release::trust::read_channel_envelope(&path, &cancel)
                    },
                )?;
                scope.accept(work.wait().await?)?.transpose()
            })
            .await?;
            let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
                "0.6.0-beta"
            } else {
                env!("CARGO_PKG_VERSION")
            };
            SubscriptionRequest::Observe {
                envelope: bytes.iter().copied().collect(),
                now: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs()
                    .try_into()?,
                engine: semver::Version::parse(version)?,
            }
        }
        _ => anyhow::bail!("Expected a subscription command"),
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Unexpected subscription input request")
        };
        let OperationPreview::Subscription(view) = prepared.view() else {
            anyhow::bail!("Unexpected subscription preview")
        };
        session.display().status().info(&format!(
            "Publisher for {} / {} at {}: {} enrolled keys; sequence {}",
            view.record.pack,
            view.record.channel,
            reqwest::Url::parse(&view.record.url)?
                .origin()
                .ascii_serialization(),
            view.record.keys.len(),
            view.record.floor.as_ref().map_or(0, |floor| floor.sequence)
        ));
        for key in keys(view.record.keys.clone())? {
            session.display().status().info(&format!(
                "Publisher key fingerprint: {}",
                crate::engine::release::trust::key_id(&key)
            ));
        }
        apply(session, &engine, prepared, "Publisher trust", |receipt| {
            let ExecutionReceipt::Subscription(receipt) = receipt else {
                anyhow::bail!("Unexpected subscription receipt")
            };
            Ok(format!(
                "Saved subscription for {} / {}",
                receipt.record.pack, receipt.record.channel
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
