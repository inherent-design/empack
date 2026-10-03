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
    async fn cancelled(&self) {
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
            let mut child = command.spawn().context("Failed to spawn command")?;
            let tree = ProcessTree::new(&child)?;
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
struct ProcessTree {
    job: Option<WindowsJob>,
    pid: u32,
}
#[cfg(windows)]
impl ProcessTree {
    fn new(child: &tokio::process::Child) -> Result<Self> {
        Self::from_registration(child, WindowsJob::attach(child))
    }
    fn from_registration(
        child: &tokio::process::Child,
        result: Result<WindowsJob>,
    ) -> Result<Self> {
        let pid = child.id().context("Child process ID unavailable")?;
        let job = match result {
            Ok(job) => Some(job),
            Err(error) => {
                tracing::warn!(%error, "Windows job registration unavailable; using bounded process-tree cleanup");
                None
            }
        };
        Ok(Self { job, pid })
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.job.is_some() {
            return;
        }
        // Restrictive host jobs may refuse nesting. Keep output/deadline handling
        // available and attempt bounded tree cleanup without changing host job limits.
        use std::os::windows::process::CommandExt;
        let program = std::env::var_os("SystemRoot")
            .map(|root| {
                std::path::PathBuf::from(root)
                    .join("System32")
                    .join("taskkill.exe")
            })
            .unwrap_or_else(|| "taskkill.exe".into());
        let Ok(mut cleanup) = std::process::Command::new(program)
            .args(["/PID", &self.pid.to_string(), "/T", "/F"])
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match cleanup.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => {
                    let _ = cleanup.kill();
                    return;
                }
                Ok(None) if std::time::Instant::now() >= deadline => {
                    let _ = cleanup.kill();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}
#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    fn attach(child: &tokio::process::Child) -> Result<Self> {
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
impl Drop for WindowsJob {
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
    async fn rejected_job_registration_preserves_successful_output() {
        let child = tokio::process::Command::new("cmd.exe")
            .args(["/C", "echo retained output"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let tree = ProcessTree::from_registration(
            &child,
            Err(anyhow::anyhow!("host job refuses nesting")),
        )
        .unwrap();
        let output = child.wait_with_output().await.unwrap();
        drop(tree);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("retained output"));
    }
    #[tokio::test]
    async fn rejected_job_registration_keeps_cleanup_bounded() {
        let mut child = tokio::process::Command::new("cmd.exe")
            .args(["/C", "ping -n 30 127.0.0.1 >nul"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let tree = ProcessTree::from_registration(
            &child,
            Err(anyhow::anyhow!("host job refuses nesting")),
        )
        .unwrap();
        let start = std::time::Instant::now();
        drop(tree);
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!status.success());
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
