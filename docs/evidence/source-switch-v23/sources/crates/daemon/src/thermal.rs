//! Development guard for fixture curves. Source identity and timely acquisition
//! are necessary checks, not proof of sensor accuracy or a safe thermal envelope.

use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
use omen_core::{OmenError, Result};
use omen_hardware::gpu::{valid_pci_address, GraphicsStatus};
use serde::Serialize;
use std::collections::BTreeSet;

pub const MAX_SAMPLE_AGE_MS: u64 = 2_000;
pub const MAX_TICK_GAP_MS: u64 = 3_000;

fn invalid(message: impl Into<String>) -> OmenError {
    OmenError::StateMismatch(message.into())
}

pub fn boottime_ms() -> Result<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: time points to a live timespec; CLOCK_BOOTTIME includes suspend.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0
        || time.tv_sec < 0
        || !(0..1_000_000_000).contains(&time.tv_nsec)
    {
        return Err(invalid("cannot read the suspend-aware thermal clock"));
    }
    Ok((time.tv_sec as u64).saturating_mul(1000) + time.tv_nsec as u64 / 1_000_000)
}

pub fn unix_ms() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_millis() as u64)
        .map_err(|_| invalid("thermal acquisition clock is unavailable"))
}

fn hwmon_chip(sample: &SensorSample) -> Option<&str> {
    if sample.source != SensorSource::Hwmon || sample.kind != SensorKind::Temperature {
        return None;
    }
    let mut parts = sample.id.split('/');
    let hwmon = parts.next()?.strip_prefix("hwmon")?;
    let chip = parts.next()?;
    let channel = parts.next()?.strip_prefix("temp")?.strip_suffix("_input")?;
    if parts.next().is_some()
        || hwmon.is_empty()
        || !hwmon.bytes().all(|c| c.is_ascii_digit())
        || channel.parse::<u8>().ok().is_none_or(|n| n == 0)
    {
        return None;
    }
    Some(chip)
}

fn value(sample: &SensorSample, now_ms: u64) -> Result<f64> {
    let age = now_ms
        .checked_sub(sample.measured_unix_ms)
        .ok_or_else(|| invalid(format!("temperature {} has a future timestamp", sample.id)))?;
    if sample.measured_unix_ms == 0 || age > MAX_SAMPLE_AGE_MS {
        return Err(invalid(format!(
            "temperature {} is stale or undated",
            sample.id
        )));
    }
    // A plausibility bound for development fixtures, not a thermal trip point.
    sample
        .value
        .filter(|v| v.is_finite() && (0.0..=150.0).contains(v))
        .ok_or_else(|| {
            invalid(format!(
                "temperature {} is unavailable or implausible",
                sample.id
            ))
        })
}

#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub hottest_c: f64,
    pub sensor_ids: Vec<String>,
    pub cohort: Vec<String>,
}

pub fn evaluate(samples: &[SensorSample], graphics: &GraphicsStatus, now_ms: u64) -> Result<Frame> {
    if !graphics.inventory_complete {
        return Err(invalid(
            "PCI GPU inventory is incomplete; CPU-only cooling cannot be assumed",
        ));
    }
    let mut hottest = None::<f64>;
    let mut ids = BTreeSet::new();
    let mut cohort = BTreeSet::new();
    let mut include = |sample: &SensorSample| -> Result<()> {
        let reading = value(sample, now_ms)?;
        if !ids.insert(sample.id.clone()) {
            return Err(invalid("duplicate thermal sensor identity"));
        }
        cohort.insert(format!(
            "sensor|{}|{}|{}",
            sample.source,
            sample.id,
            sample.device_id.as_deref().unwrap_or("")
        ));
        hottest = Some(hottest.map_or(reading, |last| last.max(reading)));
        Ok(())
    };
    let cpu: Vec<_> = samples
        .iter()
        .filter(|s| matches!(hwmon_chip(s), Some("k10temp" | "coretemp")))
        .collect();
    if cpu.is_empty() {
        return Err(invalid(
            "no CPU temperature from a recognized die-sensor provider",
        ));
    }
    for sample in cpu {
        include(sample)?;
    }
    let mut devices = BTreeSet::new();
    let mut gpu_cohort = Vec::new();
    for gpu in &graphics.gpus {
        if !valid_pci_address(&gpu.pci_address) || !devices.insert(&gpu.pci_address) {
            return Err(invalid("invalid or duplicate GPU identity"));
        }
        gpu_cohort.push(format!(
            "gpu|{}|{}|{:?}|{:?}",
            gpu.pci_address, gpu.vendor, gpu.driver, gpu.runtime_status
        ));
        if gpu.runtime_status.as_deref() == Some("suspended") {
            continue;
        }
        if !matches!(
            gpu.runtime_status.as_deref(),
            Some("active" | "unsupported")
        ) {
            return Err(invalid(format!(
                "GPU {} power state is unverified",
                gpu.pci_address
            )));
        }
        let readings: Vec<_> = samples
            .iter()
            .filter(|s| {
                s.kind == SensorKind::Temperature
                    && s.device_id.as_deref() == Some(&gpu.pci_address)
                    && match (gpu.vendor.as_str(), gpu.driver.as_deref()) {
                        ("AMD", Some("amdgpu")) => hwmon_chip(s) == Some("amdgpu"),
                        ("NVIDIA", Some("nvidia")) => {
                            s.source == SensorSource::NvidiaSmi && s.id == "nvidia/temp"
                        }
                        _ => false,
                    }
            })
            .collect();
        if readings.is_empty() {
            return Err(invalid(format!(
                "GPU {} has no associated recognized temperature",
                gpu.pci_address
            )));
        }
        for sample in readings {
            include(sample)?;
        }
    }
    cohort.extend(gpu_cohort);
    Ok(Frame {
        hottest_c: hottest.expect("CPU readings were included"),
        sensor_ids: ids.into_iter().collect(),
        cohort: cohort.into_iter().collect(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Guard {
    pub frame: Frame,
    pub last_boot_ms: u64,
}

impl Guard {
    pub fn check_gap(&self, now_boot_ms: u64) -> Result<()> {
        if now_boot_ms
            .checked_sub(self.last_boot_ms)
            .is_none_or(|gap| gap > MAX_TICK_GAP_MS)
        {
            return Err(invalid(
                "thermal control tick stalled or crossed a suspend gap",
            ));
        }
        Ok(())
    }

    pub fn advance(&mut self, frame: Frame, now_boot_ms: u64) -> Result<()> {
        self.check_gap(now_boot_ms)?;
        if frame.cohort != self.frame.cohort {
            return Err(invalid(
                "CPU/GPU sensor cohort or GPU state changed; rearming fan control is required",
            ));
        }
        self.frame = frame;
        self.last_boot_ms = now_boot_ms;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_hardware::gpu::GpuInfo;

    fn cpu() -> SensorSample {
        let mut sample = SensorSample::numeric(
            "hwmon1/k10temp/temp1_input",
            "arbitrary label",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            60.0,
        );
        sample.measured_unix_ms = 10_000;
        sample
    }
    fn graphics() -> GraphicsStatus {
        GraphicsStatus {
            gpus: vec![],
            inventory_complete: true,
            discrete_runtime: None,
            mux_available: false,
            mux_note: String::new(),
        }
    }
    fn gpu() -> GpuInfo {
        GpuInfo {
            pci_address: "0000:01:00.0".into(),
            vendor: "NVIDIA".into(),
            driver: Some("nvidia".into()),
            discrete: true,
            runtime_status: Some("active".into()),
        }
    }
    fn gpu_sample() -> SensorSample {
        let mut sample = cpu();
        sample.id = "nvidia/temp".into();
        sample.source = SensorSource::NvidiaSmi;
        sample.device_id = Some("0000:01:00.0".into());
        sample.value = Some(90.0);
        sample
    }

    #[test]
    fn gpu_heat_dominates_a_cool_cpu() {
        let mut status = graphics();
        status.gpus.push(gpu());
        let frame = evaluate(&[cpu(), gpu_sample()], &status, 10_000).unwrap();
        assert_eq!(frame.hottest_c, 90.0);
        assert_eq!(
            omen_core::fans::FanCurve::default().duty_at(frame.hottest_c),
            100
        );
    }

    #[test]
    fn display_labels_do_not_establish_cpu_identity() {
        let mut fake = cpu();
        fake.id = "hwmon0/acpitz/temp1_input".into();
        fake.label = "CPU Package Tctl".into();
        fake.value = Some(140.0);
        assert!(evaluate(&[fake.clone()], &graphics(), 10_000).is_err());
        assert_eq!(
            evaluate(&[cpu(), fake], &graphics(), 10_000)
                .unwrap()
                .hottest_c,
            60.0
        );
    }

    #[test]
    fn missing_or_wrong_gpu_identity_cannot_be_hidden_by_other_temperatures() {
        let mut status = graphics();
        status.gpus.push(gpu());
        let mut wrong = gpu_sample();
        wrong.device_id = Some("0000:06:00.0".into());
        assert!(evaluate(&[cpu(), wrong], &status, 10_000).is_err());
        let mut incomplete = graphics();
        incomplete.inventory_complete = false;
        assert!(evaluate(&[cpu()], &incomplete, 10_000).is_err());
    }

    #[test]
    fn each_active_gpu_needs_its_own_reading() {
        let mut status = graphics();
        status.gpus.push(gpu());
        let mut second = gpu();
        second.pci_address = "0000:02:00.0".into();
        status.gpus.push(second);
        assert!(evaluate(&[cpu(), gpu_sample()], &status, 10_000).is_err());
    }

    #[test]
    fn stale_future_undated_and_invalid_inputs_fail_closed() {
        for time in [0, 7_999, 10_001] {
            let mut sample = cpu();
            sample.measured_unix_ms = time;
            assert!(evaluate(&[sample], &graphics(), 10_000).is_err());
        }
        for number in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(151.0),
        ] {
            let mut sample = cpu();
            sample.value = number;
            assert!(evaluate(&[sample], &graphics(), 10_000).is_err());
        }
        let mut hot = cpu();
        hot.value = Some(130.0);
        assert_eq!(
            evaluate(&[hot], &graphics(), 10_000).unwrap().hottest_c,
            130.0
        );
    }

    #[test]
    fn one_missing_cpu_channel_and_duplicate_ids_are_rejected() {
        let mut missing = cpu();
        missing.id = "hwmon1/k10temp/temp2_input".into();
        missing.value = None;
        assert!(evaluate(&[cpu(), missing], &graphics(), 10_000).is_err());
        assert!(evaluate(&[cpu(), cpu()], &graphics(), 10_000).is_err());
    }

    #[test]
    fn resumed_gpu_and_sensor_cohort_changes_require_rearming() {
        let mut suspended = graphics();
        let mut card = gpu();
        card.runtime_status = Some("suspended".into());
        suspended.gpus.push(card);
        let initial = evaluate(&[cpu()], &suspended, 10_000).unwrap();
        let mut guard = Guard {
            frame: initial,
            last_boot_ms: 20_000,
        };
        let mut active = suspended.clone();
        active.gpus[0].runtime_status = Some("active".into());
        let resumed = evaluate(&[cpu(), gpu_sample()], &active, 10_000).unwrap();
        assert!(guard.advance(resumed, 21_000).is_err());
        let mut additional = cpu();
        additional.id = "hwmon2/coretemp/temp1_input".into();
        let changed = evaluate(&[cpu(), additional], &suspended, 10_000).unwrap();
        assert!(guard.advance(changed, 21_000).is_err());
    }

    #[test]
    fn suspend_aware_gap_and_clock_reversal_stop_the_guard() {
        let frame = evaluate(&[cpu()], &graphics(), 10_000).unwrap();
        let guard = Guard {
            frame,
            last_boot_ms: 20_000,
        };
        assert!(guard.check_gap(23_000).is_ok());
        assert!(guard.check_gap(23_001).is_err());
        assert!(guard.check_gap(19_999).is_err());
    }
}
