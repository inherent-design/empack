//! Real terminal and host interrupts exercise all native prompt adapters.
#![cfg(unix)]
use empack_lib::application::{
    process_runtime::Cancellation,
    session::{InteractiveProvider, LiveInteractiveProvider},
};
use empack_tests::harness::{process::offline_empack_cmd, terminal, workspace::snapshot};
use expectrl::{Expect, Regex, process::unix::Signal};

#[test]
fn native_unix_prompt_adapters_retire_input_and_host_interrupts_before_followup() {
    let root = tempfile::tempdir().unwrap();
    let before = snapshot(root.path());
    for mode in ["confirm", "text", "select", "fuzzy"] {
        for host in [false, true] {
            let environment = offline_empack_cmd(root.path());
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .current_dir(root.path())
                .args([
                    "--ignored",
                    "--exact",
                    "native_unix_prompt_child",
                    "--nocapture",
                ])
                .env("EMPACK_UNIX_PROMPT_FIXTURE", mode);
            for (key, value) in environment.get_envs() {
                if let Some(value) = value {
                    command.env(key, value);
                }
            }
            let mut session =
                expectrl::session::log(terminal::spawn(command), std::io::stderr()).unwrap();
            session.set_expect_timeout(Some(std::time::Duration::from_secs(20)));
            session.expect(Regex("Unix prompt")).unwrap();
            if host {
                session.get_process_mut().signal(Signal::SIGINT).unwrap();
            } else {
                session.send(expectrl::ControlCode::EndOfText).unwrap();
            }
            session.expect(Regex("Operation interrupted")).unwrap();
            session.expect(Regex("Follow-up prompt")).unwrap();
            session.send_line("n").unwrap();
            session.expect(Regex("Follow-up declined")).unwrap();
            session.expect(expectrl::Eof).unwrap();
            terminal::wait_for_exit(session.get_process(), 130);
            assert_eq!(
                snapshot(root.path()),
                before,
                "{mode} host={host} changed fixture state"
            );
        }
    }
}

#[test]
#[ignore = "private native PTY child, selected only by its owning fixture"]
fn native_unix_prompt_child() {
    let mode = std::env::var("EMPACK_UNIX_PROMPT_FIXTURE").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(async {
        let cancellation = Cancellation::default();
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();
        let signal_token = cancellation.clone();
        let listener = tokio::spawn(async move {
            interrupt.recv().await.unwrap();
            signal_token.cancel();
        });
        let provider = LiveInteractiveProvider::new(false, cancellation);
        let result = match mode.as_str() {
            "confirm" => provider.confirm("Unix prompt", false).map(|_| ()),
            "text" => provider
                .text_input("Unix prompt", "default".into())
                .map(|_| ()),
            "select" => provider.select("Unix prompt", &["one", "two"]).map(|_| ()),
            "fuzzy" => provider
                .fuzzy_select("Unix prompt", &["one".into(), "two".into()])
                .map(|_| ()),
            _ => panic!("unknown native prompt fixture"),
        };
        listener.abort();
        result
    });
    let error = result.expect_err("interruption must not return a user choice");
    eprintln!("Error: {error:#}");
    assert_eq!(
        empack_lib::application::classify_error(&error),
        empack_lib::application::EmpackExitCode::Interrupted
    );
    let next = LiveInteractiveProvider::new(false, Cancellation::default());
    assert!(!next.confirm("Follow-up prompt", true).unwrap());
    eprintln!("Follow-up declined");
    std::process::exit(130);
}
