//! Explicit desktop handoff. Provider URLs never come from installer text or download locators.
use super::*;
use crate::engine::{api::BuildPreview, providers::DownloadPage};
use std::{collections::BTreeSet, process::Stdio};

pub(super) async fn open_missing(
    session: &dyn Session,
    view: &BuildPreview,
    opened: &mut BTreeSet<empack_core::model::ResolvedPin>,
    remaining: Option<Duration>,
) -> Result<()> {
    let unresolved: BTreeSet<_> = view.unresolved.iter().collect();
    let pins: BTreeSet<_> = view
        .content
        .iter()
        .filter(|need| unresolved.contains(&need.key))
        .filter_map(|need| need.provider.clone())
        .filter(|pin| !opened.contains(pin))
        .collect();
    if pins.is_empty() {
        session
            .display()
            .status()
            .warning("No exact provider page is available; supply the displayed files explicitly");
        return Ok(());
    }
    ensure!(
        pins.len() + opened.len() <= 16,
        "Browser assistance is limited to 16 exact selections per invocation; associate files explicitly for larger batches"
    );
    let config = session.config().app_config();
    let catalog = ProviderCatalog::new(
        config.curseforge_api_client_key.clone(),
        Arc::new(HostBudgetRegistry::new()),
    )?;
    let timeout = Duration::from_secs(config.net_timeout);
    let deadline =
        tokio::time::Instant::now() + remaining.map_or(timeout, |left| left.min(timeout));
    for pin in pins {
        session.process().check_cancelled()?;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Browser page discovery deadline expired; continuation was retained"
        );
        let catalog = catalog.clone();
        let selected = pin.clone();
        let page = initialize::discover(session, move |mut scope| async move {
            catalog
                .download_page(
                    &mut scope,
                    pin,
                    CatalogLimits {
                        deadline: remaining,
                        ..Default::default()
                    },
                )
                .await
        })
        .await;
        let page = match page {
            Ok(page) => page,
            Err(error) => {
                session.process().check_cancelled()?;
                session.display().status().warning(&format!("Could not resolve a verified provider page: {error:#}; continuation was retained"));
                continue;
            }
        };
        session
            .display()
            .status()
            .info(&format!("Opening {}", page.as_str()));
        launch(
            session,
            &page,
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .min(Duration::from_secs(10)),
        )
        .await?;
        opened.insert(selected);
    }
    Ok(())
}
async fn launch(session: &dyn Session, page: &DownloadPage, timeout: Duration) -> Result<()> {
    ensure!(
        !timeout.is_zero(),
        "Browser page discovery deadline expired; continuation was retained"
    );
    let (program, prefix) = crate::platform::browser_open_command();
    let mut command = std::process::Command::new(program);
    command.args(prefix).arg(page.as_str());
    command.current_dir(session.filesystem().current_dir()?);
    initialize::discover(session, move |mut scope| async move {
        let work = scope.spawn(
            ResourceRequest {
                jobs: 1,
                open_files: 4,
                ..Default::default()
            },
            ResourceRequest::default(),
            move |cancel| launch_command(command, timeout, cancel),
        )?;
        scope.accept(work.wait().await?)?.transpose().map(|_| ())
    })
    .await
}
/// The desktop browser is an explicitly requested external application, not an owned
/// installer descendant. Supervise only the launcher, with no inherited capture pipes.
async fn launch_command(
    command: std::process::Command,
    timeout: Duration,
    cancel: crate::application::process_runtime::Cancellation,
) -> Result<()> {
    cancel.check()?;
    let mut command = tokio::process::Command::from(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .context("Could not start browser launcher; continuation was retained")?;
    let result = tokio::select! {
        status = child.wait() => {
            ensure!(status?.success(), "Browser launcher failed; continuation was retained");
            Ok(())
        }
        _ = cancel.cancelled() => Err(crate::application::process_runtime::Interrupted.into()),
        _ = tokio::time::sleep(timeout) => Err(anyhow::anyhow!("Browser launcher did not confirm completion; continuation was retained")),
    };
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn desktop_launcher_bounds_wait_and_preserves_literal_arguments() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("args");
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s' \"$1\" > \"$2\"",
                "launcher",
                "https://modrinth.com/mod/sodium/version/abcdefgh",
            ])
            .arg(&output);
        launch_command(command, Duration::from_secs(3), Default::default())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            "https://modrinth.com/mod/sodium/version/abcdefgh"
        );
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "exec sleep 60"]);
        let start = tokio::time::Instant::now();
        assert!(
            launch_command(command, Duration::from_millis(20), Default::default())
                .await
                .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
