//! Executable dispatch to the v0.5 engine. Publication owns its project lock.
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
    let result = dispatch(command, session).await;
    tracing::Span::current().record(
        "outcome",
        tracing::field::display(if result.is_ok() { "success" } else { "failure" }),
    );
    result
}
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Init(_) => "init",
        Commands::Add { .. } => "add",
        Commands::Remove { .. } => "remove",
        Commands::Update { .. } => "update",
        Commands::Adopt { .. } => "adopt",
        Commands::Build(_) => "build",
        Commands::Clean { .. } => "clean",
        Commands::Sync { .. } => "sync",
        Commands::Recover { .. } => "recover",
        Commands::Requirements => "requirements",
        Commands::Version => "version",
    }
}
async fn dispatch(command: Commands, session: &dyn Session) -> Result<()> {
    session.process().check_cancelled()?;
    // Preparation captures its own generation. The publisher acquires mutation ownership;
    // a legacy outer lock would deadlock it and make preview unnecessarily mutating.
    match command {
        Commands::Recover { action, operation } => {
            engine_host::recover(session, action, operation).await
        }
        Commands::Requirements => handle_requirements(session).await,
        Commands::Version => handle_version(session).await,
        Commands::Init(args) => engine_host::cli::initialize(session, &args).await,
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
            engine_host::cli::add(
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
            )
            .await
        }
        Commands::Update {
            dependencies,
            continue_independent,
        } => {
            engine_host::cli::update_with_policy(
                session,
                dependencies,
                if continue_independent {
                    crate::engine::api::BatchPolicy::ContinueIndependent
                } else {
                    crate::engine::api::BatchPolicy::AllRequested
                },
            )
            .await
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
                engine_host::cli::adopt(session, dependencies).await
            } else {
                anyhow::ensure!(
                    dependencies.is_empty(),
                    "Choose tracked keys or new source inputs for adoption"
                );
                engine_host::cli::adopt_inputs(
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
                )
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
            engine_host::remove(
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
            )
            .await
        }
        Commands::Build(args) => engine_host::cli::build(session, &args).await,
        Commands::Clean { targets } => engine_host::clean(session, &targets).await,
        Commands::Sync {
            materialize,
            continue_sync,
            files,
        } => {
            if continue_sync {
                engine_host::cli::resume_synchronization(session, files).await
            } else {
                engine_host::cli::synchronize(session, materialize).await
            }
        }
    }
}
async fn handle_requirements(session: &dyn Session) -> Result<()> {
    session.display().status().section("Runtime capabilities");
    session.display().status().success(
        "project operations",
        "native engine; no packwiz executable required",
    );
    session.display().status().success(
        "archive support",
        "native ZIP, TAR.GZ and 7z output; bounded ZIP import",
    );
    match session.process().find_program("java") {
        Some(path) => session.display().status().success("java", &path),
        None => session.display().status().warning(
            "Java was not found; Forge/NeoForge server builds and generated launchers need it",
        ),
    }
    session.display().status().info(
        "Java discovery checks the executable path, not version compatibility with the selected runtime",
    );
    session
        .display()
        .status()
        .info("Provider operations require network access; CurseForge also requires an API key");
    Ok(())
}
async fn handle_version(session: &dyn Session) -> Result<()> {
    session
        .display()
        .status()
        .emphasis(&format!("empack {}", env!("CARGO_PKG_VERSION")));
    session
        .display()
        .status()
        .message("A Minecraft modpack development and distribution tool");
    session.display().status().message("");

    let build_info = [
        (
            "Built from commit",
            option_env!("GIT_HASH").unwrap_or("unknown"),
        ),
        ("Build date", option_env!("BUILD_DATE").unwrap_or("unknown")),
        ("Target", std::env::consts::ARCH),
    ];

    session.display().table().properties(&build_info);

    Ok(())
}

#[cfg(test)]
#[path = "commands.test.rs"]
mod tests;
