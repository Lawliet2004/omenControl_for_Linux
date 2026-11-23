//! Standalone CLI parent: observe scope-creation failures without changing the
//! child's argv/environment. This runs before the CLI starts any worker threads.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus};

pub(crate) struct Signals {
    fd: File,
    previous: libc::sigset_t,
    actions: Vec<(i32, libc::sigaction)>,
}
impl Signals {
    pub(crate) fn new() -> io::Result<Self> {
        let mut watched = unsafe { std::mem::zeroed() };
        let mut previous = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut watched);
            for signal in [
                libc::SIGINT,
                libc::SIGTERM,
                libc::SIGHUP,
                libc::SIGQUIT,
                libc::SIGUSR1,
                libc::SIGUSR2,
            ] {
                libc::sigaddset(&mut watched, signal);
            }
            let error = libc::pthread_sigmask(libc::SIG_BLOCK, &watched, &mut previous);
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            let mut actions = Vec::new();
            for signal in [
                libc::SIGINT,
                libc::SIGTERM,
                libc::SIGHUP,
                libc::SIGQUIT,
                libc::SIGUSR1,
                libc::SIGUSR2,
                libc::SIGCHLD,
            ] {
                let mut action: libc::sigaction = std::mem::zeroed();
                let mut old: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(signal, &action, &mut old) != 0 {
                    let error = io::Error::last_os_error();
                    for (signal, action) in &actions {
                        libc::sigaction(*signal, action, std::ptr::null_mut());
                    }
                    libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
                    return Err(error);
                }
                actions.push((signal, old));
            }
            let fd = libc::signalfd(-1, &watched, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK);
            if fd < 0 {
                let error = io::Error::last_os_error();
                for (signal, action) in &actions {
                    libc::sigaction(*signal, action, std::ptr::null_mut());
                }
                libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
                return Err(error);
            }
            Ok(Self {
                fd: File::from_raw_fd(fd),
                previous,
                actions,
            })
        }
    }
    pub(crate) fn spawn(&self, command: &mut Command) -> io::Result<Child> {
        let previous = self.previous;
        let actions = self.actions.clone();
        // Only the async-signal-safe mask syscall runs between fork and exec.
        unsafe {
            command.pre_exec(move || {
                for (signal, action) in &actions {
                    if libc::sigaction(*signal, action, std::ptr::null_mut()) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if libc::sigprocmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) != 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        command.spawn()
    }
    pub(crate) fn wait(&self, child: &mut Child) -> io::Result<ExitStatus> {
        // An unavailable pidfd changes waiting efficiency, never identity or
        // authorization. This Child's PID cannot be reused before it is reaped.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id() as libc::pid_t, 0) };
        let pidfd = if fd >= 0 {
            Some(unsafe { File::from_raw_fd(fd as i32) })
        } else {
            None
        };
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            let mut polls = [
                libc::pollfd {
                    fd: self.fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: pidfd.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let ready = unsafe {
                libc::poll(
                    polls.as_mut_ptr(),
                    2,
                    if pidfd.is_some() { -1 } else { 100 },
                )
            };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if polls
                .iter()
                .any(|p| p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0)
            {
                return Err(io::Error::other(
                    "game process observation became unavailable",
                ));
            }
            if polls[0].revents & libc::POLLIN != 0 {
                loop {
                    let mut info: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
                    let size = unsafe {
                        libc::read(
                            self.fd.as_raw_fd(),
                            &mut info as *mut _ as *mut _,
                            std::mem::size_of_val(&info),
                        )
                    };
                    if size < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() == io::ErrorKind::WouldBlock {
                            break;
                        }
                        if error.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(error);
                    }
                    if size as usize != std::mem::size_of_val(&info) {
                        return Err(io::Error::other("short signal event"));
                    }
                    // A terminal sends its signal to the whole foreground
                    // group. Do not deliver it twice to a child in that group.
                    let terminal_group = info.ssi_code == libc::SI_KERNEL
                        && unsafe { libc::getpgid(child.id() as libc::pid_t) == libc::getpgrp() };
                    if !terminal_group {
                        let sent =
                            unsafe { libc::kill(child.id() as libc::pid_t, info.ssi_signo as i32) };
                        if sent != 0 {
                            let error = io::Error::last_os_error();
                            if error.raw_os_error() != Some(libc::ESRCH) {
                                return Err(error);
                            }
                        }
                    }
                }
            }
        }
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            for (signal, action) in &self.actions {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut());
        }
    }
}

/// Preserve the child's numeric exit code or terminating signal. Avoid a
/// second wrapper core dump when mirroring a child that already crashed.
pub fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    if let Some(signal) = status.signal() {
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0);
            libc::signal(signal, libc::SIG_DFL);
            let mut mask = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, signal);
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &mask, std::ptr::null_mut());
            libc::raise(signal);
        }
        return 128 + signal;
    }
    1
}
