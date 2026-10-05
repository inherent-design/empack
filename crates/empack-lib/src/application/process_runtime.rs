//! Child lifetime and pipe drainage share one deadline, including after either stream closes.

use super::session::{ProcessObserver, ProcessOutput, ProcessStream, decode_process_output_chunk};
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
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
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
    sender: mpsc::Sender<(ProcessStream, Vec<u8>)>,
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
            if sender
                .send((stream, std::mem::replace(&mut pending, tail)))
                .is_err()
            {
                anyhow::bail!("Process observer disconnected");
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender.send((stream, pending));
    }
    Ok(all)
}

pub(crate) fn execute(
    command: std::process::Command,
    timeout: Duration,
    cancellation: Cancellation,
    observer: &dyn ProcessObserver,
) -> Result<ProcessOutput> {
    cancellation.check()?;
    let program = command.get_program().to_string_lossy().into_owned();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<ProcessOutput> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async move {
            let mut command = tokio::process::Command::from(command);
            command.kill_on_drop(true).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
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
                    Ok(ProcessOutput { success: status.success(), stdout: decode_process_output_chunk(&stdout), stderr: decode_process_output_chunk(&stderr) })
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
        })
    });
    for (stream, bytes) in receiver {
        for line in decode_process_output_chunk(&bytes).lines() {
            observer.on_line(stream, line.trim_end_matches('\r'));
        }
    }
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("Process worker panicked"))?
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
#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // The child was spawned as its own process group leader.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
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
struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let guard = Self(handle);
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
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
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
