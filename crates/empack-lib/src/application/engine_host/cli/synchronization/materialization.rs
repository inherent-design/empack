//! Explicit acquisition precedes the final native file preview and publication approval.
use super::*;
use crate::engine::{
    acquisition::{LocalFileRequest, acquire_local_file},
    build::{
        BuildAcquisitions,
        acquisition::{
            AcquisitionKey, AcquisitionNeed, AcquisitionReason, BuildAcquisitionResult,
            BuildContentSource,
        },
    },
    content::{ContentPool, InitialObservation},
    content::{cache::ContentCache, store::ContentStoreLimits},
    mrpack::LockedFileKey,
    providers::CatalogLimits,
    synchronization::suspension::{self, ResumedSync, SyncInputContext},
};

pub(super) struct SyncInputs {
    pub context: SyncInputContext,
    pub resumed: Option<ResumedSync>,
    pub files: Vec<String>,
}
impl SyncInputs {
    pub fn initial(context: SyncInputContext) -> Self {
        Self {
            context,
            resumed: None,
            files: Vec::new(),
        }
    }
}

pub(super) async fn publish(
    session: &dyn Session,
    project: ResolvedProject,
    resolution: Option<ResolvedProject>,
    materialize: bool,
    services: dependencies::AdditionServices,
    mut inputs: SyncInputs,
) -> Result<()> {
    let evidence = SourceEvidencePolicy::Compatibility;
    let source_revision = inputs.context.revision();
    let (invocation, _) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    let supplied = parse_files(&project, &inputs.files, &invocation)?;
    let mut saved = inputs
        .resumed
        .as_mut()
        .map(|resumed| std::mem::take(&mut resumed.acquired));
    let mut resumed = inputs.resumed.take();
    let request = if materialize {
        let mut pending = Vec::new();
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                let source = match &file.acquisition {
                    AcquisitionSpec::Url(urls) => BuildContentSource::Download(urls.clone()),
                    AcquisitionSpec::Provider {
                        pin,
                        slot,
                        alternatives,
                    } => BuildContentSource::Provider {
                        pin: pin.clone(),
                        slot: slot.clone(),
                        alternatives: alternatives.clone(),
                    },
                    AcquisitionSpec::Manual { pin, .. } => {
                        BuildContentSource::Manual { pin: pin.clone() }
                    }
                    AcquisitionSpec::Local(_) | AcquisitionSpec::Embedded { .. } => continue,
                };
                session.display().status().info(&format!(
                    "Acquire and verify {} / {} ({} placements)",
                    key.as_str(),
                    file.slot.as_str(),
                    file.placements.as_slice().len()
                ));
                pending.push(AcquisitionNeed {
                    key: AcquisitionKey::Locked(LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    }),
                    reason: AcquisitionReason::MaterializedTarget,
                    expected: file.expected.clone(),
                    source,
                });
            }
        }
        if session.config().app_config().dry_run {
            session.display().status().info("Materialization preview does not download remote bytes; execution verifies every reference before publication");
            SyncRequest::Recorded {
                resolution,
                evidence,
            }
        } else if pending.is_empty() {
            SyncRequest::Recorded {
                resolution,
                evidence,
            }
        } else {
            if !approve(session, "Remote content acquisition")? {
                return Ok(());
            }
            let cache = ContentCache::new(
                content_cache_root(session.config().app_config(), &invocation)?,
                ContentStoreLimits::default(),
            )?;
            let retained = saved.take().unwrap_or_default();
            let transfer = services.files.transfer;
            let acquired = scoped(
                session,
                governor(session.config().app_config()),
                move |mut scope| async move {
                    let mut acquired = BuildAcquisitions {
                        locked: retained,
                        observed: BTreeMap::new(),
                    };
                    let mut total = acquired.retained_bytes()?;
                    ensure!(
                        total <= transfer.transfer_bytes,
                        "Retained synchronization exceeds byte allowance"
                    );
                    let mut pool =
                        ContentPool::owned(&mut scope, transfer.transfer_bytes - total).await?;
                    for (key, path) in supplied {
                        let need = pending
                            .iter()
                            .find(|need| need.key == AcquisitionKey::Locked(key.clone()))
                            .context("Supplied file is outside synchronization")?;
                        let content = acquire_local_file(
                            &mut scope,
                            LocalFileRequest {
                                source: path,
                                expected: need.expected.clone(),
                                maximum: transfer.file_bytes.min(transfer.transfer_bytes - total),
                                evidence,
                                initial: InitialObservation::RequireEvidence,
                            },
                        )
                        .await?;
                        total = total
                            .checked_add(content.content.lease().len())
                            .context("Sync byte count overflow")?;
                        acquired.locked.insert(
                            key,
                            crate::engine::mrpack::AcquiredBuildFile {
                                content: pool
                                    .consolidate_owned(&mut scope, content.content)
                                    .await?,
                                permissions: content.permissions,
                            },
                        );
                    }
                    pending.retain(|need| match &need.key {
                        AcquisitionKey::Locked(key) => !acquired.locked.contains_key(key),
                        _ => true,
                    });
                    let result = BuildAcquisitionResult { acquired, pending }
                        .acquire_cached(&cache, &mut scope, evidence, services.files.transfer)
                        .await?
                        .refresh_provider_locators(
                            &services.catalog,
                            &mut scope,
                            CatalogLimits {
                                deadline: services.files.transfer.deadline,
                                ..Default::default()
                            },
                        )
                        .await?
                        .acquire_http(
                            &services.transport,
                            &mut scope,
                            evidence,
                            services.files.transfer,
                        )
                        .await?;
                    // Acquisition has explicit approval. Cache publication does not imply that
                    // the subsequent project publication succeeded or grant it any authority.
                    cache
                        .publish(
                            &mut scope,
                            result
                                .acquired
                                .locked
                                .values()
                                .map(|file| file.content.clone())
                                .collect(),
                        )
                        .await?;
                    Ok(result)
                },
            )
            .await?;
            if !acquired.pending.is_empty() {
                for need in &acquired.pending {
                    if let AcquisitionKey::Locked(key) = &need.key {
                        session.display().status().warning(&format!(
                            "Synchronization input {}/{}",
                            key.dependency.as_str(),
                            key.slot.as_str()
                        ));
                    }
                }
                let count = acquired.pending.len();
                let context = inputs.context.clone();
                let prior = resumed.take().map(|value| value.saved);
                scoped(
                    session,
                    governor(session.config().app_config()),
                    move |mut scope| async move {
                        suspension::save_pending_sync(
                            &mut scope,
                            suspension::SyncRetentionRequest {
                                state,
                                context,
                                project,
                                acquired: acquired.acquired.locked,
                                policy: evidence,
                                prior,
                                limits: transfer,
                            },
                        )
                        .await
                    },
                )
                .await?;
                anyhow::bail!(
                    "Synchronization was not published: {count} references require supplied content; continuation was saved; use sync --continue --file DEPENDENCY/SLOT=PATH"
                );
            }
            SyncRequest::AcquiredReferences {
                source_revision: Some(source_revision),
                // Bind acquisition to the inspected intent and exact selections, including
                // when ordinary recorded synchronization needed no fresh resolution.
                resolution: Some(project),
                evidence,
                content: acquired.acquired.locked,
            }
        }
    } else {
        SyncRequest::Recorded {
            resolution,
            evidence,
        }
    };
    let published = dependencies::synchronize_with_outcome(session, request).await?;
    if published && let Some(saved) = resumed.take().map(|value| value.saved) {
        scoped(
                session,
                governor(session.config().app_config()),
                move |mut scope| async move {
                    suspension::discard_pending_sync(&mut scope, saved).await
                },
            )
            .await?;
    }
    Ok(())
}
fn parse_files(
    project: &ResolvedProject,
    files: &[String],
    invocation: &Path,
) -> Result<BTreeMap<LockedFileKey, PathBuf>> {
    ensure!(
        files.len() <= 128,
        "At most 128 synchronization associations are allowed"
    );
    let mut selected = BTreeMap::new();
    for value in files {
        let (selector, path) = value
            .split_once('=')
            .context("Synchronization association must be DEPENDENCY/SLOT=PATH")?;
        ensure!(!path.is_empty(), "Missing synchronization source path");
        let mut matches = Vec::new();
        for (key, dependency) in &project.lock().dependencies {
            for file in dependency.files.as_slice() {
                if selector == format!("{}/{}", key.as_str(), file.slot.as_str()) {
                    ensure!(
                        !matches!(
                            file.acquisition,
                            AcquisitionSpec::Local(_) | AcquisitionSpec::Embedded { .. }
                        ),
                        "Cannot replace an authored source with a synchronization association"
                    );
                    matches.push(LockedFileKey {
                        dependency: key.clone(),
                        slot: file.slot.clone(),
                    });
                }
            }
        }
        ensure!(
            matches.len() == 1,
            "Synchronization association must select one exact dependency/slot"
        );
        ensure!(
            selected
                .insert(
                    matches.pop().unwrap(),
                    absolute(invocation, Path::new(path))
                )
                .is_none(),
            "Repeated synchronization association"
        );
    }
    Ok(selected)
}
