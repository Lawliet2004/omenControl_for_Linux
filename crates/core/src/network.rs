//! Per-interface kernel counters. Rates are deltas between acquisition times,
//! never totals across physical, virtual and loopback interfaces.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkSource {
    Kernel,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkInterface {
    pub name: String,
    pub ifindex: u32,
    pub link_state: Option<String>,
    pub loopback: Option<bool>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub rx_errors: Option<u64>,
    pub tx_errors: Option<u64>,
    /// /proc/net/dev folds RX missed errors into this counter.
    pub rx_dropped_or_missed: Option<u64>,
    pub tx_dropped: Option<u64>,
    pub rx_bytes_per_second: Option<f64>,
    pub tx_bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkSnapshot {
    pub source: NetworkSource,
    pub measured_unix_ms: u64,
    /// None for the first sample or a sampling gap over 30 seconds.
    pub interval_ms: Option<u64>,
    pub interfaces: Vec<NetworkInterface>,
    pub omitted_interfaces: usize,
    pub unreadable_interfaces: usize,
}

pub fn format_rate(current_bytes: Option<u64>, bytes_per_second: Option<f64>) -> String {
    if current_bytes.is_none() {
        return "Unavailable".into();
    }
    let Some(value) = bytes_per_second.filter(|v| v.is_finite() && *v >= 0.0) else {
        return "Collecting…".into();
    };
    if value >= 1024.0 * 1024.0 {
        format!("{:.1} MiB/s", value / (1024.0 * 1024.0))
    } else if value >= 1024.0 {
        format!("{:.1} KiB/s", value / 1024.0)
    } else {
        format!("{value:.0} B/s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_use_binary_bytes_units_and_keep_missing_distinct_from_idle() {
        assert_eq!(format_rate(None, None), "Unavailable");
        assert_eq!(format_rate(Some(0), None), "Collecting…");
        assert_eq!(format_rate(Some(0), Some(f64::INFINITY)), "Collecting…");
        assert_eq!(format_rate(Some(0), Some(-1.0)), "Collecting…");
        assert_eq!(format_rate(Some(0), Some(0.0)), "0 B/s");
        assert_eq!(format_rate(Some(0), Some(1024.0)), "1.0 KiB/s");
        assert_eq!(format_rate(Some(0), Some(1_572_864.0)), "1.5 MiB/s");
    }
}
