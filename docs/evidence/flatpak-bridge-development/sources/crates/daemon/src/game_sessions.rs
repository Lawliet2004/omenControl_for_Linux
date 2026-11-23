//! Daemon-owned profile leases with desktop-owned process trees. The journal
//! stores recovery information, never reusable PPD cookies or write authority.
use omen_core::games::{GameSession, GameSessionReport, SessionPhase};
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use omen_hardware::game_scope::{GameScope, ScopeIdentity, ScopeState};
use omen_hardware::profiles::holds::{HoldCancellation, ProfileHold};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_ACTIVE: usize = 16;
const MAX_PER_UID: usize = 4;
const MAX_RECORDS: usize = 32;

struct Control {
    cancelled: AtomicBool,
    lease: HoldCancellation,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    uid: u32,
    boot_id: String,
    scope: ScopeIdentity,
    session: GameSession,
    #[serde(skip)]
    active: bool,
    #[serde(skip)]
    control: Option<Arc<Control>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    records: Vec<Record>,
}
#[derive(Default)]
struct Registry {
    records: Vec<Record>,
    error: Option<String>,
    unreadable: bool,
}

pub struct GameSessions {
    registry: Mutex<Registry>,
    path: Option<PathBuf>,
    boot_id: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn invalid(message: &str) -> OmenError {
    OmenError::InvalidArgument(message.into())
}

impl GameSessions {
    pub fn new(path: Option<PathBuf>) -> Arc<Self> {
        let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .unwrap_or_default()
            .trim()
            .into();
        let mut registry = Registry::default();
        if let Some(path) = path
            .as_ref()
            .filter(|p| p.exists() || fs::symlink_metadata(p).is_ok())
        {
            let loaded = (|| -> Result<Journal> {
                let mut file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(path)?;
                let meta = file.metadata()?;
                if !meta.is_file()
                    || meta.uid() != unsafe { libc::geteuid() }
                    || meta.permissions().mode() & 0o777 != 0o600
                {
                    return Err(invalid(
                        "session journal must be a private regular file owned by the service",
                    ));
                }
                let mut bytes = Vec::new();
                std::io::Read::by_ref(&mut file)
                    .take(1_048_577)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 1_048_576 {
                    return Err(invalid("session journal exceeds 1 MiB"));
                }
                let journal: Journal = serde_json::from_slice(&bytes)
                    .map_err(|e| OmenError::Io(format!("session journal: {e}")))?;
                if journal.version != 1
                    || journal.records.len() > MAX_RECORDS
                    || journal.records.iter().any(|r| {
                        r.uid == 0
                            || r.session.token.len() != 32
                            || !r
                                .session
                                .token
                                .bytes()
                                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                            || !omen_core::games::valid_game_id(&r.session.game_id)
                    })
                {
                    return Err(invalid("invalid session journal version or records"));
                }
                Ok(journal)
            })();
            match loaded {
                Ok(journal) => {
                    registry.records = journal.records;
                    for record in &mut registry.records {
                        record.active = false;
                        record.control = None;
                        record.session.profile_held = false;
                        if matches!(
                            record.session.phase,
                            SessionPhase::Starting | SessionPhase::Tracking
                        ) {
                            record.session.phase = SessionPhase::Interrupted;
                            record.session.detail = "Service restarted; the old PPD peer was released. No profile was reapplied. Start a new wrapped session to resume ownership.".into();
                            record.session.updated_unix = now();
                        }
                    }
                }
                Err(error) => {
                    registry.error = Some(error.to_string());
                    registry.unreadable = true;
                }
            }
        }
        Arc::new(Self {
            registry: Mutex::new(registry),
            path,
            boot_id,
        })
    }

    fn save(&self, registry: &mut Registry) -> Result<()> {
        if registry.unreadable {
            return Err(OmenError::Io("session recovery journal is unreadable; preserve and repair it before starting managed games".into()));
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let result = (|| -> Result<()> {
            let parent = path
                .parent()
                .ok_or_else(|| invalid("journal parent missing"))?;
            fs::create_dir_all(parent)?;
            let meta = fs::symlink_metadata(parent)?;
            if !meta.is_dir()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.permissions().mode() & 0o022 != 0
            {
                return Err(invalid(
                    "session journal directory must be service-owned and not group/world writable",
                ));
            }
            let mut nonce = [0u8; 16];
            File::open("/dev/urandom")?.read_exact(&mut nonce)?;
            let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
            let temporary = parent.join(format!(".managed-games-{suffix}.tmp"));
            let written = (|| -> Result<()> {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&temporary)?;
                let bytes = serde_json::to_vec(&Journal {
                    version: 1,
                    records: registry.records.clone(),
                })
                .map_err(|e| OmenError::Io(e.to_string()))?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, path)?;
                File::open(parent)?.sync_all()?;
                Ok(())
            })();
            if written.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            written
        })();
        registry.error = result.as_ref().err().map(ToString::to_string);
        result
    }

    pub fn report(&self, uid: u32) -> GameSessionReport {
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        GameSessionReport {
            sessions: registry
                .records
                .iter()
                .rev()
                .filter(|r| r.uid == uid)
                .map(|r| r.session.clone())
                .collect(),
            recovery_error: registry.error.clone(),
            mock: false,
        }
    }

    pub fn has_holds(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .iter()
            .any(|r| r.active && r.session.profile_held)
    }

    pub fn start(
        self: &Arc<Self>,
        uid: u32,
        token: String,
        game_id: String,
        profile: ProfileKind,
        scope: GameScope,
        acquire: impl FnOnce() -> Result<ProfileHold>,
    ) -> Result<GameSession> {
        if !omen_core::games::valid_game_id(&game_id)
            || uid == 0
            || !matches!(profile, ProfileKind::Performance | ProfileKind::Quiet)
        {
            return Err(invalid(
                "managed games require a desktop UID, valid ID and Performance or Quiet",
            ));
        }
        let identity = scope.identity().clone();
        {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = registry
                .records
                .iter()
                .find(|r| r.uid == uid && r.session.token == token)
            {
                if existing.active
                    && existing.scope.device == identity.device
                    && existing.scope.inode == identity.inode
                    && existing.session.game_id == game_id
                    && existing.session.requested_profile == profile
                {
                    return Ok(existing.session.clone());
                }
                return Err(invalid(
                    "session token already belongs to a different or ended session",
                ));
            }
            if let Some(error) = &registry.error {
                return Err(OmenError::Io(error.clone()));
            }
            if registry.records.iter().filter(|r| r.active).count() >= MAX_ACTIVE
                || registry
                    .records
                    .iter()
                    .filter(|r| r.active && r.uid == uid)
                    .count()
                    >= MAX_PER_UID
            {
                return Err(OmenError::SafetyLockout(
                    "managed session limit reached (four per user, sixteen total)".into(),
                ));
            }
            while registry.records.len() >= MAX_RECORDS {
                let index = registry
                    .records
                    .iter()
                    .position(|r| !r.active)
                    .ok_or_else(|| invalid("session history is full"))?;
                registry.records.remove(index);
            }
            registry.records.push(Record {
                uid,
                boot_id: self.boot_id.clone(),
                scope: identity,
                session: GameSession {
                    token: token.clone(),
                    game_id: game_id.clone(),
                    requested_profile: profile,
                    profile_held: false,
                    phase: SessionPhase::Starting,
                    detail: "Preparing an authorized profile hold for the wrapped command.".into(),
                    started_unix: now(),
                    updated_unix: now(),
                },
                active: true,
                control: None,
            });
            if let Err(error) = self.save(&mut registry) {
                if let Some(record) = registry.records.last_mut() {
                    record.active = false;
                    record.session.phase = SessionPhase::Failed;
                    record.session.detail = error.to_string();
                }
                return Err(error);
            }
        }
        let acquired = acquire().and_then(|hold| {
            if scope.state()? != ScopeState::Populated {
                return Err(invalid(
                    "wrapped command exited before profile ownership was established",
                ));
            }
            Ok(hold)
        });
        let hold = match acquired {
            Ok(hold) => hold,
            Err(error) => {
                self.update(uid, &token, true, |record| {
                    record.active = false;
                    record.session.phase = SessionPhase::Failed;
                    record.session.detail = error.to_string();
                });
                return Err(error);
            }
        };
        let control = Arc::new(Control {
            cancelled: AtomicBool::new(false),
            lease: hold.cancellation()?,
        });
        {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            let record = registry
                .records
                .iter_mut()
                .find(|r| r.uid == uid && r.session.token == token)
                .expect("reserved session");
            record.control = Some(control.clone());
            record.session.profile_held = true;
            record.session.phase = SessionPhase::Tracking;
            record.session.detail="Tracking the wrapped process tree. PPD releases this hold when the final process exits; effective profile may be selected by another hold.".into();
            if let Err(error) = self.save(&mut registry) {
                drop(hold);
                control.cancelled.store(true, Ordering::SeqCst);
                let _ = control.lease.cancel();
                if let Some(record) = registry
                    .records
                    .iter_mut()
                    .find(|r| r.uid == uid && r.session.token == token)
                {
                    record.active = false;
                    record.control = None;
                    record.session.profile_held = false;
                    record.session.phase = SessionPhase::Failed;
                    record.session.detail = error.to_string();
                }
                return Err(error);
            }
        }
        let manager = self.clone();
        let worker_token = token.clone();
        let spawned = std::thread::Builder::new()
            .name("game-session".into())
            .spawn(move || manager.watch(uid, worker_token, scope, hold, control));
        if let Err(error) = spawned {
            self.update(uid, &token, true, |r| {
                r.active = false;
                r.control = None;
                r.session.profile_held = false;
                r.session.phase = SessionPhase::Failed;
                r.session.detail = format!("Session worker could not start: {error}");
            });
            return Err(error.into());
        }
        self.report(uid)
            .sessions
            .into_iter()
            .find(|s| s.token == token)
            .ok_or_else(|| invalid("session disappeared"))
    }

    fn update(&self, uid: u32, token: &str, persist: bool, update: impl FnOnce(&mut Record)) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = registry
            .records
            .iter_mut()
            .find(|r| r.uid == uid && r.session.token == token)
        {
            update(record);
            record.session.updated_unix = now();
            if persist {
                if let Err(error) = self.save(&mut registry) {
                    log::error!("managed session journal: {error}");
                }
            }
        }
    }

    fn watch(
        &self,
        uid: u32,
        token: String,
        scope: GameScope,
        hold: ProfileHold,
        control: Arc<Control>,
    ) {
        let mut hold = Some(hold);
        let mut next_check = Instant::now();
        loop {
            if control.cancelled.load(Ordering::SeqCst) && hold.is_some() {
                drop(hold.take());
                self.update(uid, &token, true, |r| r.session.profile_held = false);
            }
            match scope.state() {
                Ok(ScopeState::Empty) => {
                    drop(hold.take());
                    self.update(uid, &token, true, |r| {
                        r.active = false;
                        r.control = None;
                        r.session.profile_held = false;
                        r.session.phase = SessionPhase::Ended;
                        r.session.detail = "Tracked game scope is empty. Its profile hold is released; later profile choices were preserved.".into();
                    });
                    return;
                }
                Err(error) => {
                    drop(hold.take());
                    self.update(uid,&token,true,|r| {r.active=false;r.control=None;r.session.profile_held=false;r.session.phase=SessionPhase::Interrupted;r.session.detail=format!("Lifetime observation lost; profile hold released without reapplying any profile: {error}");});
                    return;
                }
                Ok(ScopeState::Populated) => self.update(uid, &token, false, |_| {}),
            }
            if Instant::now() >= next_check {
                if let Some(lease) = hold.as_ref() {
                    let active = lease.active();
                    if !control.cancelled.load(Ordering::SeqCst) && !matches!(active, Ok(true)) {
                        drop(hold.take());
                        self.update(uid,&token,true,|r| {r.session.profile_held=false;r.session.detail=match active {
                            Ok(false)=>"The provider canceled this hold after a profile change. Tracking continues without automatic rearming.".into(),
                            Err(error)=>format!("Profile provider unavailable; ownership released. Tracking continues without automatic rearming: {error}"),
                            Ok(true)=>unreachable!(),
                        };});
                    }
                }
                next_check = Instant::now() + Duration::from_secs(2);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn release(&self, uid: Option<u32>, token: Option<&str>, reason: &str) -> Result<()> {
        let controls: Vec<_> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry
                .records
                .iter()
                .filter(|r| {
                    uid.is_none_or(|uid| r.uid == uid)
                        && token.is_none_or(|token| r.session.token == token)
                })
                .filter_map(|r| {
                    r.control
                        .as_ref()
                        .map(|control| (r.uid, r.session.token.clone(), control.clone()))
                })
                .collect()
        };
        if token.is_some() && controls.is_empty() {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            if !registry
                .records
                .iter()
                .any(|r| Some(r.uid) == uid && Some(r.session.token.as_str()) == token)
            {
                return Err(OmenError::NotFound(
                    "no session with this token belongs to the connecting user".into(),
                ));
            }
        }
        let mut failure = None;
        for (uid, token, control) in controls {
            control.cancelled.store(true, Ordering::SeqCst);
            match control.lease.cancel() {
                Ok(()) => self.update(uid, &token, true, |r| {
                    r.session.profile_held = false;
                    r.session.detail = reason.into();
                }),
                Err(error) => {
                    failure = Some(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_history_and_release_are_scoped_to_the_transport_uid() {
        let manager = GameSessions::new(None);
        let token = "0123456789abcdef0123456789abcdef";
        manager.registry.lock().unwrap().records.push(Record {
            uid: 1000,
            boot_id: "fixture".into(),
            scope: ScopeIdentity {
                relative_path: "fixture-only".into(),
                device: 0,
                inode: 0,
            },
            active: false,
            control: None,
            session: GameSession {
                token: token.into(),
                game_id: "custom/private".into(),
                requested_profile: ProfileKind::Performance,
                profile_held: false,
                phase: SessionPhase::Interrupted,
                detail: "No authority recovered".into(),
                started_unix: 0,
                updated_unix: 0,
            },
        });
        assert_eq!(manager.report(1000).sessions.len(), 1);
        assert!(manager.report(1001).sessions.is_empty());
        assert!(matches!(
            manager.release(Some(1001), Some(token), "other user"),
            Err(OmenError::NotFound(_))
        ));
        manager.release(Some(1000), Some(token), "owner").unwrap();
        assert_eq!(
            manager.report(1000).sessions[0].phase,
            SessionPhase::Interrupted
        );
    }
}
