//! PCI GPU enumeration and hybrid-graphics status (read-only).

use crate::fsutil;
use crate::SysRoot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    pub pci_address: String,
    pub vendor: String,
    /// Driver bound, e.g. `nvidia`, `amdgpu`, `i915`.
    pub driver: Option<String>,
    /// True identifies an NVIDIA discrete device. False means unclassified,
    /// not proven integrated; AMD/Intel vendor IDs alone do not prove topology.
    pub discrete: bool,
    /// runtime PM status: `active`, `suspended`, `unsupported`.
    pub runtime_status: Option<String>,
}

const VENDOR_NVIDIA: u32 = 0x10de;
const VENDOR_AMD: u32 = 0x1002;
const VENDOR_INTEL: u32 = 0x8086;

fn vendor_name(v: u32) -> &'static str {
    match v {
        VENDOR_NVIDIA => "NVIDIA",
        VENDOR_AMD => "AMD",
        VENDOR_INTEL => "Intel",
        _ => "Unknown",
    }
}

pub fn list_gpus(root: &SysRoot) -> Vec<GpuInfo> {
    inventory(root).0
}

pub fn valid_pci_address(address: &str) -> bool {
    let bytes = address.as_bytes();
    bytes.len() == 12
        && bytes[4] == b':'
        && bytes[7] == b':'
        && bytes[10] == b'.'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

fn inventory(root: &SysRoot) -> (Vec<GpuInfo>, bool) {
    let mut out = Vec::new();
    let pci = root.join("sys/bus/pci/devices");
    let Ok(entries) = std::fs::read_dir(&pci) else {
        return (out, false);
    };
    let mut complete = true;
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(_) => complete = false,
        }
    }
    paths.sort();
    for p in paths {
        // class 0x03* = display controller
        let Some(class) = fsutil::read_trim(&p.join("class")) else {
            complete = false;
            continue;
        };
        if !class.starts_with("0x")
            || class.len() != 8
            || u32::from_str_radix(class.trim_start_matches("0x"), 16).is_err()
        {
            complete = false;
            continue;
        }
        if !class.starts_with("0x03") {
            continue;
        }
        // PCI sysfs vendor/class are hex strings like "0x10de".
        let vendor = fsutil::read_trim(&p.join("vendor"))
            .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok());
        complete &= vendor.is_some()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(valid_pci_address);
        let vendor = vendor.unwrap_or(0);
        let driver = std::fs::read_link(p.join("driver"))
            .ok()
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()));
        let discrete = vendor == VENDOR_NVIDIA;
        let runtime_status = fsutil::read_trim(&p.join("power/runtime_status"));
        out.push(GpuInfo {
            pci_address: p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            vendor: vendor_name(vendor).to_string(),
            driver,
            discrete,
            runtime_status,
        });
    }
    (out, complete)
}

/// Hybrid graphics status summary for the Graphics screen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphicsStatus {
    pub gpus: Vec<GpuInfo>,
    /// False includes failed/partial PCI discovery, not proof of zero GPUs.
    #[serde(default)]
    pub inventory_complete: bool,
    pub discrete_runtime: Option<String>,
    pub mux_available: bool,
    pub mux_note: String,
}

pub fn graphics_status(root: &SysRoot) -> GraphicsStatus {
    let (gpus, inventory_complete) = inventory(root);
    let discrete_runtime = gpus
        .iter()
        .find(|g| g.discrete)
        .and_then(|g| g.runtime_status.clone());
    GraphicsStatus {
        gpus,
        inventory_complete,
        discrete_runtime,
        mux_available: false, // No mainline interface; see RE report §6.5.
        mux_note: "No mainline kernel MUX interface for this model. omen-linux provides \
                   monitoring only and will never issue firmware graphics-mode changes without \
                   a verified interface."
            .into(),
    }
}

pub use crate::sensors::nvidia::nvidia_smi_available;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_incomplete_pci_inventory_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::new(dir.path());
        assert!(!graphics_status(&root).inventory_complete);
        std::fs::create_dir_all(root.join("sys/bus/pci/devices")).unwrap();
        assert!(graphics_status(&root).inventory_complete);
        let device = root.join("sys/bus/pci/devices/0000:01:00.0");
        std::fs::create_dir_all(&device).unwrap();
        assert!(!graphics_status(&root).inventory_complete);
        std::fs::write(device.join("class"), "03").unwrap();
        assert!(!graphics_status(&root).inventory_complete);
    }

    #[test]
    fn enumerates_fixture_pci_gpus() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("sys/bus/pci/devices");
        for (addr, vendor, class, drv) in [
            ("0000:01:00.0", "0x10de", "0x030000", "nvidia"),
            ("0000:06:00.0", "0x1002", "0x030000", "amdgpu"),
            ("0000:00:08.1", "0x1022", "0x060400", "pcieport"), // bridge, ignored
        ] {
            let d = base.join(addr);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(d.join("class"), format!("{class}\n")).unwrap();
            if !drv.is_empty() {
                std::fs::create_dir_all(dir.path().join("drivers").join(drv)).unwrap();
                std::os::unix::fs::symlink(dir.path().join("drivers").join(drv), d.join("driver"))
                    .unwrap();
            }
        }
        let root = SysRoot::new(dir.path());
        let g = list_gpus(&root);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].vendor, "NVIDIA");
        assert!(g[0].discrete);
        assert_eq!(g[0].driver.as_deref(), Some("nvidia"));
        assert_eq!(g[1].vendor, "AMD");
        assert!(!g[1].discrete);
        let st = graphics_status(&root);
        assert!(!st.mux_available);
    }
}
