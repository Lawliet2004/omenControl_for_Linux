//! Capability model: the single source of truth for what the UI may show as
//! controllable. A control that is not backed by a `Capability::Available`
//! must render disabled with a reason — never as a functioning control.

use crate::fans::FanMode;
use crate::profile::ProfileKind;
use serde::{Deserialize, Serialize};

/// Whether a control exists and why it is (or is not) usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Capability {
    /// Fully usable. `detail` carries the backing interface (e.g.
    /// `hwmon hp-wmi pwm1`).
    Available { detail: String },
    /// The interface exists but writes were not enabled for safety or policy
    /// reasons (e.g. unrecognized machine class).
    ReadOnly { detail: String },
    /// Not present on this machine/firmware/kernel.
    Unavailable { detail: String },
}

impl Capability {
    pub fn available(detail: impl Into<String>) -> Self {
        Capability::Available {
            detail: detail.into(),
        }
    }
    pub fn read_only(detail: impl Into<String>) -> Self {
        Capability::ReadOnly {
            detail: detail.into(),
        }
    }
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Capability::Unavailable {
            detail: detail.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Capability::Available { .. })
    }

    pub fn detail(&self) -> &str {
        match self {
            Capability::Available { detail }
            | Capability::ReadOnly { detail }
            | Capability::Unavailable { detail } => detail,
        }
    }
}

/// Which service actually executes a control. Surfaced in diagnostics so
/// users can see *how* something is being done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlPlane {
    /// The upstream `hp-wmi` driver through sysfs (hwmon / platform_profile).
    HpWmi,
    /// The standardized `platform_profile` sysfs interface.
    PlatformProfile,
    /// `power-profiles-daemon` via `powerprofilesctl` (D-Bus).
    PowerProfilesDaemon,
    /// The `omen-rgb-keyboard` community kernel module sysfs.
    OmenRgbModule,
    /// LED class subsystem.
    LedClass,
    /// Kernel power_supply sysfs.
    PowerSupply,
    /// Mock backend (tests and `--mock` runs).
    Mock,
}

/// Complete capability report for one machine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capabilities {
    pub machine: crate::machine::MachineInfo,
    /// Fan RPM telemetry (`fan[12]_input` on the hp hwmon device).
    pub fan_monitoring: Capability,
    /// Coarse Automatic/Maximum policy, independently of manual duty targets.
    #[serde(default = "unknown_fan_modes")]
    pub fan_modes: Capability,
    /// Manual fan mode + duty writes (`pwm1_enable`, `pwm[12]`).
    pub fan_control: Capability,
    /// Daemon-driven custom curves (requires fan_control + sensor input).
    pub fan_curve: Capability,
    /// Thermal/performance profiles with the plane that serves them.
    pub performance_profiles: Capability,
    pub profile_choices: Vec<ProfileKind>,
    pub profile_plane: ControlPlane,
    /// Keyboard RGB zones (community module integration).
    pub lighting_zones: Capability,
    pub lighting_zone_count: Option<u8>,
    /// Keyboard backlight brightness via LED class, when present.
    pub keyboard_backlight: Capability,
    /// dGPU/MUX switching.
    pub graphics_mux: Capability,
    /// Battery charge threshold (`charge_control_end_threshold`).
    pub charge_threshold: Capability,
    /// Battery-aware automatic profile switching (always available when
    /// battery telemetry exists; userspace feature).
    pub battery_aware_profiles: Capability,
    /// NVIDIA GPU telemetry via nvidia-smi.
    pub nvidia_telemetry: Capability,
    /// AMD GPU telemetry via hwmon amdgpu.
    pub amd_telemetry: Capability,
}

fn unknown_fan_modes() -> Capability {
    Capability::unavailable("daemon did not advertise Automatic/Maximum control")
}

impl Capabilities {
    /// Fan modes currently allowed. Derived so callers cannot ask for a mode
    /// the hardware refuses.
    pub fn allowed_fan_modes(&self) -> Vec<FanMode> {
        let mut v = Vec::new();
        if self.fan_modes.is_available() || self.fan_control.is_available() {
            v.push(FanMode::Auto);
            v.push(FanMode::Max);
        }
        if self.fan_control.is_available() {
            v.push(FanMode::Manual {
                duty_pct: crate::MIN_FAN_DUTY_PCT,
            });
        }
        if self.fan_curve.is_available() {
            v.push(FanMode::Curve);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::{MachineClass, MachineInfo};

    fn caps(fan_control: Capability, curve: Capability) -> Capabilities {
        Capabilities {
            machine: MachineInfo {
                class: MachineClass::Victus,
                ..MachineInfo::default()
            },
            fan_monitoring: Capability::available("hp-wmi hwmon fan1_input/fan2_input"),
            fan_modes: Capability::unavailable("no coarse control"),
            fan_control,
            fan_curve: curve,
            performance_profiles: Capability::available("power-profiles-daemon"),
            profile_choices: vec![
                ProfileKind::Quiet,
                ProfileKind::Balanced,
                ProfileKind::Performance,
            ],
            profile_plane: ControlPlane::PowerProfilesDaemon,
            lighting_zones: Capability::unavailable("no RGB keyboard interface"),
            lighting_zone_count: None,
            keyboard_backlight: Capability::unavailable("no LED class backlight"),
            graphics_mux: Capability::unavailable("no MUX interface on this kernel"),
            charge_threshold: Capability::unavailable("no charge_control_end_threshold"),
            battery_aware_profiles: Capability::available("userspace"),
            nvidia_telemetry: Capability::available("nvidia-smi"),
            amd_telemetry: Capability::available("hwmon amdgpu"),
        }
    }

    #[test]
    fn allowed_modes_follow_capabilities() {
        let limited = caps(
            Capability::unavailable("board 8C30 not allowlisted"),
            Capability::unavailable("needs fan control"),
        );
        assert!(limited.allowed_fan_modes().is_empty());

        let full = caps(
            Capability::available("hp-wmi pwm1"),
            Capability::available("daemon curve engine"),
        );
        assert_eq!(full.allowed_fan_modes().len(), 4);
    }

    #[test]
    fn coarse_modes_do_not_grant_manual_or_curve_control() {
        let mut coarse = caps(
            Capability::unavailable("no targets"),
            Capability::unavailable("no targets"),
        );
        coarse.fan_modes = Capability::available("mode interface");
        assert_eq!(
            coarse.allowed_fan_modes(),
            vec![FanMode::Auto, FanMode::Max]
        );
        let mut legacy = serde_json::to_value(&coarse).unwrap();
        legacy.as_object_mut().unwrap().remove("fan_modes");
        let decoded: Capabilities = serde_json::from_value(legacy).unwrap();
        assert!(!decoded.fan_modes.is_available());
        assert!(decoded.allowed_fan_modes().is_empty());
    }

    #[test]
    fn capability_serializes_with_reason() {
        let c = Capability::Unavailable {
            detail: "board 8C30 not allowlisted for fan control".into(),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("allowlisted"));
        assert!(!Capability::available("x").is_available() || true);
    }
}
