//! Shared selectors delegate to the existing workflow hosts and exact cleanup handles.
use super::*;
use crate::{
    application::{BuildArgs, InitArgs, cli::PendingCommand},
    engine::pending::{PendingKind, PendingOperation, PendingReport},
};
use std::collections::{BTreeMap, BTreeSet};

async fn report(
    session: &dyn Session,
) -> Result<crate::engine::runtime::RetainedOutput<PendingReport>> {
    let (invocation, root) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                memory_bytes: 256 << 20,
                open_files: 16,
                ..Default::default()
            },
            ResourceRequest {
                memory_bytes: 128 << 20,
                ..Default::default()
            },
            move |cancel| crate::engine::pending::inspect(&state, &root, &cancel),
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await
}
fn show(session: &dyn Session, view: &PendingReport, json: bool) -> Result<()> {
    session.display().status().message(&if json {
        serde_json::to_string_pretty(view)?
    } else {
        view.text()
    });
    Ok(())
}
pub(in crate::application) async fn dispatch(
    session: &dyn Session,
    command: PendingCommand,
) -> Result<()> {
    let view = report(session).await?;
    match command {
        PendingCommand::List { json } => show(session, &view, json),
        PendingCommand::Show { id, json } => {
            let selected = view.select(&id)?.clone();
            show(
                session,
                &PendingReport {
                    schema: view.schema,
                    root: view.root.clone(),
                    operations: vec![selected],
                },
                json,
            )
        }
        PendingCommand::Discard { id } => {
            let selected = view.select(&id)?.clone();
            drop(view);
            discard(session, selected).await
        }
        PendingCommand::Resume {
            id,
            files,
            downloads_dir,
            open_downloads,
            import_options,
        } => {
            let selected = view.select(&id)?.clone();
            drop(view);
            resume(
                session,
                selected,
                files,
                downloads_dir,
                open_downloads,
                import_options,
            )
            .await
        }
    }
}
fn associations(op: &PendingOperation, files: Vec<String>) -> Result<BTreeMap<String, String>> {
    ensure!(
        files.len() <= 128,
        "At most 128 pending file associations are allowed"
    );
    let mut result = BTreeMap::new();
    for input in files {
        let (key, path) = input
            .split_once('=')
            .context("Pending file association must be KEY=PATH")?;
        ensure!(
            !key.is_empty() && !path.is_empty(),
            "Pending association requires an exact key and path"
        );
        if op.kind == PendingKind::Import {
            ensure!(
                ["declared:", "override:", "provider:"]
                    .iter()
                    .any(|prefix| key.starts_with(prefix)),
                "Import pending inputs require exact declared:/override:/provider: selectors, not display filenames"
            );
        } else {
            ensure!(
                op.inputs.iter().filter(|input| input.key == key).count() == 1,
                "Pending key must identify exactly one displayed input; filenames are not authoritative keys"
            );
        }
        ensure!(
            result.insert(key.into(), path.into()).is_none(),
            "Pending key was supplied more than once"
        );
    }
    Ok(result)
}
async fn resume(
    session: &dyn Session,
    selected: PendingOperation,
    files: Vec<String>,
    downloads: Option<PathBuf>,
    open: bool,
    import_options: crate::application::cli::PendingImportOptions,
) -> Result<()> {
    ensure!(
        selected.state != "stale",
        "Selected pending operation is stale; retain it for inspection or explicitly discard it"
    );
    ensure!(
        selected.kind == PendingKind::Import
            || (import_options.import_optional_default.is_none()
                && !import_options.import_local_files),
        "Import options require a pending import"
    );
    let mut files = associations(&selected, files)?;
    if selected.kind == PendingKind::Build {
        let dry = session.config().app_config().dry_run;
        let downloads = if dry { None } else { downloads };
        return build::continue_build_selected(
            session,
            &BuildArgs {
                continue_build: true,
                associate_downloads: files
                    .into_iter()
                    .map(|(key, path)| format!("{key}={path}"))
                    .collect(),
                downloads_dir: downloads
                    .map(|path| {
                        path.to_str()
                            .map(str::to_owned)
                            .context("Downloads directory must be UTF-8")
                    })
                    .transpose()?,
                open_downloads: open && !dry,
                ..Default::default()
            },
            Some(&selected.id),
        )
        .await;
    }
    if !session.config().app_config().dry_run {
        if let Some(downloads) = downloads {
            let (invocation, _) = project_path(session)?;
            let downloads = absolute(&invocation, &downloads);
            let requirements = selected
                .inputs
                .iter()
                .filter(|input| {
                    !files.contains_key(&input.key) && !input.verification.starts_with("retained")
                })
                .filter_map(|input| {
                    input
                        .expected
                        .clone()
                        .map(|expected| (input.key.clone(), expected))
                })
                .collect::<BTreeMap<_, _>>();
            ensure!(
                !requirements.is_empty(),
                "No missing exact assertions are available for scanning; supply exact --file associations or use the original workflow to refresh metadata"
            );
            let candidates = initialize::discover(session, move |mut scope| async move {
                crate::engine::acquisition::discovery::discover_downloads(
                    &mut scope,
                    vec![downloads],
                    requirements,
                    crate::engine::content::SourceEvidencePolicy::Compatibility,
                    crate::engine::acquisition::discovery::DiscoveryLimits {
                        deadline: Duration::from_secs(30),
                        ..Default::default()
                    },
                )
                .await
            })
            .await?;
            ensure!(
                candidates
                    .matches
                    .values()
                    .all(|matches| matches.len() <= 1),
                "Download candidates are ambiguous; select explicit KEY=PATH associations"
            );
            for (key, path) in candidates.unique_files() {
                files.insert(
                    key,
                    path.to_str()
                        .context(
                            "Use --file for a native path that cannot be represented as UTF-8",
                        )?
                        .into(),
                );
            }
            session.display().status().info(&format!(
                "Found {} digest-verified candidates; continuation will verify them again",
                candidates.unique_files().len()
            ));
        }
        if open {
            let pins = selected
                .inputs
                .iter()
                .filter(|input| {
                    !files.contains_key(&input.key) && !input.verification.starts_with("retained")
                })
                .filter_map(|input| input.pin.clone())
                .collect();
            build::browser::open_pins(session, pins, &mut BTreeSet::new(), None).await?;
        }
    }
    let files = files
        .into_iter()
        .map(|(key, path)| format!("{key}={path}"))
        .collect();
    match selected.kind {
        PendingKind::Build => {
            build::continue_build_selected(
                session,
                &BuildArgs {
                    continue_build: true,
                    associate_downloads: files,
                    ..Default::default()
                },
                Some(&selected.id),
            )
            .await
        }
        PendingKind::Import => {
            cli::initialize_selected(
                session,
                &InitArgs {
                    continue_import: true,
                    import_files: files,
                    import_optional_default: import_options.import_optional_default,
                    import_local_files: import_options.import_local_files,
                    ..Default::default()
                },
                Some(selected.id),
            )
            .await
        }
        PendingKind::Sync => {
            cli::resume_synchronization_selected(session, files, selected.id).await
        }
        PendingKind::Instance => {
            instance::resume_instance_selected(session, files, Some(&selected.id)).await
        }
    }
}
async fn discard(session: &dyn Session, selected: PendingOperation) -> Result<()> {
    let (invocation, root) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    session.display().status().info(&format!("Discard {:?} recipe {}; retained content, downloads, completed installations and recovery journals remain",selected.kind,selected.id));
    match selected.kind {
        PendingKind::Build | PendingKind::Instance => {
            let engine = engine(session.config().app_config(), &invocation)?;
            let result = async {
                if selected.kind == PendingKind::Build {
                    let saved = cancellable(session, engine.observe_saved_build(root))
                        .await?
                        .context("Pending build disappeared")?;
                    ensure!(
                        saved.identity() == selected.id,
                        "Pending build changed; select its new ID"
                    );
                    if approve(session, "Discard pending build")? {
                        ensure!(
                            cancellable(session, engine.discard_saved_build(saved)).await?,
                            "Pending build changed before cleanup"
                        );
                    }
                } else {
                    let saved = cancellable(session, engine.observe_pending_instance(root))
                        .await?
                        .context("Pending instance disappeared")?;
                    ensure!(
                        saved.identity() == selected.id,
                        "Pending instance changed; select its new ID"
                    );
                    if approve(session, "Discard pending instance")? {
                        ensure!(
                            cancellable(session, engine.discard_pending_instance(saved)).await?,
                            "Pending instance changed before cleanup"
                        );
                    }
                }
                Ok(())
            }
            .await;
            engine.shutdown().await;
            result
        }
        PendingKind::Import => {
            let saved = initialize::discover(session, move |mut scope| async move {
                crate::engine::import::observe_pending_import(&mut scope, state, root)
                    .await?
                    .context("Pending import disappeared")
            })
            .await?;
            ensure!(
                saved.identity() == selected.id,
                "Pending import changed; select its new ID"
            );
            if approve(session, "Discard pending import")? {
                let removed = initialize::discover(session, move |mut scope| async move {
                    crate::engine::import::discard_observed_import(&mut scope, saved).await
                })
                .await?;
                ensure!(removed, "Pending import changed before cleanup");
            }
            Ok(())
        }
        PendingKind::Sync => {
            let saved = initialize::discover(session, move |mut scope| async move {
                crate::engine::synchronization::suspension::observe_pending_sync(
                    &mut scope, state, root,
                )
                .await?
                .context("Pending sync disappeared")
            })
            .await?;
            ensure!(
                saved.identity() == selected.id,
                "Pending sync changed; select its new ID"
            );
            if approve(session, "Discard pending sync")? {
                let removed = initialize::discover(session, move |mut scope| async move {
                    crate::engine::synchronization::suspension::discard_observed_sync(
                        &mut scope, saved,
                    )
                    .await
                })
                .await?;
                ensure!(removed, "Pending sync changed before cleanup");
            }
            Ok(())
        }
    }
}
