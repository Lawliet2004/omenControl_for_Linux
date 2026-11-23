//! Battery / AC state vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargingState {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatteryInfo {
    pub present: bool,
    pub state: ChargingState,
    /// 0..=100
    pub capacity_pct: Option<u8>,
    /// Watt-hours now / full when exposed by the kernel.
    pub energy_wh_now: Option<f64>,
    pub energy_wh_full: Option<f64>,
    /// Design capacity Wh (health denominator).
    pub energy_wh_design: Option<f64>,
    /// Positive when charging, negative when discharging.
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub cycle_count: Option<u64>,
    /// Battery health percent = full/design when both known.
    pub health_pct: Option<f64>,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        BatteryInfo {
            present: false,
            state: ChargingState::Unknown,
            capacity_pct: None,
            energy_wh_now: None,
            energy_wh_full: None,
            energy_wh_design: None,
            power_w: None,
            voltage_v: None,
            cycle_count: None,
            health_pct: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcInfo {
    pub online: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_serializes() {
        let b = BatteryInfo {
            present: true,
            state: ChargingState::Full,
            capacity_pct: Some(100),
            ..Default::default()
        };
        let s = serde_json::to_string(&b).unwrap();
        assert!(s.contains("\"full\""));
        assert!(s.contains("100"));
    }
}
