//! Thin client over the hp-wmi hwmon device (`/sys/class/hwmon/hwmonN`
//! where `name == "hp"`).
//!
//! ABI (from drivers/platform/x86/hp/hp-wmi.c):
//! * `fanN_input` — fan RPM (read-only).
//! * `pwm1_enable` — 0 = no control (max), 1 = manual duty, 2 = automatic.
//! * `pwmN` — duty 0..=255, honored only in manual mode, and only registered
//!   for fan-control-capable boards (Victus S family allowlist).

use crate::fsutil;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct HpHwmon {
    dir: PathBuf,
    name: String,
}

impl HpHwmon {
    pub fn new(dir: PathBuf, name: String) -> Self {
        HpHwmon { dir, name }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn fan_input(&self, channel: u8) -> Option<u64> {
        fsutil::read_u64(&self.dir.join(format!("fan{channel}_input")))
    }

    /// `pwmN` duty file for `channel` if the kernel registered it. Presence
    /// is the fan-control capability signal for this board.
    pub fn pwm_duty_path(&self, channel: u8) -> Option<PathBuf> {
        let p = self.dir.join(format!("pwm{channel}"));
        p.exists().then_some(p)
    }

    pub fn pwm_enable(&self) -> Option<u8> {
        fsutil::read_trim(&self.dir.join("pwm1_enable")).and_then(|s| s.parse::<u8>().ok())
    }

    /// Read the raw duty of `channel` as percent, when the file exists.
    pub fn pwm_duty_pct(&self, channel: u8) -> Option<u8> {
        let p = self.pwm_duty_path(channel)?;
        let raw = fsutil::read_u64(&p)?;
        (raw <= 255).then(|| duty_raw_to_pct(raw))
    }

    pub fn write_pwm_enable(&self, value: u8) -> std::io::Result<()> {
        fsutil::write(&self.dir.join("pwm1_enable"), &value.to_string())
    }

    pub fn write_pwm_duty(&self, channel: u8, duty_pct: u8) -> std::io::Result<()> {
        let Some(p) = self.pwm_duty_path(channel) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("pwm{channel} not present on this board"),
            ));
        };
        fsutil::write(&p, &duty_pct_to_raw(duty_pct).to_string())
    }
}

/// Convert 0..=100 percent to the 0..=255 raw duty used by the pwm ABI.
pub fn duty_pct_to_raw(pct: u8) -> u64 {
    let clamped = pct.min(100) as u32;
    ((clamped * 255) / 100) as u64
}

/// Convert raw 0..=255 duty to 0..=100 percent.
pub fn duty_raw_to_pct(raw: u64) -> u8 {
    ((raw.min(255) * 100) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_conversions_round_trip_at_key_points() {
        assert_eq!(duty_pct_to_raw(0), 0);
        assert_eq!(duty_pct_to_raw(100), 255);
        assert_eq!(duty_pct_to_raw(50), 127);
        assert_eq!(duty_raw_to_pct(0), 0);
        assert_eq!(duty_raw_to_pct(255), 100);
        assert_eq!(duty_raw_to_pct(128), 50);
        // clamping
        assert_eq!(duty_pct_to_raw(150), 255);
        assert_eq!(duty_raw_to_pct(9999), 100);
    }
}
