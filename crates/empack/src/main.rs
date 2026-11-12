use empack_lib::application::process_runtime::Cancellation;
use empack_lib::application::{CliConfig, CliLoad, EmpackExitCode, classify_error};
use empack_lib::primitives::ConfigError;

mod bootstrap;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Only the process parent supplies this channel. Project dotenv configuration
    // must not create or replace an invocation's cancellation connection.
    let bootstrap_descriptor = std::env::var_os("EMPACK_BOOTSTRAP_CONTROL");
    let mut config = match CliConfig::load_for_process() {
        Ok(CliLoad::Ready(config)) => *config,
        Ok(CliLoad::Display(message)) => {
            print!("{message}");
            return EmpackExitCode::Success.as_process_exit_code();
        }
        Err(ConfigError::ParseError { reason, .. }) => {
            eprint!("{}", empack_lib::terminal::display_text(&reason));
            return EmpackExitCode::Usage.as_process_exit_code();
        }
        Err(error) => {
            eprintln!(
                "Error: {}",
                empack_lib::terminal::display_text(&error.to_string())
            );
            return EmpackExitCode::Usage.as_process_exit_code();
        }
    };
    if let Err(error) = config.app_config.validate() {
        eprintln!(
            "Error: {}",
            empack_lib::terminal::display_text(&error.to_string())
        );
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
        eprintln!(
            "empack: logger init failed: {}",
            empack_lib::terminal::display_text(&error.to_string())
        );
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
    // A portable Java parent cannot send native console signals uniformly. Its
    // authenticated local connection supplies cancellation and parent-loss input;
    // the existing engine still owns retirement, recovery and the terminal result.
    let bootstrap =
        match bootstrap::attach(bootstrap_descriptor.as_deref(), cancellation.clone()).await {
            Ok(control) => control,
            Err(error) => {
                empack_lib::logger::global_shutdown();
                return finish_command(Err(error.into()), &mut std::io::stderr().lock())
                    .as_process_exit_code();
            }
        };
    let result = empack_lib::application::commands::execute_command_with_cancellation(
        config,
        cancellation.clone(),
    )
    .await;
    drop(bootstrap);
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
            // A plain interruption is an expected terminal outcome. Retain the
            // full error rendering whenever there is additional failure context.
            let clean_interruption = code == EmpackExitCode::Interrupted
                && error.chain().count() == 1
                && (error.is::<empack_lib::application::process_runtime::Interrupted>()
                    || error.is::<empack_lib::application::process_runtime::RuntimeExit>());
            let _ = writeln!(
                stderr,
                "{}{}",
                if clean_interruption { "" } else { "Error: " },
                empack_lib::terminal::display_text(&empack_lib::application::diagnostics::message(
                    &error
                ))
            );
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
        let (_, stderr) = finish_after_cancel(Err(std::io::Error::other(
            "Runtime failed \u{1b}]52;c;YQ==\u{7}",
        )
        .into()));
        assert!(!stderr.contains('\u{1b}'));
        assert!(stderr.contains("\\u{1b}]52"));
    }

    #[test]
    fn clean_interruption_reports_no_recovery_warning() {
        let cancellation = Cancellation::default();
        cancellation.cancel();
        let (code, stderr) = finish_after_cancel(cancellation.check());
        assert_eq!(code, EmpackExitCode::Interrupted);
        assert_eq!(stderr, "Operation interrupted\n");
    }

    #[test]
    fn runtime_interruption_is_visible_without_a_general_error_label() {
        use empack_lib::application::process_runtime::RuntimeExit;
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        #[cfg(unix)]
        let status = std::process::ExitStatus::from_raw(130 << 8);
        #[cfg(windows)]
        let status = std::process::ExitStatus::from_raw(130);
        let mut stderr = Vec::new();
        assert_eq!(
            finish_command(Err(RuntimeExit(status).into()), &mut stderr),
            EmpackExitCode::Interrupted
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            format!("Runtime interrupted ({status})\n")
        );
        // Extra failure context is never discarded merely because interruption
        // also occurred. This includes errors returned after a completed effect.
        let error: empack_lib::Result<()> = Err(RuntimeExit(status).into());
        let mut stderr = Vec::new();
        finish_command(
            error.map_err(|error| error.context("Runtime retired, but terminal cleanup failed")),
            &mut stderr,
        );
        let diagnostic = String::from_utf8(stderr).unwrap();
        assert!(diagnostic.starts_with("Error: Runtime retired, but terminal cleanup failed:"));
        assert!(diagnostic.contains(&status.to_string()));
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
