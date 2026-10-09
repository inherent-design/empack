//! Bounded host assistance consumes saved recipes; it never grants build publication.
use super::*;
use crate::engine::api::SavedBuildResume;
use tokio::time::Instant;

pub(super) fn validate(args: &BuildArgs) -> Result<()> {
    if let Some(seconds) = args.wait_downloads {
        ensure!(
            (1..=3600).contains(&seconds),
            "Download wait must be between 1 and 3600 seconds"
        );
        ensure!(
            args.downloads_dir.is_some(),
            "Download waiting requires an explicit downloads directory"
        );
    }
    Ok(())
}

/// The second flag says this helper already removed the exact recipe used for publication.
pub(super) async fn finish(
    session: &dyn Session,
    engine: &Engine,
    prepared: Preparation,
    downloads: Option<PathBuf>,
    wait_seconds: Option<u64>,
    open_downloads: bool,
) -> Result<(bool, bool)> {
    let result = finish_once(session, engine, prepared, downloads.clone()).await;
    let Err(error) = result else {
        return result.map(|published| (published, false));
    };
    if error.downcast_ref::<PendingBuildSaved>().is_none()
        || (wait_seconds.is_none() && !open_downloads)
    {
        return Err(error);
    }
    let mut selected = error
        .downcast::<PendingBuildSaved>()
        .expect("checked pending result")
        .saved;
    let (_, project) = project_path(session)?;
    let mut opened = std::collections::BTreeSet::new();
    if open_downloads {
        open_saved(session, engine, &project, &selected, &mut opened, None).await?;
    }
    let Some(wait_seconds) = wait_seconds else {
        return Err(PendingBuildSaved { saved: selected }.into());
    };
    // A saved-input result is only produced after explicit suspension approval. A dry run
    // or declined prompt returns before reaching this wait, including execution-time input.
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(wait_seconds))
        .context("Download wait deadline overflow")?;
    session
        .display()
        .status()
        .info("Waiting for verified downloads; interruption retains the saved build");
    let mut scan_bytes = DiscoveryLimits::default().total_bytes;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!(
                "Download wait expired; build remains unpublished and continuation was retained"
            );
        }
        cancellable(session, async {
            tokio::time::sleep(remaining.min(Duration::from_secs(1))).await;
            Ok(())
        })
        .await?;
        let resumed = match cancellable(session, engine.resume_saved_build(project.clone())).await?
        {
            SavedBuildResume::Prepared(resumed) => *resumed,
            SavedBuildResume::Missing => anyhow::bail!("Saved build was removed while waiting"),
            SavedBuildResume::Stale => {
                anyhow::bail!("Saved build inputs changed while waiting; continuation was retained")
            }
        };
        ensure!(
            resumed.saved == selected,
            "Saved build was replaced while waiting; inspect the new recipe before continuing"
        );
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!(
                "Download wait expired; build remains unpublished and continuation was retained"
            );
        }
        let before = match &resumed.preparation {
            Preparation::Ready(value) => value.view(),
            Preparation::NeedsInput(value) => value.view(),
        }
        .build()
        .context("Missing build preview")?
        .content
        .len();
        let (prepared, bytes_read) = scan(
            session,
            engine,
            resumed.preparation,
            downloads.clone(),
            DiscoveryLimits {
                deadline: remaining,
                total_bytes: scan_bytes,
                ..Default::default()
            },
        )
        .await?;
        scan_bytes = scan_bytes
            .checked_sub(bytes_read)
            .context("Download discovery exceeded its cumulative byte allowance")?;
        if Instant::now() >= deadline {
            anyhow::bail!(
                "Download wait expired; build remains unpublished and continuation was retained"
            );
        }
        if let Preparation::NeedsInput(pending) = prepared {
            if pending
                .view()
                .build()
                .context("Missing build preview")?
                .content
                .len()
                < before
            {
                // Save new verified bytes once; unchanged ticks remain read-only.
                selected = cancellable(session, engine.extend_saved_build(*pending, selected))
                    .await?
                    .saved;
            }
            continue;
        }
        match finish_once(session, engine, prepared, None).await {
            Ok(published) => {
                if published {
                    cancellable(session, engine.discard_saved_build(resumed.saved))
                        .await
                        .context("Build completed, but saved-state cleanup failed")?;
                }
                return Ok((published, published));
            }
            Err(error) if error.downcast_ref::<PendingBuildSaved>().is_some() => {
                // Exact provider refresh can reveal another restricted obligation. The new
                // approved suspension replaces the recipe; the original deadline still binds.
                selected = error
                    .downcast::<PendingBuildSaved>()
                    .expect("checked pending result")
                    .saved;
                if open_downloads {
                    open_saved(
                        session,
                        engine,
                        &project,
                        &selected,
                        &mut opened,
                        Some(deadline.saturating_duration_since(Instant::now())),
                    )
                    .await?;
                }
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) async fn scan(
    session: &dyn Session,
    engine: &Engine,
    prepared: Preparation,
    downloads: Option<PathBuf>,
    limits: DiscoveryLimits,
) -> Result<(Preparation, u64)> {
    let mut bytes_read = 0;
    let prepared = match (prepared, downloads) {
        (Preparation::NeedsInput(pending), Some(downloads)) => {
            let view = pending.view().build().context("Missing build preview")?;
            let evidence = view.request().evidence;
            let requirements = view
                .content
                .iter()
                .filter(|need| view.unresolved.contains(&need.key))
                .map(|need| (need.key.clone(), need.expected.clone()))
                .collect();
            let found = initialize::discover(session, move |mut scope| async move {
                discover_downloads(&mut scope, vec![downloads], requirements, evidence, limits)
                    .await
            })
            .await?;
            let files = found.unique_files();
            bytes_read = found.bytes_read;
            if !files.is_empty() {
                session.display().status().info(&format!(
                "Download scan: {} verified associations; {} candidates inspected; {} entries skipped",
                files.len(), found.inspected_files, found.skipped_files
            ));
            }
            if found
                .matches
                .values()
                .any(|candidates| candidates.len() > 1)
            {
                session.display().status().warning("Different candidate bytes match an obligation; explicit association is required");
            }
            drop(found);
            if files.is_empty() {
                Preparation::NeedsInput(pending)
            } else {
                cancellable(session, engine.resume_with_local_files(*pending, files)).await?
            }
        }
        (prepared, _) => prepared,
    };
    Ok((prepared, bytes_read))
}

async fn open_saved(
    session: &dyn Session,
    engine: &Engine,
    project: &Path,
    selected: &crate::engine::api::SavedBuildRecord,
    opened: &mut std::collections::BTreeSet<empack_core::model::ResolvedPin>,
    remaining: Option<Duration>,
) -> Result<()> {
    let resumed = match cancellable(session, engine.resume_saved_build(project.to_owned())).await? {
        SavedBuildResume::Prepared(resumed) => *resumed,
        _ => anyhow::bail!("Saved build changed before browser assistance"),
    };
    ensure!(
        &resumed.saved == selected,
        "Saved build was replaced before browser assistance"
    );
    let view = match &resumed.preparation {
        Preparation::Ready(value) => value.view(),
        Preparation::NeedsInput(value) => value.view(),
    };
    browser::open_missing(
        session,
        view.build().context("Missing build preview")?,
        opened,
        remaining,
    )
    .await
}
