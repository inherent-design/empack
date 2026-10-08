//! Explicit acquisition precedes the final native file preview and publication approval.
use super::*;
use crate::engine::{
    build::{
        BuildAcquisitions,
        acquisition::{
            AcquisitionKey, AcquisitionNeed, AcquisitionReason, BuildAcquisitionResult,
            BuildContentSource,
        },
    },
    mrpack::LockedFileKey,
    providers::CatalogLimits,
};

pub(super) async fn publish(
    session: &dyn Session,
    project: ResolvedProject,
    resolution: Option<ResolvedProject>,
    materialize: bool,
    services: dependencies::AdditionServices,
) -> Result<()> {
    let evidence = SourceEvidencePolicy::Compatibility;
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
            let acquired = scoped(
                session,
                governor(session.config().app_config()),
                move |mut scope| async move {
                    let result = BuildAcquisitionResult {
                        acquired: BuildAcquisitions::default(),
                        pending,
                    }
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
                    ensure!(
                        result.pending.is_empty(),
                        "Synchronization was not published: {} references require supplied content",
                        result.pending.len()
                    );
                    Ok(result.acquired.locked)
                },
            )
            .await?;
            SyncRequest::AcquiredReferences {
                // Bind acquisition to the inspected intent and exact selections, including
                // when ordinary recorded synchronization needed no fresh resolution.
                resolution: Some(project),
                evidence,
                content: acquired,
            }
        }
    } else {
        SyncRequest::Recorded {
            resolution,
            evidence,
        }
    };
    dependencies::synchronize(session, request).await
}
