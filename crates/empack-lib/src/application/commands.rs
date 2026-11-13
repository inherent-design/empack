//! Executable dispatch to native engine operations. Publication owns its root lock.
use super::engine_host;
use crate::Result;
use crate::application::session::{CommandSession, Session};
use crate::application::{CliConfig, Commands};
use crate::engine::api::{RemovalSelector, RemoveRequest};
use empack_core::{
    model::NonEmpty,
    removal::{RemovalEvidencePolicy, RemovalMode},
};

/// Execute CLI commands through session-owned native engine hosts
pub async fn execute_command(config: CliConfig) -> Result<()> {
    execute_command_with_cancellation(config, Default::default()).await
}

pub async fn execute_command_with_cancellation(
    config: CliConfig,
    cancellation: super::process_runtime::Cancellation,
) -> Result<()> {
    // Create command session (owns all ephemeral state)
    let session = CommandSession::new(config.app_config).with_cancellation(cancellation.clone());
    cancellation.check()?;

    let command = match config.command {
        Some(cmd) => cmd,
        None => {
            session
                .display()
                .status()
                .message("empack - Minecraft modpack management");
            session
                .display()
                .status()
                .subtle("Run 'empack --help' for usage information");
            return Ok(());
        }
    };

    // Dispatch to session-aware command handlers
    execute_command_with_session(command, &session).await
}

/// Execute one command using the same native engine host as library clients.
/// Trace only a static command name and outcome, never selectors, paths, config or errors.
#[tracing::instrument(name = "empack.command", skip_all, fields(command = %command_name(&command), outcome = tracing::field::Empty))]
pub async fn execute_command_with_session(command: Commands, session: &dyn Session) -> Result<()> {
    let result = dispatch(command, session)
        .await
        .map_err(super::recovery_guidance::annotate);
    tracing::Span::current().record(
        "outcome",
        tracing::field::display(if result.is_ok() { "success" } else { "failure" }),
    );
    result
}
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Check(_) => "check",
        Commands::Status { .. } => "status",
        Commands::Manifest { .. } => "manifest",
        Commands::Pin { .. } => "pin",
        Commands::Unpin { .. } => "unpin",
        Commands::Pending { .. } => "pending",
        Commands::Instance { .. } => "instance",
        Commands::Release { .. } => "release",
        Commands::Init(_) => "init",
        Commands::Add { .. } => "add",
        Commands::Remove { .. } => "remove",
        Commands::Update(_) => "update",
        Commands::Migrate(_) => "migrate",
        Commands::Adopt { .. } => "adopt",
        Commands::Build(_) => "build",
        Commands::Clean { .. } => "clean",
        Commands::Sync { .. } => "sync",
        Commands::Recover { .. } => "recover",
        Commands::Doctor(_) => "doctor",
    }
}
async fn dispatch(command: Commands, session: &dyn Session) -> Result<()> {
    session.process().check_cancelled()?;
    // Preparation captures its own generation. The publisher acquires mutation ownership;
    // a legacy outer lock would deadlock it and make preview unnecessarily mutating.
    // Keep each workflow state on the heap. Otherwise debug builds embed the
    // largest workflow in every dispatch frame, exhausting the Windows CLI stack.
    match command {
        Commands::Check(options) => Box::pin(engine_host::checking::check(session, options)).await,
        Commands::Manifest { command } => {
            Box::pin(engine_host::cli::manifest::inspect(session, command)).await
        }
        Commands::Pin { dependencies } => {
            Box::pin(engine_host::cli::manifest::pin(session, dependencies, true)).await
        }
        Commands::Unpin { dependencies } => {
            Box::pin(engine_host::cli::manifest::pin(
                session,
                dependencies,
                false,
            ))
            .await
        }
        Commands::Status { context, json } => {
            Box::pin(engine_host::status(session, context, json)).await
        }
        Commands::Pending { command } => {
            Box::pin(engine_host::pending::dispatch(session, command)).await
        }
        Commands::Instance { command } => {
            Box::pin(engine_host::instance::dispatch(session, command)).await
        }
        Commands::Release { command } => {
            Box::pin(engine_host::release::dispatch(session, command)).await
        }
        Commands::Recover { action, operation } => {
            Box::pin(engine_host::recover(session, action, operation)).await
        }
        Commands::Doctor(options) => Box::pin(engine_host::doctor::inspect(session, options)).await,
        Commands::Init(args) => Box::pin(engine_host::cli::initialize(session, &args)).await,
        Commands::Add {
            mods,
            force,
            platform,
            project_type,
            version_id,
            file_id,
            file_plan,
            download_as_local,
            continue_independent,
        } => {
            Box::pin(engine_host::cli::add(
                session,
                engine_host::cli::AddOptions {
                    inputs: mods,
                    force,
                    platform,
                    kind: project_type,
                    version_id,
                    file_id,
                    file_plan,
                    download_as_local,
                    continue_independent,
                },
            ))
            .await
        }
        Commands::Migrate(options) => Box::pin(engine_host::cli::migrate(session, options)).await,
        Commands::Update(options) => {
            Box::pin(engine_host::cli::update_command(session, options)).await
        }
        Commands::Adopt {
            dependencies,
            from,
            selection,
        } => {
            if from.is_empty() {
                anyhow::ensure!(
                    selection.platform.is_none()
                        && selection.project_type.is_none()
                        && selection.version_id.is_none()
                        && selection.file_id.is_none()
                        && selection.file_plan.is_none(),
                    "Source choices require --from"
                );
                Box::pin(engine_host::cli::adopt(session, dependencies)).await
            } else {
                anyhow::ensure!(
                    dependencies.is_empty(),
                    "Choose tracked keys or new source inputs for adoption"
                );
                Box::pin(engine_host::cli::adopt_inputs(
                    session,
                    engine_host::cli::AddOptions {
                        inputs: from,
                        force: false,
                        platform: selection.platform,
                        kind: selection.project_type,
                        version_id: selection.version_id,
                        file_id: selection.file_id,
                        file_plan: selection.file_plan,
                        download_as_local: false,
                        continue_independent: false,
                    },
                ))
                .await
            }
        }
        Commands::Remove {
            mods,
            deps,
            forget,
            acknowledge_unknown,
        } => {
            anyhow::ensure!(
                !deps,
                "Automatic orphan cleanup requires complete dependency evidence; select exact dependencies to remove"
            );
            Box::pin(engine_host::remove(
                session,
                RemoveRequest {
                    selections: NonEmpty::new(
                        mods.into_iter().map(RemovalSelector::Query).collect(),
                    )?,
                    mode: if forget {
                        RemovalMode::ForgetRoots
                    } else {
                        RemovalMode::RemoveContent
                    },
                    evidence: if acknowledge_unknown {
                        RemovalEvidencePolicy::AcknowledgeUnknown
                    } else {
                        RemovalEvidencePolicy::RequireComplete
                    },
                },
            ))
            .await
        }
        Commands::Build(args) => Box::pin(engine_host::cli::build(session, &args)).await,
        Commands::Clean { targets } => Box::pin(engine_host::clean(session, &targets)).await,
        Commands::Sync {
            materialize,
            continue_sync,
            files,
        } => {
            if continue_sync {
                Box::pin(engine_host::cli::resume_synchronization(session, files)).await
            } else {
                Box::pin(engine_host::cli::synchronize(session, materialize)).await
            }
        }
    }
}
#[cfg(test)]
#[path = "commands.test.rs"]
mod tests;
