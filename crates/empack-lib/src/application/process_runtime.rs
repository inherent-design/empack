//! Child lifetime and pipe drainage share one deadline, including after either stream closes.

use super::session::{ProcessOutput, ProcessStream, decode_process_output_chunk};
use anyhow::{Context, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Debug, thiserror::Error)]
#[error("Operation interrupted")]
pub struct Interrupted;

#[derive(Clone, Default)]
pub struct Cancellation(Arc<CancellationState>);

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    parent: Option<Arc<CancellationState>>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        let mut current = Some(&*self.0);
        while let Some(state) = current {
            if state.cancelled.load(Ordering::SeqCst) {
                return true;
            }
            current = state.parent.as_deref();
        }
        false
    }
    /// Child cancellation retires one phase without cancelling its parent operation.
    pub fn child(&self) -> Self {
        Self(Arc::new(CancellationState {
            cancelled: AtomicBool::new(false),
            parent: Some(self.0.clone()),
        }))
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Interrupted.into())
        } else {
            Ok(())
        }
    }
    pub(crate) async fn cancelled(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

async fn read_stream(
    mut pipe: impl AsyncRead + Unpin,
    stream: ProcessStream,
    sender: Option<mpsc::SyncSender<(ProcessStream, Vec<u8>)>>,
) -> Result<Vec<u8>> {
    let mut all = Vec::new();
    let mut pending = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        if all.len() + count > OUTPUT_LIMIT {
            anyhow::bail!("Subprocess output exceeds 16 MiB per stream");
        }
        all.extend_from_slice(&buffer[..count]);
        pending.extend_from_slice(&buffer[..count]);
        if let Some(end) = pending.iter().rposition(|b| *b == b'\n') {
            let tail = pending.split_off(end + 1);
            if let Some(sender) = &sender {
                // Display hints may be dropped; complete bounded output is returned separately.
                let _ = sender.try_send((stream, std::mem::replace(&mut pending, tail)));
            } else {
                pending = tail;
            }
        }
    }
    if !pending.is_empty()
        && let Some(sender) = sender
    {
        let _ = sender.try_send((stream, pending));
    }
    Ok(all)
}

/// Supervise an owned process on the host runtime. Progress is bounded and nonblocking.
/// A disconnected observer does not affect process lifetime or the retained output.
pub async fn execute_async(
    command: std::process::Command,
    timeout: Duration,
    cancellation: Cancellation,
    progress: Option<mpsc::SyncSender<(ProcessStream, Vec<u8>)>>,
) -> Result<ProcessOutput> {
    cancellation.check()?;
    let program = command.get_program().to_string_lossy().into_owned();
    let sender = progress;
    let mut command = tokio::process::Command::from(command);
    command
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(unix)]
    let mut child = command.spawn().context("Failed to spawn command")?;
    #[cfg(unix)]
    let tree = ProcessTree::new(&child)?;
    #[cfg(windows)]
    let (mut child, tree) = spawn_windows_child(&mut command, ProcessTree::new)?;
    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;
    let result = {
        let collect = async {
            let (status, stdout, stderr) = tokio::try_join!(
                async { child.wait().await.map_err(anyhow::Error::from) },
                read_stream(stdout, ProcessStream::Stdout, sender.clone()),
                read_stream(stderr, ProcessStream::Stderr, sender),
            )?;
            Ok(ProcessOutput {
                success: status.success(),
                stdout: decode_process_output_chunk(&stdout),
                stderr: decode_process_output_chunk(&stderr),
            })
        };
        tokio::select! {
            result = collect => result,
            _ = cancellation.cancelled() => Err(Interrupted.into()),
            _ = tokio::time::sleep(timeout) => Err(anyhow::anyhow!("Command {program:?} timed out after {} seconds", timeout.as_secs())),
        }
    };
    // Drop pipe futures before shutdown so descendants cannot keep readers blocked.
    drop(tree);
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
    result
}

/// Supervise a long-running caller-selected runtime with inherited console streams.
/// There is no build-tool deadline or accumulated-output limit. The owning engine scope
/// must retain this future and its instance lease until process-tree retirement.
pub(crate) struct RetiredRuntime(pub Result<std::process::ExitStatus>);
pub(crate) async fn execute_inherited(
    command: std::process::Command,
    cancellation: Cancellation,
) -> Result<RetiredRuntime> {
    if let Err(error) = cancellation.check() {
        return Ok(RetiredRuntime(Err(error)));
    }
    let mut command = tokio::process::Command::from(command);
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(unix)]
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(RetiredRuntime(Err(
                anyhow::Error::from(error).context("Failed to start instance runtime")
            )));
        }
    };
    #[cfg(unix)]
    let tree = ProcessTree::new(&child)?;
    #[cfg(windows)]
    let (mut child, tree) = spawn_windows_child(&mut command, ProcessTree::new)?;
    let result = tokio::select! {
        result = child.wait() => result.map_err(anyhow::Error::from),
        _ = cancellation.cancelled() => Err(Interrupted.into()),
    };
    let termination = tree.terminate();
    if result.is_err() {
        let _ = child.start_kill();
        // Keep the caller's lease until the immediate child is actually reaped.
        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .context("Instance runtime reaping timed out; retirement is unconfirmed")?
            .context("Failed to retire instance runtime")?;
    }
    termination?;
    tree.wait_retired().await?;
    Ok(RetiredRuntime(result))
}

#[cfg(unix)]
struct ProcessTree(u32);
#[cfg(unix)]
impl ProcessTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        Ok(Self(child.id().context(
            "Child exited before process group registration",
        )?))
    }
}
impl ProcessTree {
    async fn wait_retired(self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.has_processes()? {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("Runtime descendants have not retired; recovery acknowledgment is required")??;
        self.disarm();
        Ok(())
    }
    #[cfg(unix)]
    fn disarm(mut self) {
        self.0 = 0;
    }
    #[cfg(windows)]
    fn disarm(self) {}
    #[cfg(unix)]
    fn terminate(&self) -> Result<()> {
        if unsafe { libc::kill(-(self.0 as i32), libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(anyhow::Error::from(error).context("Cannot terminate runtime process group"))
        }
    }
    #[cfg(unix)]
    fn has_processes(&self) -> Result<bool> {
        if unsafe { libc::kill(-(self.0 as i32), 0) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(anyhow::Error::from(error)
                .context("Cannot establish runtime process-group retirement"))
        }
    }
    #[cfg(windows)]
    fn terminate(&self) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        if unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 130)
        } == 0
        {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context("Cannot terminate runtime job"));
        }
        Ok(())
    }
    #[cfg(windows)]
    fn has_processes(&self) -> Result<bool> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context("Cannot establish runtime job retirement"));
        }
        Ok(info.ActiveProcesses != 0)
    }
}
#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // The child was spawned as its own process group leader.
        if self.0 != 0 {
            unsafe {
                libc::kill(-(self.0 as i32), libc::SIGKILL);
            }
        }
    }
}

#[cfg(windows)]
fn spawn_windows_child(
    command: &mut tokio::process::Command,
    register: impl FnOnce(&tokio::process::Child) -> Result<ProcessTree>,
) -> Result<(tokio::process::Child, ProcessTree)> {
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
    command.kill_on_drop(true).creation_flags(CREATE_SUSPENDED);
    let child = command.spawn().context("Failed to spawn command")?;
    let tree = register(&child).context(
        "Cannot establish Windows subprocess ownership: the host must permit nested job registration; the child was not started",
    )?;
    resume_windows_child(&child)?;
    Ok((child, tree))
}

#[cfg(windows)]
fn resume_windows_child(child: &tokio::process::Child) -> Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    let pid = child.id().context("Suspended child ID unavailable")?;
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut present = unsafe { Thread32First(snapshot.as_raw_handle() as _, &mut entry) };
    while present != 0 {
        if entry.th32OwnerProcessID == pid {
            let raw = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                )
            };
            if raw.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let thread = unsafe { OwnedHandle::from_raw_handle(raw as _) };
            // Validate the handle, not the possibly stale thread ID. The live child
            // handle keeps its PID from being reused while registration/resume run.
            if unsafe { GetProcessIdOfThread(thread.as_raw_handle() as _) } != pid {
                anyhow::bail!("Suspended child thread identity changed before resume");
            }
            let previous = unsafe { ResumeThread(thread.as_raw_handle() as _) };
            if previous == u32::MAX {
                return Err(std::io::Error::last_os_error().into());
            }
            if previous > 0 {
                return Ok(());
            }
        }
        present = unsafe { Thread32Next(snapshot.as_raw_handle() as _, &mut entry) };
    }
    anyhow::bail!("Suspended child has no resumable thread")
}

#[cfg(windows)]
struct ProcessTree(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
impl ProcessTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let guard = Self(OwnedHandle::from_raw_handle(handle));
            let handle = guard.0.as_raw_handle();
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
                || AssignProcessToJobObject(
                    handle,
                    child.raw_handle().context("Child handle unavailable")? as _,
                ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(guard)
        }
    }
}
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[tokio::test]
    async fn rejected_job_registration_prevents_child_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("cmd.exe");
        command
            .args(["/C", "echo started>started.txt"])
            .current_dir(dir.path());
        let result = spawn_windows_child(&mut command, |_| {
            Err(anyhow::anyhow!("host refuses nesting"))
        });
        let error = match result {
            Ok(_) => panic!("registration must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("child was not started"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!dir.path().join("started.txt").exists());
    }

    #[tokio::test]
    async fn owned_job_closes_grandchild_pipes_after_ancestors_exit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("intermediate.cmd"),
            "@echo off\r\nstart /b ping.exe -n 30 127.0.0.1\r\necho done>intermediate-done\r\nexit /b 0\r\n").unwrap();
        let mut command = tokio::process::Command::new("cmd.exe");
        command
            .args(["/C", "start /b cmd.exe /c intermediate.cmd"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let (mut child, tree) = spawn_windows_child(&mut command, ProcessTree::new).unwrap();
        let mut stdout = child.stdout.take().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            while !dir.path().join("intermediate-done").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut output = Vec::new();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), stdout.read_to_end(&mut output))
                .await
                .is_err()
        );
        drop(tree);
        tokio::time::timeout(Duration::from_secs(3), stdout.read_to_end(&mut output))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stopping_one_owned_job_preserves_another() {
        let mut first = tokio::process::Command::new("cmd.exe");
        first.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        let (mut first_child, first_tree) =
            spawn_windows_child(&mut first, ProcessTree::new).unwrap();
        let mut second = tokio::process::Command::new("cmd.exe");
        second.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        let (mut second_child, second_tree) =
            spawn_windows_child(&mut second, ProcessTree::new).unwrap();
        drop(first_tree);
        tokio::time::timeout(Duration::from_secs(3), first_child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(second_child.try_wait().unwrap().is_none());
        drop(second_tree);
        tokio::time::timeout(Duration::from_secs(3), second_child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}

#[cfg(all(test, unix))]
mod async_tests {
    use super::*;
    fn shell(script: &str) -> std::process::Command {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", script]);
        command
    }
    #[tokio::test]
    async fn full_or_disconnected_progress_cannot_block_process_supervision() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let output = execute_async(
            shell("i=0; while [ $i -lt 200 ]; do echo line; i=$((i+1)); done"),
            Duration::from_secs(2),
            Cancellation::default(),
            Some(sender),
        )
        .await
        .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.lines().count(), 200);
        drop(receiver);
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let output = execute_async(
            shell("echo retained"),
            Duration::from_secs(2),
            Cancellation::default(),
            Some(sender),
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, "retained\n");
    }
    #[tokio::test]
    async fn host_runtime_remains_responsive_and_closed_pipes_do_not_remove_deadline() {
        let began = std::time::Instant::now();
        let process = execute_async(
            shell("exec 1>&- 2>&-; sleep 30"),
            Duration::from_millis(100),
            Cancellation::default(),
            None,
        );
        let heartbeat = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(began.elapsed() < Duration::from_secs(1));
        };
        let (output, ()) = tokio::join!(process, heartbeat);
        assert!(output.unwrap_err().to_string().contains("timed out"));
        assert!(began.elapsed() < Duration::from_secs(3));
    }
    #[tokio::test]
    async fn inherited_descendant_pipes_and_parent_cancellation_retire() {
        let cancel = Cancellation::default();
        let child_cancel = cancel.child();
        let process = execute_async(
            shell("sleep 30 & exit 0"),
            Duration::from_secs(20),
            child_cancel,
            None,
        );
        let interrupt = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel.cancel();
        };
        let began = std::time::Instant::now();
        let (output, ()) = tokio::join!(process, interrupt);
        assert!(output.unwrap_err().is::<Interrupted>());
        assert!(began.elapsed() < Duration::from_secs(3));
    }
}
