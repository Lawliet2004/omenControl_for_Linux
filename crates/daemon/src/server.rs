//! Socket server: accepts NDJSON requests, authorizes write ops via peer
//! credentials + polkit (`pkcheck`), and dispatches to the hardware service.

use crate::service::HardwareService;
use omen_core::OmenError;
use omen_ipc::protocol::{Op, Request, Response};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_REQUEST: usize = 65_536;
const MAX_CLIENTS: usize = 16;
const MAX_CLIENTS_PER_UID: usize = 4;
const MAX_SESSION: Duration = Duration::from_secs(60);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTS_PER_SESSION: usize = 64;
const EVICTION_WAIT: Duration = Duration::from_millis(250);

struct Session {
    id: u64,
    uid: u32,
    socket: UnixStream,
    busy: bool,
    evicted: bool,
}

#[derive(Default)]
struct Sessions {
    entries: Mutex<Vec<Session>>,
    changed: Condvar,
    next_id: std::sync::atomic::AtomicU64,
}

struct SessionPermit {
    sessions: Arc<Sessions>,
    id: u64,
}

impl Sessions {
    // Idle sessions can be displaced by a newly arriving client. A UID at its
    // quota may displace only its own sessions; another user retains capacity.
    fn admit(
        self: &Arc<Self>,
        stream: &UnixStream,
        uid: u32,
    ) -> std::io::Result<Option<SessionPermit>> {
        let deadline = Instant::now() + EVICTION_WAIT;
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let own = entries.iter().filter(|entry| entry.uid == uid).count();
            if own < MAX_CLIENTS_PER_UID && entries.len() < MAX_CLIENTS {
                let id = self.next_id.fetch_add(1, Ordering::Relaxed);
                entries.push(Session {
                    id,
                    uid,
                    socket: stream.try_clone()?,
                    busy: false,
                    evicted: false,
                });
                return Ok(Some(SessionPermit {
                    sessions: self.clone(),
                    id,
                }));
            }
            if let Some(victim) = entries.iter_mut().find(|entry| {
                !entry.busy && !entry.evicted && (own < MAX_CLIENTS_PER_UID || entry.uid == uid)
            }) {
                victim.evicted = true;
                let _ = victim.socket.shutdown(std::net::Shutdown::Both);
            } else if !entries
                .iter()
                .any(|entry| entry.evicted && (own < MAX_CLIENTS_PER_UID || entry.uid == uid))
            {
                return Ok(None);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(None);
            };
            let (guard, result) = self
                .changed
                .wait_timeout(entries, remaining)
                .unwrap_or_else(|e| e.into_inner());
            entries = guard;
            if result.timed_out() {
                return Ok(None);
            }
        }
    }
}

impl SessionPermit {
    // Synchronize eviction with dispatch so an active hardware operation is
    // never displaced midway through a request.
    fn set_busy(&self, busy: bool) -> bool {
        let mut entries = self
            .sessions
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = entries.iter_mut().find(|entry| entry.id == self.id) {
            if entry.evicted {
                return false;
            }
            entry.busy = busy;
            return true;
        }
        false
    }
}

impl Drop for SessionPermit {
    fn drop(&mut self) {
        let mut entries = self
            .sessions
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        entries.retain(|entry| entry.id != self.id);
        self.sessions.changed.notify_all();
    }
}

// BufRead::read_until may perform many reads. Recompute the remaining session
// time on every underlying read so a peer cannot keep a slot by trickling bytes.
struct DeadlineReader {
    socket: UnixStream,
    deadline: Instant,
}

impl Read for DeadlineReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "session expired"))?;
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "session expired",
            ));
        }
        self.socket
            .set_read_timeout(Some(remaining.min(IO_TIMEOUT)))?;
        self.socket.read(buf)
    }
}

pub struct Server {
    listener: UnixListener,
    socket_path: std::path::PathBuf,
    service: Arc<HardwareService>,
}

impl Server {
    /// Production uses a world-accessible socket so desktop users can connect;
    /// peer credentials and polkit authorize each write. Mock sockets are
    /// owner-only because mock mode deliberately bypasses authorization.
    pub fn bind(path: &Path, service: Arc<HardwareService>) -> std::io::Result<Server> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if !meta.file_type().is_socket() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "socket path is not a socket",
                ));
            }
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "another daemon owns this socket",
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(if service.mock { 0o600 } else { 0o666 });
        std::fs::set_permissions(path, perms)?;
        Ok(Server {
            listener,
            socket_path: path.to_path_buf(),
            service,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Accept loop. Runs on the calling thread.
    pub fn run(&self) -> std::io::Result<()> {
        let sessions = Arc::new(Sessions::default());
        loop {
            if self.service.shutdown_requested() || crate::TERMINATE.load(Ordering::SeqCst) {
                return Ok(());
            }
            let (stream, _addr) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    continue;
                }
                Err(e) => return Err(e),
            };
            let peer = match PeerCredentials::of(&stream) {
                Ok(peer) => peer,
                Err(_) => continue,
            };
            let permit = match sessions.admit(&stream, peer.uid) {
                Ok(Some(permit)) => permit,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("cannot track client session: {e}");
                    continue;
                }
            };
            let service = self.service.clone();
            std::thread::spawn(move || {
                let _ =
                    handle_connection_tracked(stream, peer, service, Some(&permit), MAX_SESSION);
            });
        }
    }
}

#[cfg(test)]
fn handle_connection(stream: UnixStream, service: Arc<HardwareService>) -> std::io::Result<()> {
    let peer = PeerCredentials::of(&stream)?;
    handle_connection_tracked(stream, peer, service, None, MAX_SESSION)
}

fn handle_connection_tracked(
    stream: UnixStream,
    peer: PeerCredentials,
    service: Arc<HardwareService>,
    permit: Option<&SessionPermit>,
    session_duration: Duration,
) -> std::io::Result<()> {
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let deadline = Instant::now() + session_duration;
    let mut reader = BufReader::new(DeadlineReader {
        socket: stream.try_clone()?,
        deadline,
    });
    let mut writer = stream;
    let mut requests = 0;
    loop {
        let mut bytes = Vec::new();
        let n = reader
            .by_ref()
            .take((MAX_REQUEST + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if bytes.len() > MAX_REQUEST || !bytes.ends_with(b"\n") {
            let response = Response::err(
                0,
                OmenError::InvalidArgument("request exceeds 64 KiB or is unterminated".into()),
            );
            writeln!(writer, "{}", serde_json::to_string(&response)?)?;
            break;
        }
        // Reject non-UTF-8 frames before parsing, authorization, or dispatch.
        // Lossy decoding would let invalid bytes inside otherwise valid JSON
        // reach a request that passed framing.
        let line = match std::str::from_utf8(&bytes) {
            Ok(line) => line,
            Err(_) => {
                let response = Response::err(
                    0,
                    OmenError::InvalidArgument(
                        "request frame is not valid UTF-8; reconnect and resend".into(),
                    ),
                );
                writeln!(writer, "{}", serde_json::to_string(&response)?)?;
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if Instant::now() >= deadline || requests >= MAX_REQUESTS_PER_SESSION {
            break;
        }
        if permit.is_some_and(|permit| !permit.set_busy(true)) {
            break;
        }
        requests += 1;
        let resp: Response = match serde_json::from_str::<Request>(line) {
            Ok(req) => dispatch(req, &peer, &service),
            Err(e) => Response::err(
                0,
                OmenError::InvalidArgument(format!("bad request line: {e}")),
            ),
        };
        let mut out = serde_json::to_string(&resp)
            .map_err(|e| OmenError::Internal(format!("encode response: {e}")))
            .unwrap_or_else(|e| serde_json::to_string(&Response::err(0, e)).unwrap());
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
        if permit.is_some_and(|permit| !permit.set_busy(false)) {
            break;
        }
    }
    Ok(())
}

/// Peer identity from SO_PEERCRED.
pub struct PeerCredentials {
    pub uid: u32,
    pub pid: i32,
    pidfd: Option<std::fs::File>,
}

impl PeerCredentials {
    fn of(stream: &UnixStream) -> std::io::Result<PeerCredentials> {
        use std::os::fd::AsRawFd;
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let ret = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut (std::mem::size_of::<libc::ucred>() as libc::socklen_t),
            )
        };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut fd: libc::c_int = -1;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERPIDFD,
                &mut fd as *mut _ as *mut libc::c_void,
                &mut (std::mem::size_of::<libc::c_int>() as libc::socklen_t),
            )
        };
        use std::os::fd::FromRawFd;
        let pidfd = if result == 0 && fd >= 0 {
            Some(unsafe { std::fs::File::from_raw_fd(fd) })
        } else {
            None
        };
        Ok(PeerCredentials {
            uid: cred.uid,
            pid: cred.pid,
            pidfd,
        })
    }
    fn managed_scope(
        &self,
        token: &str,
    ) -> omen_core::Result<omen_hardware::game_scope::GameScope> {
        use std::os::fd::AsRawFd;
        let alive = || {
            let file = self.pidfd.as_ref().ok_or_else(|| {
                OmenError::Unsupported(
                    "managed sessions require kernel socket-peer pidfd support".into(),
                )
            })?;
            let mut pfd = libc::pollfd {
                fd: file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut pfd, 1, 0) } != 0 || pfd.revents != 0 {
                return Err(OmenError::PermissionDenied(
                    "the original socket peer has exited".into(),
                ));
            }
            Ok(())
        };
        alive()?;
        let scope = omen_hardware::game_scope::GameScope::for_process(
            self.uid,
            self.pid
                .try_into()
                .map_err(|_| OmenError::PermissionDenied("invalid peer process".into()))?,
            token,
        )?;
        alive()?;
        Ok(scope)
    }
}

/// polkit action ids (packaging/polkit/com.omenlinux.policy).
pub const ACTION_CONTROL: &str = "com.omenlinux.control";
pub const ACTION_ADMIN: &str = "com.omenlinux.admin";

fn action_for(op: &Op) -> &'static str {
    if matches!(
        op,
        Op::Shutdown | Op::SetConfig { .. } | Op::SetServiceSettings { .. }
    ) {
        ACTION_ADMIN
    } else {
        ACTION_CONTROL
    }
}

/// Authorize a write op: root always passes; otherwise polkit via `pkcheck`
/// against the connecting process. When polkit is unavailable we fail
/// closed (write denied) — never silently open. Mock mode (--mock) is the
/// documented exception: it exists for development and testing without any
/// real hardware, so authorization is bypassed there and loudly logged.
pub fn authorize(peer: &PeerCredentials, op: &Op, mock: bool) -> Result<(), OmenError> {
    if mock {
        log::warn!(
            "mock mode: skipping authorization for '{}' from uid {}",
            op.name(),
            peer.uid
        );
        return Ok(());
    }
    if peer.uid == 0 {
        return Ok(());
    }
    let action = action_for(op);
    // polkit expects pid,starttime; read starttime from /proc/<pid>/stat.
    let starttime = std::fs::read_to_string(format!("/proc/{}/stat", peer.pid))
        .ok()
        .and_then(|stat| {
            // Field 22 (1-based) after the comm field in parens.
            let close = stat.rfind(')')?;
            let rest = stat[close + 1..].trim_start();
            let mut it = rest.split_whitespace();
            // skip state(3) ppid(4) pgrp(5) session(6) tty(7) tpgid(8) flags(9)
            // minflt(10) cminflt(11) majflt(12) cmajflt(13) utime(14) stime(15)
            // cutime(16) cstime(17) priority(18) nice(19) num_threads(20)
            // itrealvalue(21) → starttime(22)
            for _ in 3..=21 {
                it.next()?;
            }
            it.next().map(|s| s.to_string())
        });
    let Some(starttime) = starttime else {
        return Err(OmenError::PermissionDenied(
            "cannot identify peer process start time for polkit".into(),
        ));
    };
    let st = omen_hardware::command::output(
        std::process::Command::new("/usr/bin/pkcheck").args([
            "--process",
            &format!("{},{},{}", peer.pid, starttime, peer.uid),
            "--action-id",
            action,
            "-u", // allow user interaction (polkit agent prompt)
        ]),
        std::time::Duration::from_secs(60),
    );
    match st {
        Ok(s) if s.status.success() => Ok(()),
        Ok(_) => Err(OmenError::PermissionDenied(format!(
            "polkit denied '{action}' (authenticate as admin or run as root)"
        ))),
        Err(e) => Err(OmenError::PermissionDenied(format!(
            "polkit unavailable ({e}); write operations require root or a working polkit"
        ))),
    }
}

fn dispatch(req: Request, peer: &PeerCredentials, service: &HardwareService) -> Response {
    if req.v != omen_ipc::protocol::PROTOCOL_VERSION {
        return Response::err(
            req.id,
            OmenError::Unsupported(format!("protocol version {} is unsupported", req.v)),
        );
    }
    let op_name = req.op.name();
    if req.op.is_write() {
        if let Err(e) = authorize(peer, &req.op, service.mock) {
            log::warn!(
                "denied op '{op_name}' for uid {} pid {}: {e}",
                peer.uid,
                peer.pid
            );
            return Response::err(req.id, e);
        }
    }
    let started = std::time::Instant::now();
    let resp = match req.op {
        Op::StartManagedGame {
            token,
            game_id,
            profile,
        } => match peer
            .managed_scope(&token)
            .and_then(|scope| service.start_managed_game(peer.uid, token, game_id, profile, scope))
        {
            Ok(session) => Response::ok(req.id, session),
            Err(error) => Response::err(req.id, error),
        },
        Op::GetGameSessions => Response::ok(req.id, service.game_sessions(peer.uid)),
        Op::ReleaseGameSession { token } => match service.release_game_session(peer.uid, &token) {
            Ok(()) => Response::ok(req.id, "Profile hold released; the game is not stopped"),
            Err(error) => Response::err(req.id, error),
        },
        op => service.handle(req.id, op),
    };
    log::info!(
        "op '{op_name}' by uid {} pid {} → {} in {:?}",
        peer.uid,
        peer.pid,
        if resp.ok { "ok" } else { "err" },
        started.elapsed()
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::config::OmenConfig;
    use omen_hardware::mock::{MockBackend, MockModel};

    #[test]
    fn scoped_service_settings_require_admin_action() {
        let op = Op::SetServiceSettings {
            settings: omen_core::settings::ServiceSettings::from(&OmenConfig::default()),
        };
        assert!(op.is_write());
        assert_eq!(action_for(&op), ACTION_ADMIN);
    }

    fn service() -> Arc<HardwareService> {
        HardwareService::new(
            Arc::new(MockBackend::new(MockModel::VictusFb)),
            true,
            OmenConfig::default(),
        )
    }

    #[test]
    fn production_connection_handles_garbage_and_recovers() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || handle_connection(server, service()).unwrap());
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        writeln!(client, "garbage").unwrap();
        writeln!(
            client,
            "{}",
            serde_json::to_string(&Request {
                v: omen_ipc::protocol::PROTOCOL_VERSION,
                id: 42,
                op: Op::Ping
            })
            .unwrap()
        )
        .unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(!serde_json::from_str::<Response>(&line).unwrap().ok);
        line.clear();
        reader.read_line(&mut line).unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(response.ok);
        assert_eq!(response.id, 42);
        drop(reader);
        thread.join().unwrap();
    }

    #[test]
    fn non_utf8_frame_is_rejected_without_dispatch() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || handle_connection(server, service()).unwrap());
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        // A structurally valid request whose only string carries an invalid
        // byte. Lossy decoding turned it into U+FFFD and dispatched it.
        let mut frame = b"{\"v\":1,\"id\":7,\"op\":{\"op\":\"Ping\",\"note\":\"".to_vec();
        frame.extend_from_slice(&[0xff, 0xfe]);
        frame.extend_from_slice(b"\"}}\n");
        client.write_all(&frame).unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(!response.ok);
        assert_eq!(response.id, 0);
        assert!(response
            .error
            .as_ref()
            .is_some_and(|error| error.to_string().contains("not valid UTF-8")));
        // The connection is closed after the framing error: no dispatch, no reply.
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0);
        drop(reader);
        thread.join().unwrap();
    }

    #[test]
    fn non_utf8_frame_never_reaches_a_mutating_operation() {
        let service = service();
        let before = service.handle(1, Op::GetFanState);
        let (mut client, server) = UnixStream::pair().unwrap();
        let worker = service.clone();
        let thread = std::thread::spawn(move || handle_connection(server, worker).unwrap());
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut frame =
            b"{\"v\":1,\"id\":9,\"op\":{\"op\":\"SetFanMode\",\"mode\":\"Max\",\"why\":\"".to_vec();
        frame.extend_from_slice(&[0x80]);
        frame.extend_from_slice(b"\"}}\n");
        client.write_all(&frame).unwrap();
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        assert!(!serde_json::from_str::<Response>(&line).unwrap().ok);
        thread.join().unwrap();
        assert_eq!(
            service.handle(2, Op::GetFanState).data,
            before.data,
            "a rejected frame must leave fan policy untouched"
        );
    }

    #[test]
    fn oversized_request_is_rejected_without_dispatch() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || handle_connection(server, service()).unwrap());
        client.write_all(&vec![b'x'; MAX_REQUEST + 1]).unwrap();
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        assert!(!serde_json::from_str::<Response>(&line).unwrap().ok);
        thread.join().unwrap();
    }

    #[test]
    fn saturated_idle_clients_do_not_block_new_requests() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("socket");
        let service = service();
        let server = Server::bind(&path, service.clone()).unwrap();
        let worker = std::thread::spawn(move || server.run().unwrap());
        let mut idle = Vec::new();
        for _ in 0..MAX_CLIENTS {
            idle.push(UnixStream::connect(&path).unwrap());
        }
        let client = omen_ipc::client::IpcClient::new(&path);
        let response = client.call(Op::Ping).unwrap();
        assert!(response.ok);
        service.restore_safe_state();
        worker.join().unwrap();
        drop(idle);
    }

    #[test]
    fn trickling_bytes_cannot_extend_session_deadline() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let peer = PeerCredentials::of(&server).unwrap();
        let worker = std::thread::spawn(move || {
            handle_connection_tracked(server, peer, service(), None, Duration::from_millis(180))
        });
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(350) {
            if client.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let result = worker.join().unwrap();
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn active_requests_are_not_evicted_and_other_users_keep_capacity() {
        let sessions = Arc::new(Sessions::default());
        let mut peers = Vec::new();
        let mut permits = Vec::new();
        for _ in 0..MAX_CLIENTS_PER_UID {
            let (client, server) = UnixStream::pair().unwrap();
            let permit = sessions.admit(&server, 1000).unwrap().unwrap();
            assert!(permit.set_busy(true));
            peers.push((client, server));
            permits.push(permit);
        }
        let (_client, server) = UnixStream::pair().unwrap();
        assert!(sessions.admit(&server, 1000).unwrap().is_none());
        assert!(sessions.admit(&server, 1001).unwrap().is_some());
        assert_eq!(sessions.entries.lock().unwrap().len(), MAX_CLIENTS_PER_UID);
    }

    #[test]
    fn binding_never_unlinks_regular_files_or_active_sockets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("socket");
        std::fs::write(&path, "preserve").unwrap();
        assert!(Server::bind(&path, service()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "preserve");
        std::fs::remove_file(&path).unwrap();
        let _listener = UnixListener::bind(&path).unwrap();
        assert!(Server::bind(&path, service()).is_err());
    }

    #[test]
    fn unidentified_peer_fails_closed_before_polkit() {
        let peer = PeerCredentials {
            uid: 1000,
            pid: -1,
            pidfd: None,
        };
        assert!(matches!(
            authorize(&peer, &Op::Shutdown, false),
            Err(OmenError::PermissionDenied(_))
        ));
    }
}
