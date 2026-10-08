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
    if let Err(error) = empack_lib::Logger::init(config.app_config.to_logger_config(&caps)) {
        eprintln!("empack: logger init failed: {error}");
    }
    empack_lib::terminal::cursor::force_show_cursor();
    empack_lib::terminal::cursor::install_panic_hook();
    let cancellation = Cancellation::default();
    let signal_token = cancellation.clone();
    let listener = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_token.cancel();
        }
    });
    let result = empack_lib::application::commands::execute_command_with_cancellation(
        config,
        cancellation.clone(),
    )
    .await;
    listener.abort();
    empack_lib::terminal::cursor::force_show_cursor();
    empack_lib::logger::global_shutdown();
    if cancellation.is_cancelled() {
        return EmpackExitCode::Interrupted.as_process_exit_code();
    }
    match result {
        Ok(()) => EmpackExitCode::Success.as_process_exit_code(),
        Err(error) => {
            let code = classify_error(&error);
            eprintln!("Error: {error:#}");
            code.as_process_exit_code()
        }
    }
}
