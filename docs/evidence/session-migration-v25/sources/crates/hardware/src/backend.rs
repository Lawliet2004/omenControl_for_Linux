//! `SysfsBackend` — the real [`HardwareBackend`] implementation over sysfs,
//! procfs, power_supply, LED class and helper CLIs. All writes go through the
//! validated adapters (fan controller, profile backend, lighting interface).

use crate::detection;
use crate::games;
use crate::gpu;
use crate::lighting::LightingInterface;
use crate::power::PowerSupplyReader;
use crate::profiles::ProfileBackend;
use crate::sensors::SensorReader;
use crate::{HardwareBackend, SysRoot};
use omen_core::capability::Capabilities;
use omen_core::fans::{FanMode, FanState};
use omen_core::games::Game;
use omen_core::lighting::LightingState;
use omen_core::machine::MachineInfo;
use omen_core::power::{AcInfo, BatteryInfo};
use omen_core::profile::ProfileKind;
use omen_core::sensors::SensorSample;
use omen_core::{OmenError, Result};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct SensorCache {
    updated: Option<Instant>,
    samples: Vec<SensorSample>,
    cpu_ticks: Option<(u64, u64)>,
    nvidia_updated: Option<Instant>,
    nvidia_samples: Vec<SensorSample>,
}

pub struct SysfsBackend {
    root: SysRoot,
    /// Spawn nvidia-smi for GPU telemetry (disabled for fixture roots).
    nvidia: bool,
    isolated_nvidia: Option<bool>,
    /// Fixtures exercise adapters; live hardware starts read-only. No live
    /// fan/RGB/charge allowlist has completed the required hardware validation.
    fixture_writes: bool,
    profile_target: Option<(String, String)>,
    sensors: Mutex<SensorCache>,
    network: Mutex<crate::network::NetworkReader>,
}

impl SysfsBackend {
    /// Backend over the real filesystem.
    pub fn new() -> Self {
        let nvidia = gpu::nvidia_smi_available();
        SysfsBackend {
            root: SysRoot::default(),
            nvidia,
            isolated_nvidia: None,
            fixture_writes: false,
            profile_target: None,
            sensors: Mutex::new(SensorCache::default()),
            network: Mutex::new(crate::network::NetworkReader::default()),
        }
    }

    /// Backend over a fixture root (tests, `--mock`).
    pub fn with_root(root: SysRoot) -> Self {
        let fixture_writes = root
            .path()
            .canonicalize()
            .is_ok_and(|p| p != std::path::Path::new("/"));
        SysfsBackend {
            root,
            nvidia: false,
            isolated_nvidia: None,
            fixture_writes,
            profile_target: None,
            sensors: Mutex::new(SensorCache::default()),
            network: Mutex::new(crate::network::NetworkReader::default()),
        }
    }

    pub fn enable_isolated_nvidia(&mut self, fixture: bool) {
        self.isolated_nvidia = Some(fixture);
        self.nvidia = false;
    }
    pub fn root(&self) -> &SysRoot {
        &self.root
    }

    /// Explicit operator opt-in for standard profile testing, pinned to exact
    /// DMI board and BIOS. This does not enable device-specific controls.
    pub fn enable_profiles_for(&mut self, board: String, bios: String) -> Result<()> {
        let m = self.detect();
        let known_board = crate::board_tables::board_has_thermal_profile(&m.board_name)
            || (m.board_name == "8C30" && m.bios_version == "F.10");
        if board.is_empty()
            || bios.is_empty()
            || !m.class.is_hp()
            || matches!(m.class, omen_core::machine::MachineClass::OtherHp)
            || !known_board
            || m.board_name != board
            || m.bios_version != bios
        {
            return Err(OmenError::Unsupported(
                "profile target must match an identified OMEN/Victus board and BIOS".into(),
            ));
        }
        self.profile_target = Some((board, bios));
        Ok(())
    }

    fn profiles_enabled(&self) -> bool {
        self.fixture_writes
            || self.profile_target.as_ref().is_some_and(|(board, bios)| {
                let m = self.detect();
                &m.board_name == board && &m.bios_version == bios
            })
    }

    fn require_fixture_writes(&self) -> Result<()> {
        if self.fixture_writes {
            Ok(())
        } else {
            Err(OmenError::Unsupported("live device-specific writes are disabled pending model/BIOS validation; monitoring remains available".into()))
        }
    }
}

impl Default for SysfsBackend {
    fn default() -> Self {
        SysfsBackend::new()
    }
}

impl HardwareBackend for SysfsBackend {
    fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        self.network
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot(&self.root)
    }
    fn graphics(&self) -> crate::gpu::GraphicsStatus {
        gpu::graphics_status(&self.root)
    }
    fn detect(&self) -> MachineInfo {
        detection::detect_machine(&self.root)
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = detection::detect_capabilities(&self.root);
        if !self.fixture_writes {
            for cap in [
                &mut caps.fan_modes,
                &mut caps.fan_control,
                &mut caps.fan_curve,
                &mut caps.lighting_zones,
                &mut caps.keyboard_backlight,
                &mut caps.charge_threshold,
            ] {
                if cap.is_available() {
                    *cap = omen_core::capability::Capability::read_only(
                        "interface detected; live model/BIOS validation pending",
                    );
                }
            }
        }
        if !self.profiles_enabled() {
            for cap in [
                &mut caps.performance_profiles,
                &mut caps.battery_aware_profiles,
            ] {
                if cap.is_available() {
                    *cap = omen_core::capability::Capability::read_only("monitoring mode; profile control requires explicit daemon opt-in for this board and BIOS");
                }
            }
        }
        caps
    }

    fn read_sensors(&self) -> Result<Vec<SensorSample>> {
        let mut cache = self.sensors.lock().unwrap_or_else(|e| e.into_inner());
        if !self.fixture_writes
            && cache
                .updated
                .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return Ok(cache.samples.clone());
        }
        let mut samples =
            SensorReader::new(self.root.sysfs(), self.root.join("proc"), false).snapshot();
        if let Some(ticks) = crate::sensors::cpu_ticks(&self.root.join("proc/stat")) {
            if let Some(previous) = cache.cpu_ticks {
                if let Some(value) = crate::sensors::cpu_utilization(previous, ticks) {
                    samples.push(omen_core::sensors::SensorSample::numeric(
                        "cpu/utilization",
                        "CPU utilization",
                        omen_core::sensors::SensorKind::Percent,
                        omen_core::sensors::SensorSource::Procfs,
                        value,
                    ));
                }
            }
            cache.cpu_ticks = Some(ticks);
        }
        samples.extend(crate::sensors::amd_gpu_samples(&self.root));
        if !self.fixture_writes {
            samples.extend(crate::sensors::storage_samples(self.root.path()));
        }
        if self.nvidia
            && cache
                .nvidia_updated
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            let active = gpu::list_gpus(&self.root)
                .iter()
                .any(|g| g.vendor == "NVIDIA" && g.runtime_status.as_deref() != Some("suspended"));
            cache.nvidia_samples = if active {
                crate::sensors::nvidia::read_nvidia_samples()
            } else {
                Vec::new()
            };
            cache.nvidia_updated = Some(Instant::now());
        }
        if let Some(fixture) = self.isolated_nvidia {
            // No user cache is consumed. The root-private PID-1 socket and
            // per-sample timestamps are verified for every reader frame.
            samples.extend(crate::nvidia_bridge::read(fixture).unwrap_or_default());
        }
        samples.extend(cache.nvidia_samples.clone());
        cache.samples = samples.clone();
        cache.updated = Some(Instant::now());
        Ok(samples)
    }

    fn fan_state(&self) -> Result<FanState> {
        let hp = detection::find_hp_hwmon(&self.root)
            .ok_or_else(|| OmenError::Unsupported("hp hwmon disappeared".into()))?;
        Ok(crate::fans::read_state(&hp))
    }

    fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        self.require_fixture_writes()?;
        let caps = self.capabilities();
        let permitted = match mode {
            FanMode::Auto | FanMode::Max => &caps.fan_modes,
            _ => &caps.fan_control,
        };
        if !permitted.is_available() {
            return Err(OmenError::Unsupported(permitted.detail().into()));
        }
        let controller = if matches!(mode, FanMode::Auto | FanMode::Max) {
            detection::find_hp_hwmon(&self.root).map(crate::fans::FanController::new)
        } else {
            detection::fan_controller(&self.root)
        }
        .ok_or_else(|| OmenError::Unsupported("fan controller vanished".into()))?;
        controller.set_mode(mode)
    }

    fn profile(&self) -> Result<ProfileKind> {
        ProfileBackend::detect(&self.root).get()
    }

    fn set_profile(&self, p: ProfileKind) -> Result<()> {
        if !self.profiles_enabled() {
            return Err(OmenError::Unsupported(
                "profile writes disabled in monitoring mode".into(),
            ));
        }
        ProfileBackend::detect(&self.root).set(p)
    }

    fn hold_profile(
        &self,
        profile: ProfileKind,
        reason: &str,
    ) -> Result<crate::profiles::holds::ProfileHold> {
        if !self.profiles_enabled() {
            return Err(OmenError::Unsupported(
                "profile holds disabled in monitoring mode".into(),
            ));
        }
        ProfileBackend::detect(&self.root).hold(profile, reason)
    }

    fn lighting_state(&self) -> Result<LightingState> {
        LightingInterface::detect(&self.root).read()
    }

    fn set_lighting(&self, st: &LightingState) -> Result<()> {
        self.require_fixture_writes()?;
        LightingInterface::detect(&self.root).write(st)
    }

    fn battery(&self) -> Result<BatteryInfo> {
        let reader = PowerSupplyReader::new(self.root.sysfs().join("class/power_supply"));
        reader
            .batteries()
            .into_iter()
            .next()
            .ok_or_else(|| OmenError::Unsupported("no battery on this machine".into()))
    }

    fn ac(&self) -> Result<AcInfo> {
        let reader = PowerSupplyReader::new(self.root.sysfs().join("class/power_supply"));
        let devices = reader.ac_devices();
        if devices.is_empty() {
            return Err(OmenError::Unsupported(
                "no AC online interface is exposed".into(),
            ));
        }
        Ok(AcInfo {
            online: devices.iter().any(|d| d.online),
        })
    }

    fn set_charge_threshold(&self, pct: u8) -> Result<()> {
        self.require_fixture_writes()?;
        PowerSupplyReader::new(self.root.sysfs().join("class/power_supply"))
            .write_charge_threshold(pct)
    }

    fn charge_threshold(&self) -> Result<Option<u8>> {
        Ok(
            PowerSupplyReader::new(self.root.sysfs().join("class/power_supply"))
                .read_charge_threshold(),
        )
    }

    fn detect_games(&self) -> Vec<Game> {
        games::detect_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_backend_refuses_writes_without_invoking_any_adapter() {
        let backend = SysfsBackend::with_root(SysRoot::default());
        assert!(backend.set_profile(ProfileKind::Performance).is_err());
        assert!(backend.set_fan_mode(FanMode::Max).is_err());
        assert!(backend.set_charge_threshold(80).is_err());
    }

    #[test]
    fn unknown_board_cannot_opt_in_from_marketing_name_alone() {
        let dir = tempfile::tempdir().unwrap();
        crate::mock::build_victus_fb(dir.path());
        std::fs::write(dir.path().join("sys/class/dmi/id/board_name"), "UNKNOWN").unwrap();
        let mut backend = SysfsBackend::with_root(SysRoot::new(dir.path()));
        assert!(backend
            .enable_profiles_for("UNKNOWN".into(), "F.10".into())
            .is_err());
        assert!(backend
            .enable_profiles_for("8C30".into(), "F.10".into())
            .is_err());
    }

    #[test]
    fn readable_coarse_mode_does_not_grant_fan_control() {
        let dir = tempfile::tempdir().unwrap();
        crate::mock::build_victus_fb(dir.path());
        let backend = SysfsBackend::with_root(SysRoot::new(dir.path()));
        let enable = dir.path().join("sys/class/hwmon/hwmon0/pwm1_enable");
        assert!(detection::fan_controller(backend.root()).is_none());
        assert!(!backend.capabilities().fan_control.is_available());
        assert!(!backend.capabilities().fan_curve.is_available());
        assert_eq!(backend.fan_state().unwrap().mode, Some(FanMode::Auto));
        for mode in [
            FanMode::Auto,
            FanMode::Max,
            FanMode::Manual { duty_pct: 80 },
        ] {
            if matches!(mode, FanMode::Auto | FanMode::Max) {
                backend.set_fan_mode(mode).unwrap();
                backend.set_fan_mode(FanMode::Auto).unwrap();
            } else {
                assert!(backend.set_fan_mode(mode).is_err());
            }
            assert_eq!(std::fs::read_to_string(&enable).unwrap().trim(), "2");
        }
    }

    #[test]
    fn omencore_conflict_blocks_fixture_adapter_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        crate::mock::build_fan_control_omen(dir.path());
        let backend = SysfsBackend::with_root(SysRoot::new(dir.path()));
        assert!(backend.capabilities().fan_control.is_available());
        let process = dir.path().join("proc/1234");
        std::fs::create_dir_all(&process).unwrap();
        std::fs::write(process.join("comm"), "omencore-cli\n").unwrap();
        let caps = backend.capabilities();
        assert!(!caps.fan_control.is_available());
        assert!(!caps.fan_curve.is_available());
        assert!(caps.fan_control.detail().contains("omencore-cli"));
        assert!(backend.set_fan_mode(FanMode::Max).is_err());
        let hwmon = detection::find_hp_hwmon(backend.root()).unwrap();
        assert_eq!(hwmon.pwm_enable(), Some(2));
        assert_eq!(hwmon.pwm_duty_pct(1), Some(50));
        std::fs::remove_dir_all(process).unwrap();
        assert!(backend.capabilities().fan_control.is_available());
    }
}
