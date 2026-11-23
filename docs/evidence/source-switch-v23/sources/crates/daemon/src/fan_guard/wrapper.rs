use super::client::Client;
use omen_core::capability::Capabilities;
use omen_core::fans::{FanMode, FanState, FanSupervisorStatus};
use omen_core::games::Game;
use omen_core::lighting::LightingState;
use omen_core::machine::MachineInfo;
use omen_core::power::{AcInfo, BatteryInfo};
use omen_core::profile::ProfileKind;
use omen_core::sensors::SensorSample;
use omen_core::{OmenError, Result};
use omen_hardware::{HardwareBackend, SysRoot};
use std::sync::Arc;

enum WriterClient {
    Fixture(Client),
    Coarse(crate::coarse::Client),
}
impl WriterClient {
    fn begin(&self) -> Result<()> {
        match self {
            Self::Fixture(c) => c.begin(),
            Self::Coarse(c) => c.begin(),
        }
    }
    fn apply(&self, mode: FanMode) -> Result<()> {
        match self {
            Self::Fixture(c) => c.apply(mode),
            Self::Coarse(c) => c.apply(mode),
        }
    }
    fn pulse(&self) -> Result<()> {
        match self {
            Self::Fixture(c) => c.pulse(),
            Self::Coarse(c) => c.pulse(),
        }
    }
    fn status(&self) -> Option<FanSupervisorStatus> {
        match self {
            Self::Fixture(c) => c.status(),
            Self::Coarse(c) => c.status(),
        }
    }
}
pub struct SupervisedBackend {
    // Drop the writer before the backend removes its private fixture tree.
    client: Option<WriterClient>,
    inner: Arc<dyn HardwareBackend>,
}
impl SupervisedBackend {
    pub fn fixture(inner: Arc<dyn HardwareBackend>, root: &SysRoot) -> Self {
        let client = omen_hardware::detection::find_hp_hwmon(root)
            .map(|hw| WriterClient::Fixture(Client::new(hw.dir().to_path_buf())));
        Self { client, inner }
    }
    pub fn coarse(inner: Arc<dyn HardwareBackend>, client: crate::coarse::Client) -> Self {
        Self {
            client: Some(WriterClient::Coarse(client)),
            inner,
        }
    }
    fn client(&self) -> Result<&WriterClient> {
        self.client
            .as_ref()
            .ok_or_else(|| OmenError::Unsupported("no fixture controller to supervise".into()))
    }
}
impl HardwareBackend for SupervisedBackend {
    fn graphics(&self) -> omen_hardware::gpu::GraphicsStatus {
        self.inner.graphics()
    }
    fn detect(&self) -> MachineInfo {
        self.inner.detect()
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        if let Some(WriterClient::Coarse(client)) = &self.client {
            let ready = if client.is_fixture() {
                caps.fan_modes.is_available()
            } else {
                crate::coarse::machine_target(&omen_hardware::SysRoot::default())
                    == crate::coarse::TARGET
                    && omen_hardware::detection::detect_capabilities(
                        &omen_hardware::SysRoot::default(),
                    )
                    .fan_modes
                    .is_available()
            };
            caps.fan_modes = if !ready {
                caps.fan_modes
            } else {
                match client.check() {
                    Ok(status) if !(status.lease_active && status.recovery_error.is_some()) => {
                        omen_core::capability::Capability::available(
                            "independent Automatic/Maximum writer ready; Maximum trial ends after 10 seconds",
                        )
                    }
                    Ok(_) => omen_core::capability::Capability::read_only(
                        "Automatic recovery is pending",
                    ),
                    Err(e) => omen_core::capability::Capability::read_only(format!(
                        "independent fan recovery unavailable: {e}"
                    )),
                }
            };
            caps.fan_control = omen_core::capability::Capability::unavailable(
                "mode-only writer; no manual PWM access",
            );
            caps.fan_curve =
                omen_core::capability::Capability::unavailable("mode-only writer; no curve access");
        }
        caps
    }
    fn read_sensors(&self) -> Result<Vec<SensorSample>> {
        self.inner.read_sensors()
    }
    fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        self.inner.network()
    }
    fn fan_state(&self) -> Result<FanState> {
        self.inner.fan_state()
    }
    fn begin_fan_control(&self) -> Result<()> {
        self.client()?.begin()
    }
    fn maintain_fan_lease(&self) -> Result<()> {
        if matches!(self.client, Some(WriterClient::Coarse(_)))
            && !self.capabilities().fan_modes.is_available()
        {
            return Err(OmenError::Unsupported(
                "coarse capability was lost before lease renewal".into(),
            ));
        }
        self.client()?.pulse()
    }
    fn fan_supervisor(&self) -> Option<FanSupervisorStatus> {
        self.client.as_ref().and_then(WriterClient::status)
    }
    fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        // Recovery uses only our already pinned actuator. It does not start
        // an ongoing second controller or sweep the current hwmon inventory.
        let caps = self.capabilities();
        let capability = if matches!(mode, FanMode::Auto | FanMode::Max) {
            &caps.fan_modes
        } else {
            &caps.fan_control
        };
        if mode != FanMode::Auto && !capability.is_available() {
            return Err(OmenError::Unsupported(capability.detail().into()));
        }
        self.client()?.apply(mode)
    }
    fn profile(&self) -> Result<ProfileKind> {
        self.inner.profile()
    }
    fn set_profile(&self, profile: ProfileKind) -> Result<()> {
        self.inner.set_profile(profile)
    }
    fn hold_profile(
        &self,
        profile: ProfileKind,
        reason: &str,
    ) -> Result<omen_hardware::profiles::holds::ProfileHold> {
        self.inner.hold_profile(profile, reason)
    }
    fn lighting_state(&self) -> Result<LightingState> {
        self.inner.lighting_state()
    }
    fn set_lighting(&self, state: &LightingState) -> Result<()> {
        self.inner.set_lighting(state)
    }
    fn battery(&self) -> Result<BatteryInfo> {
        self.inner.battery()
    }
    fn ac(&self) -> Result<AcInfo> {
        self.inner.ac()
    }
    fn set_charge_threshold(&self, pct: u8) -> Result<()> {
        self.inner.set_charge_threshold(pct)
    }
    fn charge_threshold(&self) -> Result<Option<u8>> {
        self.inner.charge_threshold()
    }
    fn detect_games(&self) -> Vec<Game> {
        self.inner.detect_games()
    }
}
