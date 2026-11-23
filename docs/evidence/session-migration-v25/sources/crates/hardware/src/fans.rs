//! Fan control over the hp-wmi hwmon pwm interface, with safety rails:
//! clamping to the duty floor, mode write ordering, readback verification
//! and auto-restore helpers.

use crate::hp_wmi::{duty_raw_to_pct, HpHwmon};
use omen_core::fans::{FanMode, FanState};
use omen_core::{OmenError, Result, MAX_FAN_DUTY_PCT, MIN_FAN_DUTY_PCT};

/// hp-wmi `pwm1_enable` values (kernel `enum pwm_modes`).
pub const PWM_ENABLE_MAX: u8 = 0;
pub const PWM_ENABLE_MANUAL: u8 = 1;
pub const PWM_ENABLE_AUTO: u8 = 2;

/// Read fan telemetry independently of whether manual targets are exposed.
/// `pwm1_enable` is a kernel policy indication, not proof of physical cooling
/// or permission to write. A missing manual duty must never become a fake 0%.
pub fn read_state(hwmon: &HpHwmon) -> FanState {
    let duty_pct = hwmon.pwm_duty_pct(1);
    let mode = match hwmon.pwm_enable() {
        Some(PWM_ENABLE_AUTO) => Some(FanMode::Auto),
        Some(PWM_ENABLE_MAX) => Some(FanMode::Max),
        Some(PWM_ENABLE_MANUAL) => duty_pct.map(|duty_pct| FanMode::Manual { duty_pct }),
        _ => None,
    };
    FanState {
        cpu_rpm: hwmon.fan_input(1),
        gpu_rpm: hwmon.fan_input(2),
        mode,
        duty_pct,
    }
}

#[derive(Debug, Clone)]
pub struct FanController {
    hwmon: HpHwmon,
}

impl FanController {
    pub fn new(hwmon: HpHwmon) -> Self {
        FanController { hwmon }
    }

    pub fn state(&self) -> FanState {
        read_state(&self.hwmon)
    }

    /// Apply a fan mode. Order matters: enable manual *before* writing duty
    /// so the EC never receives duty while in auto mode; verify after.
    pub fn set_mode(&self, mode: FanMode) -> Result<()> {
        let result = self.apply_mode(mode);
        if result.is_err() && !matches!(mode, FanMode::Curve | FanMode::Auto) {
            if let Err(e) = self.hwmon.write_pwm_enable(PWM_ENABLE_AUTO) {
                log::error!("partial fan-write recovery failed: {e}");
            }
        }
        result
    }

    fn apply_mode(&self, mode: FanMode) -> Result<()> {
        if matches!(mode, FanMode::Manual { .. }) && self.hwmon.pwm_duty_path(1).is_none() {
            return Err(OmenError::Unsupported(
                "this board exposes no pwm1 (hp-wmi fan control not registered)".into(),
            ));
        }
        match mode {
            FanMode::Auto => {
                self.hwmon.write_pwm_enable(PWM_ENABLE_AUTO)?;
                self.verify(PWM_ENABLE_AUTO, None)?;
            }
            FanMode::Max => {
                self.hwmon.write_pwm_enable(PWM_ENABLE_MAX)?;
                self.verify(PWM_ENABLE_MAX, None)?;
            }
            FanMode::Manual { duty_pct } => {
                let clamped = duty_pct.clamp(MIN_FAN_DUTY_PCT, MAX_FAN_DUTY_PCT);
                self.hwmon.write_pwm_enable(PWM_ENABLE_MANUAL)?;
                for ch in [1u8, 2u8] {
                    if self.hwmon.pwm_duty_path(ch).is_some() {
                        self.hwmon.write_pwm_duty(ch, clamped)?;
                    }
                }
                self.verify(PWM_ENABLE_MANUAL, Some(clamped))?;
                if clamped != duty_pct {
                    log::info!("fan duty {duty_pct}% clamped to hardware safety floor {clamped}%");
                }
            }
            FanMode::Curve => {
                return Err(OmenError::InvalidArgument(
                    "Curve is a daemon-managed mode; the hardware layer only applies Auto/Max/Manual".into(),
                ));
            }
        }
        Ok(())
    }

    /// Readback verification: `pwm1_enable` must equal the requested mode and
    /// (in manual) `pwm1` must round-trip to the requested duty within the
    /// ABI's 255-step quantization.
    fn verify(&self, enable: u8, duty: Option<u8>) -> Result<()> {
        let got = self
            .hwmon
            .pwm_enable()
            .ok_or_else(|| OmenError::StateMismatch("pwm1_enable unreadable after write".into()))?;
        if got != enable {
            return Err(OmenError::StateMismatch(format!(
                "pwm1_enable readback {got} != requested {enable}"
            )));
        }
        if let Some(duty) = duty {
            let raw = crate::hp_wmi::duty_pct_to_raw(duty);
            for channel in [1, 2] {
                let got_raw = self
                    .hwmon
                    .pwm_duty_path(channel)
                    .and_then(|p| crate::fsutil::read_u64(&p))
                    .ok_or_else(|| {
                        OmenError::StateMismatch("pwm1 unreadable after write".into())
                    })?;
                if got_raw > 255 {
                    return Err(OmenError::StateMismatch(format!(
                        "pwm{channel} readback {got_raw} is outside the 0..=255 ABI range"
                    )));
                }
                let quantized = duty_raw_to_pct(raw);
                let got_pct = duty_raw_to_pct(got_raw);
                // Allow ±1 step of quantization error (rounding in the EC).
                if got_pct.abs_diff(quantized) > 1 {
                    return Err(OmenError::StateMismatch(format!(
                    "pwm1 readback {got_pct}% != requested {quantized}% (raw {got_raw} vs {raw})"
                )));
                }
            }
        }
        Ok(())
    }

    /// Restore firmware automatic control — the safety "give control back"
    /// primitive used at shutdown and on failure paths.
    pub fn restore_auto(&self) {
        if let Err(e) = self.set_mode(FanMode::Auto) {
            log::error!("failed to restore automatic fan control: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, FanController) {
        let dir = tempfile::tempdir().unwrap();
        let hw = dir.path().join("hwmon9");
        fs::create_dir_all(&hw).unwrap();
        fs::write(hw.join("name"), "hp\n").unwrap();
        fs::write(hw.join("fan1_input"), "2400\n").unwrap();
        fs::write(hw.join("fan2_input"), "2100\n").unwrap();
        fs::write(hw.join("pwm1_enable"), "2\n").unwrap();
        fs::write(hw.join("pwm1"), "128\n").unwrap();
        fs::write(hw.join("pwm2"), "128\n").unwrap();
        let ctrl = FanController::new(HpHwmon::new(hw.clone(), "hp".into()));
        (dir, ctrl)
    }

    #[test]
    fn reads_state() {
        let (_d, c) = fixture();
        let st = c.state();
        assert_eq!(st.cpu_rpm, Some(2400));
        assert_eq!(st.gpu_rpm, Some(2100));
        assert_eq!(st.mode, Some(FanMode::Auto));
        assert_eq!(st.duty_pct, Some(50));
    }

    #[test]
    fn mode_only_telemetry_needs_no_manual_targets() {
        let (_d, c) = fixture();
        for channel in [1, 2] {
            fs::remove_file(c.hwmon.dir().join(format!("pwm{channel}"))).unwrap();
        }
        for (raw, expected) in [("2", FanMode::Auto), ("0", FanMode::Max)] {
            fs::write(c.hwmon.dir().join("pwm1_enable"), raw).unwrap();
            let state = c.state();
            assert_eq!(state.mode, Some(expected));
            assert_eq!(state.duty_pct, None);
            assert_eq!(state.cpu_rpm, Some(2400));
            assert_eq!(state.gpu_rpm, Some(2100));
        }
    }

    #[test]
    fn missing_or_invalid_mode_does_not_hide_rpm() {
        let (_d, c) = fixture();
        for raw in ["3", "255", "-1", "unavailable", ""] {
            fs::write(c.hwmon.dir().join("pwm1_enable"), raw).unwrap();
            let state = c.state();
            assert_eq!(state.mode, None);
            assert_eq!(state.cpu_rpm, Some(2400));
        }
        fs::remove_file(c.hwmon.dir().join("pwm1_enable")).unwrap();
        assert_eq!(c.state().mode, None);
        assert_eq!(c.state().gpu_rpm, Some(2100));
    }

    #[test]
    fn unreadable_manual_duty_is_never_reported_as_zero() {
        let (_d, c) = fixture();
        fs::write(c.hwmon.dir().join("pwm1_enable"), "1").unwrap();
        for raw in ["256", "18446744073709551615", "-1", "invalid", ""] {
            fs::write(c.hwmon.dir().join("pwm1"), raw).unwrap();
            let state = c.state();
            assert_eq!(state.mode, None);
            assert_eq!(state.duty_pct, None);
        }
        fs::remove_file(c.hwmon.dir().join("pwm1")).unwrap();
        assert_eq!(c.state().mode, None);
        assert_eq!(c.state().duty_pct, None);
    }

    #[test]
    fn invalid_raw_readback_cannot_verify_as_full_duty() {
        for channel in [1, 2] {
            let (_d, c) = fixture();
            fs::write(c.hwmon.dir().join("pwm1_enable"), "1").unwrap();
            for number in [1, 2] {
                fs::write(c.hwmon.dir().join(format!("pwm{number}")), "255").unwrap();
            }
            fs::write(c.hwmon.dir().join(format!("pwm{channel}")), "256").unwrap();
            assert!(matches!(
                c.verify(PWM_ENABLE_MANUAL, Some(100)),
                Err(OmenError::StateMismatch(_))
            ));
        }
    }

    #[test]
    fn manual_mode_writes_and_verifies() {
        let (_d, c) = fixture();
        c.set_mode(FanMode::Manual { duty_pct: 60 }).unwrap();
        let st = c.state();
        assert!(
            matches!(st.mode, Some(FanMode::Manual { duty_pct }) if (59..=61).contains(&duty_pct))
        );
    }

    #[test]
    fn duty_below_floor_is_clamped_and_reported() {
        let (_d, c) = fixture();
        c.set_mode(FanMode::Manual { duty_pct: 0 }).unwrap();
        let st = c.state();
        let duty = match st.mode {
            Some(FanMode::Manual { duty_pct }) => duty_pct,
            other => panic!("expected manual, got {other:?}"),
        };
        assert!(duty >= MIN_FAN_DUTY_PCT, "fan-stop not prevented: {duty}%");
    }

    #[test]
    fn restore_auto_works() {
        let (_d, c) = fixture();
        c.set_mode(FanMode::Manual { duty_pct: 80 }).unwrap();
        c.restore_auto();
        assert_eq!(c.state().mode, Some(FanMode::Auto));
    }

    #[test]
    fn curve_is_rejected_at_hardware_layer() {
        let (_d, c) = fixture();
        assert!(matches!(
            c.set_mode(FanMode::Curve),
            Err(OmenError::InvalidArgument(_))
        ));
    }

    #[test]
    fn mode_only_controller_does_not_require_manual_targets() {
        let dir = tempfile::tempdir().unwrap();
        let hw = dir.path().join("hwmon7");
        fs::create_dir_all(&hw).unwrap();
        fs::write(hw.join("name"), "hp\n").unwrap();
        fs::write(hw.join("fan1_input"), "2400\n").unwrap();
        fs::write(hw.join("pwm1_enable"), "0\n").unwrap();
        let c = FanController::new(HpHwmon::new(hw, "hp".into()));
        c.set_mode(FanMode::Max).unwrap();
        assert_eq!(c.state().mode, Some(FanMode::Max));
        assert!(matches!(
            c.set_mode(FanMode::Manual { duty_pct: 50 }),
            Err(OmenError::Unsupported(_))
        ));
        c.set_mode(FanMode::Auto).unwrap();
        assert_eq!(c.state().mode, Some(FanMode::Auto));
    }

    #[test]
    fn state_mismatch_is_detected() {
        let (d, c) = fixture();
        c.set_mode(FanMode::Manual { duty_pct: 50 }).unwrap();
        // Simulate the firmware reverting the duty write (as hp-wmi.c warns
        // it can): readback no longer matches the request.
        fs::write(d.path().join("hwmon9/pwm1"), "10\n").unwrap();
        let r = c.verify(PWM_ENABLE_MANUAL, Some(50));
        assert!(matches!(r, Err(OmenError::StateMismatch(_))));
        // Mode mismatch is caught too.
        fs::write(d.path().join("hwmon9/pwm1_enable"), "2\n").unwrap();
        assert!(matches!(
            c.verify(PWM_ENABLE_MANUAL, None),
            Err(OmenError::StateMismatch(_))
        ));
    }

    #[test]
    fn failed_second_channel_write_restores_auto() {
        let (dir, controller) = fixture();
        let pwm2 = dir.path().join("hwmon9/pwm2");
        fs::remove_file(&pwm2).unwrap();
        fs::create_dir(&pwm2).unwrap();
        assert!(controller
            .set_mode(FanMode::Manual { duty_pct: 70 })
            .is_err());
        assert_eq!(controller.state().mode, Some(FanMode::Auto));
    }
}
