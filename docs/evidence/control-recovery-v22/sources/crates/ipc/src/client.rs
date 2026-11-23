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
}

impl IpcClient {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        IpcClient {
            path: path.into(),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Clone pointing at the same socket with an independent id counter.
    pub fn share(&self) -> IpcClient {
        IpcClient {
            path: self.path.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Connect to the default socket.
    pub fn default_path() -> Self {
        IpcClient::new(crate::default_socket_path())
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = Request {
            v: crate::protocol::PROTOCOL_VERSION,
            id,
            op,
        };
        let mut line = serde_json::to_string(&req)
            .map_err(|e| OmenError::Internal(format!("encode request: {e}")))?;
        line.push('\n');

        let mut stream = UnixStream::connect(&self.path).map_err(|e| {
            OmenError::DaemonUnavailable(format!(
                "cannot connect to {}: {e} (is omen-daemon running?)",
                self.path.display()
            ))
        })?;
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

        let mut reader = BufReader::new(stream);
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
}
