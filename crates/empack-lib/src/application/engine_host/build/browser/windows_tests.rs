//! Native launcher supervision, separate from the registered browser's handoff.
use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

const CHILD: &str =
    "application::engine_host::build::browser::windows_tests::launcher_child_fixture";

fn child(root: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(root);
    command
}

#[test]
#[ignore = "private launcher child, selected only by its native fixture"]
fn launcher_child_fixture() {
    assert_eq!(std::fs::read("launcher-mode").unwrap(), b"wait");
    std::fs::write("launcher.pid.tmp", std::process::id().to_string()).unwrap();
    std::fs::rename("launcher.pid.tmp", "launcher.pid").unwrap();
    // No subprocess is created: the supervisor owns this launcher, not a GUI or
    // an arbitrary descendant. Publishing the PID establishes actual startup.
    loop {
        std::thread::park();
    }
}

#[tokio::test]
async fn desktop_launcher_bounds_wait_and_preserves_literal_arguments() {
    let root = tempfile::Builder::new()
        .prefix("browser launcher λ ")
        .tempdir()
        .unwrap();
    let script = b"@echo off\r\n<nul set /p \"=%~1\" > \"%~2\"\r\nexit /b 0\r\n";
    std::fs::write(root.path().join("literal.cmd"), script).unwrap();
    let page = "https://modrinth.com/mod/sodium/version/abcdefgh";
    let mut command = std::process::Command::new("cmd.exe");
    command
        .args([
            "/D",
            "/Q",
            "/C",
            "literal.cmd",
            page,
            "output with spaces.txt",
        ])
        .current_dir(root.path());
    launch_command(command, Duration::from_secs(5), Default::default())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("output with spaces.txt")).unwrap(),
        page.as_bytes()
    );
    assert_eq!(
        std::fs::read(root.path().join("literal.cmd")).unwrap(),
        script
    );

    let mut command = std::process::Command::new("cmd.exe");
    command.args(["/D", "/Q", "/C", "exit /b 7"]);
    let error = launch_command(command, Duration::from_secs(5), Default::default())
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Browser launcher failed; continuation was retained"
    );

    std::fs::write(root.path().join("launcher-mode"), b"wait").unwrap();
    let start = tokio::time::Instant::now();
    let error = launch_command(
        child(root.path()),
        Duration::from_millis(20),
        Default::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Browser launcher did not confirm completion; continuation was retained"
    );
    assert!(start.elapsed() < Duration::from_secs(5));

    // Use another directory so an identifier from the timed-out child cannot
    // stand in for startup of the child whose cancellation is being checked.
    let cancelled = root.path().join("cancelled launcher");
    std::fs::create_dir(&cancelled).unwrap();
    std::fs::write(cancelled.join("launcher-mode"), b"wait").unwrap();
    let cancel = crate::application::process_runtime::Cancellation::default();
    let task = tokio::spawn(launch_command(
        child(&cancelled),
        Duration::from_secs(30),
        cancel.clone(),
    ));
    let pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(pid) = std::fs::read_to_string(cancelled.join("launcher.pid"))
                .ok()
                .and_then(|text| text.parse::<u32>().ok())
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let pid = match pid {
        Ok(pid) => pid,
        Err(error) => {
            cancel.cancel();
            let retired = tokio::time::timeout(Duration::from_secs(5), task).await;
            panic!("launcher did not announce startup: {error}; cleanup: {retired:?}");
        }
    };
    // Retain the real process object before cancellation; checking this handle
    // afterwards cannot confuse a reused PID with the fixture's retirement.
    // SAFETY: the fixture atomically published its own PID and is still waiting.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        cancel.cancel();
        let retired = tokio::time::timeout(Duration::from_secs(5), task).await;
        panic!("could not retain fixture process: {error}; cleanup: {retired:?}");
    }
    // SAFETY: OpenProcess returned this uniquely owned valid handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.is::<crate::application::process_runtime::Interrupted>());
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
}
