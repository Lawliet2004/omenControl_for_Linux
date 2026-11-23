//! Best-effort profile transactions around desktop launcher submission. This is
//! not a global lease: external same-value/ABA changes cannot be detected.
use omen_core::config::OmenConfig;
use omen_core::games::Game;
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use omen_hardware::games::{LaunchReceipt, PreparedLaunch};
use omen_ipc::protocol::{GameProfileFinished, GameProfileStarted};
use omen_ipc::{IpcClient, Op};
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    Preparation,
    ProfileRead,
    ProfileApply,
    Launcher,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileRecovery {
    NotNeeded,
    AlreadyOriginal { profile: ProfileKind },
    Restored { profile: ProfileKind },
    ExternalChangePreserved { observed: ProfileKind },
    Unverified { detail: String },
}
impl std::fmt::Display for ProfileRecovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotNeeded => write!(f, "this launch made no profile change"),
            Self::AlreadyOriginal { profile } => write!(f, "original {profile} profile confirmed"),
            Self::Restored { profile } => write!(f, "restored {profile} profile"),
            Self::ExternalChangePreserved { observed } => write!(
                f,
                "another controller selected {observed}; left that profile in place"
            ),
            Self::Unverified { detail } => {
                write!(f, "profile recovery could not be verified: {detail}")
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LaunchFailure {
    pub stage: LaunchStage,
    pub cause: OmenError,
    pub recovery: ProfileRecovery,
    pub original_profile: Option<ProfileKind>,
    pub requested_profile: Option<ProfileKind>,
}
impl std::fmt::Display for LaunchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cause {
            OmenError::LaunchFailed(message) => write!(f, "{message}; {}", self.recovery)?,
            cause => write!(f, "{cause}; {}", self.recovery)?,
        }
        if let (ProfileRecovery::Unverified { .. }, Some(original)) =
            (&self.recovery, self.original_profile)
        {
            write!(f, " (previous profile: {original})")?;
        }
        Ok(())
    }
}
impl std::error::Error for LaunchFailure {}
impl LaunchFailure {
    fn before_change(stage: LaunchStage, cause: OmenError) -> Self {
        Self {
            stage,
            cause,
            recovery: ProfileRecovery::NotNeeded,
            original_profile: None,
            requested_profile: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProfiledLaunch<T> {
    pub launcher: T,
    pub original_profile: Option<ProfileKind>,
    pub requested_profile: Option<ProfileKind>,
    pub profile_was_applied: bool,
    pub managed: bool,
}
impl ProfiledLaunch<LaunchReceipt> {
    pub fn message(&self) -> String {
        if self.managed {
            return "Launch requested, not verified. The configured game wrapper manages profile ownership and exit restoration; check Game sessions for tracking status.".into();
        }
        match (self.requested_profile, self.original_profile) {
            (Some(profile), Some(original)) if self.profile_was_applied => format!(
                "Launch requested, not verified; {profile} profile applied. Game exit is not tracked; restore the previous {original} profile manually after play"
            ),
            (Some(profile), _) => format!("Launch requested, not verified; {profile} profile was already active"),
            _ => "Launch requested, not verified".into(),
        }
    }
}

// A token is generated before the begin request so that a timed-out begin can
// still be followed by an ownership-checked recovery request.
pub(crate) fn new_transaction_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn owned_recovery(result: Result<GameProfileFinished>) -> ProfileRecovery {
    match result {
        Ok(GameProfileFinished::AlreadyOriginal { profile }) => ProfileRecovery::AlreadyOriginal { profile },
        Ok(GameProfileFinished::Restored { profile }) => ProfileRecovery::Restored { profile },
        Ok(GameProfileFinished::ExternalChangePreserved { observed }) => ProfileRecovery::ExternalChangePreserved { observed },
        Ok(GameProfileFinished::NotOwned) => ProfileRecovery::Unverified { detail: "transaction ownership was lost or the daemon restarted; no blind overwrite attempted".into() },
        Ok(GameProfileFinished::Released) => ProfileRecovery::Unverified { detail: "transaction was released without restoration".into() },
        Err(error) => ProfileRecovery::Unverified { detail: error.to_string() },
    }
}

fn owned_profiled_launch<T>(
    target: Option<ProfileKind>,
    token: String,
    mut begin: impl FnMut(&str, ProfileKind) -> Result<GameProfileStarted>,
    mut finish: impl FnMut(&str, bool) -> Result<GameProfileFinished>,
    launch: impl FnOnce() -> Result<T>,
) -> std::result::Result<ProfiledLaunch<T>, LaunchFailure> {
    let Some(target) = target else {
        return launch()
            .map(|launcher| ProfiledLaunch {
                launcher,
                original_profile: None,
                requested_profile: None,
                profile_was_applied: false,
                managed: false,
            })
            .map_err(|cause| LaunchFailure::before_change(LaunchStage::Launcher, cause));
    };
    let started = match begin(&token, target) {
        Ok(started) => started,
        Err(cause) => {
            return Err(LaunchFailure {
                stage: LaunchStage::ProfileApply,
                cause,
                recovery: owned_recovery(finish(&token, true)),
                original_profile: None,
                requested_profile: Some(target),
            })
        }
    };
    match launch() {
        Ok(launcher) => {
            // This lease only protects submission, not the game lifetime.
            // If release fails, it expires without making a hardware write.
            let _ = finish(&token, false);
            Ok(ProfiledLaunch {
                launcher,
                original_profile: Some(started.original),
                requested_profile: Some(target),
                profile_was_applied: started.changed,
                managed: false,
            })
        }
        Err(cause) => {
            let finished = finish(&token, started.changed);
            Err(LaunchFailure {
                stage: LaunchStage::Launcher,
                cause,
                recovery: if started.changed {
                    owned_recovery(finished)
                } else {
                    ProfileRecovery::NotNeeded
                },
                original_profile: Some(started.original),
                requested_profile: Some(target),
            })
        }
    }
}

pub struct DesktopGames {
    daemon: Option<IpcClient>,
    user_config: PathBuf,
}
impl DesktopGames {
    pub fn new(daemon: Option<IpcClient>, user_config: PathBuf) -> Self {
        Self {
            daemon,
            user_config,
        }
    }
    pub fn games(&self) -> Result<Vec<Game>> {
        let config = OmenConfig::load_from(&self.user_config)?;
        let mut games =
            omen_hardware::games::detect_with_locations(&config.games.library_locations);
        games.extend(config.games.custom_games);
        Ok(games)
    }
    fn daemon(&self) -> Result<&IpcClient> {
        self.daemon.as_ref().ok_or_else(|| OmenError::DaemonUnavailable("a bound profile needs the authorized daemon; remove the binding to launch without it".into()))
    }
    fn check_policy_conflict(&self) -> Result<()> {
        let settings: omen_core::settings::ServiceSettings =
            self.daemon()?.call(Op::GetServiceSettings)?.decode()?;
        if settings.power.battery_aware {
            return Err(OmenError::SafetyLockout("battery automation is enabled; disable it or remove the per-game binding before launch".into()));
        }
        Ok(())
    }
    pub fn launch_game(
        &self,
        id: &str,
    ) -> std::result::Result<ProfiledLaunch<LaunchReceipt>, LaunchFailure> {
        let request = new_transaction_token().ok();
        let outcome = self.launch_game_inner(id, request.as_deref());
        if let (Err(failure), Some(request)) = (&outcome, request) {
            use crate::launch_issues::{best_effort, History, Issue, Reason, Stage};
            let stage = match failure.stage {
                LaunchStage::Preparation => Stage::Preparation,
                LaunchStage::ProfileRead | LaunchStage::ProfileApply => Stage::Registration,
                LaunchStage::Launcher => Stage::Submission,
            };
            best_effort(
                &History::for_config(&self.user_config),
                Issue::new(
                    request,
                    id.into(),
                    None,
                    stage,
                    Reason::from_error(stage, &failure.cause),
                ),
            );
        }
        outcome
    }

    fn launch_game_inner(
        &self,
        id: &str,
        request: Option<&str>,
    ) -> std::result::Result<ProfiledLaunch<LaunchReceipt>, LaunchFailure> {
        let config = OmenConfig::load_from(&self.user_config)
            .map_err(|cause| LaunchFailure::before_change(LaunchStage::Preparation, cause))?;
        let mut games =
            omen_hardware::games::detect_with_locations(&config.games.library_locations);
        games.extend(config.games.custom_games);
        let game = games
            .into_iter()
            .find(|game| game.id == id)
            .ok_or_else(|| {
                LaunchFailure::before_change(
                    LaunchStage::Preparation,
                    OmenError::NotFound(format!("unknown game '{id}'")),
                )
            })?;
        let mut prepared = PreparedLaunch::prepare(&game)
            .map_err(|cause| LaunchFailure::before_change(LaunchStage::Preparation, cause))?;
        if config.games.per_game_profiles.contains_key(id) {
            self.check_policy_conflict()
                .map_err(|cause| LaunchFailure::before_change(LaunchStage::ProfileApply, cause))?;
        }
        let target = config.games.per_game_profiles.get(id).copied();
        let managed = config.games.managed_game_ids.contains(id);
        if managed && game.source == omen_core::games::GameSource::Custom {
            let cli = std::env::current_exe()
                .map_err(|cause| {
                    LaunchFailure::before_change(LaunchStage::Preparation, cause.into())
                })?
                .with_file_name("omen-cli");
            prepared = prepared
                .wrap_actual_command(&cli, id)
                .map_err(|cause| LaunchFailure::before_change(LaunchStage::Preparation, cause))?;
        }
        if let Some(request) = request {
            use crate::launch_issues::{best_effort, History, Issue, Reason, Stage};
            let history = History::for_config(&self.user_config);
            let request = request.to_owned();
            let game_id = id.to_owned();
            let correlate = if managed && game.source == omen_core::games::GameSource::Custom {
                Some(request.as_str())
            } else {
                None
            };
            prepared = prepared.observe_failure(correlate, {
                let request = request.clone();
                move |failure| {
                    let reason = match failure {
                        omen_hardware::games::LateLaunchFailure::Exited(status) => {
                            Reason::from_status(status).unwrap_or(Reason::Observation)
                        }
                        omen_hardware::games::LateLaunchFailure::Observation => Reason::Observation,
                    };
                    best_effort(
                        &history,
                        Issue::new(request, game_id, None, Stage::Submission, reason),
                    );
                }
            });
        }
        if managed {
            return prepared
                .start()
                .map(|launcher| ProfiledLaunch {
                    launcher,
                    original_profile: None,
                    requested_profile: target,
                    profile_was_applied: false,
                    managed: true,
                })
                .map_err(|cause| LaunchFailure::before_change(LaunchStage::Launcher, cause));
        }
        let token = if target.is_some() {
            new_transaction_token()
                .map_err(|cause| LaunchFailure::before_change(LaunchStage::Preparation, cause))?
        } else {
            String::new()
        };
        owned_profiled_launch(
            target,
            token,
            |token, profile| {
                self.daemon()?
                    .call(Op::BeginGameProfile {
                        token: token.into(),
                        profile,
                    })?
                    .decode()
            },
            |token, restore| {
                self.daemon()?
                    .call(Op::FinishGameProfile {
                        token: token.into(),
                        restore,
                    })?
                    .decode()
            },
            || prepared.start(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn failed() -> OmenError {
        OmenError::LaunchFailed("fixture failure".into())
    }
    #[test]
    fn receipt_explains_manual_restoration_and_uncertain_launch() {
        let launch = ProfiledLaunch {
            launcher: LaunchReceipt {
                process_id: 7,
                launcher_state: omen_hardware::games::LauncherState::RequestCompleted,
                startup_window_ms: 300,
                game_execution_verified: false,
            },
            original_profile: Some(ProfileKind::Balanced),
            requested_profile: Some(ProfileKind::Performance),
            profile_was_applied: true,
            managed: false,
        };
        let message = launch.message();
        assert!(message.contains("not verified"));
        assert!(message.contains("Balanced"));
        assert!(message.contains("manually"));
    }
    #[test]
    fn unbound_launch_does_not_access_profile_service() {
        let result = owned_profiled_launch(
            None,
            String::new(),
            |_, _| panic!("no begin"),
            |_, _| panic!("no finish"),
            || Ok(42),
        )
        .unwrap();
        assert_eq!(result.launcher, 42);
        assert!(!result.profile_was_applied);
    }

    #[test]
    fn failed_launch_recovers_owned_transaction() {
        let finish_called = Cell::new(false);
        let error = owned_profiled_launch::<()>(
            Some(ProfileKind::Performance),
            "token".into(),
            |token, profile| {
                assert_eq!(token, "token");
                Ok(GameProfileStarted {
                    original: ProfileKind::Balanced,
                    requested: profile,
                    changed: true,
                })
            },
            |token, restore| {
                assert_eq!(token, "token");
                assert!(restore);
                finish_called.set(true);
                Ok(GameProfileFinished::Restored {
                    profile: ProfileKind::Balanced,
                })
            },
            || Err(failed()),
        )
        .unwrap_err();
        assert!(finish_called.get());
        assert_eq!(error.stage, LaunchStage::Launcher);
        assert_eq!(
            error.recovery,
            ProfileRecovery::Restored {
                profile: ProfileKind::Balanced
            }
        );
    }

    #[test]
    fn uncertain_begin_attempts_owned_recovery_without_launching() {
        let error = owned_profiled_launch::<()>(
            Some(ProfileKind::Performance),
            "token".into(),
            |_, _| Err(OmenError::DaemonUnavailable("timed out".into())),
            |token, restore| {
                assert_eq!(token, "token");
                assert!(restore);
                Ok(GameProfileFinished::NotOwned)
            },
            || panic!("must not launch"),
        )
        .unwrap_err();
        assert!(matches!(error.recovery, ProfileRecovery::Unverified { .. }));
    }

    #[test]
    fn older_daemon_never_falls_back_to_an_unprotected_write() {
        let error = owned_profiled_launch::<()>(
            Some(ProfileKind::Performance),
            "token".into(),
            |_, _| Err(OmenError::Unsupported("unknown operation".into())),
            |_, _| Err(OmenError::Unsupported("unknown operation".into())),
            || panic!("bound launch must not proceed"),
        )
        .unwrap_err();
        assert_eq!(error.stage, LaunchStage::ProfileApply);
        assert!(matches!(error.recovery, ProfileRecovery::Unverified { .. }));
    }

    #[test]
    fn successful_launch_releases_transaction_without_restoring() {
        let result = owned_profiled_launch(
            Some(ProfileKind::Performance),
            "token".into(),
            |_, profile| {
                Ok(GameProfileStarted {
                    original: ProfileKind::Balanced,
                    requested: profile,
                    changed: true,
                })
            },
            |_, restore| {
                assert!(!restore);
                Ok(GameProfileFinished::Released)
            },
            || Ok(42),
        )
        .unwrap();
        assert_eq!(result.launcher, 42);
        assert_eq!(result.original_profile, Some(ProfileKind::Balanced));
        assert!(result.profile_was_applied);
    }
}
