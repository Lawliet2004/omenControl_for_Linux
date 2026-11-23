//! Error type shared across the daemon, CLI, IPC and GUI boundaries.

use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, OmenError>;

/// Errors are serialized over IPC, so the enum is data-oriented: every variant
/// carries human-readable context rather than wrapping non-serializable
/// `std::io::Error` values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum OmenError {
    /// A required sysfs/procfs interface does not exist on this machine.
    NotFound(String),
    /// The caller is not authorized for this operation.
    PermissionDenied(String),
    /// The hardware/firmware/kernel combination does not support the feature.
    /// This is the honest answer for e.g. manual fan control on a board that
    /// the hp-wmi driver has not allowlisted.
    Unsupported(String),
    /// The feature exists but is currently disabled for safety (for example a
    /// conflicting fan daemon was detected).
    SafetyLockout(String),
    /// The operation is not implemented in this build.
    NotImplemented(String),
    /// Invalid input from the caller.
    InvalidArgument(String),
    /// The daemon is not reachable.
    DaemonUnavailable(String),
    /// A hardware read or write failed.
    Io(String),
    /// State verification after a write failed (readback mismatch).
    StateMismatch(String),
    /// Anything else.
    Internal(String),
    /// A desktop launcher failed; may include profile recovery details.
    LaunchFailed(String),
}

impl fmt::Display for OmenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (tag, msg) = match self {
            OmenError::NotFound(m) => ("not found", m),
            OmenError::PermissionDenied(m) => ("permission denied", m),
            OmenError::Unsupported(m) => ("unsupported on this hardware", m),
            OmenError::SafetyLockout(m) => ("blocked by safety policy", m),
            OmenError::NotImplemented(m) => ("not implemented", m),
            OmenError::InvalidArgument(m) => ("invalid argument", m),
            OmenError::DaemonUnavailable(m) => ("daemon unavailable", m),
            OmenError::Io(m) => ("hardware I/O error", m),
            OmenError::StateMismatch(m) => ("state verification failed", m),
            OmenError::Internal(m) => ("internal error", m),
            OmenError::LaunchFailed(m) => ("launch failed", m),
        };
        write!(f, "{tag}: {msg}")
    }
}

impl std::error::Error for OmenError {}

impl From<std::io::Error> for OmenError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => OmenError::NotFound(e.to_string()),
            std::io::ErrorKind::PermissionDenied => OmenError::PermissionDenied(e.to_string()),
            _ => OmenError::Io(e.to_string()),
        }
    }
}

impl OmenError {
    /// True when the error means "this will never work on this machine" as
    /// opposed to "transient failure". The GUI uses this to disable controls.
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            OmenError::Unsupported(_) | OmenError::NotImplemented(_) | OmenError::NotFound(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_json() {
        let e = OmenError::Unsupported("board 8C30 not allowlisted for fan control".into());
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("unsupported"));
        let back: OmenError = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
        assert!(back.is_permanent());
    }
}
