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
    _job: Option<WindowsJob>,
    fallback: Option<WindowsProcess>,
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
        let job = match result {
            Ok(job) => Some(job),
            Err(error) => {
                tracing::warn!(%error, "Windows job registration unavailable; using bounded process-tree cleanup");
                None
            }
        };
        let fallback = if job.is_none() {
            Some(WindowsProcess::from_child(child)?)
        } else {
            None
        };
        Ok(Self {
            _job: job,
            fallback,
        })
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // Retain both the job and process handles until cleanup completes.
        if let Some(root) = self.fallback.take() {
            terminate_windows_tree(root);
        }
    }
}

#[cfg(windows)]
struct WindowsProcess {
    handle: std::os::windows::io::OwnedHandle,
    pid: u32,
    created: u64,
}
#[cfg(windows)]
impl WindowsProcess {
    fn from_child(child: &tokio::process::Child) -> Result<Self> {
        use std::os::windows::io::BorrowedHandle;
        let raw = child.raw_handle().context("Child handle unavailable")?;
        let handle = unsafe { BorrowedHandle::borrow_raw(raw) }.try_clone_to_owned()?;
        Self::from_handle(handle, child.id().context("Child process ID unavailable")?)
    }
    fn from_handle(handle: std::os::windows::io::OwnedHandle, pid: u32) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited = created;
        let mut kernel = created;
        let mut user = created;
        if unsafe {
            GetProcessTimes(
                handle.as_raw_handle() as _,
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        Ok(Self {
            handle,
            pid,
            created,
        })
    }
    fn open(pid: u32) -> Result<Self> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::*;
        let raw = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        Self::from_handle(unsafe { OwnedHandle::from_raw_handle(raw as _) }, pid)
    }
    fn terminate(&self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(
                self.handle.as_raw_handle() as _,
                1,
            );
        }
    }
}

#[cfg(windows)]
fn windows_process_snapshot() -> Result<Vec<(u32, u32)>> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, System::Diagnostics::ToolHelp::*};
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut entries = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot.as_raw_handle() as _, &mut entry) };
    while present != 0 {
        entries.push((entry.th32ProcessID, entry.th32ParentProcessID));
        present = unsafe { Process32NextW(snapshot.as_raw_handle() as _, &mut entry) };
    }
    Ok(entries)
}

#[cfg(windows)]
fn is_current_windows_child(child: &WindowsProcess, parent: &WindowsProcess) -> bool {
    // Both identities are now pinned by handles. The initial snapshot may have
    // raced PID reuse before OpenProcess; re-read the parent relation only after
    // opening the handle, so an unrelated replacement cannot pass validation.
    child.created >= parent.created
        && windows_process_snapshot()
            .is_ok_and(|entries| entries.contains(&(child.pid, parent.pid)))
}

#[cfg(windows)]
fn terminate_windows_tree(root: WindowsProcess) {
    // Open handles prevent PID reuse, including after a parent exits. Creation
    // times reject stale parent IDs that predate the process we actually launched.
    let mut tracked = std::collections::BTreeMap::from([(root.pid, root)]);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        for process in tracked.values() {
            process.terminate();
        }
        let Ok(entries) = windows_process_snapshot() else {
            return;
        };
        let mut discovered = false;
        loop {
            let mut added = false;
            for &(pid, parent) in &entries {
                if std::time::Instant::now() >= deadline {
                    return;
                }
                if tracked.contains_key(&pid) {
                    continue;
                }
                if let Some(parent) = tracked.get(&parent)
                    && let Ok(child) = WindowsProcess::open(pid)
                    && is_current_windows_child(&child, parent)
                {
                    child.terminate();
                    tracked.insert(pid, child);
                    added = true;
                    discovered = true;
                }
            }
            if !added {
                break;
            }
        }
        if !discovered || std::time::Instant::now() >= deadline {
            return;
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
    #[tokio::test]
    async fn fallback_closes_inherited_pipes_after_parent_exits() {
        use std::os::windows::io::AsRawHandle;
        let mut child = tokio::process::Command::new("cmd.exe")
            .args(["/C", "start /b ping.exe -n 30 127.0.0.1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let tree = ProcessTree::from_registration(
            &child,
            Err(anyhow::anyhow!("host job refuses nesting")),
        )
        .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        let descendant_id = windows_process_snapshot()
            .unwrap()
            .into_iter()
            .find_map(|(id, parent)| (parent == pid).then_some(id))
            .expect("descendant should outlive parent");
        let descendant = WindowsProcess::open(descendant_id).unwrap();
        drop(tree);
        assert_eq!(
            unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(
                    descendant.handle.as_raw_handle() as _,
                    3000,
                )
            },
            0
        );
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), stdout.read_to_end(&mut output))
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn stale_snapshot_cannot_claim_an_unrelated_process() {
        let mut parent = tokio::process::Command::new("cmd.exe")
            .args(["/C", "ping -n 30 127.0.0.1 >nul"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let parent_tree =
            ProcessTree::from_registration(&parent, Err(anyhow::anyhow!("fixture"))).unwrap();
        let mut unrelated = tokio::process::Command::new("cmd.exe")
            .args(["/C", "ping -n 30 127.0.0.1 >nul"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let unrelated_tree =
            ProcessTree::from_registration(&unrelated, Err(anyhow::anyhow!("fixture"))).unwrap();
        let owned = parent_tree.fallback.as_ref().unwrap();
        let replacement = unrelated_tree.fallback.as_ref().unwrap();
        // An old snapshot could pair replacement.pid with owned.pid. The later
        // creation-time comparison alone accepts that false relationship.
        assert!(replacement.created >= owned.created);
        assert!(!is_current_windows_child(replacement, owned));
        drop(parent_tree);
        assert!(unrelated.try_wait().unwrap().is_none());
        drop(unrelated_tree);
        tokio::time::timeout(Duration::from_secs(3), parent.wait())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), unrelated.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
