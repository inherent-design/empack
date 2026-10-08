//! Executable dispatch to the v0.5 engine. Publication owns its project lock.
use super::engine_host;
use crate::Result;
use crate::application::session::{CommandSession, Session};
use crate::application::{CliConfig, Commands};
use crate::engine::{
    api::{RemovalSelector, RemoveRequest, SyncRequest},
    content::SourceEvidencePolicy,
};
use empack_core::{
    model::NonEmpty,
    removal::{RemovalEvidencePolicy, RemovalMode},
};

/// Execute CLI commands using the new session-based architecture
pub async fn execute_command(config: CliConfig) -> Result<()> {
    execute_command_with_cancellation(config, Default::default()).await
}

pub async fn execute_command_with_cancellation(
    config: CliConfig,
    cancellation: super::process_runtime::Cancellation,
) -> Result<()> {
    // Create command session (owns all ephemeral state)
    let session = CommandSession::new_async(config.app_config)
        .await
        .with_cancellation(cancellation.clone());
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
pub async fn execute_command_with_session(command: Commands, session: &dyn Session) -> Result<()> {
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
                },
            )
            .await
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
        Commands::Sync {} => {
            engine_host::synchronize(
                session,
                SyncRequest::Recorded {
                    resolution: None,
                    evidence: SourceEvidencePolicy::Compatibility,
                },
            )
            .await
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
        None => session
            .display()
            .status()
            .warning("Java is unavailable; installer and executable server preparation require it"),
    }
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
