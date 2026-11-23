//! `omen-hardware` — hardware abstraction layer for omen-linux.
//!
//! Design rules enforced here:
//!
//! 1. **Evidence-based capabilities.** A control is offered only when the
//!    backing kernel interface is present (e.g. the hp-wmi hwmon exposes
//!    `pwm1` only for fan-control-capable boards). Presence checks are the
//!    gate; nothing is assumed from marketing names.
//! 2. **No invented interfaces.** Every sysfs path used in this crate traces
//!    to the upstream kernel (`hp-wmi.c`, hwmon ABI, power_supply ABI,
//!    platform_profile ABI) — see `docs/REVERSE_ENGINEERING_REPORT.md`.
//! 3. **Writes validate and verify.** Control writes clamp inputs, then read
//!    back state and report mismatches as `OmenError::StateMismatch`.
//! 4. **Testability.** All filesystem access goes through [`SysRoot`], so
//!    tests and `--mock` runs point at fixture trees instead of `/`.

pub mod backend;
pub mod board_tables;
pub mod command;
pub mod detection;
pub mod fans;
pub mod game_scope;
pub mod games;
pub mod gpu;
pub mod hp_wmi;
pub mod lighting;
pub mod mock;
pub mod network;
pub mod nvidia_bridge;
pub mod power;
pub mod profiles;
pub mod sensors;

use omen_core::capability::Capabilities;
use omen_core::fans::{FanMode, FanState};
use omen_core::games::Game;
use omen_core::lighting::LightingState;
use omen_core::machine::MachineInfo;
use omen_core::power::{AcInfo, BatteryInfo};
use omen_core::profile::ProfileKind;
use omen_core::sensors::SensorSample;
use omen_core::Result;

/// Root prefix for every filesystem access this crate performs. `Default` is
/// the real filesystem; tests inject fixture roots.
#[derive(Debug, Clone)]
pub struct SysRoot(std::path::PathBuf);

impl Default for SysRoot {
    fn default() -> Self {
        SysRoot(std::path::PathBuf::from("/"))
    }
}

impl SysRoot {
    pub fn new(p: impl Into<std::path::PathBuf>) -> Self {
        SysRoot(p.into())
    }

    pub fn join(&self, rel: &str) -> std::path::PathBuf {
        self.0.join(rel.trim_start_matches('/'))
    }

    pub fn sysfs(&self) -> std::path::PathBuf {
        self.join("sys")
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

/// The complete hardware API consumed by the daemon (and, read-only, by the
/// CLI when the daemon is absent).
pub trait HardwareBackend: Send + Sync {
    fn graphics(&self) -> crate::gpu::GraphicsStatus;
    fn detect(&self) -> MachineInfo;
    fn capabilities(&self) -> Capabilities;
    fn read_sensors(&self) -> Result<Vec<SensorSample>>;
    fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        Err(omen_core::OmenError::Unsupported(
            "network monitoring is unavailable in this backend".into(),
        ))
    }
    fn fan_state(&self) -> Result<FanState>;
    /// Apply a fan mode with validation, clamping and readback verification.
    fn set_fan_mode(&self, mode: FanMode) -> Result<()>;
    /// Explicitly arm an independent writer before a user-requested override.
    /// Ordinary live backends cannot bypass supervision by default.
    fn begin_fan_control(&self) -> Result<()> {
        Err(omen_core::OmenError::Unsupported(
            "independent fan supervision is unavailable".into(),
        ))
    }
    fn maintain_fan_lease(&self) -> Result<()> {
        Err(omen_core::OmenError::Unsupported(
            "independent fan supervision is unavailable".into(),
        ))
    }
    fn fan_supervisor(&self) -> Option<omen_core::fans::FanSupervisorStatus> {
        None
    }
    fn profile(&self) -> Result<ProfileKind>;
    fn set_profile(&self, p: ProfileKind) -> Result<()>;
    /// A PPD-owned lease, independent of a launcher or GUI process. Release
    /// removes only this hold; it never writes a remembered profile value.
    fn hold_profile(
        &self,
        _profile: ProfileKind,
        _reason: &str,
    ) -> Result<profiles::holds::ProfileHold> {
        Err(omen_core::OmenError::Unsupported(
            "managed profiles require an enabled Power Profiles Daemon hold interface".into(),
        ))
    }
    fn lighting_state(&self) -> Result<LightingState>;
    fn set_lighting(&self, st: &LightingState) -> Result<()>;
    fn battery(&self) -> Result<BatteryInfo>;
    fn ac(&self) -> Result<AcInfo>;
    fn set_charge_threshold(&self, pct: u8) -> Result<()>;
    fn charge_threshold(&self) -> Result<Option<u8>>;
    fn detect_games(&self) -> Vec<Game>;
}

/// Small filesystem helpers shared by all modules. Every reader returns
/// `Option` so a missing sensor is simply absent from reports — never zero,
/// never fabricated.
pub(crate) mod fsutil {
    use std::fs;
    use std::path::Path;

    pub fn read_trim(p: &Path) -> Option<String> {
        fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }

    pub fn read_f64(p: &Path) -> Option<f64> {
        read_trim(p).and_then(|s| s.parse::<f64>().ok())
    }

    pub fn read_u64(p: &Path) -> Option<u64> {
        read_trim(p).and_then(|s| s.parse::<u64>().ok())
    }

    /// Write trailing-newline content, mapping permission errors faithfully.
    pub fn write(p: &Path, value: &str) -> std::io::Result<()> {
        fs::write(p, format!("{value}\n").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysroot_joins_strips_leading_slash() {
        let r = SysRoot::new("/tmp/fixture");
        assert_eq!(
            r.join("sys/class/dmi/id"),
            std::path::PathBuf::from("/tmp/fixture/sys/class/dmi/id")
        );
        assert_eq!(
            r.join("/sys/class"),
            std::path::PathBuf::from("/tmp/fixture/sys/class")
        );
    }
}
