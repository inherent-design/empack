//! Foreground ownership for inherited Unix consoles; captured jobs never use this path.
use anyhow::{Context, Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io::{self, IsTerminal},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
};

static OWNER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) struct ForegroundTerminal {
    tty: File,
    group: libc::pid_t,
    settings: libc::termios,
    restored: bool,
    _owner: tokio::sync::MutexGuard<'static, ()>,
}
impl ForegroundTerminal {
    pub(super) fn capture() -> Result<Option<Self>> {
        if !io::stdin().is_terminal() && !io::stdout().is_terminal() && !io::stderr().is_terminal()
        {
            return Ok(None);
        }
        let tty = match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOCTTY)
            .open("/dev/tty")
        {
            Ok(tty) => tty,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENXIO | libc::ENODEV | libc::ENOENT)
                ) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error).context("Cannot open the runtime controlling terminal");
            }
        };
        let owner = OWNER
            .try_lock()
            .context("Another runtime already owns this process's terminal")?;
        let group = unsafe { libc::tcgetpgrp(tty.as_raw_fd()) };
        if group < 0 {
            return Err(io::Error::last_os_error())
                .context("Cannot observe terminal foreground ownership");
        }
        ensure!(
            group == unsafe { libc::getpgrp() },
            "Cannot launch an interactive runtime from a background process group; bring empack to the foreground or redirect all console streams"
        );
        let mut settings = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(tty.as_raw_fd(), &mut settings) } != 0 {
            return Err(io::Error::last_os_error()).context("Cannot capture terminal settings");
        }
        Ok(Some(Self {
            tty,
            group,
            settings,
            restored: false,
            _owner: owner,
        }))
    }
    pub(super) fn configure_child(&self, command: &mut tokio::process::Command) {
        let fd = self.tty.as_raw_fd();
        let mut runtime_settings = self.settings;
        // This supervisor is not a shell job-control manager. Disable the tty's
        // suspend characters while preserving interrupt/quit and input behavior.
        runtime_settings.c_cc[libc::VSUSP] = libc::_POSIX_VDISABLE;
        #[cfg(any(
            target_os = "macos",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        {
            runtime_settings.c_cc[libc::VDSUSP] = libc::_POSIX_VDISABLE;
        }
        // The group is created by process_group(0) before this hook. Transfer before
        // exec: a parent-side transfer races the child's first read or tcsetattr.
        // The inherited descriptor is close-on-exec; only libc calls run after fork.
        unsafe {
            command.pre_exec(move || transfer(fd, libc::getpid(), Some(&runtime_settings)));
        }
    }
    pub(super) fn restore(&mut self) -> Result<()> {
        if !self.restored {
            transfer(self.tty.as_raw_fd(), self.group, Some(&self.settings))
                .context("Cannot restore the runtime terminal foreground group and settings")?;
            self.restored = true;
        }
        Ok(())
    }
}
impl Drop for ForegroundTerminal {
    fn drop(&mut self) {
        // Also covers spawn failure and early retirement errors. Normal completion
        // uses restore explicitly so a failed restoration is reported to the caller.
        let _ = self.restore();
    }
}

fn transfer(
    fd: libc::c_int,
    group: libc::pid_t,
    settings: Option<&libc::termios>,
) -> io::Result<()> {
    unsafe {
        let mut blocked = std::mem::zeroed();
        let mut previous = std::mem::zeroed();
        libc::sigemptyset(&mut blocked);
        libc::sigaddset(&mut blocked, libc::SIGTTOU);
        // Block on this thread only, rather than changing a process-wide handler
        // while other host tasks execute. Restore the inherited mask before exec.
        let code = libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code));
        }
        let result = if libc::tcsetpgrp(fd, group) != 0 {
            Err(io::Error::last_os_error())
        } else if let Some(settings) = settings {
            if libc::tcsetattr(fd, libc::TCSANOW, settings) != 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        let code = libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
        result?;
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code));
        }
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn failed_terminal_transfer_restores_the_calling_threads_signal_mask() {
    unsafe {
        let mut before = std::mem::zeroed();
        let mut after = std::mem::zeroed();
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut before),
            0
        );
        let error = transfer(-1, libc::getpgrp(), None).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut after),
            0
        );
        assert_eq!(
            libc::sigismember(&before, libc::SIGTTOU),
            libc::sigismember(&after, libc::SIGTTOU)
        );
    }
}
