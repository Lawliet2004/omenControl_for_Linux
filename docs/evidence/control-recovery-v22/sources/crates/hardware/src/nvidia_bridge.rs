//! Read-only telemetry from a dedicated systemd socket-activated identity.
//! The root-owned socket/PID-1 listener is the trust boundary, never a user cache.
use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
use omen_core::{OmenError, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

pub const SOCKET: &str = "/run/omen-nvidia-reader/control.sock";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub fixture: bool,
    pub samples: Vec<SensorSample>,
}
fn invalid(message: &str) -> OmenError {
    OmenError::StateMismatch(message.into())
}
pub fn validate(frame: Frame, fixture: bool, now: u64) -> Result<Vec<SensorSample>> {
    if frame.fixture != fixture || frame.samples.len() > 1 {
        return Err(invalid(
            "NVIDIA reader fixture/live mismatch or invalid sample count",
        ));
    }
    for sample in &frame.samples {
        if sample.source != SensorSource::NvidiaSmi
            || sample.kind != SensorKind::Temperature
            || sample.id != "nvidia/temp"
            || sample
                .device_id
                .as_deref()
                .is_none_or(|v| !crate::gpu::valid_pci_address(v))
            || sample.measured_unix_ms == 0
            || now
                .checked_sub(sample.measured_unix_ms)
                .is_none_or(|age| age > 2000)
            || sample
                .value
                .is_none_or(|v| !v.is_finite() || !(0.0..=150.0).contains(&v))
        {
            return Err(invalid(
                "NVIDIA reader temperature is stale, unassociated or invalid",
            ));
        }
    }
    Ok(frame.samples)
}
pub fn read(fixture: bool) -> Result<Vec<SensorSample>> {
    let parent = Path::new(SOCKET).parent().unwrap();
    let dir = std::fs::symlink_metadata(parent)?;
    let metadata = std::fs::symlink_metadata(SOCKET)?;
    if !dir.is_dir()
        || dir.uid() != 0
        || dir.mode() & 0o077 != 0
        || !metadata.file_type().is_socket()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid("NVIDIA reader endpoint is not root-private"));
    }
    let mut stream = UnixStream::connect(SOCKET)?;
    let after = std::fs::symlink_metadata(SOCKET)?;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if (after.dev(), after.ino()) != (metadata.dev(), metadata.ino())
        || unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut length,
            )
        } != 0
        || length as usize != std::mem::size_of::<libc::ucred>()
        || cred.uid != 0
        || cred.pid != 1
    {
        return Err(invalid(
            "NVIDIA reader is not the system manager's activated socket",
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(1800);
    let mut bytes = Vec::new();
    loop {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| invalid("NVIDIA reader timed out"))?;
        stream.set_read_timeout(Some(timeout))?;
        let mut b = [0];
        if stream.read(&mut b)? == 0 {
            return Err(invalid("NVIDIA reader closed without a frame"));
        }
        if b[0] == b'\n' {
            break;
        }
        bytes.push(b[0]);
        if bytes.len() > 4096 {
            return Err(invalid("oversized NVIDIA reader reply"));
        }
    }
    let frame: Frame =
        serde_json::from_slice(&bytes).map_err(|e| OmenError::StateMismatch(e.to_string()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid("acquisition clock unavailable"))?
        .as_millis() as u64;
    validate(frame, fixture, now)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> SensorSample {
        let mut s = SensorSample::numeric(
            "nvidia/temp",
            "GPU temperature",
            SensorKind::Temperature,
            SensorSource::NvidiaSmi,
            45.0,
        );
        s.device_id = Some("0000:01:00.0".into());
        s.measured_unix_ms = 1000;
        s
    }
    #[test]
    fn rejects_stale_future_unassociated_and_fixture_data_for_live_control() {
        let s = sample();
        assert!(validate(
            Frame {
                fixture: false,
                samples: vec![s.clone()]
            },
            false,
            1001
        )
        .is_ok());
        for now in [999, 3001] {
            assert!(validate(
                Frame {
                    fixture: false,
                    samples: vec![s.clone()]
                },
                false,
                now
            )
            .is_err());
        }
        assert!(validate(
            Frame {
                fixture: true,
                samples: vec![s.clone()]
            },
            false,
            1001
        )
        .is_err());
        let mut missing = s.clone();
        missing.device_id = None;
        assert!(validate(
            Frame {
                fixture: false,
                samples: vec![missing]
            },
            false,
            1001
        )
        .is_err());
        let mut wrong = s;
        wrong.source = SensorSource::Mock;
        assert!(validate(
            Frame {
                fixture: false,
                samples: vec![wrong]
            },
            false,
            1001
        )
        .is_err());
    }
}
