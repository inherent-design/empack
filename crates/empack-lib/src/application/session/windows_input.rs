//! Retire a blocking prompt read before its native caller can continue.
use crate::application::process_runtime::Cancellation;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use windows_sys::Win32::System::{
    IO::CancelSynchronousIo,
    Threading::{GetCurrentThreadId, OpenThread, THREAD_TERMINATE},
};

struct Finished<'a>(&'a AtomicBool);
impl Drop for Finished<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

pub(super) fn read<T>(cancellation: &Cancellation, read: impl FnOnce() -> T) -> io::Result<T> {
    // This is a real handle to this prompt's thread, not the calling thread's
    // pseudo-handle evaluated later on the cancellation worker.
    // THREAD_TERMINATE is the access right required by CancelSynchronousIo.
    let handle = unsafe { OpenThread(THREAD_TERMINATE, 0, GetCurrentThreadId()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let thread = unsafe { OwnedHandle::from_raw_handle(handle) };
    let finished = AtomicBool::new(false);
    Ok(std::thread::scope(|scope| {
        // The guard also stops the monitor if a renderer panics. Scope joins it
        // before returning, so it cannot cancel a later user of this thread.
        let _finished = Finished(&finished);
        scope.spawn(|| {
            while !finished.load(Ordering::SeqCst) {
                if cancellation.is_cancelled() {
                    // Retry the cancellation request across the race between
                    // prompt rendering and the next synchronous input read.
                    // No input is injected and no process/thread is terminated.
                    unsafe { CancelSynchronousIo(thread.as_raw_handle()) };
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        read()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_retires_the_prompt_monitor_before_unwinding() {
        let cancellation = Cancellation::default();
        let panic = std::panic::catch_unwind(|| {
            read::<()>(&cancellation, || panic!("renderer fixture"))
                .expect("establish native prompt monitor");
        })
        .expect_err("renderer panic must propagate after monitor retirement");
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"renderer fixture"));
        assert_eq!(
            read(&cancellation, || "next caller").unwrap(),
            "next caller"
        );
    }
}
