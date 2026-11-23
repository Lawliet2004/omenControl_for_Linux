//! Private independent writer for ordinary-file fixtures. No live filesystem
//! or privileged public endpoint is accepted by this development transport.

mod client;
mod files;
mod wrapper;
pub use wrapper::SupervisedBackend;

use omen_core::fans::{FanMode, FanSupervisorStatus};
use omen_core::{OmenError, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const LEASE_MS: u64 = 4_000;
const MAX_FRAME: usize = 2_048;
static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Apply { mode: FanMode },
    Pulse,
    Status,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    seq: u64,
    command: Command,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    seq: u64,
    ok: bool,
    error: Option<String>,
    status: FanSupervisorStatus,
}

struct Writer {
    files: files::Files,
    status: FanSupervisorStatus,
    deadline: Option<u64>,
    last_seq: u64,
}

impl Writer {
    fn restore(&mut self) {
        if !self.status.lease_active {
            return;
        }
        match self.files.apply(FanMode::Auto) {
            Ok(()) => {
                self.status.lease_active = false;
                self.status.recovery_error = None;
            }
            Err(error) => self.status.recovery_error = Some(error.to_string()),
        }
    }
    fn revoke(&mut self) {
        self.status.revoked = true;
        self.deadline = None;
        self.restore();
    }
    fn expire(&mut self, now: u64) {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.revoke();
        }
        if self.status.revoked && self.status.lease_active {
            self.restore();
        }
    }
    fn handle(&mut self, request: Request, now: u64) -> Reply {
        self.expire(now);
        let outcome = (|| -> Result<()> {
            if self.last_seq.checked_add(1) != Some(request.seq) {
                self.revoke();
                return Err(OmenError::StateMismatch(
                    "fan guardian sequence mismatch".into(),
                ));
            }
            self.last_seq = request.seq;
            match request.command {
                Command::Status => Ok(()),
                Command::Apply {
                    mode: FanMode::Auto,
                } => {
                    self.status.lease_active = true;
                    self.deadline = None;
                    self.restore();
                    if let Some(error) = &self.status.recovery_error {
                        self.status.revoked = true;
                        return Err(OmenError::Io(error.clone()));
                    }
                    Ok(())
                }
                _ if self.status.revoked => Err(OmenError::PermissionDenied(
                    "fan lease was revoked; explicitly rearm before another override".into(),
                )),
                Command::Apply {
                    mode: FanMode::Curve,
                } => {
                    self.revoke();
                    Err(OmenError::InvalidArgument(
                        "curve intent is not an actuator command".into(),
                    ))
                }
                Command::Apply { mode } => {
                    self.status.lease_active = true;
                    if let Err(error) = self.files.apply(mode) {
                        self.revoke();
                        return Err(error);
                    }
                    self.deadline = Some(now.saturating_add(LEASE_MS));
                    Ok(())
                }
                Command::Pulse => {
                    if !self.status.lease_active || self.deadline.is_none() {
                        return Err(OmenError::StateMismatch(
                            "no active fan lease to renew".into(),
                        ));
                    }
                    if let Err(error) = self.files.validate() {
                        self.revoke();
                        return Err(error);
                    }
                    self.deadline = Some(now.saturating_add(LEASE_MS));
                    Ok(())
                }
            }
        })();
        Reply {
            seq: request.seq,
            ok: outcome.is_ok(),
            error: outcome.err().map(|e| e.to_string()),
            status: self.status.clone(),
        }
    }
}

fn send(stream: &mut UnixStream, reply: &Reply) -> std::io::Result<()> {
    serde_json::to_writer(&mut *stream, reply)?;
    stream.write_all(b"\n")
}

extern "C" fn stop(_signal: i32) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn child_entry() -> Result<()> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred/length are writable. A non-socket descriptor fails here
    // before any inherited actuator file is touched.
    if unsafe {
        libc::getsockopt(
            3,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut length,
        )
    } != 0
        || length as usize != std::mem::size_of::<libc::ucred>()
        || cred.pid <= 0
        || cred.uid != unsafe { libc::geteuid() }
    {
        return Err(OmenError::PermissionDenied(
            "invalid private guardian channel".into(),
        ));
    }
    // SAFETY: fd 3 is the private Unix stream transferred to this helper once.
    let mut stream = unsafe { UnixStream::from_raw_fd(3) };
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;
    let mut writer = Writer {
        files: files::Files::inherited()?,
        status: FanSupervisorStatus {
            pid: Some(std::process::id()),
            ..Default::default()
        },
        deadline: None,
        last_seq: 0,
    };
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    send(
        &mut stream,
        &Reply {
            seq: 0,
            ok: true,
            error: None,
            status: writer.status.clone(),
        },
    )?;
    let mut pending = Vec::new();
    let mut closing = None::<Instant>;
    loop {
        let now = match crate::thermal::boottime_ms() {
            Ok(now) => now,
            Err(error) => {
                writer.revoke();
                return Err(error);
            }
        };
        writer.expire(now);
        if STOP.load(Ordering::SeqCst) {
            closing.get_or_insert_with(Instant::now);
            writer.revoke();
        }
        if let Some(start) = closing {
            writer.revoke();
            if !writer.status.lease_active {
                return Ok(());
            }
            if start.elapsed() >= Duration::from_secs(3) {
                return Err(OmenError::Io(format!(
                    "guardian could not confirm Automatic: {:?}",
                    writer.status.recovery_error
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut buffer = [0u8; 256];
        match stream.read(&mut buffer) {
            Ok(0) => {
                closing = Some(Instant::now());
                writer.revoke();
            }
            Ok(count) => {
                for byte in &buffer[..count] {
                    if *byte == b'\n' {
                        let response = match serde_json::from_slice::<Request>(&pending) {
                            Ok(request) => writer.handle(request, crate::thermal::boottime_ms()?),
                            Err(error) => {
                                writer.revoke();
                                Reply {
                                    seq: 0,
                                    ok: false,
                                    error: Some(format!("invalid guardian frame: {error}")),
                                    status: writer.status.clone(),
                                }
                            }
                        };
                        pending.clear();
                        if send(&mut stream, &response).is_err() {
                            closing = Some(Instant::now());
                            writer.revoke();
                            break;
                        }
                    } else {
                        pending.push(*byte);
                        if pending.len() > MAX_FRAME {
                            closing = Some(Instant::now());
                            writer.revoke();
                            break;
                        }
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => {
                closing = Some(Instant::now());
                writer.revoke();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, Writer) {
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("name", "hp"),
            ("pwm1_enable", "0"),
            ("pwm1", "128"),
            ("pwm2", "128"),
        ] {
            let path = directory.path().join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let writer = Writer {
            files: files::Files::open(directory.path()).unwrap(),
            status: FanSupervisorStatus::default(),
            deadline: None,
            last_seq: 0,
        };
        (directory, writer)
    }

    fn command(writer: &mut Writer, command: Command, now: u64) -> Reply {
        writer.handle(
            Request {
                seq: writer.last_seq + 1,
                command,
            },
            now,
        )
    }

    #[test]
    fn expiry_restores_auto_and_refuses_late_overrides() {
        let (directory, mut writer) = fixture();
        assert!(command(&mut writer, Command::Apply { mode: FanMode::Max }, 100).ok);
        writer.expire(100 + LEASE_MS);
        assert!(writer.status.revoked);
        assert!(!writer.status.lease_active);
        for mode in [FanMode::Max, FanMode::Manual { duty_pct: 40 }] {
            assert!(!command(&mut writer, Command::Apply { mode }, 5_000).ok);
        }
        assert!(!command(&mut writer, Command::Pulse, 5_001).ok);
        assert_eq!(
            fs::read_to_string(directory.path().join("pwm1_enable")).unwrap(),
            "2\n"
        );
    }

    #[test]
    fn only_an_active_pulse_renews_the_deadline() {
        let (_directory, mut writer) = fixture();
        assert!(!command(&mut writer, Command::Pulse, 0).ok);
        assert!(command(&mut writer, Command::Apply { mode: FanMode::Max }, 100).ok);
        assert!(command(&mut writer, Command::Status, 3_000).ok);
        assert_eq!(writer.deadline, Some(4_100));
        assert!(command(&mut writer, Command::Pulse, 3_001).ok);
        writer.expire(4_100);
        assert!(writer.status.lease_active);
        writer.expire(7_001);
        assert!(writer.status.revoked && !writer.status.lease_active);
    }

    #[test]
    fn duplicate_or_skipped_sequence_revokes_before_dispatch() {
        for sequence in [1, 3] {
            let (directory, mut writer) = fixture();
            assert!(command(&mut writer, Command::Apply { mode: FanMode::Max }, 100).ok);
            assert!(
                !writer
                    .handle(
                        Request {
                            seq: sequence,
                            command: Command::Pulse
                        },
                        101
                    )
                    .ok
            );
            assert!(writer.status.revoked && !writer.status.lease_active);
            assert_eq!(
                fs::read_to_string(directory.path().join("pwm1_enable")).unwrap(),
                "2\n"
            );
        }
    }

    #[test]
    fn idle_revocation_preserves_an_unowned_external_override() {
        let (directory, mut writer) = fixture();
        writer.revoke();
        assert_eq!(
            fs::read_to_string(directory.path().join("pwm1_enable")).unwrap(),
            "0"
        );
        assert!(writer.status.revoked && !writer.status.lease_active);
    }

    #[test]
    fn failed_recovery_remains_owned_and_retries_the_same_descriptors() {
        let (directory, mut writer) = fixture();
        assert!(command(&mut writer, Command::Apply { mode: FanMode::Max }, 100).ok);
        fs::write(directory.path().join("name"), "other").unwrap();
        writer.expire(4_100);
        assert!(writer.status.revoked && writer.status.lease_active);
        assert!(writer.status.recovery_error.is_some());
        fs::write(directory.path().join("name"), "hp").unwrap();
        writer.expire(4_101);
        assert!(writer.status.revoked && !writer.status.lease_active);
        assert!(writer.status.recovery_error.is_none());
        assert_eq!(
            fs::read_to_string(directory.path().join("pwm1_enable")).unwrap(),
            "2\n"
        );
    }

    #[test]
    fn heartbeat_cannot_extend_ownership_of_an_invalid_controller() {
        let (directory, mut writer) = fixture();
        assert!(command(&mut writer, Command::Apply { mode: FanMode::Max }, 100).ok);
        fs::write(directory.path().join("name"), "other").unwrap();
        assert!(!command(&mut writer, Command::Pulse, 101).ok);
        assert!(writer.status.revoked && writer.status.lease_active);
        assert!(writer.status.recovery_error.is_some());
        assert_eq!(writer.deadline, None);
        fs::write(directory.path().join("name"), "hp").unwrap();
        writer.expire(102);
        assert!(!writer.status.lease_active);
    }

    #[test]
    fn manual_floor_is_enforced_and_curve_intent_revokes() {
        let (directory, mut writer) = fixture();
        assert!(
            command(
                &mut writer,
                Command::Apply {
                    mode: FanMode::Manual { duty_pct: 0 }
                },
                100
            )
            .ok
        );
        let raw = omen_hardware::hp_wmi::duty_pct_to_raw(omen_core::MIN_FAN_DUTY_PCT);
        for name in ["pwm1", "pwm2"] {
            assert_eq!(
                fs::read_to_string(directory.path().join(name)).unwrap(),
                format!("{raw}\n")
            );
        }
        assert!(
            !command(
                &mut writer,
                Command::Apply {
                    mode: FanMode::Curve
                },
                101
            )
            .ok
        );
        assert!(writer.status.revoked && !writer.status.lease_active);
    }
}
