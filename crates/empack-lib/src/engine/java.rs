//! Bounded native Java observation, separate from runtime approval and pack policy.
use crate::{
    application::process_runtime::{self, Cancellation},
    engine::{
        resources::ResourceRequest,
        runtime::{WorkProgress, WorkScope},
    },
};
use anyhow::{Context, Result, ensure};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

/// Caller chooses the local executable. Runtime launch must retain its approval and run lease.
pub(crate) async fn inspect(runtime: &Command, cancel: Cancellation) -> Result<Result<u16>> {
    let mut command = Command::new(runtime.get_program());
    command
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null());
    if let Some(directory) = runtime.get_current_dir() {
        command.current_dir(directory);
    }
    let output =
        process_runtime::execute_captured(command, Duration::from_secs(15), cancel, None, 64 << 10)
            .await?;
    Ok(output.0.and_then(|output| {
        // JVM properties and injected startup options can contain unrelated private data.
        ensure!(
            output.success,
            "Selected Java failed its version check; run that executable's version command separately for its output"
        );
        java_major(&output.stdout, &output.stderr)
    }))
}
pub(crate) async fn inspect_scoped(
    scope: &mut WorkScope,
    program: PathBuf,
    directory: PathBuf,
) -> Result<u16> {
    scope.report_progress(WorkProgress::CheckingJava);
    let worker = scope.spawn(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            open_files: 8,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: 64 << 10,
            ..Default::default()
        },
        move |cancel| async move {
            let mut command = Command::new(program);
            command.current_dir(directory);
            inspect(&command, cancel).await.and_then(|result| result)
        },
    )?;
    // A cancelled observer cannot discard cleanup or unconfirmed process-retirement evidence.
    let (result, _retained) = scope.accept_retirement(worker.wait().await?)?.into_parts();
    result
}
fn java_major(stdout: &str, stderr: &str) -> Result<u16> {
    let mut values = stdout.lines().chain(stderr.lines()).filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "java.specification.version").then_some(value.trim())
    });
    let value = values
        .next()
        .context("Selected program did not report a Java specification version")?;
    ensure!(
        values.next().is_none(),
        "Java reported ambiguous specification versions"
    );
    let version = value.strip_prefix("1.").unwrap_or(value);
    ensure!(
        !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()),
        "Invalid Java specification version"
    );
    let version = version
        .parse::<u16>()
        .context("Invalid Java specification version")?;
    ensure!(version >= 8, "Selected Java is older than supported Java 8");
    Ok(version)
}

#[cfg(test)]
mod tests;
