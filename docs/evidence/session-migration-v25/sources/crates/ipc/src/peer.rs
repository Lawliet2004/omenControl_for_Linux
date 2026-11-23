//! Optional client binding to an observed server. No authority is conferred by
//! these identifiers; they prevent an existing request from changing targets.
use omen_core::{OmenError, Result};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{
    fs::{FileTypeExt, MetadataExt},
    net::UnixStream,
};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
    owner: u32,
}

impl SocketIdentity {
    fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.file_type().is_socket() {
            return Err(changed());
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
        })
    }
}

pub(crate) struct PeerBinding {
    socket: SocketIdentity,
    uid: u32,
    pid: i32,
    pidfd: Option<File>,
    start_time: Option<u64>,
}

fn changed() -> OmenError {
    OmenError::DaemonUnavailable(
        "service endpoint changed; wait for a fresh monitoring update and retry".into(),
    )
}

fn credentials(stream: &UnixStream) -> Result<libc::ucred> {
    let mut credential: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credential as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() || credential.pid <= 0 {
        return Err(changed());
    }
    Ok(credential)
}

fn process_start(pid: i32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // comm may contain spaces and ')' characters. Fields after its final ')'
    // start at field 3; starttime is field 22.
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .and_then(|value| value.parse().ok())
        .ok_or_else(changed)
}

impl PeerBinding {
    pub(crate) fn connect(path: &Path) -> Result<(Self, UnixStream)> {
        let socket = SocketIdentity::read(path)?;
        let stream = UnixStream::connect(path)?;
        let credential = credentials(&stream)?;
        let mut fd: libc::c_int = -1;
        let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERPIDFD,
                &mut fd as *mut _ as *mut libc::c_void,
                &mut length,
            )
        };
        let pidfd = if result == 0 && fd >= 0 {
            Some(unsafe { File::from_raw_fd(fd) })
        } else {
            None
        };
        if result == 0 && length as usize != std::mem::size_of::<libc::c_int>() {
            return Err(changed());
        }
        let start_time = process_start(credential.pid).ok();
        if pidfd.is_none() && start_time.is_none() {
            return Err(changed());
        }
        let binding = Self {
            socket,
            uid: credential.uid,
            pid: credential.pid,
            pidfd,
            start_time,
        };
        binding.check(path, &stream)?;
        Ok((binding, stream))
    }

    pub(crate) fn check(&self, path: &Path, stream: &UnixStream) -> Result<()> {
        let credential = credentials(stream)?;
        if SocketIdentity::read(path)? != self.socket
            || credential.uid != self.uid
            || credential.pid != self.pid
        {
            return Err(changed());
        }
        let alive = if let Some(pidfd) = &self.pidfd {
            let mut poll = libc::pollfd {
                fd: pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            unsafe { libc::poll(&mut poll, 1, 0) == 0 }
        } else {
            process_start(self.pid).ok() == self.start_time
        };
        if !alive {
            return Err(changed());
        }
        Ok(())
    }

    pub(crate) fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.socket.device,
            self.socket.inode,
            self.uid,
            self.pid,
            self.start_time.unwrap_or(0)
        )
    }
    pub(crate) fn root_owned(&self) -> bool {
        self.uid == 0 && self.socket.owner == 0
    }
}
