//! Data source abstraction for the GUI: talk to the daemon when available,
//! else fall back to direct read-only hardware access. Writes are refused
//! (with a clear explanation) unless the daemon is up.

use omen_core::capability::Capabilities;
use omen_core::fans::{FanCurve, FanMode, FanState};
use omen_core::games::Game;
use omen_core::lighting::LightingState;
use omen_core::power::{AcInfo, BatteryInfo};
use omen_core::profile::ProfileKind;
use omen_core::sensors::SensorSample;
use omen_core::{OmenError, Result};
use omen_hardware::backend::SysfsBackend;
use omen_hardware::gpu::GraphicsStatus;
use omen_hardware::HardwareBackend;
use omen_ipc::protocol::Op;
use omen_ipc::IpcClient;
use std::sync::Arc;

#[derive(Clone)]
pub enum DataSource {
    Daemon(Arc<omen_client::monitoring::MonitoringClient>),
    Direct(Arc<SysfsBackend>),
}

impl DataSource {
    pub fn fixture(&self) -> Result<bool> {
        match self {
            Self::Daemon(client) => client
                .call(Op::GetInfo)?
                .decode::<omen_ipc::protocol::DaemonInfo>()
                .map(|info| info.mock),
            Self::Direct(_) => Ok(false),
        }
    }
    pub fn detect() -> DataSource {
        let client = IpcClient::default_path();
        if client.is_daemon_alive() {
            DataSource::Daemon(Arc::new(omen_client::monitoring::MonitoringClient::new(
                client,
            )))
        } else {
            DataSource::Direct(Arc::new(SysfsBackend::new()))
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            DataSource::Daemon(c) => {
                let mock = c
                    .call(Op::GetInfo)
                    .and_then(|r| r.decode::<omen_ipc::protocol::DaemonInfo>())
                    .map(|i| i.mock)
                    .unwrap_or(false);
                if mock {
                    "MOCK — fixture data, no real hardware"
                } else {
                    "omen-daemon + local read-only GPU fallback"
                }
            }
            DataSource::Direct(_) if crate::victus::available() => {
                "direct monitoring + Victus fan service"
            }
            DataSource::Direct(_) => "direct read-only (daemon not running)",
        }
    }

    #[allow(dead_code)]
    pub fn daemon(&self) -> Option<&IpcClient> {
        match self {
            DataSource::Daemon(c) => Some(c.ipc()),
            DataSource::Direct(_) => None,
        }
    }

    fn need_daemon(&self) -> OmenError {
        OmenError::DaemonUnavailable(
            "this change needs omen-daemon (systemctl start omen-linux.service)".into(),
        )
    }

    pub fn capabilities(&self) -> Capabilities {
        match self {
            DataSource::Daemon(c) => c
                .call(Op::GetCapabilities)
                .and_then(|r| r.decode())
                .unwrap_or_else(|_| fallback_caps()),
            DataSource::Direct(b) => {
                let mut caps = b.capabilities();
                if crate::victus::available() {
                    caps.fan_modes = omen_core::capability::Capability::available(
                        "Controlled by the installed Victus fan service; the selected mode persists until changed or the service restarts",
                    );
                }
                caps
            }
        }
    }

    pub fn read_sensors(&self) -> Result<Vec<SensorSample>> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetSensors)?.decode(),
            DataSource::Direct(b) => b.read_sensors(),
        }
    }
    pub fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        match self {
            Self::Daemon(client) => client.call(Op::GetNetwork)?.decode(),
            Self::Direct(backend) => backend.network(),
        }
    }

    pub fn fan_state(&self) -> Result<FanState> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetFanState)?.decode(),
            DataSource::Direct(b) => b.fan_state(),
        }
    }

    pub fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        match self {
            DataSource::Daemon(c) => match c.call(Op::SetFanMode { mode })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_)
                if matches!(mode, FanMode::Auto | FanMode::Max) && crate::victus::available() =>
            {
                crate::victus::set_mode(mode)
            }
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn fan_curve(&self) -> Result<FanCurve> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetFanCurve)?.decode(),
            DataSource::Direct(_) => Ok(omen_core::config::OmenConfig::load()?.fan.curve),
        }
    }

    pub fn set_fan_curve(&self, curve: FanCurve) -> Result<()> {
        match self {
            DataSource::Daemon(c) => match c.call(Op::SetFanCurve { curve })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => {
                let mut cfg = omen_core::config::OmenConfig::load()?;
                cfg.fan.curve = curve;
                cfg.save()
            }
        }
    }

    pub fn profile(&self) -> Result<ProfileKind> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetProfile)?.decode(),
            DataSource::Direct(b) => b.profile(),
        }
    }

    pub fn set_profile(&self, p: ProfileKind) -> Result<()> {
        match self {
            DataSource::Daemon(c) => match c.call(Op::SetProfile { profile: p })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn lighting(&self) -> Result<LightingState> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetLighting)?.decode(),
            DataSource::Direct(b) => b.lighting_state(),
        }
    }

    pub fn set_lighting(&self, st: &LightingState) -> Result<()> {
        match self {
            DataSource::Daemon(c) => match c.call(Op::SetLighting { state: st.clone() })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn battery(&self) -> Result<BatteryInfo> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetBattery)?.decode(),
            DataSource::Direct(b) => b.battery(),
        }
    }

    pub fn ac(&self) -> Result<AcInfo> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetAc)?.decode(),
            DataSource::Direct(b) => b.ac(),
        }
    }

    pub fn charge_threshold(&self) -> Result<Option<u8>> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetChargeThreshold)?.decode(),
            DataSource::Direct(b) => b.charge_threshold(),
        }
    }

    pub fn set_charge_threshold(&self, pct: u8) -> Result<()> {
        match self {
            DataSource::Daemon(c) => match c.call(Op::SetChargeThreshold { pct })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn graphics(&self) -> Result<GraphicsStatus> {
        match self {
            DataSource::Daemon(c) => c.call(Op::GetGraphics).and_then(|r| r.decode()),
            DataSource::Direct(b) => Ok(omen_hardware::gpu::graphics_status(b.root())),
        }
    }

    pub fn games(&self) -> Result<Vec<Game>> {
        omen_client::games::DesktopGames::new(
            self.daemon().cloned(),
            omen_core::config::OmenConfig::path(),
        )
        .games()
    }

    pub fn game_sessions(&self) -> Result<omen_core::games::GameSessionReport> {
        self.daemon()
            .ok_or_else(|| self.need_daemon())?
            .call_decode(Op::GetGameSessions)
    }
    pub fn release_game_session(&self, token: &str) -> Result<String> {
        self.daemon()
            .ok_or_else(|| self.need_daemon())?
            .call_decode(Op::ReleaseGameSession {
                token: token.into(),
            })
    }

    /// Discovery and launch always run as the desktop user, never as root.
    pub fn launch_game(
        &self,
        id: &str,
    ) -> Result<omen_client::games::ProfiledLaunch<omen_hardware::games::LaunchReceipt>> {
        omen_client::games::DesktopGames::new(
            self.daemon().cloned(),
            omen_core::config::OmenConfig::path(),
        )
        .launch_game(id)
        .map_err(|error| OmenError::LaunchFailed(error.to_string()))
    }

    pub fn restore_game_profile(
        &self,
        expected: ProfileKind,
        original: ProfileKind,
    ) -> Result<String> {
        let client = self.daemon().ok_or_else(|| self.need_daemon())?;
        match client
            .call(Op::CompareSetProfile {
                expected,
                profile: original,
            })?
            .decode::<omen_ipc::protocol::CompareSetProfileOutcome>()?
        {
            omen_ipc::protocol::CompareSetProfileOutcome::Applied => {
                Ok(format!("restored {original}"))
            }
            omen_ipc::protocol::CompareSetProfileOutcome::Mismatch { observed } => Ok(format!(
                "current {observed} profile was selected elsewhere; preserved it"
            )),
        }
    }

    pub fn diagnostics_json(&self) -> Result<String> {
        let value = match self {
            DataSource::Daemon(client) => client
                .call(Op::ExportDiagnostics)?
                .decode::<serde_json::Value>()?,
            DataSource::Direct(backend) => {
                let caps = backend.capabilities();
                serde_json::json!({
                    "mode": "direct read-only", "mock": false,
                    "machine": caps.machine, "capabilities": caps,
                    "compatibility_evidence": omen_core::compatibility::evidence(&caps.machine, false),
                    "sensors": backend.read_sensors()?,
                    "network": backend.network().ok(),
                    "battery": backend.battery().ok(), "ac": backend.ac().ok(),
                    "graphics": backend.graphics(),
                })
            }
        };
        serde_json::to_string_pretty(&value)
            .map_err(|error| OmenError::Internal(format!("diagnostics JSON: {error}")))
    }
}

pub(crate) fn fallback_caps() -> Capabilities {
    let mut caps = Capabilities {
        machine: Default::default(),
        fan_monitoring: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        fan_modes: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        fan_control: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        fan_curve: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        performance_profiles: omen_core::capability::Capability::unavailable(
            "capabilities unreadable",
        ),
        profile_choices: vec![],
        profile_plane: omen_core::capability::ControlPlane::PlatformProfile,
        lighting_zones: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        lighting_zone_count: None,
        keyboard_backlight: omen_core::capability::Capability::unavailable(
            "capabilities unreadable",
        ),
        graphics_mux: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        charge_threshold: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        battery_aware_profiles: omen_core::capability::Capability::unavailable(
            "capabilities unreadable",
        ),
        nvidia_telemetry: omen_core::capability::Capability::unavailable("capabilities unreadable"),
        amd_telemetry: omen_core::capability::Capability::unavailable("capabilities unreadable"),
    };
    caps.machine = omen_core::machine::MachineInfo::default();
    caps
}

/// Optional control readbacks acquired by the monitoring worker. `None` means
/// not requested/expired; a failed read retains its reason without a value.
#[derive(Clone, Default)]
pub struct ControlReadings {
    pub fan_curve: Option<std::result::Result<FanCurve, String>>,
    pub lighting: Option<std::result::Result<LightingState, String>>,
    pub charge_threshold: Option<std::result::Result<Option<u8>, String>>,
    pub graphics: Option<std::result::Result<GraphicsStatus, String>>,
}

#[derive(Clone)]
/// One polling snapshot sent to the UI thread.
pub struct Snapshot {
    pub controls: ControlReadings,
    pub started_unix_ms: u64,
    pub network: Option<omen_core::network::NetworkSnapshot>,
    pub network_error: Option<String>,
    pub capabilities: Capabilities,
    pub sensors: Vec<SensorSample>,
    pub fan: Option<FanState>,
    pub fan_err: Option<String>,
    pub profile: Option<ProfileKind>,
    pub battery: Option<BatteryInfo>,
    pub ac_online: Option<bool>,
    pub error: Option<String>,
}

impl DataSource {
    pub fn snapshot(&self) -> Snapshot {
        let started_unix_ms = crate::overlay::now_ms();
        let network = self.network();
        let (sensors, error) = match self.read_sensors() {
            Ok(s) => (s, None),
            Err(e) => (Vec::new(), Some(format!("Sensor update failed: {e}"))),
        };
        let (fan, fan_err) = match self.fan_state() {
            Ok(f) => (Some(f), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let profile = self.profile().ok();
        let battery = self.battery().ok();
        let ac_online = self.ac().ok().map(|a| a.online);
        let capabilities = self.capabilities();
        // Readbacks belong to the polling worker. Constructing a page or
        // receiving a capability update must never block GTK on hardware/IPC.
        let controls = ControlReadings {
            fan_curve: capabilities
                .fan_curve
                .is_available()
                .then(|| self.fan_curve().map_err(|e| e.to_string())),
            lighting: capabilities
                .lighting_zone_count
                .is_some()
                .then(|| self.lighting().map_err(|e| e.to_string())),
            charge_threshold: (!matches!(
                capabilities.charge_threshold,
                omen_core::capability::Capability::Unavailable { .. }
            ))
            .then(|| self.charge_threshold().map_err(|e| e.to_string())),
            graphics: Some(self.graphics().map_err(|e| e.to_string())),
        };
        Snapshot {
            controls,
            started_unix_ms,
            network: network.as_ref().ok().cloned(),
            network_error: network.err().map(|e| e.to_string()),
            capabilities,
            sensors,
            fan,
            fan_err,
            profile,
            battery,
            ac_online,
            error,
        }
    }
}
