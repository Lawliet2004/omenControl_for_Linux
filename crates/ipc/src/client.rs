//! Unix-socket NDJSON client.

use crate::protocol::{Request, Response};
use omen_core::OmenError;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct IpcClient {
    path: PathBuf,
    next_id: Arc<AtomicU64>,
    peer: Option<Arc<crate::peer::PeerBinding>>,
    observed_info: Option<crate::protocol::DaemonInfo>,
}

impl IpcClient {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        IpcClient {
            path: path.into(),
            next_id: Arc::new(AtomicU64::new(1)),
            peer: None,
            observed_info: None,
        }
    }

    /// Clone pointing at the same socket with an independent id counter.
    pub fn share(&self) -> IpcClient {
        IpcClient {
            path: self.path.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
            peer: self.peer.clone(),
            observed_info: self.observed_info.clone(),
        }
    }

    /// Connect to the default socket.
    pub fn default_path() -> Self {
        IpcClient::new(crate::default_socket_path())
    }

    /// Pin the socket inode and live server process before observing its data.
    /// Ordinary CLI clients remain unbound unless their caller requests this.
    pub fn pin_peer(&self) -> omen_core::Result<Self> {
        let (peer, _) = crate::peer::PeerBinding::connect(&self.path)?;
        Ok(Self {
            path: self.path.clone(),
            next_id: self.next_id.clone(),
            peer: Some(Arc::new(peer)),
            observed_info: None,
        })
    }

    pub fn bind_observed_info(
        mut self,
        info: crate::protocol::DaemonInfo,
    ) -> omen_core::Result<Self> {
        if self.peer.is_none() || info.protocol_version != crate::protocol::PROTOCOL_VERSION {
            return Err(OmenError::DaemonUnavailable(
                "service identity or protocol is unverified".into(),
            ));
        }
        self.observed_info = Some(info);
        Ok(self)
    }

    pub fn peer_key(&self) -> Option<String> {
        self.peer.as_ref().map(|peer| peer.key())
    }
    pub fn root_owned_peer(&self) -> bool {
        self.peer.as_ref().is_some_and(|peer| peer.root_owned())
    }

    pub fn is_daemon_alive(&self) -> bool {
        self.path.exists()
            && UnixStream::connect(&self.path)
                .map(|_| true)
                .unwrap_or(false)
    }

    /// Send one request, read one response. Each call opens a fresh
    /// connection: simple, avoids partial-read state across commands, and
    /// matches the CLI/GUI usage pattern.
    pub fn call(&self, op: crate::protocol::Op) -> omen_core::Result<Response> {
        let stream = UnixStream::connect(&self.path).map_err(|e| {
            OmenError::DaemonUnavailable(format!(
                "cannot connect to {}: {e} (is omen-daemon running?)",
                self.path.display()
            ))
        })?;
        if let Some(peer) = &self.peer {
            peer.check(&self.path, &stream)?;
        }
        let mut reader = BufReader::new(DeadlineReader {
            socket: stream,
            deadline: Instant::now() + read_budget(&op),
        });
        if op.is_write() {
            if let Some(expected) = &self.observed_info {
                let observed: crate::protocol::DaemonInfo = self
                    .exchange(&mut reader, crate::protocol::Op::GetInfo)?
                    .decode()?;
                if &observed != expected {
                    return Err(OmenError::DaemonUnavailable("service identity changed; review a fresh report before retrying this action".into()));
                }
            }
        }
        self.exchange(&mut reader, op)
    }

    fn exchange(
        &self,
        reader: &mut BufReader<DeadlineReader>,
        op: crate::protocol::Op,
    ) -> omen_core::Result<Response> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = Request {
            v: crate::protocol::PROTOCOL_VERSION,
            id,
            op,
        };
        let mut line = serde_json::to_string(&req)
            .map_err(|e| OmenError::Internal(format!("encode request: {e}")))?;
        line.push('\n');

        let budget = read_budget(&req.op);
        let deadline = Instant::now() + budget;
        reader.get_mut().deadline = deadline;
        let stream = &mut reader.get_mut().socket;
        // Absolute end-to-end budget: the deadline is fixed before the first
        // write and every read is re-armed with only the time that remains.
        // A trickling or silent peer therefore cannot extend a call past this
        // bound by sending one byte per per-read timeout.
        stream
            .set_read_timeout(Some(budget))
            .map_err(|e| OmenError::Io(e.to_string()))?;
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .map_err(|e| OmenError::Io(e.to_string()))?;
        stream
            .write_all(line.as_bytes())
            .map_err(|e| OmenError::Io(format!("send: {e}")))?;
        let _ = stream.flush();

        for _ in 0..8 {
            let mut bytes = Vec::new();
            let n = reader
                .by_ref()
                .take((MAX_REPLY + 1) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(|e| {
                    if is_timeout(&e) {
                        timeout_error()
                    } else {
                        OmenError::Io(format!("recv: {e}"))
                    }
                })?;
            if n == 0 {
                break;
            }
            if n > MAX_REPLY || !bytes.ends_with(b"\n") {
                return Err(OmenError::InvalidArgument(
                    "oversized or unterminated daemon response".into(),
                ));
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                OmenError::InvalidArgument("daemon response is not valid UTF-8".into())
            })?;
            if text.trim().is_empty() {
                continue;
            }
            let resp: Response = serde_json::from_str(text)
                .map_err(|e| OmenError::Internal(format!("decode response line: {e}")))?;
            if resp.id == id {
                return Ok(resp);
            }
            // Out-of-order lines are logged and skipped; ids correlate.
            log::debug!("skipping response with foreign id {}", resp.id);
        }
        if deadline.checked_duration_since(Instant::now()).is_none() {
            return Err(timeout_error());
        }
        Err(OmenError::DaemonUnavailable(
            "daemon closed the connection without answering".into(),
        ))
    }

    /// Typed convenience: call and decode in one step.
    pub fn call_decode<T: for<'de> serde::Deserialize<'de>>(
        &self,
        op: crate::protocol::Op,
    ) -> omen_core::Result<T> {
        let resp = self.call(op)?;
        resp.decode()
    }

    pub fn socket_path(&self) -> &Path {
        &self.path
    }
}

/// Interactive authorization is the only operation allowed a long reply
/// budget; everything else, including ordinary writes, answers promptly.
const AUTHORIZE_BUDGET: std::time::Duration = std::time::Duration::from_secs(65);
const DEFAULT_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);
/// Largest accepted daemon reply line, including the trailing newline.
const MAX_REPLY: usize = 1_048_576;

// Shortens the end-to-end budget for tests that exercise the deadline itself.
// Only the calling thread sees the override, so it cannot leak into other
// tests that share the same binary.
thread_local! {
    static TEST_BUDGET: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

fn read_budget(op: &crate::protocol::Op) -> std::time::Duration {
    if let Some(override_budget) = TEST_BUDGET.with(|budget| budget.get()) {
        return override_budget;
    }
    if op.is_interactive_authorization() {
        AUTHORIZE_BUDGET
    } else {
        DEFAULT_BUDGET
    }
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// Read one newline-terminated line straight from the socket, re-arming the
/// read timeout with the time left before `deadline` before every chunk.
///
/// `BufRead::read_line` cannot do this: when its buffer is empty it fills the
/// buffer with a single `read`, and a trickling peer keeps resetting a per-read
/// timeout without ever completing a line. Reading the socket directly makes
/// the deadline absolute over the whole reply, including skipped frames.
struct DeadlineReader {
    socket: UnixStream,
    deadline: Instant,
}

impl Read for DeadlineReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "deadline exceeded")
            })?;
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "deadline exceeded",
            ));
        }
        self.socket.set_read_timeout(Some(remaining))?;
        self.socket.read(buf)
    }
}

fn timeout_error() -> OmenError {
    OmenError::DaemonUnavailable(
        "daemon did not answer within the request deadline; the request may or may not have been applied".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Op;
    use omen_core::sensors::SensorSample;
    use std::io::BufRead;

    #[test]
    fn missing_socket_is_daemon_unavailable() {
        let c = IpcClient::new("/nonexistent/omen.sock");
        assert!(!c.is_daemon_alive());
        let err = c.call(Op::Ping).unwrap_err();
        assert!(matches!(err, OmenError::DaemonUnavailable(_)));
        let _sample: Option<SensorSample> = None; // type check import
    }

    struct Server {
        stop: Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
        info: Arc<std::sync::Mutex<crate::protocol::DaemonInfo>>,
        writes: Arc<AtomicU64>,
    }

    impl Server {
        fn new(path: &Path) -> Self {
            use std::os::unix::net::UnixListener;
            let listener = UnixListener::bind(path).unwrap();
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let info = Arc::new(std::sync::Mutex::new(crate::protocol::DaemonInfo {
                protocol_version: crate::protocol::PROTOCOL_VERSION,
                daemon_version: "test".into(),
                machine: Default::default(),
                mock: true,
                started_unix: 1,
            }));
            let writes = Arc::new(AtomicU64::new(0));
            let (done, observed, count) = (stop.clone(), info.clone(), writes.clone());
            let worker = std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let Ok((stream, _)) = listener.accept() else {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_millis(100)))
                        .unwrap();
                    let mut reader = BufReader::new(stream);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        let request: Request = serde_json::from_str(&line).unwrap();
                        if request.op.is_write() {
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                        let response = if matches!(request.op, Op::GetInfo) {
                            Response::ok(request.id, observed.lock().unwrap().clone())
                        } else {
                            Response::ok(request.id, ())
                        };
                        let mut encoded = serde_json::to_vec(&response).unwrap();
                        encoded.push(b'\n');
                        if reader.get_mut().write_all(&encoded).is_err() {
                            break;
                        }
                    }
                }
            });
            Self {
                stop,
                worker: Some(worker),
                info,
                writes,
            }
        }
        fn bound(&self, path: &Path) -> IpcClient {
            let client = IpcClient::new(path).pin_peer().unwrap();
            let info = client.call_decode(Op::GetInfo).unwrap();
            client.bind_observed_info(info).unwrap()
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.worker.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn queued_clones_refuse_a_replaced_socket_even_in_the_same_process() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service");
        let original = Server::new(&path);
        let queued = original.bound(&path).share();
        std::fs::remove_file(&path).unwrap();
        let replacement = Server::new(&path);
        assert!(queued
            .call(Op::SetProfile {
                profile: omen_core::profile::ProfileKind::Balanced
            })
            .is_err());
        assert_eq!(original.writes.load(Ordering::SeqCst), 0);
        assert_eq!(replacement.writes.load(Ordering::SeqCst), 0);
        let current = replacement.bound(&path);
        current
            .call(Op::SetProfile {
                profile: omen_core::profile::ProfileKind::Balanced,
            })
            .unwrap();
        assert_eq!(replacement.writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn observed_identity_is_rechecked_on_the_write_connection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service");
        let server = Server::new(&path);
        let queued = server.bound(&path);
        server.info.lock().unwrap().mock = false;
        let error = queued.call(Op::Shutdown).unwrap_err();
        assert!(error.to_string().contains("identity changed"));
        assert_eq!(server.writes.load(Ordering::SeqCst), 0);
        // A freshly observed identity may be used, and an ordinary unbound CLI
        // client still sends its original single request.
        server.bound(&path).call(Op::Shutdown).unwrap();
        IpcClient::new(path).call(Op::Shutdown).unwrap();
        assert_eq!(server.writes.load(Ordering::SeqCst), 2);
    }

    /// A peer that trickles one byte per read must not extend the call past
    /// the absolute deadline: the per-read timeout is re-armed with only the
    /// remaining budget, so a silent or slow peer is bounded end to end.
    #[test]
    fn trickling_reply_cannot_extend_the_end_to_end_deadline() {
        use std::io::Write as _;
        use std::os::unix::net::UnixListener;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service");
        let listener = UnixListener::bind(&path).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf).unwrap();
            // One byte every 300 ms: the full 30-byte reply would take ~9 s,
            // so only an absolute deadline can bound the call.
            for byte in b"{\"v\":1,\"id\":1,\"ok\":true}\n" {
                if stream.write_all(&[*byte]).is_err() {
                    return;
                }
                let _ = stream.flush();
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
        TEST_BUDGET.with(|budget| budget.set(Some(std::time::Duration::from_millis(600))));
        let started = Instant::now();
        let error = IpcClient::new(&path).call(Op::Ping).unwrap_err();
        let elapsed = started.elapsed();
        TEST_BUDGET.with(|budget| budget.set(None));
        peer.join().unwrap();
        assert!(
            elapsed < std::time::Duration::from_millis(1_500),
            "deadline must bound the whole reply, took {elapsed:?}"
        );
        let message = error.to_string();
        assert!(message.contains("deadline"), "{message}");
        assert!(
            message.contains("may or may not have been applied"),
            "{message}"
        );
    }

    #[test]
    fn incompatible_protocol_cannot_be_bound_for_gui_actions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service");
        let server = Server::new(&path);
        server.info.lock().unwrap().protocol_version += 1;
        let client = IpcClient::new(path).pin_peer().unwrap();
        let info = client.call_decode(Op::GetInfo).unwrap();
        assert!(client.bind_observed_info(info).is_err());
        assert_eq!(server.writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn response_framing_rejections_and_recovery() {
        use std::io::Write as _;
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();

        // 1. Truncated EOF (no trailing newline)
        let path1 = dir.path().join("truncated.sock");
        let listener1 = UnixListener::bind(&path1).unwrap();
        let peer1 = std::thread::spawn(move || {
            let (mut stream, _) = listener1.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = stream.write_all(b"{\"v\":1,\"id\":1,\"ok\":true}"); // no newline
        });
        let client1 = IpcClient::new(&path1);
        let err = client1.call(Op::Ping).unwrap_err();
        assert!(matches!(err, OmenError::InvalidArgument(_)));
        peer1.join().unwrap();

        // 2. Blank lines and foreign id before matching response
        let path2 = dir.path().join("foreign.sock");
        let listener2 = UnixListener::bind(&path2).unwrap();
        let peer2 = std::thread::spawn(move || {
            let (mut stream, _) = listener2.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = stream.write_all(
                b"   \n\n{\"v\":1,\"id\":999,\"ok\":true}\n{\"v\":1,\"id\":1,\"ok\":true}\n",
            );
        });
        let client2 = IpcClient::new(&path2);
        let resp = client2.call(Op::Ping).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.id, 1);
        peer2.join().unwrap();

        // 3. Bad JSON line
        let path3 = dir.path().join("badjson.sock");
        let listener3 = UnixListener::bind(&path3).unwrap();
        let peer3 = std::thread::spawn(move || {
            let (mut stream, _) = listener3.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = stream.write_all(b"{not valid json\n");
        });
        let client3 = IpcClient::new(&path3);
        let err3 = client3.call(Op::Ping).unwrap_err();
        assert!(matches!(err3, OmenError::Internal(_)));
        peer3.join().unwrap();
    }
}
