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
    Daemon(Arc<ServiceSource>),
    Offline,
    Direct(Arc<SysfsBackend>),
}

pub struct ServiceSource {
    client: omen_client::monitoring::MonitoringClient,
    info: omen_ipc::protocol::DaemonInfo,
}

impl std::ops::Deref for ServiceSource {
    type Target = omen_client::monitoring::MonitoringClient;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

/// Provenance travels with its readings, never with a cached startup label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOrigin {
    pub identity: Option<String>,
    pub label: &'static str,
    pub fixture: Option<bool>,
    pub status: &'static str,
}

impl Default for SourceOrigin {
    fn default() -> Self {
        Self {
            identity: None,
            label: "monitoring source unavailable",
            fixture: None,
            status: "Unavailable",
        }
    }
}

impl SourceOrigin {
    fn service(client: &IpcClient, info: &omen_ipc::protocol::DaemonInfo) -> Self {
        let fixture = if info.mock {
            Some(true)
        } else if client.root_owned_peer() {
            Some(false)
        } else {
            None
        };
        Self {
            identity: Some(format!(
                "{}:{}",
                client.peer_key().unwrap_or_default(),
                serde_json::to_string(info).unwrap_or_default()
            )),
            label: match fixture {
                Some(true) => "MOCK — fixture data, no real hardware",
                Some(false) => "omen-daemon + local read-only GPU fallback",
                None => "service source unverified",
            },
            fixture,
            status: match fixture {
                Some(true) => "Fixture",
                Some(false) => "Service",
                None => "Source unverified",
            },
        }
    }
}

/// An outage has no identity. Keep edits for the same returning process, but
/// never carry an editor or history into another verified source.
#[derive(Default)]
pub struct SourceTracker(Option<String>);

impl SourceTracker {
    pub fn changed(&mut self, origin: &SourceOrigin) -> bool {
        let Some(identity) = &origin.identity else {
            return false;
        };
        let changed = self.0.as_ref().is_some_and(|previous| previous != identity);
        self.0 = Some(identity.clone());
        changed
    }
}

/// Runs only on the monitoring worker. Every selected route is immutable so
/// cloning it for an action cannot redirect that action after a reconnect.
pub struct SourcePoller {
    endpoint: IpcClient,
    allow_direct: bool,
    direct: Option<Arc<SysfsBackend>>,
    selected: DataSource,
    origin: SourceOrigin,
}

impl SourcePoller {
    pub fn new() -> Self {
        Self::at(
            IpcClient::default_path(),
            std::env::var_os("OMEN_SOCK").is_none(),
        )
    }

    fn at(endpoint: IpcClient, allow_direct: bool) -> Self {
        Self {
            endpoint,
            allow_direct,
            direct: None,
            selected: DataSource::Offline,
            origin: SourceOrigin::default(),
        }
    }

    fn select(&mut self) -> Result<()> {
        let observed = self.endpoint.pin_peer().and_then(|client| {
            let info = client.call_decode::<omen_ipc::protocol::DaemonInfo>(Op::GetInfo)?;
            let client = client.bind_observed_info(info.clone())?;
            Ok((client, info))
        });
        match observed {
            Ok((client, info)) => {
                let origin = SourceOrigin::service(&client, &info);
                if origin != self.origin {
                    self.selected = DataSource::Daemon(Arc::new(ServiceSource {
                        client: omen_client::monitoring::MonitoringClient::new(client),
                        info,
                    }));
                }
                self.origin = origin;
                Ok(())
            }
            Err(error) if !self.allow_direct => {
                self.selected = DataSource::Offline;
                self.origin = SourceOrigin::default();
                Err(error)
            }
            Err(_) => {
                let backend = self
                    .direct
                    .get_or_insert_with(|| Arc::new(SysfsBackend::new()));
                self.selected = DataSource::Direct(backend.clone());
                self.origin = SourceOrigin {
                    identity: Some("local hardware".into()),
                    fixture: Some(false),
                    status: "Direct",
                    label: if crate::victus::available() {
                        "direct monitoring + Victus fan service"
                    } else {
                        "direct read-only (daemon not running)"
                    },
                };
                Ok(())
            }
        }
    }

    pub fn poll(&mut self) -> Snapshot {
        let started = crate::overlay::now_ms();
        if let Err(error) = self.select() {
            return Snapshot::unavailable(started, error.to_string());
        }
        let mut snapshot = self.selected.snapshot();
        if let DataSource::Daemon(service) = &self.selected {
            let unchanged = service
                .ipc()
                .call_decode::<omen_ipc::protocol::DaemonInfo>(Op::GetInfo)
                .is_ok_and(|info| info == service.info);
            if !unchanged {
                return Snapshot::unavailable(
                    started,
                    "Service changed during the update; waiting for a fresh report".into(),
                );
            }
        }
        snapshot.origin = self.origin.clone();
        snapshot
    }
}

impl Snapshot {
    fn unavailable(started_unix_ms: u64, error: String) -> Self {
        Self {
            origin: SourceOrigin::default(),
            route: DataSource::Offline,
            controls: ControlReadings::default(),
            started_unix_ms,
            network: None,
            network_error: Some(error.clone()),
            capabilities: fallback_caps(),
            sensors: vec![],
            fan: None,
            fan_err: Some(error.clone()),
            profile: None,
            battery: None,
            ac_online: None,
            error: Some(error),
        }
    }
}

impl DataSource {
    pub fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Daemon(a), Self::Daemon(b)) => {
                a.ipc().peer_key() == b.ipc().peer_key() && a.info == b.info
            }
            (Self::Direct(a), Self::Direct(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    #[allow(dead_code)]
    pub fn daemon(&self) -> Option<&IpcClient> {
        match self {
            DataSource::Daemon(c) => Some(c.ipc()),
            DataSource::Direct(_) | DataSource::Offline => None,
        }
    }

    fn need_daemon(&self) -> OmenError {
        OmenError::DaemonUnavailable(
            "this change needs omen-daemon (systemctl start omen-linux.service)".into(),
        )
    }

    pub fn capabilities(&self) -> Capabilities {
        match self {
            Self::Offline => fallback_caps(),
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
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetSensors)?.decode(),
            DataSource::Direct(b) => b.read_sensors(),
        }
    }
    pub fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            Self::Daemon(client) => client.call(Op::GetNetwork)?.decode(),
            Self::Direct(backend) => backend.network(),
        }
    }

    pub fn fan_state(&self) -> Result<FanState> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetFanState)?.decode(),
            DataSource::Direct(b) => b.fan_state(),
        }
    }

    pub fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        match self {
            Self::Offline => Err(self.need_daemon()),
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
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetFanCurve)?.decode(),
            DataSource::Direct(_) => Ok(omen_core::config::OmenConfig::load()?.fan.curve),
        }
    }

    pub fn set_fan_curve(&self, curve: FanCurve) -> Result<()> {
        match self {
            Self::Offline => Err(self.need_daemon()),
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
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetProfile)?.decode(),
            DataSource::Direct(b) => b.profile(),
        }
    }

    pub fn set_profile(&self, p: ProfileKind) -> Result<()> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => match c.call(Op::SetProfile { profile: p })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn lighting(&self) -> Result<LightingState> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetLighting)?.decode(),
            DataSource::Direct(b) => b.lighting_state(),
        }
    }

    pub fn set_lighting(&self, st: &LightingState) -> Result<()> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => match c.call(Op::SetLighting { state: st.clone() })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn battery(&self) -> Result<BatteryInfo> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetBattery)?.decode(),
            DataSource::Direct(b) => b.battery(),
        }
    }

    pub fn ac(&self) -> Result<AcInfo> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetAc)?.decode(),
            DataSource::Direct(b) => b.ac(),
        }
    }

    pub fn charge_threshold(&self) -> Result<Option<u8>> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => c.call(Op::GetChargeThreshold)?.decode(),
            DataSource::Direct(b) => b.charge_threshold(),
        }
    }

    pub fn set_charge_threshold(&self, pct: u8) -> Result<()> {
        match self {
            Self::Offline => Err(self.need_daemon()),
            DataSource::Daemon(c) => match c.call(Op::SetChargeThreshold { pct })? {
                r if r.ok => Ok(()),
                r => Err(r.error.unwrap_or(OmenError::Internal("?".into()))),
            },
            DataSource::Direct(_) => Err(self.need_daemon()),
        }
    }

    pub fn graphics(&self) -> Result<GraphicsStatus> {
        match self {
            Self::Offline => Err(self.need_daemon()),
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
        let mut value = match self {
            Self::Offline => return Err(self.need_daemon()),
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
        if let DataSource::Daemon(service) = self {
            let info: omen_ipc::protocol::DaemonInfo = service.ipc().call_decode(Op::GetInfo)?;
            if info != service.info {
                return Err(OmenError::DaemonUnavailable(
                    "Service changed while collecting diagnostics; request a fresh report".into(),
                ));
            }
            let origin = SourceOrigin::service(service.ipc(), &info);
            value["client_source"] =
                serde_json::json!({"label":origin.label,"fixture":origin.fixture});
            if origin.fixture.is_none() {
                value["compatibility_evidence"] =
                    serde_json::to_value(omen_core::compatibility::unknown_origin())
                        .map_err(|e| OmenError::Internal(e.to_string()))?;
            }
        }
        serde_json::to_string_pretty(&value)
            .map_err(|error| OmenError::Internal(format!("diagnostics JSON: {error}")))
    }
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;

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
    pub origin: SourceOrigin,
    pub route: DataSource,
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
            origin: SourceOrigin::default(),
            route: self.clone(),
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
