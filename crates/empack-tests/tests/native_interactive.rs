//! Windows console reads are cancellation-aware and retire before the host returns.
#![cfg(windows)]
use empack_lib::application::{
    process_runtime::Cancellation,
    session::{InteractiveProvider, LiveInteractiveProvider},
};
use empack_tests::harness::{process::offline_empack_cmd, terminal, workspace::snapshot};
use expectrl::{Expect, Regex};

#[test]
fn native_windows_prompt_reads_retire_after_console_interrupt() {
    let root = tempfile::tempdir().unwrap();
    let before = snapshot(root.path());
    for mode in ["confirm", "text", "select", "fuzzy"] {
        // Reproduce the verification runner's inherited ignored-signal policy.
        assert_ne!(
            unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1) },
            0,
            "establish inherited Ctrl-C suppression: {}",
            std::io::Error::last_os_error()
        );
        let environment = offline_empack_cmd(root.path());
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env_clear()
            .current_dir(root.path())
            .args(["--ignored", "--exact", "native_prompt_child", "--nocapture"])
            .env("EMPACK_CONSOLE_PROMPT_FIXTURE", mode);
        for (key, value) in environment.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        let mut terminal =
            expectrl::session::log(terminal::spawn(command), std::io::stderr()).unwrap();
        terminal.set_expect_timeout(Some(std::time::Duration::from_secs(20)));
        terminal.expect(Regex("Console prompt")).unwrap();
        terminal.send(expectrl::ControlCode::EndOfText).unwrap();
        terminal.expect(Regex("Host interrupt observed")).unwrap();
        terminal.expect(Regex("Operation interrupted")).unwrap();
        terminal.expect(Regex("Follow-up prompt")).unwrap();
        terminal.send_line("n").unwrap();
        terminal.expect(Regex("Follow-up declined")).unwrap();
        assert_eq!(terminal.get_process().wait(Some(20_000)).unwrap(), 130);
        assert_eq!(
            snapshot(root.path()),
            before,
            "{mode} changed fixture state"
        );
    }
}

#[test]
#[ignore = "private native console child, selected only by its owning fixture"]
fn native_prompt_child() {
    let mode = std::env::var("EMPACK_CONSOLE_PROMPT_FIXTURE").unwrap();
    let mut input_mode = 0;
    let console_mode_available = unsafe {
        use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE};
        GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut input_mode) != 0
    };
    let input_mode = if console_mode_available {
        format!("{input_mode:#x}")
    } else {
        format!("unavailable: {}", std::io::Error::last_os_error())
    };
    eprintln!("Console fixture: {mode}; input mode: {input_mode}");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(async {
        let cancellation = Cancellation::default();
        // Register before displaying the prompt. The real CLI is exercised by
        // cli_recovery; this child isolates the four native prompt adapters.
        let mut interrupt = tokio::signal::windows::ctrl_c().unwrap();
        let signal_token = cancellation.clone();
        let listener = tokio::spawn(async move {
            interrupt.recv().await.unwrap();
            eprintln!("Host interrupt observed");
            signal_token.cancel();
        });
        let provider = LiveInteractiveProvider::new(false, cancellation);
        let result = match mode.as_str() {
            "confirm" => provider.confirm("Console prompt", false).map(|_| ()),
            "text" => provider
                .text_input("Console prompt", "default".into())
                .map(|_| ()),
            "select" => provider
                .select("Console prompt", &["one", "two"])
                .map(|_| ()),
            "fuzzy" => provider
                .fuzzy_select("Console prompt", &["one".into(), "two".into()])
                .map(|_| ()),
            _ => panic!("unknown native prompt fixture"),
        };
        listener.abort();
        result
    });
    empack_lib::terminal::cursor::force_show_cursor();
    let error = result.expect_err("console interruption must not return a user choice");
    eprintln!("Error: {error:#}");
    assert_eq!(
        empack_lib::application::classify_error(&error),
        empack_lib::application::EmpackExitCode::Interrupted
    );
    // Reuse this same thread after the cancelled prompt. A scoped cancellation
    // monitor must not interfere with the next, separately owned decision.
    let next = LiveInteractiveProvider::new(false, Cancellation::default());
    assert!(!next.confirm("Follow-up prompt", false).unwrap());
    eprintln!("Follow-up declined");
    std::process::exit(130);
}
