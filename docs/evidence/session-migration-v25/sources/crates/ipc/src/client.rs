//! Unix-socket NDJSON client.

use crate::protocol::{Request, Response};
use omen_core::OmenError;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
        let mut reader = BufReader::new(stream);
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
        reader: &mut BufReader<UnixStream>,
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

        let stream = reader.get_mut();
        let timeout = if req.op.is_write() { 65 } else { 5 };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(timeout)))
            .map_err(|e| OmenError::Io(e.to_string()))?;
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .map_err(|e| OmenError::Io(e.to_string()))?;
        stream
            .write_all(line.as_bytes())
            .map_err(|e| OmenError::Io(format!("send: {e}")))?;
        let _ = stream.flush();

        for _ in 0..8 {
            let mut l = String::new();
            let n = reader
                .by_ref()
                .take(1_048_577)
                .read_line(&mut l)
                .map_err(|e| OmenError::Io(format!("recv: {e}")))?;
            if n == 0 {
                break;
            }
            if n > 1_048_576 || !l.ends_with('\n') {
                return Err(OmenError::InvalidArgument(
                    "oversized or unterminated daemon response".into(),
                ));
            }
            if l.trim().is_empty() {
                continue;
            }
            let resp: Response = serde_json::from_str(&l)
                .map_err(|e| OmenError::Internal(format!("decode response line: {e}")))?;
            if resp.id == id {
                return Ok(resp);
            }
            // Out-of-order lines are logged and skipped; ids correlate.
            log::debug!("skipping response with foreign id {}", resp.id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Op;
    use omen_core::sensors::SensorSample;

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
}
