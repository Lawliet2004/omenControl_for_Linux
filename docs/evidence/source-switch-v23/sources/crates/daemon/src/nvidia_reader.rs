//! Fixed read-only NVIDIA query in a separate DynamicUser service. The
//! controller retains PrivateDevices; no command/path is accepted from clients.
use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
use omen_core::{OmenError, Result};
use omen_hardware::nvidia_bridge::{Frame, SOCKET};
use omen_hardware::SysRoot;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

fn invalid(message: &str) -> OmenError {
    OmenError::PermissionDenied(message.into())
}
fn frame(fixture: Option<&Path>) -> Result<Frame> {
    if let Some(path) = fixture {
        let m = std::fs::symlink_metadata(path)?;
        if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 || m.len() > 4096 {
            return Err(invalid("invalid root-owned NVIDIA fixture"));
        }
        let mut s: SensorSample = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|_| invalid("invalid NVIDIA fixture sample"))?;
        // Fixture-only negative tests keep a supplied timestamp; live paths
        // always use acquisition start, never refresh a cached timestamp.
        if s.measured_unix_ms == 0 {
            s.measured_unix_ms = crate::thermal::unix_ms()?;
        }
        return Ok(Frame {
            fixture: true,
            samples: vec![s],
        });
    }
    let root = SysRoot::default();
    let before = omen_hardware::gpu::graphics_status(&root);
    let started = crate::thermal::boottime_ms()?;
    let measured = crate::thermal::unix_ms()?;
    let mut samples = omen_hardware::sensors::nvidia::read_nvidia_samples();
    let after = omen_hardware::gpu::graphics_status(&root);
    if before != after || crate::thermal::boottime_ms()?.saturating_sub(started) > 1500 {
        samples.clear();
    }
    samples.retain(|s| {
        s.id == "nvidia/temp"
            && s.source == SensorSource::NvidiaSmi
            && s.kind == SensorKind::Temperature
    });
    for sample in &mut samples {
        sample.measured_unix_ms = measured;
    }
    Ok(Frame {
        fixture: false,
        samples,
    })
}
pub fn run(fixture: Option<&Path>) -> Result<()> {
    if unsafe { libc::geteuid() } == 0
        || std::env::var("LISTEN_PID").ok().as_deref() != Some(&std::process::id().to_string())
        || std::env::var("LISTEN_FDS").ok().as_deref() != Some("1")
        || !std::fs::read_to_string("/proc/self/cgroup")?
            .trim()
            .ends_with("/omen-nvidia-reader.service")
    {
        return Err(invalid(
            "NVIDIA reader requires its unprivileged activated systemd service",
        ));
    }
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            3,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            &mut kind as *mut _ as *mut libc::c_void,
            &mut length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Err(invalid("invalid activated NVIDIA listener"));
    }
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    if listener.local_addr()?.as_pathname() != Some(Path::new(SOCKET)) {
        return Err(invalid("unexpected NVIDIA listener path"));
    }
    // Socket ownership is assigned by PID 1, not by this DynamicUser.
    for accepted in listener.incoming() {
        let mut stream = accepted?;
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        } != 0
            || cred.uid != 0
        {
            continue;
        }
        stream.set_write_timeout(Some(Duration::from_millis(250)))?;
        let frame = frame(fixture)?;
        let mut bytes =
            serde_json::to_vec(&frame).map_err(|_| invalid("NVIDIA frame serialization failed"))?;
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes);
    }
    Ok(())
}
