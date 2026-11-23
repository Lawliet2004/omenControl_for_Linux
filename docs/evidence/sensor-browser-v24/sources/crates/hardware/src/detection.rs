//! Machine detection: DMI identity, sysfs interface inventory, capability
//! assembly. Read-only by construction — this module never writes anything.

use crate::board_tables;
use crate::fans::FanController;
use crate::gpu;
use crate::hp_wmi::HpHwmon;
use crate::lighting::LightingInterface;
use crate::power::PowerSupplyReader;
use crate::profiles::ProfileBackend;
use crate::sensors::hwmon_devices;
use crate::SysRoot;
use omen_core::capability::{Capabilities, Capability, ControlPlane};
use omen_core::machine::{MachineClass, MachineInfo};
use omen_core::OmenError;
use std::path::PathBuf;

/// Names of potential third-party fan writers. A CLI can be monitoring-only,
/// but its presence is conservatively treated as a conflict. This best-effort
/// process inventory does not establish exclusive ownership of the hardware.
pub const COMPETING_FAN_DAEMONS: &[&str] = &[
    "thinkfan",
    "nbfc_service",
    "isw",
    "fw-fan-ctrl",
    "asus-fan",
    "fancontrol",
    "nbfc",
    "coolercontrold",
    "tuxedofancontrol",
    "omen-space-daemon",
    "openomen",
    "omen-fand",
    "omencore-cli", // verified upstream `omencore-cli daemon --run` executable
];

pub fn detect_machine(root: &SysRoot) -> MachineInfo {
    let dmi = root.join("sys/class/dmi/id");
    let mut info = MachineInfo {
        sys_vendor: crate::fsutil::read_trim(&dmi.join("sys_vendor")).unwrap_or_default(),
        product_name: crate::fsutil::read_trim(&dmi.join("product_name")).unwrap_or_default(),
        product_family: crate::fsutil::read_trim(&dmi.join("product_family")).unwrap_or_default(),
        board_name: crate::fsutil::read_trim(&dmi.join("board_name")).unwrap_or_default(),
        board_vendor: crate::fsutil::read_trim(&dmi.join("board_vendor")).unwrap_or_default(),
        bios_version: crate::fsutil::read_trim(&dmi.join("bios_version")).unwrap_or_default(),
        bios_vendor: crate::fsutil::read_trim(&dmi.join("bios_vendor")).unwrap_or_default(),
        kernel_version: crate::fsutil::read_trim(&root.join("proc/sys/kernel/osrelease"))
            .unwrap_or_default(),
        class: MachineClass::NonHp,
    };
    let mut class = info.classify();
    // Refine with board tables: a board on the Victus-S fan list is VictusS.
    if matches!(class, MachineClass::Victus)
        && board_tables::board_fan_capability(&info.board_name)
            == board_tables::BoardFanCapability::Manual
    {
        class = MachineClass::VictusS;
    }
    info.class = class;
    info
}

/// Locate the hp-wmi hwmon device (name == "hp") under this root.
pub fn find_hp_hwmon(root: &SysRoot) -> Option<HpHwmon> {
    let hwmons = hwmon_devices(&root.sysfs().join("class/hwmon"));
    for h in hwmons {
        if h.name == "hp" {
            return Some(HpHwmon::new(h.path, h.name));
        }
    }
    None
}

/// Look for running competing fan-control daemons via /proc/*/comm.
/// Returns the first name found (for reporting), or None.
pub fn competing_fan_daemon(root: &SysRoot) -> Option<String> {
    competing_fan_daemons(root).into_iter().next()
}

/// Return every recognized competing controller, so callers can distinguish
/// their selected backend from a second concurrently running writer.
pub fn competing_fan_daemons(root: &SysRoot) -> Vec<String> {
    let mut found = Vec::new();
    let proc = root.join("proc");
    let Ok(entries) = std::fs::read_dir(&proc) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // These services commonly run interpreted programs, whose /proc/comm
        // is only "python3". Identify the systemd unit rather than treating
        // every Python process as a fan controller.
        if let Ok(cgroup) = std::fs::read_to_string(entry.path().join("cgroup")) {
            for unit in ["victus-fan.service", "victus-backend.service"] {
                if cgroup.lines().any(|line| {
                    line.split_once(':')
                        .and_then(|(_, rest)| rest.split_once(':'))
                        .is_some_and(|(_, path)| path.split('/').any(|part| part == unit))
                }) {
                    found.push(unit.to_owned());
                }
            }
        }
        // FanPilot's helper is also Python. Its fixed script name identifies
        // an active session without classifying other Python programs as owners.
        if let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) {
            if cmdline.split(|b| *b == 0).any(|arg| {
                arg.ends_with(b"/fanpilot/helper.py") || arg.ends_with(b"/FanPilot/helper.py")
            }) {
                found.push("FanPilot helper".into());
            }
        }
        if let Some(comm) = crate::fsutil::read_trim(&entry.path().join("comm")) {
            if COMPETING_FAN_DAEMONS
                .iter()
                .any(|known| comm == known.get(..15).unwrap_or(known))
            {
                found.push(comm);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Assemble the full capability report. Presence of real interfaces decides;
/// board tables only annotate the `detail` strings.
pub fn detect_capabilities(root: &SysRoot) -> Capabilities {
    let machine = detect_machine(root);
    let hp = find_hp_hwmon(root);

    // --- Fan monitoring ---
    let fan_monitoring = match &hp {
        Some(h) if h.fan_input(1).is_some() || h.fan_input(2).is_some() => {
            Capability::available("hp-wmi hwmon fan*_input")
        }
        _ => Capability::unavailable("no hp-wmi fan telemetry on this machine"),
    };

    // --- Fan control: gated on pwm1 (kernel only exposes it for allowlisted
    // boards) plus no competing daemon. ---
    let competing = competing_fan_daemon(root);
    let fan_modes = match &hp {
        Some(h)
            if machine.class.is_hp()
                && (machine.board_name == "8C30"
                    || board_tables::board_fan_capability(&machine.board_name)
                        != board_tables::BoardFanCapability::Unknown)
                && matches!(h.pwm_enable(), Some(0..=2)) =>
        {
            if let Some(other) = &competing {
                Capability::read_only(format!(
                    "Automatic/Maximum disabled: competing daemon '{other}' is running"
                ))
            } else {
                Capability::available(
                    "hp-wmi pwm1_enable; Automatic/Maximum only, independent of manual PWM",
                )
            }
        }
        Some(_) => Capability::unavailable(
            "Automatic/Maximum interface or exact board support is unverified",
        ),
        None => Capability::unavailable("no hp-wmi mode interface"),
    };
    let fan_control = match &hp {
        Some(h)
            if h.pwm_duty_path(1).is_some()
                && h.pwm_duty_path(2).is_some()
                && machine.class.is_hp()
                && board_tables::board_fan_capability(&machine.board_name)
                    == board_tables::BoardFanCapability::Manual =>
        {
            if let Some(other) = competing {
                Capability::read_only(format!(
                    "manual fan control disabled: competing daemon '{other}' is running"
                ))
            } else {
                Capability::available("hp-wmi hwmon pwm1_enable/pwm1")
            }
        }
        Some(_) => {
            let board = if machine.board_name.is_empty() {
                "unknown board".to_string()
            } else {
                format!("board {}", machine.board_name)
            };
            let allowlisted = board_tables::board_fan_capability(&machine.board_name);
            let extra = match allowlisted {
                board_tables::BoardFanCapability::Manual => {
                    " (board is allowlisted; pwm1 missing — kernel too old?)"
                }
                board_tables::BoardFanCapability::ThermalOnly => {
                    " (board has thermal profiles but no manual fan control in hp-wmi)"
                }
                board_tables::BoardFanCapability::Unknown => {
                    " (board is not on the hp-wmi fan-control allowlist)"
                }
            };
            Capability::unavailable(format!("hp-wmi exposes no pwm1 for {board}{extra}"))
        }
        None => Capability::unavailable("no hp-wmi hwmon device (is the hp_wmi module loaded?)"),
    };

    let fan_curve = if fan_control.is_available() {
        match &fan_monitoring {
            c if c.is_available() => {
                Capability::available("daemon curve engine over manual fan control")
            }
            c => Capability::read_only(format!("curve engine needs fan telemetry: {}", c.detail())),
        }
    } else {
        Capability::unavailable("requires manual fan control")
    };

    // --- Performance profiles: prefer platform_profile, fall back to PPD. ---
    let profile_backend = ProfileBackend::detect(root);
    let (performance_profiles, profile_choices, profile_plane) = match &profile_backend {
        ProfileBackend::PlatformProfile { choices, .. } if !choices.is_empty() => (
            Capability::available("sysfs platform_profile (hp-wmi thermal profiles)"),
            choices.clone(),
            ControlPlane::PlatformProfile,
        ),
        ProfileBackend::Ppd { choices, .. } | ProfileBackend::Fixture { choices, .. }
            if !choices.is_empty() =>
        {
            (
                Capability::available("power-profiles-daemon (powerprofilesctl)"),
                choices.clone(),
                ControlPlane::PowerProfilesDaemon,
            )
        }
        ProfileBackend::PlatformProfile { .. }
        | ProfileBackend::Ppd { .. }
        | ProfileBackend::Fixture { .. } => (
            Capability::unavailable(
                "neither platform_profile nor power-profiles-daemon is available",
            ),
            Vec::new(),
            ControlPlane::PlatformProfile,
        ),
        ProfileBackend::None => (
            Capability::unavailable(
                "neither /sys/firmware/acpi/platform_profile nor power-profiles-daemon found",
            ),
            Vec::new(),
            ControlPlane::PlatformProfile,
        ),
    };

    // --- Lighting ---
    let lighting = LightingInterface::detect(root);
    let (lighting_zones, zone_count) = match &lighting {
        LightingInterface::OmenRgbModule { zones, .. } => (
            Capability::available(format!(
                "omen-rgb-keyboard module sysfs ({zones} zones)"
            )),
            Some(*zones),
        ),
        LightingInterface::LedBacklight(_) => (
            Capability::unavailable("only a plain keyboard backlight LED was found (no RGB zones)"),
            None,
        ),
        LightingInterface::None => (
            Capability::unavailable("no kernel keyboard-lighting interface (model without RGB keyboard or module not installed)"),
            None,
        ),
    };
    let keyboard_backlight = match &lighting {
        LightingInterface::LedBacklight(_) => {
            Capability::available("LED class kbd backlight brightness")
        }
        LightingInterface::OmenRgbModule { .. } => {
            Capability::available("omen-rgb-keyboard module brightness")
        }
        LightingInterface::None => Capability::unavailable("no keyboard backlight LED found"),
    };

    // --- Graphics MUX: no mainline interface; honestly unavailable. ---
    let graphics_mux = match gpu::list_gpus(root).iter().filter(|g| g.discrete).count() {
        0 => Capability::unavailable("no discrete GPU detected"),
        _ => Capability::unavailable(
            "no mainline kernel MUX interface for this model (status/monitoring only)",
        ),
    };

    // --- Battery / power ---
    let ps_reader = PowerSupplyReader::new(root.sysfs().join("class/power_supply"));
    let batteries = ps_reader.batteries();
    let ac = ps_reader.ac_devices();
    let (battery_avail, charge_threshold) = if batteries.is_empty() {
        (
            Capability::unavailable("no battery present (desktop?)"),
            Capability::unavailable("no battery"),
        )
    } else {
        let thresh = ps_reader
            .charge_threshold_path()
            .map(|_| Capability::available("sysfs charge_control_end_threshold"))
            .unwrap_or_else(|| {
                Capability::unavailable(
                    "firmware/kernel does not expose charge_control_end_threshold",
                )
            });
        (Capability::available("sysfs power_supply class"), thresh)
    };
    let battery_aware =
        if battery_avail.is_available() && ac.iter().any(|a| a.online) || !batteries.is_empty() {
            Capability::available("userspace battery-aware profile switching")
        } else {
            Capability::unavailable("no battery telemetry")
        };

    // --- GPU telemetry ---
    let nvidia_telemetry = match root.path() == std::path::Path::new("/")
        && gpu::nvidia_smi_available()
        && crate::sensors::nvidia::device_nodes_available()
        && gpu::list_gpus(root)
            .iter()
            .any(|g| g.vendor == "NVIDIA" && g.driver.as_deref() == Some("nvidia"))
    {
        true => Capability::available("nvidia-smi interface and NVIDIA device nodes accessible; suspended GPU sampling is skipped"),
        false => Capability::unavailable("NVIDIA telemetry unavailable in this process (nvidia-smi, bound GPU or device-node access missing)"),
    };
    let amd_telemetry = if hwmon_named(root, "amdgpu") {
        Capability::available("hwmon amdgpu")
    } else {
        Capability::unavailable("no amdgpu hwmon device")
    };

    Capabilities {
        machine,
        fan_monitoring,
        fan_modes,
        fan_control,
        fan_curve,
        performance_profiles,
        profile_choices,
        profile_plane,
        lighting_zones,
        lighting_zone_count: zone_count,
        keyboard_backlight,
        graphics_mux,
        charge_threshold,
        battery_aware_profiles: battery_aware,
        nvidia_telemetry,
        amd_telemetry,
    }
}

fn hwmon_named(root: &SysRoot, name: &str) -> bool {
    hwmon_devices(&root.sysfs().join("class/hwmon"))
        .iter()
        .any(|h| h.name == name)
}

/// Fan controller if this machine has one worth using.
pub fn fan_controller(root: &SysRoot) -> Option<FanController> {
    let hp = find_hp_hwmon(root)?;
    if hp.pwm_duty_path(1).is_some() {
        Some(FanController::new(hp))
    } else {
        None
    }
}

/// Convenience error for callers that know a feature is missing.
pub fn unsupported_here(what: &str) -> omen_core::OmenError {
    OmenError::Unsupported(what.to_string())
}

/// Path helper: DMI directory under a root (used by tests and diagnostics).
pub fn dmi_dir(root: &SysRoot) -> PathBuf {
    root.join("sys/class/dmi/id")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn known_fan_writers_match_their_linux_comm_names() {
        for comm in [
            "omencore-cli",
            "omen-space-daem",
            "fancontrol",
            "tuxedofancontro",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let process = dir.path().join("proc/42");
            fs::create_dir_all(&process).unwrap();
            fs::write(process.join("comm"), format!("{comm}\n")).unwrap();
            assert_eq!(
                competing_fan_daemon(&SysRoot::new(dir.path())),
                Some(comm.into())
            );
        }
    }

    #[test]
    fn unrelated_and_non_process_entries_do_not_match() {
        let dir = tempfile::tempdir().unwrap();
        for (entry, comm) in [
            ("42", "omen-daemon"),
            ("43", "omencore-helper"),
            ("self", "omencore-cli"),
        ] {
            let process = dir.path().join("proc").join(entry);
            fs::create_dir_all(&process).unwrap();
            fs::write(process.join("comm"), comm).unwrap();
        }
        fs::create_dir_all(dir.path().join("proc/44")).unwrap();
        assert_eq!(competing_fan_daemon(&SysRoot::new(dir.path())), None);
    }

    #[test]
    fn identifies_interpreted_victus_services_and_fanpilot_sessions() {
        for (file, contents, expected) in [
            (
                "cgroup",
                b"0::/system.slice/victus-fan.service\n".as_slice(),
                "victus-fan.service",
            ),
            (
                "cgroup",
                b"0::/system.slice/victus-backend.service\n".as_slice(),
                "victus-backend.service",
            ),
            (
                "cmdline",
                b"/usr/bin/python3\0-I\0/home/user/.local/share/fanpilot/helper.py\0".as_slice(),
                "FanPilot helper",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let process = dir.path().join("proc/42");
            fs::create_dir_all(&process).unwrap();
            fs::write(process.join("comm"), "python3\n").unwrap();
            fs::write(process.join(file), contents).unwrap();
            assert_eq!(
                competing_fan_daemon(&SysRoot::new(dir.path())),
                Some(expected.into())
            );
        }
    }
}
