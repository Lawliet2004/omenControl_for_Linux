//! Sensor sample vocabulary. Every number shown in the UI is a
//! `SensorSample` produced by a real source; there is no code path that
//! fabricates samples.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Temperature,
    FanRpm,
    Frequency,
    Power,
    Voltage,
    Current,
    Percent,
    Count,
    /// Free-form informational value (e.g. "Full", drive model).
    Text,
}

impl SensorKind {
    pub fn unit(self) -> &'static str {
        match self {
            SensorKind::Temperature => "°C",
            SensorKind::FanRpm => "RPM",
            SensorKind::Frequency => "MHz",
            SensorKind::Power => "W",
            SensorKind::Voltage => "V",
            SensorKind::Current => "A",
            SensorKind::Percent => "%",
            SensorKind::Text | SensorKind::Count => "",
        }
    }
}

/// Which subsystem produced a sample. Surfaced in the UI so a user can tell
/// e.g. hp-wmi fan telemetry from lm-sensors chip readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorSource {
    Hwmon,
    HpWmi,
    PowerSupply,
    Procfs,
    Sysfs,
    NvidiaSmi,
    Amdgpu,
    Mock,
}

impl fmt::Display for SensorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SensorSource::Hwmon => "hwmon",
            SensorSource::HpWmi => "hp-wmi",
            SensorSource::PowerSupply => "power_supply",
            SensorSource::Procfs => "procfs",
            SensorSource::Sysfs => "sysfs",
            SensorSource::NvidiaSmi => "nvidia-smi",
            SensorSource::Amdgpu => "amdgpu",
            SensorSource::Mock => "mock",
        };
        f.write_str(s)
    }
}

/// A single measured value at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SensorSample {
    /// Acquisition time; cached metrics retain their original timestamp.
    #[serde(default)]
    pub measured_unix_ms: u64,
    /// Stable identifier, e.g. `hwmon6:k10temp/temp1_input`.
    pub id: String,
    /// Human label, e.g. `Tctl (k10temp)`.
    pub label: String,
    pub kind: SensorKind,
    pub source: SensorSource,
    /// PCI function associated by the provider, when known. Display labels
    /// never establish device identity. Older reports may omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Measured value; `None` for `Text` kinds that carry a string instead.
    pub value: Option<f64>,
    pub text: Option<String>,
}

impl SensorSample {
    pub fn unavailable(
        id: impl Into<String>,
        label: impl Into<String>,
        kind: SensorKind,
        source: SensorSource,
    ) -> Self {
        Self {
            measured_unix_ms: now_ms(),
            id: id.into(),
            label: label.into(),
            kind,
            source,
            device_id: None,
            value: None,
            text: None,
        }
    }

    pub fn numeric(
        id: impl Into<String>,
        label: impl Into<String>,
        kind: SensorKind,
        source: SensorSource,
        value: f64,
    ) -> Self {
        SensorSample {
            measured_unix_ms: now_ms(),
            id: id.into(),
            label: label.into(),
            kind,
            source,
            device_id: None,
            value: Some(value),
            text: None,
        }
    }

    pub fn textual(
        id: impl Into<String>,
        label: impl Into<String>,
        source: SensorSource,
        text: impl Into<String>,
    ) -> Self {
        SensorSample {
            measured_unix_ms: now_ms(),
            id: id.into(),
            label: label.into(),
            kind: SensorKind::Text,
            source,
            device_id: None,
            value: None,
            text: Some(text.into()),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_millis() as u64)
        .unwrap_or(0)
}

/// A lightweight process resource sample for System Vitals / Oasis monitoring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_numeric_and_textual() {
        let n = SensorSample::numeric(
            "h:6/t1",
            "Tctl",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            61.5,
        );
        assert_eq!(n.value, Some(61.5));
        assert_eq!(n.kind.unit(), "°C");
        let t = SensorSample::textual("bat/status", "Battery", SensorSource::PowerSupply, "Full");
        assert_eq!(t.text.as_deref(), Some("Full"));
    }

    #[test]
    fn older_samples_have_unknown_device_identity() {
        let sample = SensorSample::numeric(
            "nvidia/temp",
            "GPU",
            SensorKind::Temperature,
            SensorSource::NvidiaSmi,
            60.0,
        );
        let serialized = serde_json::to_value(&sample).unwrap();
        assert!(serialized.get("device_id").is_none());
        let decoded: SensorSample = serde_json::from_value(serialized).unwrap();
        assert!(decoded.device_id.is_none());
    }
}
