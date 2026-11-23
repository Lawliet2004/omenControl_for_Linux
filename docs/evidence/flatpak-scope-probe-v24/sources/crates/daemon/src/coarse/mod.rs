//! A separately managed, mode-only writer with restart-safe recovery intent.
//! No paths, manual targets or thermal samples are accepted over its protocol.
mod actuator;
mod transport;
pub use actuator::{instance_valid, machine_target, TARGET};
use actuator::{invalid, Actuator, Identity};
use omen_core::fans::FanSupervisorStatus;
use omen_core::Result;
use serde::{Deserialize, Serialize};
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
pub use transport::fixture_probe;
pub use transport::Client;

pub const SOCKET: &str = "/run/omen-fan-recovery/control.sock";
const LEASE_MS: u64 = 4_000;
// First physical validation is a bounded trial even if the controller stays alive.
const TRIAL_MS: u64 = 10_000;
static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Peer {
    uid: u32,
    pid: i32,
    start: String,
    cgroup: String,
    exe: (u64, u64),
}
fn credentials(stream: &UnixStream, fixture: bool, controller: bool) -> Result<Peer> {
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
        || len as usize != std::mem::size_of::<libc::ucred>()
        || cred.pid <= 0
        || cred.uid != unsafe { libc::geteuid() }
        || (!fixture && cred.uid != 0)
    {
        return Err(invalid("unauthorized recovery peer credentials"));
    }
    let proc = PathBuf::from(format!("/proc/{}", cred.pid));
    let stat = std::fs::read_to_string(proc.join("stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or_else(|| invalid("invalid peer stat"))?
        .1
        .split_whitespace()
        .collect();
    if fields.len() < 20 || matches!(fields[0], "Z" | "X") {
        return Err(invalid("dead recovery peer"));
    }
    let exe = std::fs::metadata(proc.join("exe"))?;
    let trusted = std::fs::metadata(if fixture {
        std::env::current_exe()?
    } else {
        PathBuf::from("/usr/bin/omen-daemon")
    })?;
    if (exe.dev(), exe.ino()) != (trusted.dev(), trusted.ino())
        || (!fixture && (trusted.uid() != 0 || trusted.mode() & 0o022 != 0))
    {
        return Err(invalid("peer is not the trusted daemon executable"));
    }
    let cgroup = std::fs::read_to_string(proc.join("cgroup"))?;
    if !fixture
        && ((controller && cgroup.trim() != "0::/system.slice/omen-linux.service")
            || (!controller && !cgroup.trim().ends_with(".service")))
    {
        return Err(invalid("peer is outside its required systemd service"));
    }
    Ok(Peer {
        uid: cred.uid,
        pid: cred.pid,
        start: fields[19].into(),
        cgroup,
        exe: (exe.dev(), exe.ino()),
    })
}
fn alive(owner: &Peer) -> bool {
    let proc = PathBuf::from(format!("/proc/{}", owner.pid));
    let Ok(stat) = std::fs::read_to_string(proc.join("stat")) else {
        return false;
    };
    let Some((_, rest)) = stat.rsplit_once(')') else {
        return false;
    };
    let fields: Vec<_> = rest.split_whitespace().collect();
    fields.get(19).is_some_and(|v| *v == owner.start)
        && !fields.first().is_some_and(|v| matches!(*v, "Z" | "X"))
        && std::fs::read_to_string(proc.join("cgroup")).is_ok_and(|v| v == owner.cgroup)
        && std::fs::metadata(&proc).is_ok_and(|m| m.uid() == owner.uid)
        && std::fs::metadata(proc.join("exe"))
            .is_ok_and(|m| (m.dev(), m.ino()) == owner.exe && m.uid() == owner.uid)
}
fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    op: String,
    token: Option<String>,
    seq: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    seq: u64,
    ok: bool,
    error: Option<String>,
    token: Option<String>,
    fixture: bool,
    identity: Identity,
    status: FanSupervisorStatus,
}

struct Lease {
    actuator: Actuator,
    journal: PathBuf,
    owner: Option<Peer>,
    token: Option<String>,
    seq: u64,
    deadline: Option<u64>,
    trial_deadline: Option<u64>,
    owned: bool,
    error: Option<String>,
}
impl Lease {
    fn save_intent(&self) -> Result<()> {
        let parent = self
            .journal
            .parent()
            .ok_or_else(|| invalid("journal directory absent"))?;
        let temp = parent.join(format!(".intent-{}", nonce()?));
        let result = (|| -> Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            f.write_all(
                &serde_json::to_vec(&self.actuator.identity).map_err(|e| invalid(e.to_string()))?,
            )?;
            f.sync_all()?;
            std::fs::rename(&temp, &self.journal)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        let _ = std::fs::remove_file(temp);
        result
    }
    fn recover(&mut self) -> Result<()> {
        if !self.owned {
            return Ok(());
        }
        let result = (|| -> Result<()> {
            self.actuator.apply(false)?;
            match std::fs::remove_file(&self.journal) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            File::open(self.journal.parent().unwrap())?.sync_all()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.owned = false;
                self.error = None;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.to_string());
                Err(e)
            }
        }
    }
    fn revoke(&mut self) {
        self.owner = None;
        self.token = None;
        self.deadline = None;
        self.trial_deadline = None;
        let _ = self.recover();
    }
    fn tick(&mut self, now: u64) {
        if self.deadline.is_some_and(|d| now >= d)
            || self.trial_deadline.is_some_and(|d| now >= d)
            || self.owner.as_ref().is_some_and(|p| !alive(p))
        {
            self.revoke();
        }
        if self.token.is_none() && self.owned {
            let _ = self.recover();
        }
    }
    fn status(&self) -> FanSupervisorStatus {
        FanSupervisorStatus {
            pid: Some(std::process::id()),
            lease_active: self.owned,
            revoked: self.token.is_none(),
            recovery_error: self.error.clone(),
        }
    }
    fn command(&mut self, owner: Peer, request: Request, now: u64) -> Reply {
        self.tick(now);
        let mut issued = None;
        let result = (|| -> Result<()> {
            match request.op.as_str() {
                "status" => return self.actuator.validate(),
                "arm" => {
                    if self.owner.as_ref().is_some_and(|p| p != &owner) && self.token.is_some() {
                        return Err(invalid("another controller owns the lease"));
                    }
                    self.revoke();
                    if self.owned {
                        return Err(invalid("Automatic recovery is pending"));
                    }
                    self.actuator.validate()?;
                    self.owner = Some(owner.clone());
                    self.token = Some(nonce()?);
                    self.seq = 0;
                    self.deadline = Some(now.saturating_add(LEASE_MS));
                    issued = self.token.clone();
                    return Ok(());
                }
                _ => {}
            }
            if self.owner.as_ref() != Some(&owner)
                || self.token.is_none()
                || request.token != self.token
                || self.seq.checked_add(1) != Some(request.seq)
            {
                if self.owner.as_ref() == Some(&owner) {
                    self.revoke();
                }
                return Err(invalid(
                    "expired, foreign or replayed lease; explicitly rearm",
                ));
            }
            self.seq = request.seq;
            match request.op.as_str() {
                "max" => {
                    self.save_intent()?;
                    self.owned = true;
                    self.trial_deadline
                        .get_or_insert(now.saturating_add(TRIAL_MS));
                    self.actuator.apply(true)?;
                    self.deadline = Some(now.saturating_add(LEASE_MS));
                }
                "auto" => {
                    if !self.owned {
                        self.save_intent()?;
                        self.owned = true;
                    }
                    self.revoke();
                    if self.owned {
                        return Err(invalid("Automatic recovery failed; intent retained"));
                    }
                }
                "pulse" => {
                    if !self.owned {
                        return Err(invalid("no active Maximum lease"));
                    }
                    self.actuator.verify_maximum()?;
                    // HP firmware may time out its maximum-fan request even
                    // while our lease is healthy. Refresh the request only
                    // after confirming that no other owner changed the mode.
                    self.actuator.apply(true)?;
                    self.deadline = Some(now.saturating_add(LEASE_MS));
                }
                _ => return Err(invalid("only Automatic/Maximum are supported")),
            }
            Ok(())
        })();
        if let Err(e) = &result {
            if request.op != "status" && self.owner.as_ref() == Some(&owner) {
                self.revoke();
            }
            if self.owned && self.token.is_none() && self.error.is_none() {
                self.error = Some(e.to_string());
            }
        } else if request.op != "status" {
            self.error = None;
        }
        Reply {
            seq: request.seq,
            ok: result.is_ok(),
            error: result.err().map(|e| e.to_string()),
            token: issued,
            fixture: self.actuator.fixture,
            identity: self.actuator.identity.clone(),
            status: self.status(),
        }
    }
}
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn run(instance: &str, fixture: Option<&Path>) -> Result<()> {
    if !instance_valid(instance) || (fixture.is_none() && unsafe { libc::geteuid() } != 0) {
        return Err(invalid("invalid recovery service target or UID"));
    }
    if fixture.is_none()
        && !std::fs::read_to_string("/proc/self/cgroup")?
            .trim()
            .ends_with(&format!("/omen-fan-recovery@{instance}.service"))
    {
        return Err(invalid(
            "live writer must run in its independent systemd service",
        ));
    }
    let area = fixture.map_or_else(
        || PathBuf::from("/run/omen-fan-recovery"),
        |p| p.join("run/omen-fan-recovery"),
    );
    if let Some(parent) = area.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match DirBuilder::new().mode(0o700).create(&area) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = std::fs::symlink_metadata(&area)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(invalid("recovery directory is not private and owned"));
    }
    // Acquire singleton ownership BEFORE opening/recovering any actuator.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(area.join("writer.lock"))?;
    let meta = lock.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
        || unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
    {
        return Err(invalid(
            "another writer owns recovery or lock identity is invalid",
        ));
    }
    let actuator = Actuator::open(instance, fixture)?;
    let journal = area.join("owned.json");
    let owned = match std::fs::symlink_metadata(&journal) {
        Ok(m) => {
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.mode() & 0o077 != 0
                || m.len() > 2048
            {
                return Err(invalid("invalid recovery intent"));
            }
            let mut bytes = Vec::new();
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&journal)?
                .take(2049)
                .read_to_end(&mut bytes)?;
            let identity: Identity =
                serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
            if identity != actuator.identity {
                return Err(invalid(
                    "intent belongs to another actuator; refusing replacement",
                ));
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    let mut lease = Lease {
        actuator,
        journal,
        owner: None,
        token: None,
        seq: 0,
        deadline: None,
        trial_deadline: None,
        owned,
        error: None,
    };
    lease.recover()?;
    let socket = area.join("control.sock");
    if let Ok(m) = std::fs::symlink_metadata(&socket) {
        if !m.file_type().is_socket() || m.uid() != unsafe { libc::geteuid() } {
            return Err(invalid("unexpected recovery socket identity"));
        }
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    println!("{}", socket.display());
    let result = (|| -> Result<()> {
        while !STOP.load(Ordering::SeqCst) {
            lease.tick(crate::thermal::boottime_ms()?);
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
                    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
                    let Ok(owner) = credentials(&stream, fixture.is_some(), true) else {
                        continue;
                    };
                    match transport::read_frame::<Request>(&mut stream, 2048) {
                        Ok(request) => {
                            let reply =
                                lease.command(owner, request, crate::thermal::boottime_ms()?);
                            let _ = transport::send(&mut stream, &reply);
                        }
                        Err(_) if lease.owner.as_ref() == Some(&owner) => lease.revoke(),
                        Err(_) => {}
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    })();
    lease.revoke();
    let _ = std::fs::remove_file(socket);
    result?;
    if lease.owned {
        return Err(invalid(
            "Automatic recovery failed; journal retained for restart",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_hardware::{
        mock::{MockBackend, MockModel},
        HardwareBackend,
    };
    fn owner() -> Peer {
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
        let exe = std::fs::metadata("/proc/self/exe").unwrap();
        Peer {
            uid: unsafe { libc::geteuid() },
            pid: std::process::id() as i32,
            start: stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .nth(19)
                .unwrap()
                .into(),
            cgroup: std::fs::read_to_string("/proc/self/cgroup").unwrap(),
            exe: (exe.dev(), exe.ino()),
        }
    }
    fn fixture() -> (MockBackend, Lease) {
        let m = MockBackend::new(MockModel::VictusFb);
        let area = m.inner.root().join("run/recovery");
        std::fs::create_dir_all(&area).unwrap();
        let actuator = Actuator::open("hwmon0", Some(m.inner.root().path())).unwrap();
        let lease = Lease {
            actuator,
            journal: area.join("owned.json"),
            owner: None,
            token: None,
            seq: 0,
            deadline: None,
            trial_deadline: None,
            owned: false,
            error: None,
        };
        (m, lease)
    }
    fn command(l: &mut Lease, op: &str, now: u64) -> Reply {
        l.command(
            owner(),
            Request {
                op: op.into(),
                token: l.token.clone(),
                seq: if matches!(op, "arm" | "status") {
                    0
                } else {
                    l.seq + 1
                },
            },
            now,
        )
    }
    #[test]
    fn idle_start_does_not_restore_an_external_override() {
        let (m, mut l) = fixture();
        m.set_hwmon_dir("hwmon0", "pwm1_enable", "0");
        l.recover().unwrap();
        l.tick(1);
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Max)
        );
        assert!(!l.journal.exists());
    }
    #[test]
    fn expiry_including_suspend_gap_requires_new_random_ownership() {
        let (m, mut l) = fixture();
        assert!(command(&mut l, "arm", 1).ok);
        let old = l.token.clone();
        assert!(command(&mut l, "max", 2).ok);
        assert!(l.journal.exists());
        assert!(command(&mut l, "pulse", 3000).ok);
        l.tick(5000);
        assert!(l.owned);
        l.tick(7000);
        assert!(!l.owned && l.token.is_none());
        assert!(!l.journal.exists());
        assert!(
            !l.command(
                owner(),
                Request {
                    op: "max".into(),
                    token: old.clone(),
                    seq: 3
                },
                7001
            )
            .ok
        );
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Auto)
        );
        assert!(command(&mut l, "arm", 7002).ok);
        assert_ne!(old, l.token);
    }
    #[test]
    fn replay_and_manual_commands_revoke_before_dispatch() {
        for op in ["pulse", "manual"] {
            let (m, mut l) = fixture();
            command(&mut l, "arm", 0);
            command(&mut l, "max", 1);
            let seq = if op == "pulse" { l.seq } else { l.seq + 1 };
            let token = l.token.clone();
            assert!(
                !l.command(
                    owner(),
                    Request {
                        op: op.into(),
                        token,
                        seq
                    },
                    2
                )
                .ok
            );
            assert!(!l.owned && l.token.is_none());
            assert_eq!(
                m.fan_state().unwrap().mode,
                Some(omen_core::fans::FanMode::Auto)
            );
        }
    }
    #[test]
    fn competing_owner_cannot_take_over_or_renew() {
        let (_m, mut l) = fixture();
        command(&mut l, "arm", 0);
        command(&mut l, "max", 1);
        let mut other = owner();
        other.start = "different".into();
        assert!(
            !l.command(
                other,
                Request {
                    op: "arm".into(),
                    token: None,
                    seq: 0
                },
                2
            )
            .ok
        );
        assert!(l.owned && l.owner == Some(owner()));
        assert!(l.error.is_none());
        l.revoke();
    }
    #[test]
    fn restart_recovers_only_matching_durable_intent() {
        let (m, mut l) = fixture();
        command(&mut l, "arm", 0);
        command(&mut l, "max", 1);
        let saved: Identity = serde_json::from_slice(&std::fs::read(&l.journal).unwrap()).unwrap();
        let actuator = Actuator::open("hwmon0", Some(m.inner.root().path())).unwrap();
        assert_eq!(saved, actuator.identity);
        let mut restarted = Lease {
            actuator,
            journal: l.journal.clone(),
            owner: None,
            token: None,
            seq: 0,
            deadline: None,
            trial_deadline: None,
            owned: true,
            error: None,
        };
        restarted.recover().unwrap();
        assert!(!restarted.owned && !restarted.journal.exists());
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Auto)
        );
    }
    #[test]
    fn lost_controller_keeps_intent_and_retries_only_pinned_actuator() {
        let (m, mut l) = fixture();
        command(&mut l, "arm", 0);
        command(&mut l, "max", 1);
        m.set_hwmon_dir("hwmon0", "name", "other");
        l.tick(5000);
        assert!(l.owned && l.journal.exists() && l.error.is_some());
        m.set_hwmon_dir("hwmon0", "name", "hp");
        l.tick(5001);
        assert!(!l.owned && !l.journal.exists());
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Auto)
        );
    }
    #[test]
    fn inode_replacement_cannot_redirect_recovery() {
        let (m, mut l) = fixture();
        command(&mut l, "arm", 0);
        command(&mut l, "max", 1);
        let path = m.inner.root().join("sys/class/hwmon/hwmon0/pwm1_enable");
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "0").unwrap();
        l.revoke();
        assert!(l.owned && l.journal.exists());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "0");
    }
    #[test]
    fn idle_arm_expires_and_invalid_instances_never_open_hardware() {
        let (_m, mut l) = fixture();
        command(&mut l, "arm", 0);
        l.tick(4000);
        assert!(l.token.is_none());
        for s in [
            "",
            "../hwmon7",
            "hwmon7/pwm1_enable",
            "hwmon",
            "hwmon1234567",
        ] {
            assert!(!instance_valid(s));
            assert!(Actuator::open(s, None).is_err());
        }
    }
    #[test]
    fn healthy_heartbeats_cannot_extend_the_physical_trial_ceiling() {
        let (m, mut l) = fixture();
        command(&mut l, "arm", 0);
        command(&mut l, "max", 1);
        for now in (1000..10000).step_by(1000) {
            assert!(command(&mut l, "pulse", now).ok);
        }
        l.tick(10001);
        assert!(!l.owned && l.token.is_none() && !l.journal.exists());
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Auto)
        );
        assert!(!command(&mut l, "pulse", 10002).ok);
    }
    #[test]
    fn pulse_refreshes_maximum_but_refuses_an_external_mode_change() {
        let (m, mut l) = fixture();
        assert!(command(&mut l, "arm", 0).ok);
        assert!(command(&mut l, "max", 1).ok);
        let mode = m.inner.root().join("sys/class/hwmon/hwmon0/pwm1_enable");
        std::fs::write(&mode, "0").unwrap();
        assert!(command(&mut l, "pulse", 1000).ok);
        assert_eq!(std::fs::read_to_string(&mode).unwrap(), "0\n");
        std::fs::write(&mode, "2").unwrap();
        assert!(!command(&mut l, "pulse", 2000).ok);
        assert!(!l.owned && l.token.is_none());
        assert_eq!(
            m.fan_state().unwrap().mode,
            Some(omen_core::fans::FanMode::Auto)
        );
    }
}
