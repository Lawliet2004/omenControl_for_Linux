//! `omen-ipc` — the wire protocol and client library for the omen daemon.
//!
//! Transport: a Unix stream socket carrying newline-delimited JSON (NDJSON).
//! One request per line, one response per line, ids correlate them. The
//! socket lives at `/run/omen-linux/ctl.sock` (override with `$OMEN_SOCK`
//! for tests).

pub mod client;
pub mod protocol;
pub mod settings;

pub use client::IpcClient;
pub use protocol::{Op, Request, Response};

/// Default daemon socket path.
pub fn default_socket_path() -> std::path::PathBuf {
    std::env::var("OMEN_SOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/run/omen-linux/ctl.sock"))
}
