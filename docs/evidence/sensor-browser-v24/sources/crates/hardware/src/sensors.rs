//! Sensor reading: hwmon enumeration, power-supply class, procfs and
//! nvidia-smi. All readers return `Option` per sensor; absent sensors are
//! omitted, never zero-filled.

use crate::fsutil;
use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
use std::path::{Path, PathBuf};

/// One enumerated hwmon device.
#[derive(Debug, Clone)]
pub struct HwmonDevice {
    pub path: PathBuf,
    pub name: String,
}

/// Enumerate `/sys/class/hwmon/hwmon*` (works for real symlinks and fixture
/// directories alike).
pub fn hwmon_devices(class_hwmon: &Path) -> Vec<HwmonDevice> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(class_hwmon) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("hwmon"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for p in paths {
        if let Some(name) = fsutil::read_trim(&p.join("name")) {
            out.push(HwmonDevice { path: p, name });
        }
    }
    out
}

/// Pretty names for well-known hwmon chips.
fn pretty_label(chip: &str, base: &str, channel: u8) -> String {
    let chip_label = match chip {
        "k10temp" | "coretemp" => "CPU".into(),
        "amdgpu" => "GPU (AMD)".into(),
        "nvme" => "NVMe".into(),
        "acpitz" => "ACPI thermal zone".into(),
        "spd5118" => "Memory module".into(),
        "hp" => "hp-wmi".into(),
        "BAT0" | "BAT1" => "Battery".into(),
        other => other.to_string(),
    };
    match base {
        "temp" => format!("{chip_label} temp {channel}"),
        "fan" => format!("{chip_label} fan {channel}"),
        "power" => format!("{chip_label} power {channel}"),
        "in" => format!("{chip_label} voltage {channel}"),
        "curr" => format!("{chip_label} current {channel}"),
        "freq" => format!("{chip_label} freq {channel}"),
        other => format!("{chip_label} {other} {channel}"),
    }
}

/// Unit scale applied to raw hwmon values by convention of the ABI:
/// temps are milli-°C, power micro-W, voltage milli-V, current micro-A,
/// fans plain RPM, freq (amdgpu) plain MHz or milli-Hz per device — the
/// kernel exposes `freqN_input` in Hz for amdgpu. We normalize where the
/// chip family is unambiguous and label units accordingly.
fn scaled(kind: SensorKind, _chip: &str, raw: f64) -> f64 {
    match kind {
        SensorKind::Temperature => raw / 1000.0,
        SensorKind::Power => raw / 1_000_000.0,
        SensorKind::Voltage => raw / 1000.0,
        SensorKind::Current => raw / 1000.0,
        SensorKind::Frequency => raw / 1_000_000.0, // hwmon ABI: Hz → MHz
        SensorKind::FanRpm => raw,
        _ => raw,
    }
}

/// Read all numeric samples from one hwmon device.
pub fn read_hwmon_samples(dev: &HwmonDevice) -> Vec<SensorSample> {
    let mut samples = Vec::new();
    let device = dev.path.join("device").canonicalize().ok();
    let device_id = device
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| crate::gpu::valid_pci_address(name))
        .map(str::to_string);
    if dev.name == "amdgpu"
        && device.as_ref().is_some_and(|path| {
            fsutil::read_trim(&path.join("power/runtime_status")).as_deref() == Some("suspended")
        })
    {
        return samples;
    }
    let Ok(entries) = std::fs::read_dir(&dev.path) else {
        return samples;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    names.sort();

    for file in names {
        for base in ["temp", "fan", "power", "in", "curr", "freq"] {
            let Some(rest) = file.strip_prefix(base) else {
                continue;
            };
            let Some(ch_str) = rest.strip_suffix("_input").or_else(|| {
                (base == "power")
                    .then(|| rest.strip_suffix("_average"))
                    .flatten()
            }) else {
                continue;
            };
            let Ok(channel) = ch_str.parse::<u8>() else {
                continue;
            };
            let raw = fsutil::read_f64(&dev.path.join(&file)).filter(|v| v.is_finite());
            if raw.is_none() && base != "temp" {
                continue;
            }
            let kind = match base {
                "temp" => SensorKind::Temperature,
                "fan" => SensorKind::FanRpm,
                "power" => SensorKind::Power,
                "in" => SensorKind::Voltage,
                "curr" => SensorKind::Current,
                _ => SensorKind::Frequency,
            };
            let label_override =
                fsutil::read_trim(&dev.path.join(format!("{base}{channel}_label")));
            let label = label_override
                .map(|l| format!("{} ({})", l, dev.name))
                .unwrap_or_else(|| pretty_label(&dev.name, base, channel));
            let id = format!(
                "{}/{}/{}",
                dev.path.file_name().unwrap_or_default().to_string_lossy(),
                dev.name,
                file
            );
            let mut sample = if let Some(raw) = raw {
                SensorSample::numeric(
                    id,
                    label,
                    kind,
                    SensorSource::Hwmon,
                    (scaled(kind, &dev.name, raw) * 100.0).round() / 100.0,
                )
            } else {
                SensorSample::unavailable(id, label, kind, SensorSource::Hwmon)
            };
            sample.device_id = device_id.clone();
            samples.push(sample);
        }
    }
    samples
}

/// Snapshot reader the daemon polls.
pub struct SensorReader {
    hwmon_class: PathBuf,
    proc_root: PathBuf,
    nvidia: bool,
}

impl SensorReader {
    pub fn new(sysfs: PathBuf, proc_root: PathBuf, nvidia: bool) -> Self {
        SensorReader {
            hwmon_class: sysfs.join("class/hwmon"),
            proc_root,
            nvidia,
        }
    }

    pub fn snapshot(&self) -> Vec<SensorSample> {
        let mut out = Vec::new();
        for dev in hwmon_devices(&self.hwmon_class) {
            out.extend(read_hwmon_samples(&dev));
        }
        out.extend(procfs_samples(&self.proc_root));
        if self.nvidia {
            out.extend(crate::sensors::nvidia::read_nvidia_samples());
        }
        out
    }
}

/// /proc-derived samples: CPU frequency, memory, load average.
pub fn procfs_samples(proc_root: &Path) -> Vec<SensorSample> {
    let mut out = Vec::new();
    // CPU frequencies from cpufreq policy dirs (kHz), else /proc/cpuinfo.
    let policy_dir = proc_root
        .parent()
        .unwrap_or(proc_root)
        .join("sys/devices/system/cpu/cpufreq");
    if policy_dir.exists() {
        let mut freqs: Vec<f64> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&policy_dir) {
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().starts_with("policy"))
                        .unwrap_or(false)
                })
                .collect();
            paths.sort();
            for p in paths {
                if let Some(khz) = fsutil::read_f64(&p.join("scaling_cur_freq")) {
                    freqs.push(khz / 1000.0); // kHz → MHz
                }
            }
        }
        if !freqs.is_empty() {
            let avg = freqs.iter().sum::<f64>() / freqs.len() as f64;
            out.push(SensorSample::numeric(
                "cpu/avg_freq",
                format!("CPU clock ({} cores)", freqs.len()),
                SensorKind::Frequency,
                SensorSource::Sysfs,
                (avg * 100.0).round() / 100.0,
            ));
        }
    }
    if !out.iter().any(|s| s.id == "cpu/avg_freq") {
        if let Some(info) = fsutil::read_trim(&proc_root.join("cpuinfo")) {
            let frequencies: Vec<f64> = info
                .lines()
                .filter_map(|l| l.strip_prefix("cpu MHz"))
                .filter_map(|s| s.trim().strip_prefix(':')?.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .collect();
            if !frequencies.is_empty() {
                out.push(SensorSample::numeric(
                    "cpu/avg_freq",
                    "CPU clock (reported average)",
                    SensorKind::Frequency,
                    SensorSource::Procfs,
                    frequencies.iter().sum::<f64>() / frequencies.len() as f64,
                ));
            }
        }
    }
    // Memory from /proc/meminfo (kB).
    if let Some(content) = fsutil::read_trim(&proc_root.join("meminfo")) {
        let mut total = None;
        let mut available = None;
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("MemTotal:") {
                total = v.trim().trim_end_matches(" kB").parse::<f64>().ok();
            } else if let Some(v) = line.strip_prefix("MemAvailable:") {
                available = v.trim().trim_end_matches(" kB").parse::<f64>().ok();
            }
        }
        if let (Some(t), Some(a)) = (total, available) {
            out.push(SensorSample::textual(
                "mem/total",
                "Memory total",
                SensorSource::Procfs,
                format!("{:.1} GiB", t / 1_048_576.0),
            ));
            let used_pct = if t > 0.0 { ((t - a) / t) * 100.0 } else { 0.0 };
            out.push(SensorSample::numeric(
                "mem/used_pct",
                "Memory used",
                SensorKind::Percent,
                SensorSource::Procfs,
                (used_pct * 10.0).round() / 10.0,
            ));
        }
    }
    // Load average.
    if let Some(la) = fsutil::read_trim(&proc_root.join("loadavg")) {
        if let Some(one) = la.split_whitespace().next() {
            if let Ok(v) = one.parse::<f64>() {
                out.push(SensorSample::numeric(
                    "cpu/load1",
                    "Load average (1 min)",
                    SensorKind::Count,
                    SensorSource::Procfs,
                    v,
                ));
            }
        }
    }
    out
}

/// Aggregate Linux CPU ticks excluding guest fields (already in user/nice).
pub fn cpu_ticks(path: &Path) -> Option<(u64, u64)> {
    let stat = fsutil::read_trim(path)?;
    let mut fields = stat
        .lines()
        .find(|l| l.starts_with("cpu "))?
        .split_whitespace()
        .skip(1);
    let values: Vec<u64> = fields
        .by_ref()
        .take(8)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if values.len() != 8 {
        return None;
    }
    Some((values.iter().sum(), values[3].saturating_add(values[4])))
}

pub fn cpu_utilization(previous: (u64, u64), current: (u64, u64)) -> Option<f64> {
    let total = current.0.checked_sub(previous.0)?;
    let idle = current.1.checked_sub(previous.1)?;
    if total == 0 || idle > total {
        return None;
    }
    Some((total - idle) as f64 * 100.0 / total as f64)
}

pub fn storage_samples(path: &Path) -> Vec<SensorSample> {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return vec![];
    };
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: CString is NUL-terminated; stat points to a valid output buffer.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return vec![];
    }
    let stat = unsafe { stat.assume_init() };
    if stat.f_blocks == 0 {
        return vec![];
    }
    vec![SensorSample::numeric(
        "storage/root_used_pct",
        "Root filesystem used",
        SensorKind::Percent,
        SensorSource::Sysfs,
        (stat.f_blocks.saturating_sub(stat.f_bfree)) as f64 * 100.0 / stat.f_blocks as f64,
    )]
}

pub fn amd_gpu_samples(root: &crate::SysRoot) -> Vec<SensorSample> {
    let mut samples = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("sys/class/drm")) else {
        return samples;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let device = entry.path().join("device");
        if fsutil::read_trim(&device.join("vendor")).as_deref() != Some("0x1002") {
            continue;
        }
        if let Some(value) =
            fsutil::read_f64(&device.join("gpu_busy_percent")).filter(|v| (0.0..=100.0).contains(v))
        {
            samples.push(SensorSample::numeric(
                format!("{name}/amd/utilization"),
                format!("GPU utilization (AMD {name})"),
                SensorKind::Percent,
                SensorSource::Amdgpu,
                value,
            ));
        }
        if let (Some(used), Some(total)) = (
            fsutil::read_f64(&device.join("mem_info_vram_used")),
            fsutil::read_f64(&device.join("mem_info_vram_total")),
        ) {
            if total > 0.0 {
                samples.push(SensorSample::numeric(
                    format!("{name}/amd/vram_used_pct"),
                    format!("GPU VRAM used (AMD {name})"),
                    SensorKind::Percent,
                    SensorSource::Amdgpu,
                    used * 100.0 / total,
                ));
            }
        }
    }
    samples
}

pub mod nvidia {
    //! NVIDIA GPU telemetry via `nvidia-smi`. Uses the documented
    //! `--query-gpu` CSV interface; failures yield no samples. Capability
    //! detection describes interface/device access, not support for every metric.

    use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
    use std::os::unix::{ffi::OsStrExt, fs::FileTypeExt};
    use std::path::Path;

    /// Presence/access checks do not open device files or wake the GPU.
    pub fn device_nodes_available() -> bool {
        device_nodes_at(Path::new("/dev"))
    }

    fn device_nodes_at(dev: &Path) -> bool {
        let usable = |path: &Path| {
            let Ok(path_string) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
                return false;
            };
            path.metadata().is_ok_and(|m| m.file_type().is_char_device())
                // SAFETY: path_string is a live, NUL-terminated pathname.
                && unsafe { libc::access(path_string.as_ptr(), libc::R_OK | libc::W_OK) } == 0
        };
        usable(&dev.join("nvidiactl"))
            && std::fs::read_dir(dev).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry.file_name().to_str().is_some_and(|name| {
                        name.strip_prefix("nvidia").is_some_and(|suffix| {
                            !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit())
                        })
                    }) && usable(&entry.path())
                })
            })
    }

    /// The existing single-GPU metrics refer to the first bound NVIDIA display
    /// adapter in PCI order. Selecting it explicitly avoids querying other,
    /// potentially suspended adapters through an unfiltered nvidia-smi call.
    pub fn primary_gpu(root: &crate::SysRoot) -> Option<crate::gpu::GpuInfo> {
        crate::gpu::list_gpus(root)
            .into_iter()
            .find(|gpu| gpu.vendor == "NVIDIA" && gpu.driver.as_deref() == Some("nvidia"))
    }

    pub fn nvidia_smi_available() -> bool {
        // Presence check does not wake a runtime-suspended dGPU.
        std::path::Path::new("/usr/bin/nvidia-smi").exists()
    }

    pub fn read_nvidia_samples() -> Vec<SensorSample> {
        let Some(gpu) = primary_gpu(&crate::SysRoot::default()) else {
            return Vec::new();
        };
        if gpu.runtime_status.as_deref() == Some("suspended") || !device_nodes_available() {
            return Vec::new();
        }
        let Ok(out) = crate::command::output(std::process::Command::new("/usr/bin/nvidia-smi")
            .arg(format!("--id={}", gpu.pci_address))
            .args([
                "--query-gpu=name,temperature.gpu,utilization.gpu,power.draw,clocks.sm,memory.used,memory.total",
                "--format=csv,noheader,nounits",
            ]), std::time::Duration::from_secs(2))
        else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        let mut samples = parse_csv(&String::from_utf8_lossy(&out.stdout));
        for sample in &mut samples {
            sample.device_id = Some(gpu.pci_address.clone());
        }
        samples
    }

    fn parse_csv(text: &str) -> Vec<SensorSample> {
        let line = match text.lines().next() {
            Some(l) => l,
            None => return Vec::new(),
        };
        let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if fields.len() != 7 || fields[0].is_empty() {
            return Vec::new();
        }
        let name = fields[0];
        let num = |s: &str| -> Option<f64> {
            if s.eq_ignore_ascii_case("N/A") || s.eq_ignore_ascii_case("[N/A]") {
                None
            } else {
                s.parse::<f64>().ok().filter(|v| v.is_finite())
            }
        };
        let mut v = vec![SensorSample::textual(
            "nvidia/name",
            "GPU",
            SensorSource::NvidiaSmi,
            name,
        )];
        if let Some(t) = num(fields[1]) {
            v.push(SensorSample::numeric(
                "nvidia/temp",
                "GPU temperature (NVIDIA)",
                SensorKind::Temperature,
                SensorSource::NvidiaSmi,
                t,
            ));
        }
        if let Some(u) = num(fields[2]).filter(|v| (0.0..=100.0).contains(v)) {
            v.push(SensorSample::numeric(
                "nvidia/util",
                "GPU utilization (NVIDIA)",
                SensorKind::Percent,
                SensorSource::NvidiaSmi,
                u,
            ));
        }
        if let Some(p) = num(fields[3]).filter(|v| *v >= 0.0) {
            v.push(SensorSample::numeric(
                "nvidia/power",
                "GPU power (NVIDIA)",
                SensorKind::Power,
                SensorSource::NvidiaSmi,
                p,
            ));
        }
        if let Some(c) = num(fields[4]).filter(|v| *v >= 0.0) {
            v.push(SensorSample::numeric(
                "nvidia/sm_clock",
                "GPU SM clock (NVIDIA)",
                SensorKind::Frequency,
                SensorSource::NvidiaSmi,
                c,
            ));
        }
        if let (Some(used), Some(total)) = (num(fields[5]), num(fields[6])) {
            if total <= 0.0 || used < 0.0 || used > total {
                return v;
            }
            v.push(SensorSample::numeric(
                "nvidia/vram_used_pct",
                "GPU VRAM used (NVIDIA)",
                SensorKind::Percent,
                SensorSource::NvidiaSmi,
                used / total * 100.0,
            ));
        }
        v
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn device_detection_requires_control_and_numbered_character_nodes() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("nvidiactl"), "").unwrap();
            std::fs::write(dir.path().join("nvidia0"), "").unwrap();
            assert!(!device_nodes_at(dir.path()));
            std::fs::remove_file(dir.path().join("nvidiactl")).unwrap();
            std::os::unix::fs::symlink("/dev/null", dir.path().join("nvidiactl")).unwrap();
            assert!(!device_nodes_at(dir.path()));
            std::os::unix::fs::symlink("/dev/null", dir.path().join("nvidia7")).unwrap();
            assert!(device_nodes_at(dir.path()));
        }

        #[test]
        fn csv_values_preserve_units_and_omit_invalid_numbers() {
            let values = parse_csv("GPU fixture, 45, 17, 12.5, 210, 1024, 4096\n");
            assert_eq!(values.len(), 6);
            assert_eq!(
                values
                    .iter()
                    .find(|s| s.id == "nvidia/vram_used_pct")
                    .unwrap()
                    .value,
                Some(25.0)
            );
            assert_eq!(
                values
                    .iter()
                    .find(|s| s.id == "nvidia/power")
                    .unwrap()
                    .value,
                Some(12.5)
            );
            for csv in [
                "GPU, NaN, inf, -1, -2, 10, 0",
                "GPU, N/A, [N/A], nan, NaN, -1, 100",
                "GPU, N/A, 101, N/A, N/A, 101, 100",
            ] {
                let values = parse_csv(csv);
                assert_eq!(values.len(), 1, "{csv}");
                assert_eq!(values[0].id, "nvidia/name");
            }
            assert!(parse_csv("bad, truncated").is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_uses_deltas_excludes_guests_and_handles_counter_reset() {
        let dir = tempfile::tempdir().unwrap();
        let stat = dir.path().join("stat");
        std::fs::write(&stat, "cpu 10 5 5 70 10 0 0 0 100 100\n").unwrap();
        assert_eq!(cpu_ticks(&stat), Some((100, 80)));
        assert_eq!(cpu_utilization((100, 20), (200, 70)), Some(50.0));
        assert_eq!(cpu_utilization((100, 80), (100, 80)), None);
        assert_eq!(cpu_utilization((100, 80), (10, 2)), None);
    }

    #[test]
    fn hwmon_current_frequency_and_average_power_follow_abi_units() {
        let dir = tempfile::tempdir().unwrap();
        for (f, value) in [
            ("curr1_input", "1500"),
            ("freq1_input", "1500000000"),
            ("power1_average", "12500000"),
        ] {
            std::fs::write(dir.path().join(f), value).unwrap();
        }
        let samples = read_hwmon_samples(&HwmonDevice {
            path: dir.path().to_owned(),
            name: "amdgpu".into(),
        });
        assert!(samples
            .iter()
            .any(|s| s.kind == SensorKind::Current && s.value == Some(1.5)));
        assert!(samples
            .iter()
            .any(|s| s.kind == SensorKind::Frequency && s.value == Some(1500.0)));
        assert!(samples
            .iter()
            .any(|s| s.kind == SensorKind::Power && s.value == Some(12.5)));
    }

    #[test]
    fn reads_fixture_hwmon_tree() {
        let dir = tempfile::tempdir().unwrap();
        let hw = dir.path().join("hwmon0");
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::write(hw.join("name"), "k10temp\n").unwrap();
        std::fs::write(hw.join("temp1_input"), "61500\n").unwrap();
        std::fs::write(hw.join("temp1_label"), "Tctl\n").unwrap();

        let devs = hwmon_devices(dir.path());
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].name, "k10temp");
        let samples = read_hwmon_samples(&devs[0]);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].label, "Tctl (k10temp)");
        assert_eq!(samples[0].value, Some(61.5));
        assert_eq!(samples[0].kind, SensorKind::Temperature);
    }

    #[test]
    fn gpu_identity_unavailable_channels_and_suspend_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("0000:06:00.0");
        std::fs::create_dir_all(device.join("power")).unwrap();
        std::fs::write(device.join("power/runtime_status"), "active").unwrap();
        let hw = dir.path().join("hwmon2");
        std::fs::create_dir_all(&hw).unwrap();
        std::os::unix::fs::symlink(&device, hw.join("device")).unwrap();
        std::fs::write(hw.join("temp1_input"), "45000").unwrap();
        std::fs::write(hw.join("temp2_input"), "invalid").unwrap();
        let sensor = HwmonDevice {
            path: hw,
            name: "amdgpu".into(),
        };
        let samples = read_hwmon_samples(&sensor);
        assert_eq!(samples.len(), 2);
        assert!(samples
            .iter()
            .all(|s| s.device_id.as_deref() == Some("0000:06:00.0")));
        assert_eq!(samples[0].value, Some(45.0));
        assert_eq!(samples[1].value, None);
        std::fs::write(device.join("power/runtime_status"), "suspended").unwrap();
        assert!(read_hwmon_samples(&sensor).is_empty());
    }

    #[test]
    fn missing_files_yield_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(hwmon_devices(dir.path()).is_empty());
    }

    #[test]
    fn meminfo_parses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("meminfo"),
            "MemTotal:       16000000 kB\nMemAvailable:    8000000 kB\n",
        )
        .unwrap();
        let s = procfs_samples(dir.path());
        let used = s.iter().find(|x| x.id == "mem/used_pct").unwrap();
        assert_eq!(used.value, Some(50.0));
    }
}
