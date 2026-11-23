//! Wire protocol types. Versioned by the `v` field; unknown ops fail
//! cleanly with `Unsupported` on the daemon side.

use omen_core::fans::FanMode;
use omen_core::lighting::LightingState;
use omen_core::profile::ProfileKind;
use omen_core::OmenError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    #[serde(default = "protocol_version")]
    pub v: u32,
    pub id: u64,
    pub op: Op,
}

fn protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum Op {
    Ping,
    /// Daemon + machine identity, protocol version.
    GetInfo,
    GetCapabilities,
    GetSensors,
    GetNetwork,
    GetFanState,
    SetFanMode {
        mode: FanMode,
    },
    /// Effective curve the daemon applies (when curve mode is active).
    GetFanCurve,
    SetFanCurve {
        curve: omen_core::fans::FanCurve,
    },
    GetProfile,
    GetProfileChoices,
    SetProfile {
        profile: ProfileKind,
    },
    /// Conditionally write while serialized with daemon profile changes.
    /// External hardware writers cannot be serialized by this operation.
    CompareSetProfile {
        expected: ProfileKind,
        profile: ProfileKind,
    },
    /// Start an exclusive, short-lived launch profile transaction.
    BeginGameProfile {
        token: String,
        profile: ProfileKind,
    },
    /// End a transaction; restore only if ownership and the expected state remain.
    FinishGameProfile {
        token: String,
        restore: bool,
    },
    /// Register the calling actual-command worker in its dedicated user scope.
    StartManagedGame {
        token: String,
        game_id: String,
        profile: ProfileKind,
    },
    GetGameSessions,
    /// Release this user's hold; never terminates the game.
    ReleaseGameSession {
        token: String,
    },
    GetLighting,
    SetLighting {
        state: LightingState,
    },
    GetBattery,
    GetAc,
    GetChargeThreshold,
    SetChargeThreshold {
        pct: u8,
    },
    GetGraphics,
    GetGames,
    /// Launch by game id; applies the per-game profile when configured.
    LaunchGame {
        id: String,
    },
    GetConfig,
    SetConfig {
        config: omen_core::config::OmenConfig,
    },
    GetServiceSettings,
    SetServiceSettings {
        settings: omen_core::settings::ServiceSettings,
    },
    ExportDiagnostics,
    /// Ask the daemon to restore firmware auto fan control and exit.
    Shutdown,
}

impl Op {
    /// Ops that change hardware or system state require authorization.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Op::SetFanMode { .. }
                | Op::SetFanCurve { .. }
                | Op::SetProfile { .. }
                | Op::CompareSetProfile { .. }
                | Op::BeginGameProfile { .. }
                | Op::FinishGameProfile { .. }
                | Op::StartManagedGame { .. }
                | Op::ReleaseGameSession { .. }
                | Op::SetLighting { .. }
                | Op::SetChargeThreshold { .. }
                | Op::LaunchGame { .. }
                | Op::SetConfig { .. }
                | Op::SetServiceSettings { .. }
                | Op::Shutdown
        )
    }

    /// Ops whose reply waits on an interactive polkit prompt. These get a
    /// separate, longer end-to-end reply budget in the client.
    pub fn is_interactive_authorization(&self) -> bool {
        self.is_write()
    }

    pub fn name(&self) -> &'static str {
        match self {
            Op::Ping => "ping",
            Op::GetInfo => "get_info",
            Op::GetCapabilities => "get_capabilities",
            Op::GetSensors => "get_sensors",
            Op::GetNetwork => "get_network",
            Op::GetFanState => "get_fan_state",
            Op::SetFanMode { .. } => "set_fan_mode",
            Op::GetFanCurve => "get_fan_curve",
            Op::SetFanCurve { .. } => "set_fan_curve",
            Op::GetProfile => "get_profile",
            Op::GetProfileChoices => "get_profile_choices",
            Op::SetProfile { .. } => "set_profile",
            Op::CompareSetProfile { .. } => "compare_set_profile",
            Op::BeginGameProfile { .. } => "begin_game_profile",
            Op::FinishGameProfile { .. } => "finish_game_profile",
            Op::StartManagedGame { .. } => "start_managed_game",
            Op::GetGameSessions => "get_game_sessions",
            Op::ReleaseGameSession { .. } => "release_game_session",
            Op::GetLighting => "get_lighting",
            Op::SetLighting { .. } => "set_lighting",
            Op::GetBattery => "get_battery",
            Op::GetAc => "get_ac",
            Op::GetChargeThreshold => "get_charge_threshold",
            Op::SetChargeThreshold { .. } => "set_charge_threshold",
            Op::GetGraphics => "get_graphics",
            Op::GetGames => "get_games",
            Op::LaunchGame { .. } => "launch_game",
            Op::GetConfig => "get_config",
            Op::SetConfig { .. } => "set_config",
            Op::GetServiceSettings => "get_service_settings",
            Op::SetServiceSettings { .. } => "set_service_settings",
            Op::ExportDiagnostics => "export_diagnostics",
            Op::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CompareSetProfileOutcome {
    Applied,
    Mismatch { observed: ProfileKind },
}

/// The transaction is only held during launcher submission, not game play.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameProfileStarted {
    pub original: ProfileKind,
    pub requested: ProfileKind,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GameProfileFinished {
    Released,
    NotOwned,
    AlreadyOriginal { profile: ProfileKind },
    Restored { profile: ProfileKind },
    ExternalChangePreserved { observed: ProfileKind },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<OmenError>,
}

impl Response {
    pub fn ok(id: u64, data: impl Serialize) -> Response {
        Response {
            id,
            ok: true,
            data: serde_json::to_value(data).ok(),
            error: None,
        }
    }

    pub fn err(id: u64, e: OmenError) -> Response {
        Response {
            id,
            ok: false,
            data: None,
            error: Some(e),
        }
    }

    /// Decode `data` into `T` or produce an internal error.
    pub fn decode<T: for<'de> Deserialize<'de>>(&self) -> omen_core::Result<T> {
        match (&self.data, &self.error) {
            (Some(v), _) => serde_json::from_value(v.clone())
                .map_err(|e| OmenError::Internal(format!("decode response: {e}"))),
            (None, Some(e)) => Err(e.clone()),
            (None, None) => Err(OmenError::Internal(
                "response carries neither data nor error".into(),
            )),
        }
    }
}

/// Daemon identity returned by `GetInfo`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonInfo {
    pub protocol_version: u32,
    pub daemon_version: String,
    pub machine: omen_core::machine::MachineInfo,
    pub mock: bool,
    pub started_unix: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::fans::{FanMode, FanState};

    #[test]
    fn request_roundtrip() {
        let req = Request {
            v: PROTOCOL_VERSION,
            id: 7,
            op: Op::SetFanMode {
                mode: FanMode::Manual { duty_pct: 65 },
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"set_fan_mode\""));
        let back: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
        assert!(back.op.is_write());
    }

    #[test]
    fn response_data_and_error_roundtrip() {
        let st = FanState {
            cpu_rpm: Some(2400),
            ..FanState::default()
        };
        let r = Response::ok(3, &st);
        let json = serde_json::to_string(&r).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert!(back.ok);
        let decoded: FanState = back.decode().unwrap();
        assert_eq!(decoded.cpu_rpm, Some(2400));

        let e = Response::err(4, OmenError::Unsupported("no pwm1".into()));
        let back: Response = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert!(!back.ok);
        assert!(matches!(
            back.decode::<FanState>(),
            Err(OmenError::Unsupported(_))
        ));
    }

    #[test]
    fn read_ops_are_not_writes() {
        assert!(!Op::GetSensors.is_write());
        assert!(!Op::GetCapabilities.is_write());
        assert!(Op::SetProfile {
            profile: ProfileKind::Balanced
        }
        .is_write());
        assert!(Op::CompareSetProfile {
            expected: ProfileKind::Performance,
            profile: ProfileKind::Balanced
        }
        .is_write());
        assert!(Op::BeginGameProfile {
            token: "fixture".into(),
            profile: ProfileKind::Balanced
        }
        .is_write());
        assert!(Op::FinishGameProfile {
            token: "fixture".into(),
            restore: true
        }
        .is_write());
        assert!(Op::Shutdown.is_write());
    }
}
