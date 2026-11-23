//! HardwareService — the privileged brain. Owns the backend, the fan-curve
//! control loop, battery-aware profile switching and configuration.

use omen_core::capability::Capabilities;
use omen_core::config::OmenConfig;
use omen_core::fans::FanMode;
use omen_core::lighting::LightingState;
use omen_core::power::{AcInfo, BatteryInfo};
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use omen_hardware::HardwareBackend;
use omen_ipc::protocol::{
    CompareSetProfileOutcome, DaemonInfo, GameProfileFinished, GameProfileStarted, Op, Response,
    PROTOCOL_VERSION,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct FanPolicy {
    requested: FanMode,
    guard: Option<crate::thermal::Guard>,
    last_error: Option<String>,
    recovery_error: Option<String>,
}

impl Default for FanPolicy {
    fn default() -> Self {
        Self {
            requested: FanMode::Auto,
            guard: None,
            last_error: None,
            recovery_error: None,
        }
    }
}

struct GameProfileTransaction {
    token: String,
    original: ProfileKind,
    requested: ProfileKind,
    expires_boottime_ms: u64,
}

#[derive(Default)]
struct ProfileWriteState {
    game: Option<GameProfileTransaction>,
}

pub struct HardwareService {
    pub backend: Arc<dyn HardwareBackend>,
    pub mock: bool,
    config: Mutex<OmenConfig>,
    /// Serialize manual and automatic profile writes against current policy.
    profile_write: Mutex<ProfileWriteState>,
    managed_games: Arc<crate::game_sessions::GameSessions>,
    started: std::time::SystemTime,
    /// Active fan mode *requested* through the daemon (curve engine drives
    /// manual writes when set to Curve).
    fan_mode: Mutex<FanPolicy>,
    /// True only after this service has attempted to take fan ownership.
    fan_owned: AtomicBool,
    shutdown_requested: AtomicBool,
}

impl HardwareService {
    pub fn new(backend: Arc<dyn HardwareBackend>, mock: bool, config: OmenConfig) -> Arc<Self> {
        let journal =
            (!mock).then(|| OmenConfig::daemon_path().with_file_name("managed-game-sessions.json"));
        Self::new_with_game_journal(backend, mock, config, journal)
    }

    pub fn new_with_game_journal(
        backend: Arc<dyn HardwareBackend>,
        mock: bool,
        config: OmenConfig,
        journal: Option<std::path::PathBuf>,
    ) -> Arc<Self> {
        Arc::new(HardwareService {
            backend,
            mock,
            config: Mutex::new(config),
            profile_write: Mutex::new(ProfileWriteState::default()),
            managed_games: crate::game_sessions::GameSessions::new(journal),
            started: std::time::SystemTime::now(),
            fan_mode: Mutex::new(FanPolicy::default()),
            fan_owned: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
        })
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::SeqCst)
    }

    /// Startup is read-only. Saved curves and lighting are not applied merely
    /// because the daemon was installed or restarted.
    pub fn apply_startup_policy(&self) {
        log::info!("startup discovery complete; no hardware state changed");
    }

    /// Serialize recovery against all curve/manual writes. A failed restore
    /// remains owned so a subsequent recovery attempt can retry it.
    pub fn restore_safe_state(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        let mut mode = self.fan_mode.lock().unwrap_or_else(|e| e.into_inner());
        mode.requested = FanMode::Auto;
        mode.guard = None;
        self.recover_auto(&mut mode);
        drop(mode);
        // Restore owned fan control before waiting on possibly slow profile IO.
        let _profile = self.profile_write.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = self.managed_games.release(
            None,
            None,
            "Service stopped; its owned profile hold was released.",
        ) {
            log::error!("managed game shutdown: {error}");
        }
    }

    pub fn handle(&self, id: u64, op: Op) -> Response {
        if self.shutdown_requested() && op.is_write() && !matches!(op, Op::Shutdown) {
            return Response::err(
                id,
                OmenError::PermissionDenied("daemon is shutting down".into()),
            );
        }
        match op {
            Op::StartManagedGame { .. } | Op::GetGameSessions | Op::ReleaseGameSession { .. } => Response::err(id, OmenError::PermissionDenied("managed sessions require the authenticated socket peer".into())),
            Op::Ping => Response::ok(id, "pong"),
            Op::GetInfo => Response::ok(
                id,
                DaemonInfo {
                    protocol_version: PROTOCOL_VERSION,
                    daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                    machine: self.backend.detect(),
                    mock: self.mock,
                    started_unix: self
                        .started
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                },
            ),
            Op::GetCapabilities => match self.try_capabilities() {
                Ok(c) => Response::ok(id, &c),
                Err(e) => Response::err(id, e),
            },
            Op::GetSensors => match self.backend.read_sensors() {
                Ok(s) => Response::ok(id, &s),
                Err(e) => Response::err(id, e),
            },
            Op::GetNetwork => match self.backend.network() {
                Ok(network) => Response::ok(id, network),
                Err(error) => Response::err(id, error),
            },
            Op::GetFanState => match self.backend.fan_state() {
                Ok(s) => Response::ok(id, &s),
                Err(e) => Response::err(id, e),
            },
            Op::SetFanMode { mode } => match self.set_fan_mode(mode) {
                Ok(()) => Response::ok(id, "applied"),
                Err(e) => Response::err(id, e),
            },
            Op::GetFanCurve => {
                let cfg = self.config.lock().unwrap();
                Response::ok(id, &cfg.fan.curve)
            }
            Op::SetFanCurve { curve } => match curve.validate() {
                Ok(()) => {
                    let saved = {
                        let mut cfg = self.config.lock().unwrap();
                        let mut next = cfg.clone();
                        next.fan.curve = curve;
                        next.save_to(&OmenConfig::daemon_path()).map(|()| *cfg = next)
                    };
                    match saved {
                        Ok(()) => Response::ok(id, "saved"),
                        Err(e) => Response::err(id, e),
                    }
                }
                Err(e) => Response::err(id, e),
            },
            Op::GetProfile => match self.read_profile() {
                Ok(p) => Response::ok(id, p),
                Err(e) => Response::err(id, e),
            },
            Op::GetProfileChoices => Response::ok(id, self.profile_choices()),
            Op::SetProfile { profile } => match self.set_manual_profile(profile) {
                Ok(()) => Response::ok(id, "applied"),
                Err(e) => Response::err(id, e),
            },
            Op::CompareSetProfile { expected, profile } => {
                match self.compare_set_profile(expected, profile) {
                    Ok(outcome) => Response::ok(id, outcome),
                    Err(e) => Response::err(id, e),
                }
            }
            Op::BeginGameProfile { token, profile } => match self.begin_game_profile(token, profile) {
                Ok(started) => Response::ok(id, started),
                Err(error) => Response::err(id, error),
            },
            Op::FinishGameProfile { token, restore } => match self.finish_game_profile(&token, restore) {
                Ok(outcome) => Response::ok(id, outcome),
                Err(error) => Response::err(id, error),
            },
            Op::GetLighting => match self.backend.lighting_state() {
                Ok(s) => Response::ok(id, &s),
                Err(e) => Response::err(id, e),
            },
            Op::SetLighting { state } => match self.backend.set_lighting(&state) {
                Ok(()) => Response::ok(id, "applied"),
                Err(e) => Response::err(id, e),
            },
            Op::GetBattery => match self.backend.battery() {
                Ok(b) => Response::ok(id, &b),
                Err(e) => Response::err(id, e),
            },
            Op::GetAc => match self.backend.ac() {
                Ok(a) => Response::ok(id, &a),
                Err(e) => Response::err(id, e),
            },
            Op::GetChargeThreshold => match self.backend.charge_threshold() {
                Ok(t) => Response::ok(id, t),
                Err(e) => Response::err(id, e),
            },
            Op::SetChargeThreshold { pct } => match self.backend.set_charge_threshold(pct) {
                Ok(()) => Response::ok(id, "applied"),
                Err(e) => Response::err(id, e),
            },
            Op::GetGraphics => {
                Response::ok(id, self.backend.graphics())
            }
            Op::GetGames => Response::err(id, OmenError::Unsupported(
                "game discovery belongs to the signed-in desktop user".into())),
            Op::LaunchGame { .. } => Response::err(id, OmenError::Unsupported(
                "the privileged service never launches user commands; launch from the desktop app".into())),
            Op::GetConfig => {
                let cfg = self.config.lock().unwrap().clone();
                Response::ok(id, &cfg)
            }
            Op::SetConfig { config } => match self.put_config(config) {
                Ok(()) => Response::ok(id, "saved"),
                Err(e) => Response::err(id, e),
            },
            Op::GetServiceSettings => {
                let config = self.config.lock().unwrap();
                Response::ok(id, omen_core::settings::ServiceSettings::from(&*config))
            },
            Op::SetServiceSettings { settings } => match self.put_service_settings(settings) {
                Ok(()) => Response::ok(id, "saved"),
                Err(error) => Response::err(id, error),
            },
            Op::ExportDiagnostics => match self.diagnostics() {
                Ok(v) => Response::ok(id, &v),
                Err(e) => Response::err(id, e),
            },
            Op::Shutdown => {
                self.shutdown_requested.store(true, Ordering::SeqCst);
                Response::ok(id, "shutting down")
            }
        }
    }

    fn try_capabilities(&self) -> Result<Capabilities> {
        let mut caps = self.backend.capabilities();
        if caps.performance_profiles.is_available()
            && self.config.lock().unwrap().power.battery_aware
        {
            caps.performance_profiles = omen_core::capability::Capability::read_only("battery automation owns profile selection; disable it before manual profile changes");
        }
        Ok(caps)
    }

    fn set_manual_profile(&self, profile: ProfileKind) -> Result<()> {
        let mut state = self.profile_write.lock().unwrap();
        self.check_manual_profile_policy()?;
        self.validate_profile_choice(profile)?;
        self.managed_games.release(None,None,"A manual daemon profile change released this hold. Tracking continues without rearming.")?;
        state.game = None;
        self.backend.set_profile(profile)
    }

    fn check_manual_profile_policy(&self) -> Result<()> {
        let config = self.config.lock().unwrap().clone();
        if self.shutdown_requested() {
            return Err(OmenError::PermissionDenied(
                "daemon is shutting down".into(),
            ));
        }
        if config.power.battery_aware {
            return Err(OmenError::SafetyLockout(
                "battery automation is enabled; disable it before manual profile changes".into(),
            ));
        }
        Ok(())
    }

    fn validate_profile_choice(&self, profile: ProfileKind) -> Result<()> {
        let caps = self.backend.capabilities();
        if !caps.performance_profiles.is_available() || !caps.profile_choices.contains(&profile) {
            return Err(OmenError::Unsupported(
                "requested profile control is unavailable".into(),
            ));
        }
        Ok(())
    }

    pub fn game_sessions(&self, uid: u32) -> omen_core::games::GameSessionReport {
        let mut report = self.managed_games.report(uid);
        report.mock = self.mock;
        report
    }

    pub fn start_managed_game(
        &self,
        uid: u32,
        token: String,
        game_id: String,
        profile: ProfileKind,
        scope: omen_hardware::game_scope::GameScope,
    ) -> Result<omen_core::games::GameSession> {
        let mut state = self.profile_write.lock().unwrap_or_else(|e| e.into_inner());
        self.check_manual_profile_policy()?;
        self.validate_profile_choice(profile)?;
        let now = crate::thermal::boottime_ms()?;
        if state
            .game
            .as_ref()
            .is_some_and(|g| g.expires_boottime_ms > now)
        {
            return Err(OmenError::SafetyLockout("another launcher submission currently owns profile selection; try again after it completes".into()));
        }
        state.game = None;
        let reason = format!("Game session: {game_id}");
        self.managed_games
            .start(uid, token, game_id, profile, scope, || {
                self.backend.hold_profile(profile, &reason)
            })
    }

    pub fn release_game_session(&self, uid: u32, token: &str) -> Result<()> {
        let _state = self.profile_write.lock().unwrap_or_else(|e| e.into_inner());
        self.managed_games.release(Some(uid),Some(token),"The desktop user released this profile hold. The process tree continues to be tracked.")
    }

    /// This protects against other daemon operations, not direct firmware or
    /// sysfs writers. In particular it cannot detect an external ABA change.
    fn compare_set_profile(
        &self,
        expected: ProfileKind,
        profile: ProfileKind,
    ) -> Result<CompareSetProfileOutcome> {
        let mut state = self.profile_write.lock().unwrap();
        self.check_manual_profile_policy()?;
        state.game = None;
        let observed = self.backend.profile()?;
        if observed != expected {
            return Ok(CompareSetProfileOutcome::Mismatch { observed });
        }
        self.validate_profile_choice(profile)?;
        self.managed_games.release(
            None,
            None,
            "A manual profile restoration released this hold. Tracking continues without rearming.",
        )?;
        self.backend.set_profile(profile)?;
        Ok(CompareSetProfileOutcome::Applied)
    }

    // Ownership only serializes writes through this daemon. Direct sysfs or
    // firmware writers can still perform an undetectable same-value/ABA change.
    fn begin_game_profile(
        &self,
        token: String,
        requested: ProfileKind,
    ) -> Result<GameProfileStarted> {
        if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(OmenError::InvalidArgument(
                "invalid game transaction token".into(),
            ));
        }
        let mut state = self.profile_write.lock().unwrap();
        self.check_manual_profile_policy()?;
        if self.managed_games.has_holds() {
            return Err(OmenError::SafetyLockout("managed games currently own profile holds; use their wrappers or release their holds before a manual bound launch".into()));
        }
        let now = crate::thermal::boottime_ms()?;
        if state
            .game
            .as_ref()
            .is_some_and(|t| t.expires_boottime_ms > now)
        {
            return Err(OmenError::SafetyLockout(
                "another game launch profile transaction is in progress".into(),
            ));
        }
        state.game = None;
        let caps = self.backend.capabilities();
        if !caps.performance_profiles.is_available() || !caps.profile_choices.contains(&requested) {
            return Err(OmenError::Unsupported(
                "requested game profile is not available".into(),
            ));
        }
        let original = self.backend.profile()?;
        let changed = original != requested;
        // Record ownership before attempting the write: a failed or timed-out
        // backend write might still have changed the hardware.
        state.game = Some(GameProfileTransaction {
            token,
            original,
            requested,
            expires_boottime_ms: now.saturating_add(30_000),
        });
        if changed {
            self.backend.set_profile(requested)?;
            let observed = self.backend.profile()?;
            if observed != requested {
                return Err(OmenError::StateMismatch(format!(
                    "requested {requested}, read back {observed}"
                )));
            }
        }
        Ok(GameProfileStarted {
            original,
            requested,
            changed,
        })
    }

    fn finish_game_profile(&self, token: &str, restore: bool) -> Result<GameProfileFinished> {
        let mut state = self.profile_write.lock().unwrap();
        let Some(transaction) = state.game.as_ref() else {
            return Ok(GameProfileFinished::NotOwned);
        };
        if transaction.token != token
            || transaction.expires_boottime_ms <= crate::thermal::boottime_ms()?
        {
            return Ok(GameProfileFinished::NotOwned);
        }
        if !restore {
            state.game = None;
            return Ok(GameProfileFinished::Released);
        }
        self.check_manual_profile_policy()?;
        let original = transaction.original;
        let requested = transaction.requested;
        let observed = self.backend.profile()?;
        if observed == original {
            state.game = None;
            return Ok(GameProfileFinished::AlreadyOriginal { profile: original });
        }
        if observed != requested {
            state.game = None;
            return Ok(GameProfileFinished::ExternalChangePreserved { observed });
        }
        self.backend.set_profile(original)?;
        let observed = self.backend.profile()?;
        if observed != original {
            return Err(OmenError::StateMismatch(format!(
                "requested {original}, read back {observed}"
            )));
        }
        state.game = None;
        Ok(GameProfileFinished::Restored { profile: original })
    }

    fn read_profile(&self) -> Result<ProfileKind> {
        // A recovery read must not overtake a still-running write whose
        // client timed out. It may time out too, but must not confirm stale state.
        let _profile = self.profile_write.lock().unwrap();
        self.backend.profile()
    }

    fn set_fan_mode(&self, requested: FanMode) -> Result<()> {
        let mut mode = self.fan_mode.lock().unwrap_or_else(|e| e.into_inner());
        if self.shutdown_requested() {
            return Err(OmenError::PermissionDenied(
                "daemon is shutting down".into(),
            ));
        }
        let caps = self.backend.capabilities();
        let capability = if matches!(requested, FanMode::Auto | FanMode::Max) {
            &caps.fan_modes
        } else {
            &caps.fan_control
        };
        if !capability.is_available() {
            return Err(OmenError::Unsupported(capability.detail().into()));
        }
        let curve = self.config.lock().unwrap().fan.curve.clone();
        if requested == FanMode::Curve {
            curve.validate()?;
        }
        if let Err(error) = self.backend.begin_fan_control() {
            self.stop_curve(&mut mode, &error);
            return Err(error);
        }
        // Read thermal inputs after the independent writer is ready: process
        // startup/configuration waits must not age a preflight sample.
        let guard = if requested != FanMode::Auto {
            match self.thermal_frame() {
                Ok((frame, last_boot_ms)) => Some(crate::thermal::Guard {
                    frame,
                    last_boot_ms,
                }),
                Err(error) => {
                    self.stop_curve(&mut mode, &error);
                    return Err(error);
                }
            }
        } else {
            None
        };
        // Mark ownership before an attempt: partial writes also need recovery.
        self.fan_owned.store(true, Ordering::SeqCst);
        let result = if let (FanMode::Curve, Some(guard)) = (requested, &guard) {
            self.backend.set_fan_mode(FanMode::Manual {
                duty_pct: curve.duty_at(guard.frame.hottest_c),
            })
        } else {
            self.backend.set_fan_mode(requested)
        };
        match result {
            Ok(()) => {
                mode.requested = requested;
                mode.guard = guard;
                mode.last_error = None;
                mode.recovery_error = None;
                if requested == FanMode::Auto {
                    self.fan_owned.store(false, Ordering::SeqCst);
                }
                Ok(())
            }
            Err(e) => {
                self.stop_curve(&mut mode, &e);
                Err(e)
            }
        }
    }

    fn thermal_frame(&self) -> Result<(crate::thermal::Frame, u64)> {
        let start = crate::thermal::boottime_ms()?;
        let before = self.backend.graphics();
        let samples = self.backend.read_sensors()?;
        let after = self.backend.graphics();
        let end = crate::thermal::boottime_ms()?;
        if before != after
            || end
                .checked_sub(start)
                .is_none_or(|elapsed| elapsed > crate::thermal::MAX_SAMPLE_AGE_MS)
        {
            return Err(OmenError::StateMismatch(
                "thermal acquisition stalled or GPU inventory changed".into(),
            ));
        }
        Ok((
            crate::thermal::evaluate(&samples, &after, crate::thermal::unix_ms()?)?,
            end,
        ))
    }

    fn recover_auto(&self, mode: &mut FanPolicy) {
        if !self.fan_owned.load(Ordering::SeqCst) {
            return;
        }
        match self.backend.set_fan_mode(FanMode::Auto) {
            Ok(()) => {
                self.fan_owned.store(false, Ordering::SeqCst);
                mode.recovery_error = None;
                log::info!(
                    "automatic fan policy readback verified; physical cooling remains unverified"
                );
            }
            Err(error) => {
                mode.recovery_error = Some(error.to_string());
                log::error!("automatic fan recovery failed: {error}; recovery is not guaranteed");
            }
        }
    }

    fn stop_curve(&self, mode: &mut FanPolicy, error: &OmenError) {
        mode.requested = FanMode::Auto;
        mode.guard = None;
        mode.last_error = Some(error.to_string());
        self.recover_auto(mode);
    }

    /// The daemon owns a single control loop. Holding the mode lock across the
    /// write prevents a stale curve worker from overwriting Auto or Shutdown.
    pub fn fan_policy_tick(&self) {
        let mut mode = self.fan_mode.lock().unwrap_or_else(|e| e.into_inner());
        if mode.requested == FanMode::Auto {
            self.recover_auto(&mut mode);
            return;
        }
        let requested = mode.requested;
        let result = (|| -> Result<()> {
            let curve = self.config.lock().unwrap().fan.curve.clone();
            curve.validate()?;
            let guard = mode
                .guard
                .as_mut()
                .ok_or_else(|| OmenError::StateMismatch("fan thermal guard is missing".into()))?;
            guard.check_gap(crate::thermal::boottime_ms()?)?;
            let (frame, now_boot_ms) = self.thermal_frame()?;
            guard.advance(frame, now_boot_ms)?;
            if requested == FanMode::Curve {
                self.backend.set_fan_mode(FanMode::Manual {
                    duty_pct: curve.duty_at(guard.frame.hottest_c),
                })
            } else {
                // Fixed overrides need the same source/coverage/freshness and
                // suspend-gap checks before renewing their independent lease.
                self.backend.maintain_fan_lease()
            }
        })();
        if let Err(e) = result {
            log::error!("fan thermal tick failed: {e}");
            self.stop_curve(&mut mode, &e);
        }
    }

    fn profile_choices(&self) -> Vec<ProfileKind> {
        self.backend.capabilities().profile_choices
    }

    fn validate_service_policy(&self, config: &OmenConfig) -> Result<()> {
        if config.power.battery_aware {
            let caps = self.backend.capabilities();
            if !caps.performance_profiles.is_available()
                || !caps
                    .profile_choices
                    .contains(&config.power.on_battery_profile)
                || config
                    .power
                    .on_ac_profile
                    .is_some_and(|p| !caps.profile_choices.contains(&p))
            {
                return Err(OmenError::Unsupported("battery automation requires enabled profile control and advertised target profiles".into()));
            }
        }
        Ok(())
    }

    fn put_service_settings(&self, settings: omen_core::settings::ServiceSettings) -> Result<()> {
        let validation = settings.apply_to(&OmenConfig::default())?;
        self.validate_service_policy(&validation)?;
        let mut profile = self.profile_write.lock().unwrap();
        let mut current = self.config.lock().unwrap();
        let next = settings.apply_to(&current)?;
        if next.power != current.power {
            self.managed_games.release(None,None,"Profile policy changed; the managed hold was released before automation took ownership.")?;
        }
        next.save_to(&OmenConfig::daemon_path())?;
        *current = next;
        profile.game = None;
        Ok(())
    }

    fn put_config(&self, config: OmenConfig) -> Result<()> {
        config.validate()?;
        self.validate_service_policy(&config)?;
        let mut profile = self.profile_write.lock().unwrap();
        let mut current = self.config.lock().unwrap();
        if config.power != current.power {
            self.managed_games.release(None,None,"Profile policy changed; the managed hold was released before automation took ownership.")?;
        }
        config.save_to(&OmenConfig::daemon_path())?;
        *current = config;
        profile.game = None;
        Ok(())
    }

    fn diagnostics(&self) -> Result<serde_json::Value> {
        let caps = self.try_capabilities()?;
        let sensors = self.backend.read_sensors().unwrap_or_default();
        let fan = self.backend.fan_state().ok();
        let battery: Option<BatteryInfo> = self.backend.battery().ok();
        let ac: Option<AcInfo> = self.backend.ac().ok();
        let cfg = self.config.lock().unwrap().clone();
        let lighting: Option<LightingState> = self.backend.lighting_state().ok();
        let network = self.backend.network();
        let fan_policy = {
            let policy = self.fan_mode.lock().unwrap_or_else(|e| e.into_inner());
            serde_json::json!({"requested": policy.requested, "guard": policy.guard,
                "owns_override": self.fan_owned.load(Ordering::SeqCst),
                "last_error": policy.last_error, "recovery_error": policy.recovery_error})
        };
        Ok(serde_json::json!({
            "generated_unix": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            "machine": caps.machine,
            "compatibility_evidence": omen_core::compatibility::evidence(&caps.machine, self.mock),
            "capabilities": caps,
            "sensors": sensors,
            "network": network.as_ref().ok(),
            "network_error": network.as_ref().err().map(ToString::to_string),
            "fan": fan,
            "fan_policy": fan_policy,
            "fan_supervisor": self.backend.fan_supervisor(),
            "battery": battery,
            "ac": ac,
            "lighting": lighting,
            "config": cfg,
            "mock": self.mock,
        }))
    }

    /// Battery-aware profile switching, invoked from the monitor thread.
    pub fn power_policy_tick(&self) {
        let _profile = self.profile_write.lock().unwrap();
        let cfg = self.config.lock().unwrap().clone();
        if self.shutdown_requested() {
            return;
        }
        if !cfg.power.battery_aware {
            return;
        }
        let caps = self.backend.capabilities();
        if !caps.performance_profiles.is_available() {
            return;
        }
        let Ok(bat) = self.backend.battery() else {
            return;
        };
        let Ok(ac) = self.backend.ac() else {
            return;
        };
        let target = if ac.online {
            cfg.power.on_ac_profile
        } else if bat.capacity_pct.map(|c| c <= 20).unwrap_or(false) {
            Some(cfg.power.on_battery_profile)
        } else {
            None // on battery, above reserve: leave user choice alone
        };
        if let Some(p) = target {
            if !caps.profile_choices.contains(&p) {
                return;
            }
            if self.backend.profile().ok() == Some(p) {
                return;
            }
            if self.shutdown_requested() {
                return;
            }
            if let Err(e) = self.backend.set_profile(p) {
                log::debug!("power policy: set {p} failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::fans::FanState;
    use omen_hardware::mock::{MockBackend, MockModel};

    /// Serialize tests which override process-wide configuration paths.
    static SVC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct TestContext {
        _guard: std::sync::MutexGuard<'static, ()>,
        _directory: tempfile::TempDir,
        user: Option<std::ffi::OsString>,
        system: Option<std::ffi::OsString>,
    }
    impl Drop for TestContext {
        fn drop(&mut self) {
            for (name, previous) in [
                ("OMEN_CONFIG_DIR", &self.user),
                ("OMEN_SYSTEM_CONFIG_DIR", &self.system),
            ] {
                if let Some(previous) = previous {
                    std::env::set_var(name, previous);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn service(model: MockModel) -> (Arc<HardwareService>, TestContext) {
        let context = TestContext {
            _guard: SVC_LOCK.lock().unwrap(),
            _directory: tempfile::tempdir().unwrap(),
            user: std::env::var_os("OMEN_CONFIG_DIR"),
            system: std::env::var_os("OMEN_SYSTEM_CONFIG_DIR"),
        };
        let cfg = OmenConfig::default();
        std::env::set_var("OMEN_CONFIG_DIR", context._directory.path().join("user"));
        std::env::set_var(
            "OMEN_SYSTEM_CONFIG_DIR",
            context._directory.path().join("system"),
        );
        (
            HardwareService::new(Arc::new(MockBackend::new(model)), false, cfg),
            context,
        )
    }

    #[test]
    fn game_profile_transaction_serializes_and_recovers() {
        let (service, _context) = service(MockModel::VictusFb);
        let original = service.backend.profile().unwrap();
        let token = "11111111111111111111111111111111";
        let started = service
            .begin_game_profile(token.into(), ProfileKind::Performance)
            .unwrap();
        assert_eq!(started.original, original);
        assert!(started.changed);
        assert!(matches!(
            service.begin_game_profile(
                "22222222222222222222222222222222".into(),
                ProfileKind::Quiet
            ),
            Err(OmenError::SafetyLockout(_))
        ));
        assert_eq!(
            service.finish_game_profile(token, true).unwrap(),
            GameProfileFinished::Restored { profile: original }
        );
        assert_eq!(service.backend.profile().unwrap(), original);
    }

    #[test]
    fn simultaneous_game_transactions_have_one_owner() {
        let (service, _context) = service(MockModel::VictusFb);
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for token in [
            "11111111111111111111111111111111",
            "22222222222222222222222222222222",
        ] {
            let service = service.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                service.begin_game_profile(token.into(), ProfileKind::Performance)
            }));
        }
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(OmenError::SafetyLockout(_))))
                .count(),
            1
        );
    }

    #[test]
    fn intervening_manual_aba_revokes_game_ownership() {
        let (service, _context) = service(MockModel::VictusFb);
        let token = "11111111111111111111111111111111";
        service
            .begin_game_profile(token.into(), ProfileKind::Performance)
            .unwrap();
        service.set_manual_profile(ProfileKind::Quiet).unwrap();
        service
            .set_manual_profile(ProfileKind::Performance)
            .unwrap();
        assert_eq!(
            service.finish_game_profile(token, true).unwrap(),
            GameProfileFinished::NotOwned
        );
        assert_eq!(service.backend.profile().unwrap(), ProfileKind::Performance);
    }

    #[test]
    fn game_transaction_preserves_external_change_and_restart_loses_ownership() {
        let (service, _context) = service(MockModel::VictusFb);
        let token = "11111111111111111111111111111111";
        service
            .begin_game_profile(token.into(), ProfileKind::Performance)
            .unwrap();
        service.backend.set_profile(ProfileKind::Quiet).unwrap();
        assert_eq!(
            service.finish_game_profile(token, true).unwrap(),
            GameProfileFinished::ExternalChangePreserved {
                observed: ProfileKind::Quiet
            }
        );
        service
            .begin_game_profile(token.into(), ProfileKind::Performance)
            .unwrap();
        let restarted = HardwareService::new(service.backend.clone(), false, OmenConfig::default());
        assert_eq!(
            restarted.finish_game_profile(token, true).unwrap(),
            GameProfileFinished::NotOwned
        );
        assert_eq!(
            restarted.backend.profile().unwrap(),
            ProfileKind::Performance
        );
    }

    #[test]
    fn expired_transaction_does_not_restore_or_block_another_launch() {
        let (service, _context) = service(MockModel::VictusFb);
        let token = "11111111111111111111111111111111";
        service
            .begin_game_profile(token.into(), ProfileKind::Performance)
            .unwrap();
        service
            .profile_write
            .lock()
            .unwrap()
            .game
            .as_mut()
            .unwrap()
            .expires_boottime_ms = 0;
        assert_eq!(
            service.finish_game_profile(token, true).unwrap(),
            GameProfileFinished::NotOwned
        );
        assert!(service
            .begin_game_profile(
                "22222222222222222222222222222222".into(),
                ProfileKind::Quiet
            )
            .is_ok());
    }

    #[test]
    fn unsupported_automatic_profile_is_refused_before_persistence() {
        let (service, _context) = service(MockModel::VictusFb);
        assert!(!service
            .backend
            .capabilities()
            .profile_choices
            .contains(&ProfileKind::Cool));
        let previous = service.backend.profile().unwrap();
        let mut settings = omen_core::settings::ServiceSettings::from(&OmenConfig::default());
        settings.power.battery_aware = true;
        settings.power.on_battery_profile = ProfileKind::Cool;
        let result = service.handle(1, Op::SetServiceSettings { settings });
        assert!(matches!(result.error, Some(OmenError::Unsupported(_))));
        assert!(!OmenConfig::daemon_path().exists());
        assert_eq!(service.backend.profile().unwrap(), previous);
        let current: omen_core::settings::ServiceSettings =
            service.handle(2, Op::GetServiceSettings).decode().unwrap();
        assert!(!current.power.battery_aware);
    }

    #[test]
    fn enabled_battery_policy_blocks_manual_profile_writes_and_advertises_read_only() {
        let (service, _context) = service(MockModel::VictusFb);
        let original = service.backend.profile().unwrap();
        let mut settings = omen_core::settings::ServiceSettings::from(&OmenConfig::default());
        settings.power.battery_aware = true;
        settings.power.on_ac_profile = None;
        assert!(service.handle(1, Op::SetServiceSettings { settings }).ok);
        let result = service.handle(
            2,
            Op::SetProfile {
                profile: ProfileKind::Performance,
            },
        );
        assert!(matches!(result.error, Some(OmenError::SafetyLockout(_))));
        assert!(!service
            .try_capabilities()
            .unwrap()
            .performance_profiles
            .is_available());
        assert_eq!(service.backend.profile().unwrap(), original);
    }

    #[test]
    fn conditional_recovery_waits_for_inflight_profile_change() {
        let (service, _context) = service(MockModel::VictusFb);
        service
            .backend
            .set_profile(ProfileKind::Performance)
            .unwrap();
        let pending = service.profile_write.lock().unwrap();
        let other = service.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            done_tx
                .send(
                    other
                        .handle(
                            1,
                            Op::CompareSetProfile {
                                expected: ProfileKind::Performance,
                                profile: ProfileKind::Balanced,
                            },
                        )
                        .decode::<CompareSetProfileOutcome>(),
                )
                .unwrap();
        });
        started_rx.recv().unwrap();
        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_err());
        // Model an in-flight daemon write whose profile lock is still held.
        service.backend.set_profile(ProfileKind::Quiet).unwrap();
        drop(pending);
        assert_eq!(
            done_rx.recv().unwrap().unwrap(),
            CompareSetProfileOutcome::Mismatch {
                observed: ProfileKind::Quiet
            }
        );
        thread.join().unwrap();
        assert_eq!(service.backend.profile().unwrap(), ProfileKind::Quiet);
    }

    #[test]
    fn conditional_profile_write_obeys_battery_policy() {
        let (service, _context) = service(MockModel::VictusFb);
        let original = service.backend.profile().unwrap();
        let mut settings = omen_core::settings::ServiceSettings::from(&OmenConfig::default());
        settings.power.battery_aware = true;
        settings.power.on_ac_profile = None;
        assert!(service.handle(1, Op::SetServiceSettings { settings }).ok);
        let result = service.handle(
            2,
            Op::CompareSetProfile {
                expected: original,
                profile: ProfileKind::Performance,
            },
        );
        assert!(matches!(result.error, Some(OmenError::SafetyLockout(_))));
        assert_eq!(service.backend.profile().unwrap(), original);
    }

    #[test]
    fn profile_read_cannot_overtake_an_inflight_write() {
        let (service, _context) = service(MockModel::VictusFb);
        let pending_write = service.profile_write.lock().unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let other = service.clone();
        let thread = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            result_tx
                .send(other.handle(1, Op::GetProfile).decode::<ProfileKind>())
                .unwrap();
        });
        entered_rx.recv().unwrap();
        assert!(matches!(
            result_rx.recv_timeout(std::time::Duration::from_millis(30)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        service
            .backend
            .set_profile(ProfileKind::Performance)
            .unwrap();
        drop(pending_write);
        assert_eq!(
            result_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            ProfileKind::Performance
        );
        thread.join().unwrap();
    }

    #[test]
    fn fan_recovery_precedes_waiting_for_profile_io() {
        let (service, _context) = service(MockModel::FanControlOmen);
        assert!(
            service
                .handle(
                    1,
                    Op::SetFanMode {
                        mode: FanMode::Manual { duty_pct: 80 }
                    }
                )
                .ok
        );
        let pending_profile = service.profile_write.lock().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let other = service.clone();
        let thread = std::thread::spawn(move || {
            other.restore_safe_state();
            done_tx.send(()).unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut automatic = false;
        while std::time::Instant::now() < deadline {
            if service.backend.fan_state().unwrap().mode == Some(FanMode::Auto) {
                automatic = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let still_draining = done_rx.try_recv().is_err();
        drop(pending_profile);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        thread.join().unwrap();
        assert!(automatic, "fan recovery waited on unrelated profile IO");
        assert!(still_draining);
    }

    #[test]
    fn queued_fan_mutation_rechecks_shutdown_after_acquiring_its_guard() {
        let (service, _context) = service(MockModel::FanControlOmen);
        let pending = service.fan_mode.lock().unwrap();
        let other = service.clone();
        let thread =
            std::thread::spawn(move || other.set_fan_mode(FanMode::Manual { duty_pct: 80 }));
        service.shutdown_requested.store(true, Ordering::SeqCst);
        drop(pending);
        assert!(matches!(
            thread.join().unwrap(),
            Err(OmenError::PermissionDenied(_))
        ));
        assert_eq!(
            service.backend.fan_state().unwrap().mode,
            Some(FanMode::Auto)
        );
    }

    #[test]
    fn info_reports_machine() {
        let (s, _g) = service(MockModel::VictusFb);
        let resp = s.handle(1, Op::GetInfo);
        assert!(resp.ok);
        let info: DaemonInfo = resp.decode().unwrap();
        assert_eq!(info.machine.board_name, "8C30");
        assert_eq!(info.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn fan_write_refused_on_unsupported_board() {
        let (s, _g) = service(MockModel::VictusFb);
        let resp = s.handle(
            2,
            Op::SetFanMode {
                mode: FanMode::Manual { duty_pct: 70 },
            },
        );
        assert!(!resp.ok);
        let e = resp.error.unwrap();
        assert!(matches!(e, OmenError::Unsupported(_)));
    }

    #[test]
    fn curve_refused_without_fan_control() {
        let (s, _g) = service(MockModel::VictusFb);
        let resp = s.handle(
            3,
            Op::SetFanMode {
                mode: FanMode::Curve,
            },
        );
        assert!(!resp.ok);
    }

    #[test]
    fn curve_engine_applies_duty_on_supported_board() {
        let (s, _g) = service(MockModel::FanControlOmen);
        // Simulate a hot CPU through the shared fixture: the engine's backend
        // is the real SysfsBackend, so drive state through the mock's files.
        let resp = s.handle(
            4,
            Op::SetFanMode {
                mode: FanMode::Curve,
            },
        );
        assert!(resp.ok, "{resp:?}");
        // Engine thread applies within its first tick; wait a moment.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let st: FanState = s.handle(5, Op::GetFanState).decode().unwrap();
        // Fixture temp is 61.8 °C → default curve duty ~52%.
        assert!(
            matches!(st.mode, Some(FanMode::Manual { duty_pct }) if (45..=60).contains(&duty_pct)),
            "curve engine did not apply manual duty: {st:?}"
        );
        // Shut the engine down and confirm auto restore.
        s.handle(
            6,
            Op::SetFanMode {
                mode: FanMode::Auto,
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        let st: FanState = s.handle(7, Op::GetFanState).decode().unwrap();
        assert_eq!(st.mode, Some(FanMode::Auto));
    }

    #[test]
    fn restore_safe_state_returns_auto() {
        let (s, _g) = service(MockModel::FanControlOmen);
        let _ = s.handle(
            1,
            Op::SetFanMode {
                mode: FanMode::Manual { duty_pct: 90 },
            },
        );
        s.restore_safe_state();
        let st: FanState = s.handle(2, Op::GetFanState).decode().unwrap();
        assert_eq!(st.mode, Some(FanMode::Auto));
    }

    #[test]
    fn curve_config_validated_before_save() {
        let (s, _g) = service(MockModel::VictusFb);
        let bad = omen_core::fans::FanCurve {
            points: vec![
                omen_core::fans::CurvePoint {
                    temp_c: 50,
                    duty_pct: 90,
                },
                omen_core::fans::CurvePoint {
                    temp_c: 40,
                    duty_pct: 20,
                },
            ],
        };
        let resp = s.handle(1, Op::SetFanCurve { curve: bad });
        assert!(!resp.ok);
    }

    #[test]
    fn profile_roundtrip_ppd() {
        let (s, _g) = service(MockModel::VictusFb);
        let choices: Vec<ProfileKind> = s.handle(1, Op::GetProfileChoices).decode().unwrap();
        assert!(choices.contains(&ProfileKind::Performance));
        let _ = s.handle(
            2,
            Op::SetProfile {
                profile: ProfileKind::Performance,
            },
        );
        let got: ProfileKind = s.handle(3, Op::GetProfile).decode().unwrap();
        assert_eq!(got, ProfileKind::Performance);
    }

    #[test]
    fn diagnostics_contains_machine_and_capabilities() {
        let (s, _g) = service(MockModel::VictusFb);
        let v: serde_json::Value = s.handle(1, Op::ExportDiagnostics).decode().unwrap();
        assert!(v.get("machine").is_some());
        assert!(v.get("capabilities").is_some());
        assert!(v.get("sensors").is_some());
    }

    #[test]
    fn cpu_only_fixture_has_an_explicit_empty_gpu_inventory() {
        let (s, _g) = service(MockModel::FanControlOmen);
        assert_eq!(s.thermal_frame().unwrap().0.hottest_c, 61.8);
    }

    #[test]
    fn privileged_service_never_launches_user_commands() {
        let (s, _g) = service(MockModel::VictusFb);
        let response = s.handle(
            1,
            Op::LaunchGame {
                id: "custom/arbitrary".into(),
            },
        );
        assert!(matches!(response.error, Some(OmenError::Unsupported(_))));
    }

    #[test]
    fn curve_tick_cannot_overwrite_auto_and_missing_sensor_recovers() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let s = HardwareService::new(backend.clone(), true, OmenConfig::default());
        assert!(
            s.handle(
                1,
                Op::SetFanMode {
                    mode: FanMode::Curve
                }
            )
            .ok
        );
        backend.set_hwmon_dir("hwmon1", "temp1_input", "invalid");
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        backend.set_hwmon_dir("hwmon1", "temp1_input", "90000");
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
    }

    #[test]
    fn startup_and_default_power_policy_do_not_write() {
        let (s, _g) = service(MockModel::FanControlOmen);
        s.backend
            .set_fan_mode(FanMode::Manual { duty_pct: 80 })
            .unwrap();
        s.backend.set_profile(ProfileKind::Performance).unwrap();
        s.apply_startup_policy();
        s.power_policy_tick();
        assert!(matches!(
            s.backend.fan_state().unwrap().mode,
            Some(FanMode::Manual { .. })
        ));
        assert_eq!(s.backend.profile().unwrap(), ProfileKind::Performance);
    }

    fn add_fixture_gpu(backend: &MockBackend) -> std::path::PathBuf {
        let root = backend.dir.path();
        let device = root.join("sys/bus/pci/devices/0000:06:00.0");
        std::fs::create_dir_all(device.join("power")).unwrap();
        for (name, value) in [
            ("class", "0x030000"),
            ("vendor", "0x1002"),
            ("power/runtime_status", "active"),
        ] {
            std::fs::write(device.join(name), value).unwrap();
        }
        let driver = root.join("drivers/amdgpu");
        std::fs::create_dir_all(&driver).unwrap();
        std::os::unix::fs::symlink(driver, device.join("driver")).unwrap();
        let hwmon = root.join("sys/class/hwmon/hwmon2");
        std::fs::create_dir_all(&hwmon).unwrap();
        std::fs::write(hwmon.join("name"), "amdgpu").unwrap();
        std::fs::write(hwmon.join("temp1_input"), "45000").unwrap();
        std::os::unix::fs::symlink(device, hwmon.join("device")).unwrap();
        hwmon
    }

    #[test]
    fn gpu_heat_raises_fixture_curve_and_loss_stops_without_rearming() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let gpu = add_fixture_gpu(&backend);
        let s = HardwareService::new(backend.clone(), true, OmenConfig::default());
        assert!(s.set_fan_mode(FanMode::Curve).is_ok());
        std::fs::write(gpu.join("temp1_input"), "90000").unwrap();
        s.fan_policy_tick();
        assert!(backend.fan_state().unwrap().duty_pct.unwrap() >= 99);
        std::fs::write(gpu.join("temp1_input"), "invalid").unwrap();
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        std::fs::write(gpu.join("temp1_input"), "90000").unwrap();
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
    }

    #[test]
    fn failed_curve_preflight_does_not_claim_or_restore_an_external_override() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let gpu = add_fixture_gpu(&backend);
        std::fs::remove_file(gpu.join("device")).unwrap();
        backend.set_fan_mode(FanMode::Max).unwrap();
        let s = HardwareService::new(backend.clone(), true, OmenConfig::default());
        assert!(s.set_fan_mode(FanMode::Curve).is_err());
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Max));
        assert!(!s.fan_owned.load(Ordering::SeqCst));
    }

    #[test]
    fn stalled_curve_and_cohort_changes_restore_auto() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let s = HardwareService::new(backend.clone(), true, OmenConfig::default());
        s.set_fan_mode(FanMode::Curve).unwrap();
        s.fan_mode
            .lock()
            .unwrap()
            .guard
            .as_mut()
            .unwrap()
            .last_boot_ms = 0;
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        s.set_fan_mode(FanMode::Curve).unwrap();
        add_fixture_gpu(&backend);
        s.fan_policy_tick();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
    }

    #[test]
    fn failed_auto_recovery_remains_owned_and_retries_on_later_ticks() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let s = HardwareService::new(backend.clone(), true, OmenConfig::default());
        s.set_fan_mode(FanMode::Curve).unwrap();
        let conflict = backend.dir.path().join("proc/1234");
        std::fs::create_dir_all(&conflict).unwrap();
        std::fs::write(conflict.join("comm"), "omencore-cli").unwrap();
        s.fan_policy_tick();
        assert!(s.fan_owned.load(Ordering::SeqCst));
        assert!(s.fan_mode.lock().unwrap().recovery_error.is_some());
        std::fs::remove_dir_all(conflict).unwrap();
        s.fan_policy_tick();
        assert!(!s.fan_owned.load(Ordering::SeqCst));
        assert!(s.fan_mode.lock().unwrap().recovery_error.is_none());
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
    }

    #[test]
    fn queued_config_io_cannot_age_a_sample_before_a_manual_write() {
        let _g = SVC_LOCK.lock().unwrap();
        let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
        let service = HardwareService::new(backend.clone(), true, OmenConfig::default());
        service.set_fan_mode(FanMode::Curve).unwrap();
        backend.set_hwmon_dir("hwmon1", "temp1_input", "90000");
        let config = service.config.lock().unwrap();
        let worker = service.clone();
        let tick = std::thread::spawn(move || worker.fan_policy_tick());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while service.fan_mode.try_lock().is_ok() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        std::thread::sleep(std::time::Duration::from_millis(3_200));
        drop(config);
        tick.join().unwrap();
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
    }
    #[test]
    fn fixed_overrides_require_valid_preflight_and_preserve_external_state() {
        let _g = SVC_LOCK.lock().unwrap();
        for mode in [FanMode::Max, FanMode::Manual { duty_pct: 80 }] {
            let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
            let gpu = add_fixture_gpu(&backend);
            std::fs::remove_file(gpu.join("device")).unwrap();
            backend.set_fan_mode(FanMode::Max).unwrap();
            let service = HardwareService::new(backend.clone(), true, OmenConfig::default());
            assert!(service.set_fan_mode(mode).is_err());
            assert!(!service.fan_owned.load(Ordering::SeqCst));
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Max));
            // Automatic remains reachable with no usable thermal provider.
            service.set_fan_mode(FanMode::Auto).unwrap();
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        }
    }

    #[test]
    fn fixed_overrides_stop_on_gpu_data_loss_and_require_explicit_rearming() {
        let _g = SVC_LOCK.lock().unwrap();
        for mode in [FanMode::Max, FanMode::Manual { duty_pct: 80 }] {
            let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
            let gpu = add_fixture_gpu(&backend);
            let service = HardwareService::new(backend.clone(), true, OmenConfig::default());
            service.set_fan_mode(mode).unwrap();
            service.fan_policy_tick();
            assert_eq!(service.fan_mode.lock().unwrap().requested, mode);
            std::fs::write(gpu.join("temp1_input"), "invalid").unwrap();
            service.fan_policy_tick();
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
            std::fs::write(gpu.join("temp1_input"), "45000").unwrap();
            service.fan_policy_tick();
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
            service.set_fan_mode(mode).unwrap();
            assert_eq!(service.fan_mode.lock().unwrap().requested, mode);
        }
    }

    #[test]
    fn fixed_overrides_stop_on_suspend_gaps_and_gpu_cohort_changes() {
        let _g = SVC_LOCK.lock().unwrap();
        for mode in [FanMode::Max, FanMode::Manual { duty_pct: 80 }] {
            let backend = Arc::new(MockBackend::new(MockModel::FanControlOmen));
            let service = HardwareService::new(backend.clone(), true, OmenConfig::default());
            service.set_fan_mode(mode).unwrap();
            service
                .fan_mode
                .lock()
                .unwrap()
                .guard
                .as_mut()
                .unwrap()
                .last_boot_ms = 0;
            service.fan_policy_tick();
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
            service.set_fan_mode(mode).unwrap();
            add_fixture_gpu(&backend);
            service.fan_policy_tick();
            assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        }
    }
}
