//! Fan state, modes and custom curves, with validation.

use crate::{OmenError, Result, MAX_FAN_DUTY_PCT, MIN_FAN_DUTY_PCT};
use serde::{Deserialize, Serialize};

/// Fan operating mode. `Manual` carries an explicit duty cycle that the
/// backend clamps to [`MIN_FAN_DUTY_PCT`]..=[`MAX_FAN_DUTY_PCT`] before any
/// hardware write, so a fan-stop command cannot be expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FanMode {
    /// Firmware-controlled (`pwm1_enable` = 2).
    Auto,
    /// Firmware maximum (`pwm1_enable` = 0 in hp-wmi semantics).
    Max,
    /// Manual duty cycle percent (`pwm1_enable` = 1 + `pwmN` write).
    Manual { duty_pct: u8 },
    /// Daemon-managed curve: the daemon switches the hardware to manual and
    /// drives the duty from a [`FanCurve`].
    Curve,
}

impl FanMode {
    pub fn is_auto_like(self) -> bool {
        matches!(self, FanMode::Auto)
    }
}

/// Current fan readings plus the reported policy; this is not proof that
/// firmware applied the policy or that cooling is physically adequate.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FanState {
    /// CPU fan RPM if reported (`fan1_input`).
    pub cpu_rpm: Option<u64>,
    /// GPU fan RPM if reported (`fan2_input`).
    pub gpu_rpm: Option<u64>,
    /// Kernel-reported mode if known. Manual is absent if its duty is unreadable.
    pub mode: Option<FanMode>,
    /// Normalized pwm reading when available. hp-wmi derives this from active
    /// RPM; it is not an echo of the last requested target or an electrical duty.
    pub duty_pct: Option<u8>,
}

/// Software supervisor state, separate from observed hardware policy/RPM.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FanSupervisorStatus {
    pub pid: Option<u32>,
    pub lease_active: bool,
    pub revoked: bool,
    pub recovery_error: Option<String>,
}

/// One point of a fan curve: at `temp_c` degrees Celsius the fan should run
/// at `duty_pct` percent (clamped to the safety floor at evaluation time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: i32,
    pub duty_pct: u8,
}

/// A piecewise-linear fan curve, monotonically non-decreasing in both axes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurve {
    pub points: Vec<CurvePoint>,
}

impl Default for FanCurve {
    fn default() -> Self {
        // A fixture/editor default only. Its thermal safety is not validated
        // for real hardware; live custom curves remain disabled.
        FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 25,
                },
                CurvePoint {
                    temp_c: 55,
                    duty_pct: 40,
                },
                CurvePoint {
                    temp_c: 65,
                    duty_pct: 60,
                },
                CurvePoint {
                    temp_c: 75,
                    duty_pct: 80,
                },
                CurvePoint {
                    temp_c: 85,
                    duty_pct: 100,
                },
            ],
        }
    }
}

impl FanCurve {
    /// Validate invariants: at least two points, strictly increasing
    /// temperature, non-decreasing duty, duty within 0..=100. Note that we
    /// accept authored duties below the safety floor here (users may write 0)
    /// because [`FanCurve::duty_at`] applies the floor at evaluation time —
    /// refusing to *store* them would surprise users editing presets.
    pub fn validate(&self) -> Result<()> {
        if !(2..=32).contains(&self.points.len()) {
            return Err(OmenError::InvalidArgument(
                "fan curve needs 2–32 points".into(),
            ));
        }
        let mut last_temp = i32::MIN;
        let mut last_duty: i32 = -1;
        for p in &self.points {
            if !(0..=110).contains(&p.temp_c) {
                return Err(OmenError::InvalidArgument(
                    "curve temperature outside 0–110 °C".into(),
                ));
            }
            if p.temp_c <= last_temp {
                return Err(OmenError::InvalidArgument(format!(
                    "fan curve temperatures must strictly increase ({} after {})",
                    p.temp_c, last_temp
                )));
            }
            if (p.duty_pct as i32) < last_duty {
                return Err(OmenError::InvalidArgument(format!(
                    "fan curve duty must be non-decreasing ({} after {})",
                    p.duty_pct, last_duty
                )));
            }
            if p.duty_pct > 100 {
                return Err(OmenError::InvalidArgument(format!(
                    "fan curve duty {} out of range 0..=100",
                    p.duty_pct
                )));
            }
            last_temp = p.temp_c;
            last_duty = p.duty_pct as i32;
        }
        Ok(())
    }

    /// Evaluate the curve at `temp_c`, clamping outside the range and always
    /// respecting the hardware safety floor — the result can never be 0.
    pub fn duty_at(&self, temp_c: f64) -> u8 {
        let pts = &self.points;
        debug_assert!(pts.len() >= 2, "validated curves have >= 2 points");
        let duty = if temp_c <= pts[0].temp_c as f64 {
            pts[0].duty_pct
        } else if temp_c >= pts[pts.len() - 1].temp_c as f64 {
            pts[pts.len() - 1].duty_pct
        } else {
            let mut lo = 0usize;
            let mut hi = pts.len() - 1;
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if pts[mid].temp_c as f64 <= temp_c {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let a = pts[lo];
            let b = pts[hi];
            let span = (b.temp_c - a.temp_c) as f64;
            let t = if span <= 0.0 {
                0.0
            } else {
                ((temp_c - a.temp_c as f64) / span).clamp(0.0, 1.0)
            };
            let d = a.duty_pct as f64 + t * (b.duty_pct as f64 - a.duty_pct as f64);
            d.round().clamp(0.0, 100.0) as u8
        };
        duty.clamp(MIN_FAN_DUTY_PCT, MAX_FAN_DUTY_PCT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_curve_is_valid_and_monotonic() {
        assert!(FanCurve::default().validate().is_ok());
    }

    #[test]
    fn rejects_decreasing_duty() {
        let c = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 60,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 30,
                },
            ],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_temps() {
        let c = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 30,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 40,
                },
            ],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn duty_respects_floor_even_if_authored_zero() {
        let c = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 0,
                },
                CurvePoint {
                    temp_c: 90,
                    duty_pct: 100,
                },
            ],
        };
        assert!(c.validate().is_ok());
        assert_eq!(c.duty_at(20.0), MIN_FAN_DUTY_PCT);
        assert!(c.duty_at(35.0) >= MIN_FAN_DUTY_PCT);
        assert_eq!(c.duty_at(95.0), 100);
    }

    #[test]
    fn interpolates_linearly() {
        let c = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 40,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 60,
                },
            ],
        };
        assert_eq!(c.duty_at(50.0), 50);
    }

    #[test]
    fn clamps_outside_range() {
        let c = FanCurve::default();
        let first = c.points[0].duty_pct.max(MIN_FAN_DUTY_PCT);
        assert_eq!(c.duty_at(-10.0), first);
        assert_eq!(c.duty_at(200.0), 100);
    }
}
