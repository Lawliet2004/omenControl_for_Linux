use super::{Command as Operation, Reply, Request, MAX_FRAME};
use crate::fan_guard::files::Files;
use omen_core::fans::{FanMode, FanSupervisorStatus};
use omen_core::{OmenError, Result};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Session {
    stream: UnixStream,
    child: Child,
    sequence: u64,
    status: FanSupervisorStatus,
}

fn error(message: impl Into<String>) -> OmenError {
    OmenError::StateMismatch(message.into())
}

fn reply(stream: &mut UnixStream) -> Result<Reply> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| error("fan guardian reply deadline expired"))?;
        stream.set_read_timeout(Some(remaining))?;
        let mut byte = [0];
        if stream.read(&mut byte)? == 0 {
            return Err(error("fan guardian channel closed"));
        }
        if byte[0] == b'\n' {
            return serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()));
        }
        bytes.push(byte[0]);
        if bytes.len() > MAX_FRAME {
            return Err(error("oversized fan guardian reply"));
        }
    }
}

impl Session {
    fn spawn(files: &Files) -> Result<Self> {
        let (mut parent, child_stream) = UnixStream::pair()?;
        parent.set_write_timeout(Some(Duration::from_secs(2)))?;
        let socket = unsafe { libc::fcntl(child_stream.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
        if socket < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: fcntl returned a new owned descriptor.
        let socket = unsafe { OwnedFd::from_raw_fd(socket) };
        let file_copies = files.duplicated()?;
        let mut mapping = vec![(socket.as_raw_fd(), 3)];
        mapping.extend(
            file_copies
                .iter()
                .enumerate()
                .map(|(i, fd)| (fd.as_raw_fd(), i as i32 + 4)),
        );
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--internal-fan-guard")
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .process_group(0);
        // SAFETY: after fork only async-signal-safe dup2 and errno access are
        // used. Sources were moved above all fixed target descriptors.
        unsafe {
            command.pre_exec(move || {
                for (source, target) in &mapping {
                    if libc::dup2(*source, *target) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        drop(child_stream);
        let hello = match reply(&mut parent) {
            Ok(hello) if hello.seq == 0 && hello.ok && hello.status.pid == Some(child.id()) => {
                hello
            }
            other => {
                let _ = parent.shutdown(std::net::Shutdown::Both);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error(format!(
                    "fan guardian initialization failed: {}",
                    other.err().map(|e| e.to_string()).unwrap_or_default()
                )));
            }
        };
        Ok(Self {
            stream: parent,
            child,
            sequence: 0,
            status: hello.status,
        })
    }
    fn request(&mut self, command: Operation) -> Result<()> {
        if self.child.try_wait()?.is_some() {
            self.status.revoked = true;
            return Err(error("fan guardian exited"));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("fan guardian sequence exhausted"))?;
        let request = Request {
            seq: self.sequence,
            command,
        };
        let mut bytes = serde_json::to_vec(&request).map_err(|e| error(e.to_string()))?;
        bytes.push(b'\n');
        self.stream.write_all(&bytes)?;
        let result = reply(&mut self.stream)?;
        if result.seq != self.sequence {
            return Err(error("fan guardian reply sequence mismatch"));
        }
        self.status = result.status;
        if !result.ok {
            return Err(error(
                result
                    .error
                    .unwrap_or_else(|| "fan guardian refused operation".into()),
            ));
        }
        Ok(())
    }
    fn close(&mut self) -> Result<()> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        let deadline = Instant::now() + Duration::from_millis(3500);
        while self.child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                self.child.kill()?;
                self.child.wait()?;
                return Err(error(
                    "fan guardian did not finish recovery before replacement",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

pub struct Client {
    directory: PathBuf,
    state: Mutex<(Option<Files>, Option<Session>)>,
}

impl Client {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            state: Mutex::new((None, None)),
        }
    }
    fn ensure(&self, state: &mut (Option<Files>, Option<Session>)) -> Result<()> {
        if state.0.is_none() {
            state.0 = Some(Files::open(&self.directory)?);
        }
        let replace = match &mut state.1 {
            Some(session) => session.request(Operation::Status).is_err() || session.status.revoked,
            None => true,
        };
        if replace {
            if let Some(mut session) = state.1.take() {
                session.close()?;
            }
            state.1 = Some(Session::spawn(
                state.0.as_ref().expect("pinned files initialized"),
            )?);
        }
        Ok(())
    }
    pub fn begin(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure(&mut state)
    }
    pub fn apply(&self, mode: FanMode) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if mode == FanMode::Auto {
            self.ensure(&mut state)?;
        }
        let session = state
            .1
            .as_mut()
            .ok_or_else(|| error("no explicitly armed fan guardian"))?;
        session.request(Operation::Apply { mode })
    }
    pub fn pulse(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .1
            .as_mut()
            .ok_or_else(|| error("fan guardian is unavailable"))?
            .request(Operation::Pulse)
    }
    pub fn status(&self) -> Option<FanSupervisorStatus> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .as_ref()
            .map(|session| session.status.clone())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut session) = self
            .state
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .take()
        {
            let _ = session.close();
        }
    }
}
