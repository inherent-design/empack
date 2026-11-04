use empack_lib::application::process_runtime::Cancellation;
use empack_lib::application::{CliConfig, CliLoad, EmpackExitCode, classify_error};
use empack_lib::primitives::ConfigError;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut config = match CliConfig::load_for_process() {
        Ok(CliLoad::Ready(config)) => *config,
        Ok(CliLoad::Display(message)) => {
            print!("{message}");
            return EmpackExitCode::Success.as_process_exit_code();
        }
        Err(ConfigError::ParseError { reason, .. }) => {
            eprint!("{reason}");
            return EmpackExitCode::Usage.as_process_exit_code();
        }
        Err(error) => {
            eprintln!("Error: {error}");
            return EmpackExitCode::Usage.as_process_exit_code();
        }
    };
    if let Err(error) = config.app_config.validate() {
        eprintln!("Error: {error}");
        return EmpackExitCode::Usage.as_process_exit_code();
    }
    let caps = empack_lib::TerminalCapabilities::detect_from_config(config.app_config.color)
        .unwrap_or_else(|_| empack_lib::TerminalCapabilities::minimal());
    let mut logger_config = config.app_config.to_logger_config(&caps);
    if config
        .command
        .as_ref()
        .is_some_and(|command| command.emits_json())
    {
        logger_config.output = empack_lib::primitives::LogOutput::Stderr;
    }
    if let Err(error) = empack_lib::Logger::init(logger_config) {
        eprintln!("empack: logger init failed: {error}");
    }
    empack_lib::terminal::cursor::force_show_cursor();
    empack_lib::terminal::cursor::install_panic_hook();
    let cancellation = Cancellation::default();
    let signal_token = cancellation.clone();
    // Register before a command can enter a synchronous prompt. Registering
    // inside a spawned future would leave its first poll racing that prompt.
    #[cfg(unix)]
    let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());
    #[cfg(windows)]
    let interrupt = tokio::signal::windows::ctrl_c();
    let listener = interrupt.ok().map(|mut interrupt| {
        tokio::spawn(async move {
            if interrupt.recv().await.is_some() {
                signal_token.cancel();
            }
        })
    });
    let result = empack_lib::application::commands::execute_command_with_cancellation(
        config,
        cancellation.clone(),
    )
    .await;
    if let Some(listener) = listener {
        listener.abort();
    }
    empack_lib::terminal::cursor::force_show_cursor();
    empack_lib::logger::global_shutdown();
    finish_command(result, &mut std::io::stderr().lock()).as_process_exit_code()
}

// Cancellation is an execution input. Only the completed result determines the
// diagnostic and exit status, including when an interrupt arrived after success.
fn finish_command(
    result: empack_lib::Result<()>,
    stderr: &mut impl std::io::Write,
) -> EmpackExitCode {
    match result {
        Ok(()) => EmpackExitCode::Success,
        Err(error) => {
            let code = classify_error(&error);
            let _ = writeln!(stderr, "Error: {error:#}");
            code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_after_cancel(result: empack_lib::Result<()>) -> (EmpackExitCode, String) {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
        let mut stderr = Vec::new();
        let code = finish_command(result, &mut stderr);
        (code, String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn cancellation_does_not_suppress_terminal_failure_details() {
        for message in [
            "Runtime retired, but terminal restoration failed",
            "Runtime retired, but ownership cleanup requires recovery",
            "Runtime retirement is unconfirmed; inspect recovery before retrying",
            "Build completed only part of its displayed effects",
        ] {
            let error = std::io::Error::other(message).into();
            let (code, stderr) = finish_after_cancel(Err(error));
            assert_eq!(code, EmpackExitCode::General);
            assert_eq!(stderr, format!("Error: {message}\n"));
        }
    }

    #[test]
    fn clean_interruption_reports_no_recovery_warning() {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let (code, stderr) = finish_after_cancel(cancellation.check());
        assert_eq!(code, EmpackExitCode::Interrupted);
        assert_eq!(stderr, "Error: Operation interrupted\n");
    }

    #[test]
    fn completed_success_survives_late_cancellation() {
        let (code, stderr) = finish_after_cancel(Ok(()));
        assert_eq!(code, EmpackExitCode::Success);
        assert!(stderr.is_empty());
    }

    #[test]
    fn interrupted_status_still_renders_attached_failure_context() {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let error = cancellation.check().unwrap_err().context(
            "Runtime retired, but recovery evidence could not be cleared: permission denied",
        );
        let mut stderr = Vec::new();
        assert_eq!(
            finish_command(Err(error), &mut stderr),
            EmpackExitCode::Interrupted
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "Error: Runtime retired, but recovery evidence could not be cleared: permission denied: Operation interrupted\n"
        );
    }
}
