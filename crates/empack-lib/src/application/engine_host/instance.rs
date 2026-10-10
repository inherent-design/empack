//! Explicit snapshot and enrolled-release installation without author resolution.
use super::*;
use crate::{
    application::cli::InstanceCommand,
    engine::{
        api::{InstallInstanceRequest, OperationPreview},
        instance::{
            ChoiceSelection, InstanceAction, InstanceLayout, InstanceSide, SelectedRelease,
        },
        release::{DecodedRelease, trust::SelectedSnapshot},
    },
};
use std::collections::BTreeMap;

async fn install(session: &dyn Session, command: InstanceCommand) -> Result<()> {
    let prepare_current = matches!(command, InstanceCommand::Prepare { .. });
    let subscribed = matches!(command, InstanceCommand::Update { .. });
    let (release, sha256, side, layout, choices, files, conflicts) = match command {
        InstanceCommand::Install {
            conflicts,
            release,
            sha256,
            side,
            layout,
            choices,
            files,
        }
        | InstanceCommand::Prepare {
            conflicts,
            release,
            sha256,
            side,
            layout,
            choices,
            files,
        } => (
            Some(release),
            Some(sha256),
            side,
            layout,
            choices,
            files,
            conflicts,
        ),
        InstanceCommand::Update {
            conflicts,
            release,
            side,
            layout,
            choices,
            files,
        } => (release, None, side, layout, choices, files, conflicts),
        _ => anyhow::bail!("Expected an instance installation request"),
    };
    let (invocation, root) = project_path(session)?;
    let selected = release.map(|path| absolute(&invocation, &path));
    let assets = selected
        .as_ref()
        .and_then(|path| path.parent())
        .map(Path::to_owned);
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
    let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
        "0.6.0-beta"
    } else {
        env!("CARGO_PKG_VERSION")
    };
    let version = semver::Version::parse(version)?;
    enum ReleaseInput {
        Snapshot(Box<crate::engine::runtime::RetainedOutput<DecodedRelease>>),
        Subscribed(
            crate::engine::runtime::RetainedOutput<
                std::sync::Arc<crate::engine::instance::subscription::SubscribedRelease>,
            >,
        ),
    }
    let input = if subscribed {
        let version = version.clone();
        let selected_root = root.clone();
        let state = state_root(session.config().app_config(), &invocation)?;
        let proof = initialize::discover(session, move |mut scope| async move {
            let Some(selected) = selected else {
                return crate::engine::instance::subscription::fetch_release(
                    &mut scope,
                    selected_root,
                    state,
                    &crate::engine::acquisition::HttpAcquisition::new()?,
                    version,
                )
                .await;
            };
            let work = scope.spawn_blocking(
                ResourceRequest {
                    jobs: 1,
                    memory_bytes: 128 << 20,
                    open_files: 16,
                    ..Default::default()
                },
                ResourceRequest {
                    memory_bytes: 64 << 20,
                    ..Default::default()
                },
                move |cancel| {
                    let bytes =
                        crate::engine::release::trust::read_release_envelope(&selected, &cancel)?;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs()
                        .try_into()?;
                    crate::engine::instance::subscription::select_release(
                        &selected_root,
                        &bytes,
                        now,
                        &version,
                        crate::engine::publication::RecoveryReader::new(state),
                        &cancel,
                    )
                },
            )?;
            scope.accept(work.wait().await?)?.transpose()
        })
        .await?;
        ReleaseInput::Subscribed(proof.map(std::sync::Arc::new))
    } else {
        let selected = selected.context("Snapshot requires a local release descriptor")?;
        let decoded = initialize::discover(session, move |mut scope| async move {
            let mut resources = operation_resources().capture;
            resources.scratch_bytes = 0;
            let work =
                scope.spawn_blocking(resources, operation_resources().prepared, move |cancel| {
                    DecodedRelease::read(&selected, &cancel)
                })?;
            scope.accept(work.wait().await?)?.transpose()
        })
        .await?;
        ReleaseInput::Snapshot(Box::new(decoded))
    };
    let release = match &input {
        ReleaseInput::Snapshot(decoded) => SelectedRelease::Snapshot(SelectedSnapshot::select(
            decoded.bytes(),
            &sha256.context("Snapshot requires an expected digest")?,
            &version,
        )?),
        ReleaseInput::Subscribed(proof) => {
            SelectedRelease::Subscribed(std::sync::Arc::clone(proof))
        }
    };
    let request = InstallInstanceRequest {
        conflicts: conflict_resolutions(conflicts, &invocation)?,
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
        release,
        side: match side.as_str() {
            "server" => InstanceSide::Server,
            "client" => InstanceSide::Client,
            _ => anyhow::bail!("Instance side must be client or server"),
        },
        choices,
        supplied: BTreeMap::new(),
        local_files,
        assets,
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Some(prepared) = require_instance_ready(
            session,
            &engine,
            cancellable(session, engine.prepare(root, request)).await?,
        )
        .await?
        else {
            return Ok(());
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
        InstanceCommand::Continue { files } => resume_instance(session, files).await,
        InstanceCommand::DiscardPending => discard_pending(session).await,
        InstanceCommand::RecoverRuntime {
            acknowledge_stopped,
        } => {
            ensure!(
                acknowledge_stopped,
                "Confirm that the runtime and all descendants have stopped"
            );
            let (invocation, root) = project_path(session)?;
            let engine = engine(session.config().app_config(), &invocation)?;
            let result = async {
                let Preparation::Ready(prepared) = cancellable(session, engine.prepare(root, crate::engine::api::AcknowledgeStoppedRuntime)).await? else { anyhow::bail!("Unexpected runtime recovery input") };
                session.display().status().info("Clear the interrupted runtime marker after confirming all game/runtime processes have stopped");
                apply(session, &engine, prepared, "Runtime recovery", |receipt| {
                    ensure!(matches!(receipt, ExecutionReceipt::RuntimeRecovered(_)), "Unexpected runtime recovery receipt");
                    Ok("Cleared interrupted runtime evidence".into())
                }).await
            }.await;
            engine.shutdown().await;
            result
        }
        InstanceCommand::Launch { command } => launch(session, command).await,
        command @ (InstanceCommand::Subscribe { .. }
        | InstanceCommand::Trust { .. }
        | InstanceCommand::ObserveChannel { .. }) => subscription(session, command).await,
        command @ (InstanceCommand::Install { .. }
        | InstanceCommand::Prepare { .. }
        | InstanceCommand::Update { .. }) => install(session, command).await,
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
            for local in &record.local_overrides {
                session.display().status().info(&format!(
                    "Local override: {} (accepted SHA-256 {})",
                    local.destination, local.accepted.sha256
                ));
            }
            for release in &record.history {
                session
                    .display()
                    .status()
                    .info(&format!("Retained release: {release}"));
            }
            Ok(())
        }
        InstanceCommand::Options {
            conflicts,
            choices,
            assets,
            files,
        } => {
            if choices.is_empty() {
                ensure!(
                    assets.is_none()
                        && files.is_empty()
                        && conflicts.preserve.is_empty()
                        && conflicts.replace.is_empty()
                        && conflicts.merge.is_empty(),
                    "Supply --choice to change instance options"
                );
                let (invocation, root) = project_path(session)?;
                let state = state_root(session.config().app_config(), &invocation)?;
                let inspected = inspect(session, root, None, state).await?;
                let (record, release) = &*inspected;
                for definition in &release.document().choices {
                    let selected = record
                        .choices
                        .iter()
                        .find(|choice| choice.key == definition.key)
                        .context("Installed choice is missing")?;
                    session.display().status().info(&format!(
                        "{}={} (available: {})",
                        definition.key,
                        selected.value,
                        definition.alternatives.join(", ")
                    ));
                    if let Some(description) = &definition.description {
                        session.display().status().info(description);
                    }
                }
                Ok(())
            } else {
                maintain(
                    session,
                    InstanceAction::ChangeChoices,
                    None,
                    assets,
                    files,
                    choices,
                    conflicts,
                )
                .await
            }
        }
        InstanceCommand::Repair {
            assets,
            files,
            conflicts,
        } => {
            maintain(
                session,
                InstanceAction::Repair,
                None,
                assets,
                files,
                Vec::new(),
                conflicts,
            )
            .await
        }
        InstanceCommand::Rollback {
            conflicts,
            release,
            assets,
            files,
            choices,
        } => {
            maintain(
                session,
                InstanceAction::Rollback,
                Some(release),
                assets,
                files,
                choices,
                conflicts,
            )
            .await
        }
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
    action: InstanceAction,
    release: Option<String>,
    assets: Option<PathBuf>,
    files: Vec<String>,
    choices: Vec<String>,
    conflicts: crate::application::cli::InstanceConflictArgs,
) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
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
        conflicts: conflict_resolutions(conflicts, &invocation)?,
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
        let Some(prepared) = require_instance_ready(
            session,
            &engine,
            cancellable(session, engine.prepare(root, request)).await?,
        )
        .await?
        else {
            return Ok(());
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
            let path = envelope.map(|path| absolute(&invocation, &path));
            let selected_root = root.clone();
            let state = state_root(session.config().app_config(), &invocation)?;
            let bytes = initialize::discover(session, move |mut scope| async move {
                let Some(path) = path else {
                    let record = read_subscription(&mut scope, selected_root, state).await?;
                    // Revoked enrollment must fail before issuing a request.
                    record.trust()?;
                    return crate::engine::acquisition::HttpAcquisition::new()?
                        .publisher_metadata(
                            &mut scope,
                            &record.url,
                            48 << 10,
                            Duration::from_secs(30),
                        )
                        .await;
                };
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

async fn read_subscription(
    scope: &mut crate::engine::runtime::WorkScope,
    root: PathBuf,
    state: PathBuf,
) -> Result<crate::engine::runtime::RetainedOutput<crate::engine::api::SubscriptionRecord>> {
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
            crate::engine::instance::subscription::inspect(
                &root,
                crate::engine::publication::RecoveryReader::new(state),
                &cancel,
            )
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}

async fn launch(session: &dyn Session, command: Vec<std::ffi::OsString>) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let mut args = command.into_iter();
    let program = PathBuf::from(
        args.next()
            .context("Select a local runtime program after --")?,
    );
    let program = if program.is_absolute() || program.components().count() > 1 {
        absolute(&invocation, &program)
    } else {
        let found = session
            .process()
            .find_program(
                program
                    .to_str()
                    .context("Runtime program name is not UTF-8; use an absolute path")?,
            )
            .context("Runtime executable was not found on PATH")?;
        absolute(&invocation, Path::new(&found))
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) = cancellable(
            session,
            engine.prepare(
                root,
                crate::engine::api::LaunchInstanceRequest {
                    program,
                    arguments: args.collect(),
                },
            ),
        )
        .await?
        else {
            anyhow::bail!("Instance launch requires input")
        };
        let OperationPreview::Launch(view) = prepared.view() else {
            anyhow::bail!("Unexpected launch preview")
        };
        session.display().status().info(&format!(
            "Run {} against completed release {}",
            view.program.display(),
            view.record.release
        ));
        apply(session, &engine, prepared, "Instance launch", |receipt| {
            let ExecutionReceipt::Launch(receipt) = receipt else {
                anyhow::bail!("Unexpected runtime receipt")
            };
            ensure!(
                receipt.status.success(),
                "Instance runtime exited with {}",
                receipt.status
            );
            Ok(format!("Runtime completed for release {}", receipt.release))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}

fn conflict_resolutions(
    args: crate::application::cli::InstanceConflictArgs,
    invocation: &Path,
) -> Result<Vec<crate::engine::instance::ConflictResolution>> {
    use crate::engine::instance::{ConflictChoice, ConflictResolution};
    let mut decisions: Vec<_> = args
        .preserve
        .into_iter()
        .map(|destination| ConflictResolution {
            destination,
            choice: ConflictChoice::Preserve,
        })
        .chain(
            args.replace
                .into_iter()
                .map(|destination| ConflictResolution {
                    destination,
                    choice: ConflictChoice::Replace,
                }),
        )
        .collect();
    for input in args.merge {
        let (destination, file) = input
            .split_once('=')
            .context("Merge selection must be PATH=FILE")?;
        ensure!(
            !destination.is_empty() && !file.is_empty(),
            "Merge requires a destination and a local file"
        );
        decisions.push(ConflictResolution {
            destination: destination.into(),
            choice: ConflictChoice::Merge {
                file: absolute(invocation, Path::new(file)),
            },
        });
    }
    Ok(decisions)
}

pub(super) fn report_requirements(
    session: &dyn Session,
    needs: &[crate::engine::api::InstanceInputRequirement],
) {
    for need in needs {
        session.display().status().warning(&format!(
            "Missing instance file {}: {} bytes, SHA-256 {}; associate with --file {}=PATH",
            need.key, need.bytes, need.sha256, need.key
        ));
    }
}
async fn require_instance_ready(
    session: &dyn Session,
    engine: &Engine,
    preparation: Preparation,
) -> Result<Option<crate::engine::api::PreparedOperation>> {
    match preparation {
        Preparation::Ready(prepared) => Ok(Some(prepared)),
        Preparation::NeedsInput(pending) => {
            let OperationPreview::Instance(view) = pending.view() else {
                anyhow::bail!("Unexpected instance input request")
            };
            report_requirements(session, &view.manual);
            if !approve(session, "Save pending instance")? {
                return Ok(None);
            }
            save_pending(session, engine, *pending).await?;
            unreachable!()
        }
    }
}
pub(super) async fn save_pending(
    session: &dyn Session,
    engine: &Engine,
    pending: crate::engine::api::PreparationContinuation,
) -> Result<()> {
    let saved = cancellable(session, engine.suspend_instance(pending, None)).await?;
    session.display().status().info(&format!("Saved pending instance with {} verified files; resume with instance continue --file KEY=PATH",saved.retained_files));
    anyhow::bail!("Instance was not applied; exact manual inputs remain pending")
}
async fn resume_instance(session: &dyn Session, files: Vec<String>) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let mut associations = BTreeMap::new();
    for file in files {
        let (key, path) = file
            .split_once('=')
            .context("File association must be KEY=PATH")?;
        ensure!(
            !key.is_empty() && !path.is_empty(),
            "File association must name a key and path"
        );
        ensure!(
            associations
                .insert(key.to_owned(), absolute(&invocation, Path::new(path)))
                .is_none(),
            "Duplicate file association"
        );
    }
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let resumed = cancellable(
            session,
            engine.resume_saved_instance(root, associations, None),
        )
        .await?
        .context("No pending instance operation")?;
        let Some(prepared) = require_instance_ready(session, &engine, resumed.preparation).await?
        else {
            return Ok(());
        };
        let mut published = false;
        apply(
            session,
            &engine,
            prepared,
            "Instance continuation",
            |receipt| {
                let ExecutionReceipt::Instance(receipt) = receipt else {
                    anyhow::bail!("Unexpected instance receipt")
                };
                published = true;
                Ok(format!("Applied release {}", receipt.record.release))
            },
        )
        .await?;
        if published {
            // Cleanup follows publication; a cleanup failure cannot undo completed installation.
            match engine.discard_saved_instance(resumed.saved).await {
                Ok(true) => {}
                Ok(false) => session
                    .display()
                    .status()
                    .warning("Instance applied; pending state changed and was retained"),
                Err(error) => session.display().status().warning(&format!(
                    "Instance applied; pending cleanup failed: {error:#}"
                )),
            }
        }
        Ok(())
    }
    .await;
    engine.shutdown().await;
    result
}
async fn discard_pending(session: &dyn Session) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let pending = cancellable(session, engine.observe_pending_instance(root))
            .await?
            .context("No pending instance operation")?;
        if approve(session, "Discard pending instance")? {
            ensure!(
                cancellable(session, engine.discard_pending_instance(pending)).await?,
                "Pending state changed; inspect it again"
            );
            session
                .display()
                .status()
                .complete("Pending instance discarded; installed content retained");
        }
        Ok(())
    }
    .await;
    engine.shutdown().await;
    result
}
