//! power_supply class reader: battery, AC, charge thresholds.

use crate::fsutil;
use omen_core::power::{AcInfo, BatteryInfo, ChargingState};
use std::path::{Path, PathBuf};

pub struct PowerSupplyReader {
    class: PathBuf,
}

impl PowerSupplyReader {
    pub fn new(class: PathBuf) -> Self {
        PowerSupplyReader { class }
    }

    fn devices(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.class) else {
            return out;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        out.extend(paths);
        out
    }

    fn is_type(&self, dev: &Path, want: &str) -> bool {
        fsutil::read_trim(&dev.join("type"))
            .map(|t| t.eq_ignore_ascii_case(want))
            .unwrap_or(false)
    }

    pub fn batteries(&self) -> Vec<BatteryInfo> {
        self.devices()
            .iter()
            .filter(|d| self.is_type(d, "Battery"))
            .filter_map(|d| self.read_battery(d))
            .collect()
    }

    pub fn ac_devices(&self) -> Vec<AcInfo> {
        self.devices()
            .iter()
            .filter(|d| self.is_type(d, "Mains"))
            .map(|d| AcInfo {
                online: fsutil::read_u64(&d.join("online"))
                    .map(|v| v == 1)
                    .unwrap_or(false),
            })
            .collect()
    }

    fn read_battery(&self, dev: &Path) -> Option<BatteryInfo> {
        let status = fsutil::read_trim(&dev.join("status")).unwrap_or_default();
        let state = match status.as_str() {
            "Charging" => ChargingState::Charging,
            "Discharging" => ChargingState::Discharging,
            "Full" => ChargingState::Full,
            "Not charging" => ChargingState::NotCharging,
            _ => ChargingState::Unknown,
        };
        // energy_* in µWh; some firmwares expose charge_* in µAh instead.
        let voltage_v = fsutil::read_f64(&dev.join("voltage_now")).map(|v| v / 1_000_000.0);
        let energy_wh = |f: &str| fsutil::read_f64(&dev.join(f)).map(|v| v / 1_000_000.0);
        let (now, full, design) = match (
            energy_wh("energy_now"),
            energy_wh("energy_full"),
            energy_wh("energy_full_design"),
        ) {
            (Some(n), Some(f), d) => (Some(n), Some(f), d),
            _ => {
                // charge_* (µAh) × voltage (V) → Wh
                let ch = |f: &str| fsutil::read_f64(&dev.join(f)).map(|v| v / 1_000_000.0);
                (
                    ch("charge_now").zip(voltage_v).map(|(c, v)| c * v),
                    ch("charge_full").zip(voltage_v).map(|(c, v)| c * v),
                    ch("charge_full_design").zip(voltage_v).map(|(c, v)| c * v),
                )
            }
        };
        let power_w = fsutil::read_f64(&dev.join("power_now"))
            .map(|v| v / 1_000_000.0)
            .or_else(|| {
                fsutil::read_f64(&dev.join("current_now"))
                    .zip(voltage_v)
                    .map(|(a, v)| a / 1_000_000.0 * v)
            })
            .map(|w| {
                if state == ChargingState::Discharging {
                    -w.abs()
                } else {
                    w.abs()
                }
            });
        let health = match (full, design) {
            (Some(f), Some(d)) if d > 0.0 => Some((f / d) * 100.0),
            _ => None,
        };
        Some(BatteryInfo {
            present: fsutil::read_u64(&dev.join("present"))
                .map(|v| v == 1)
                .unwrap_or(true),
            state,
            capacity_pct: fsutil::read_f64(&dev.join("capacity"))
                .map(|c| c.clamp(0.0, 100.0) as u8),
            energy_wh_now: now,
            energy_wh_full: full,
            energy_wh_design: design,
            power_w,
            voltage_v,
            cycle_count: fsutil::read_u64(&dev.join("cycle_count")),
            health_pct: health,
        })
    }

    /// Path of the first battery's charge threshold file, if the kernel
    /// exposes one. Presence == capability.
    pub fn charge_threshold_path(&self) -> Option<PathBuf> {
        self.devices()
            .iter()
            .filter(|d| self.is_type(d, "Battery"))
            .find_map(|d| {
                let p = d.join("charge_control_end_threshold");
                p.exists().then_some(p)
            })
    }

    pub fn read_charge_threshold(&self) -> Option<u8> {
        self.charge_threshold_path()
            .and_then(|p| fsutil::read_u64(&p))
            .map(|v| v.clamp(0, 100) as u8)
    }

    pub fn write_charge_threshold(&self, pct: u8) -> omen_core::Result<()> {
        let Some(p) = self.charge_threshold_path() else {
            return Err(omen_core::OmenError::Unsupported(
                "charge_control_end_threshold not exposed by this firmware".into(),
            ));
        };
        let v = pct.clamp(50, 100); // firmware floors; below 50% is unhealthy and usually rejected
        fsutil::write(&p, &v.to_string())
            .map_err(|e| omen_core::OmenError::Io(format!("write threshold: {e}")))?;
        match self.read_charge_threshold() {
            Some(got) if got == v => Ok(()),
            Some(got) => Err(omen_core::OmenError::StateMismatch(format!(
                "threshold readback {got}% != {v}%"
            ))),
            None => Err(omen_core::OmenError::StateMismatch(
                "threshold unreadable after write".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &tempfile::TempDir, energy: bool) {
        let bat = dir.path().join("BAT0");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("type"), "Battery\n").unwrap();
        std::fs::write(bat.join("status"), "Discharging\n").unwrap();
        std::fs::write(bat.join("capacity"), "77\n").unwrap();
        std::fs::write(bat.join("present"), "1\n").unwrap();
        if energy {
            std::fs::write(bat.join("energy_now"), "30000000\n").unwrap(); // 30 Wh
            std::fs::write(bat.join("energy_full"), "40000000\n").unwrap(); // 40 Wh
            std::fs::write(bat.join("energy_full_design"), "45000000\n").unwrap();
        // 45 Wh
        } else {
            std::fs::write(bat.join("charge_now"), "2500000\n").unwrap(); // 2.5 Ah
            std::fs::write(bat.join("charge_full"), "4000000\n").unwrap();
            std::fs::write(bat.join("charge_full_design"), "4400000\n").unwrap();
            std::fs::write(bat.join("voltage_now"), "12000000\n").unwrap(); // 12 V
        }
        std::fs::write(bat.join("power_now"), "-12000000\n").unwrap(); // -12 W
        std::fs::write(bat.join("cycle_count"), "42\n").unwrap();
        let ac = dir.path().join("AC");
        std::fs::create_dir_all(&ac).unwrap();
        std::fs::write(ac.join("type"), "Mains\n").unwrap();
        std::fs::write(ac.join("online"), "0\n").unwrap();
    }

    #[test]
    fn reads_energy_firmware() {
        let dir = tempfile::tempdir().unwrap();
        fixture(&dir, true);
        let r = PowerSupplyReader::new(dir.path().to_path_buf());
        let bats = r.batteries();
        assert_eq!(bats.len(), 1);
        let b = &bats[0];
        assert_eq!(b.state, ChargingState::Discharging);
        assert_eq!(b.capacity_pct, Some(77));
        assert_eq!(b.energy_wh_now, Some(30.0));
        assert_eq!(b.power_w, Some(-12.0));
        let health = b.health_pct.unwrap();
        assert!((health - 88.88).abs() < 0.1);
        assert!(!r.ac_devices()[0].online);
        assert!(r.charge_threshold_path().is_none());
    }

    #[test]
    fn reads_charge_firmware() {
        let dir = tempfile::tempdir().unwrap();
        fixture(&dir, false);
        let r = PowerSupplyReader::new(dir.path().to_path_buf());
        let b = &r.batteries()[0];
        assert_eq!(b.energy_wh_now, Some(30.0)); // 2.5 Ah × 12 V
        assert_eq!(b.voltage_v, Some(12.0));
    }

    #[test]
    fn threshold_roundtrip_when_present() {
        let dir = tempfile::tempdir().unwrap();
        fixture(&dir, true);
        let bat = dir.path().join("BAT0");
        std::fs::write(bat.join("charge_control_end_threshold"), "100\n").unwrap();
        let r = PowerSupplyReader::new(dir.path().to_path_buf());
        assert_eq!(r.read_charge_threshold(), Some(100));
        r.write_charge_threshold(80).unwrap();
        assert_eq!(r.read_charge_threshold(), Some(80));
        // Clamped to firmware floor.
        r.write_charge_threshold(20).unwrap();
        assert_eq!(r.read_charge_threshold(), Some(50));
    }

    #[test]
    fn threshold_absent_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        fixture(&dir, true);
        let r = PowerSupplyReader::new(dir.path().to_path_buf());
        assert!(matches!(
            r.write_charge_threshold(80),
            Err(omen_core::OmenError::Unsupported(_))
        ));
    }
}
