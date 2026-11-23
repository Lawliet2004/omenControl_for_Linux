//! Mock hardware backends.
//!
//! Mocks are built as *real fixture trees* consumed by the same
//! [`SysfsBackend`] code path as production — so tests exercise the actual
//! parsing/writing logic, not a parallel implementation that could drift.
//! A shell shim stands in for `powerprofilesctl` so profile switching can be
//! tested end-to-end without D-Bus.

use crate::backend::SysfsBackend;
use crate::{HardwareBackend, SysRoot};
use std::fs;
use std::path::PathBuf;

/// The reference machines used across the test suite.
pub enum MockModel {
    /// HP Victus 15 fb (board 8C30-style): coarse Auto/Max fixture, PPD
    /// profiles, no manual fan targets or RGB, hybrid AMD+NVIDIA.
    VictusFb,
    /// An OMEN-class board with pwm1/pwm2 (victus_s allowlist, e.g. 8A3D):
    /// manual fan control + platform_profile.
    FanControlOmen,
    /// Non-HP laptop: PPD profiles only.
    PlainLaptop,
}

/// Serializes tests that touch the global OMEN_PPDCTL environment variable
/// (Rust tests share one process, so env races would be flaky otherwise).
pub static PPD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub struct MockBackend {
    pub inner: SysfsBackend,
    pub dir: tempfile::TempDir,
    pub model: MockModel,
}

impl MockBackend {
    pub fn new(model: MockModel) -> MockBackend {
        let dir = tempfile::tempdir().expect("tempdir");
        match model {
            MockModel::VictusFb => build_victus_fb(dir.path()),
            MockModel::FanControlOmen => build_fan_control_omen(dir.path()),
            MockModel::PlainLaptop => build_plain_laptop(dir.path()),
        }
        w(dir.path().join("sys/class/net/fixture0/ifindex"), "7");
        w(dir.path().join("sys/class/net/fixture0/operstate"), "up");
        w(dir.path().join("sys/class/net/fixture0/flags"), "0x1003");
        w(dir.path().join("proc/net/dev"), "Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\nfixture0: 1048576 100 0 0 0 0 0 0 524288 50 0 0 0 0 0 0\n");
        // PPD shim: one per process, idempotent; never uninstalled so
        // concurrent backends cannot unset each other's env var. Tests that
        // need "no PPD" behavior take PPD_LOCK and override explicitly.
        w(dir.path().join("run/omen-fixture/ppd-current"), "balanced");
        w(
            dir.path().join("run/omen-fixture/ppd-choices"),
            "power-saver balanced performance",
        );
        MockBackend {
            inner: SysfsBackend::with_root(SysRoot::new(dir.path().to_path_buf())),
            dir,
            model,
        }
    }

    /// Live-mutate a fixture hwmon value (tests simulate load).
    pub fn set_hwmon_dir(&self, dev: &str, file: &str, value: &str) {
        let p = self
            .dir
            .path()
            .join(format!("sys/class/hwmon/{dev}/{file}"));
        if p.exists() {
            fs::write(p, format!("{value}\n")).unwrap();
        }
    }
}

impl HardwareBackend for MockBackend {
    // In-process unit fixtures. The daemon wraps its mock backend with the
    // separate writer; these no-ops are not a live implementation.
    fn begin_fan_control(&self) -> omen_core::Result<()> {
        Ok(())
    }
    fn maintain_fan_lease(&self) -> omen_core::Result<()> {
        Ok(())
    }
    fn network(&self) -> omen_core::Result<omen_core::network::NetworkSnapshot> {
        self.inner.network()
    }
    fn graphics(&self) -> crate::gpu::GraphicsStatus {
        self.inner.graphics()
    }
    fn detect(&self) -> omen_core::machine::MachineInfo {
        self.inner.detect()
    }
    fn capabilities(&self) -> omen_core::capability::Capabilities {
        self.inner.capabilities()
    }
    fn read_sensors(&self) -> omen_core::Result<Vec<omen_core::sensors::SensorSample>> {
        self.inner.read_sensors()
    }
    fn fan_state(&self) -> omen_core::Result<omen_core::fans::FanState> {
        self.inner.fan_state()
    }
    fn set_fan_mode(&self, mode: omen_core::fans::FanMode) -> omen_core::Result<()> {
        self.inner.set_fan_mode(mode)
    }
    fn profile(&self) -> omen_core::Result<omen_core::profile::ProfileKind> {
        self.inner.profile()
    }
    fn set_profile(&self, p: omen_core::profile::ProfileKind) -> omen_core::Result<()> {
        self.inner.set_profile(p)
    }
    fn lighting_state(&self) -> omen_core::Result<omen_core::lighting::LightingState> {
        self.inner.lighting_state()
    }
    fn set_lighting(&self, st: &omen_core::lighting::LightingState) -> omen_core::Result<()> {
        self.inner.set_lighting(st)
    }
    fn battery(&self) -> omen_core::Result<omen_core::power::BatteryInfo> {
        self.inner.battery()
    }
    fn ac(&self) -> omen_core::Result<omen_core::power::AcInfo> {
        self.inner.ac()
    }
    fn set_charge_threshold(&self, pct: u8) -> omen_core::Result<()> {
        self.inner.set_charge_threshold(pct)
    }
    fn charge_threshold(&self) -> omen_core::Result<Option<u8>> {
        self.inner.charge_threshold()
    }
    fn detect_games(&self) -> Vec<omen_core::games::Game> {
        self.inner.detect_games()
    }
}

// --------------------------------------------------------------- fixtures --

fn w(path: PathBuf, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("{content}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// The project reference laptop as observed live (see RE report §4).
pub fn build_victus_fb(root: &std::path::Path) {
    let dmi = root.join("sys/class/dmi/id");
    for (f, v) in [
        ("sys_vendor", "HP"),
        (
            "product_name",
            "Victus by HP 15.6 inch Gaming Laptop PC-fb1002AX",
        ),
        ("product_family", "103C_5335M7 HP Victus"),
        ("board_name", "8C30"),
        ("board_vendor", "HP"),
        ("bios_vendor", "AMI"),
        ("bios_version", "F.10"),
    ] {
        w(dmi.join(f), v);
    }
    w(root.join("proc/sys/kernel/osrelease"), "7.0.0-38-generic");
    // hp-wmi hwmon: monitoring only (no pwm1!).
    let hp = root.join("sys/class/hwmon/hwmon0");
    w(hp.join("name"), "hp");
    w(hp.join("fan1_input"), "2400");
    w(hp.join("fan2_input"), "2300");
    w(hp.join("pwm1_enable"), "2");
    // CPU + GPU temps.
    let k10 = root.join("sys/class/hwmon/hwmon1");
    w(k10.join("name"), "k10temp");
    w(k10.join("temp1_input"), "61800");
    let amd = root.join("sys/class/hwmon/hwmon2");
    w(amd.join("name"), "amdgpu");
    w(amd.join("temp1_input"), "45200");
    w(amd.join("power1_average"), "1500000");
    // Battery + AC.
    let ps = root.join("sys/class/power_supply");
    w(ps.join("BAT0/type"), "Battery");
    w(ps.join("BAT0/status"), "Full");
    w(ps.join("BAT0/capacity"), "100");
    w(ps.join("BAT0/present"), "1");
    w(ps.join("BAT0/energy_now"), "40000000");
    w(ps.join("BAT0/energy_full"), "40000000");
    w(ps.join("BAT0/energy_full_design"), "45000000");
    w(ps.join("BAT0/power_now"), "0");
    w(ps.join("ACAD/type"), "Mains");
    w(ps.join("ACAD/online"), "1");
    // Hybrid GPUs (01:00 NVIDIA dGPU, 06:00 AMD iGPU).
    let pci = root.join("sys/bus/pci/devices");
    w(pci.join("0000:01:00.0/vendor"), "0x10de");
    w(pci.join("0000:01:00.0/class"), "0x030000");
    w(pci.join("0000:01:00.0/power/runtime_status"), "suspended");
    w(pci.join("0000:06:00.0/vendor"), "0x1002");
    w(pci.join("0000:06:00.0/class"), "0x030000");
    w(pci.join("0000:06:00.0/power/runtime_status"), "active");
}

/// An OMEN-class board with hp-wmi fan control (pwm1/pwm2 present) and
/// platform_profile registered.
pub fn build_fan_control_omen(root: &std::path::Path) {
    // An explicitly present empty PCI inventory: this base fixture is CPU-only.
    // GPU-aware curve scenarios add individually identified display devices.
    fs::create_dir_all(root.join("sys/bus/pci/devices")).unwrap();
    let dmi = root.join("sys/class/dmi/id");
    for (f, v) in [
        ("sys_vendor", "HP"),
        ("product_name", "HP OMEN 16"),
        ("product_family", "103C_5336NF HP OMEN"),
        ("board_name", "8A3D"),
        ("board_vendor", "HP"),
        ("bios_vendor", "AMI"),
        ("bios_version", "F.05"),
    ] {
        w(dmi.join(f), v);
    }
    w(root.join("proc/sys/kernel/osrelease"), "7.0.0-38-generic");
    let hp = root.join("sys/class/hwmon/hwmon0");
    w(hp.join("name"), "hp");
    w(hp.join("fan1_input"), "2800");
    w(hp.join("fan2_input"), "2600");
    w(hp.join("pwm1_enable"), "2");
    w(hp.join("pwm1"), "128");
    w(hp.join("pwm2"), "128");
    let k10 = root.join("sys/class/hwmon/hwmon1");
    w(k10.join("name"), "k10temp");
    w(k10.join("temp1_input"), "61800");
    let acpi = root.join("sys/firmware/acpi");
    w(acpi.join("platform_profile"), "balanced");
    w(
        acpi.join("platform_profile_choices"),
        "quiet balanced performance",
    );
    let ps = root.join("sys/class/power_supply");
    w(ps.join("BAT0/type"), "Battery");
    w(ps.join("BAT0/status"), "Discharging");
    w(ps.join("BAT0/capacity"), "50");
    w(ps.join("ACAD/type"), "Mains");
    w(ps.join("ACAD/online"), "0");
}

/// Non-HP laptop: no hp hwmon, PPD-only profiles.
pub fn build_plain_laptop(root: &std::path::Path) {
    let dmi = root.join("sys/class/dmi/id");
    for (f, v) in [
        ("sys_vendor", "LENOVO"),
        ("product_name", "82XY"),
        ("board_name", "LNVNB161216"),
        ("board_vendor", "LENOVO"),
    ] {
        w(dmi.join(f), v);
    }
    w(root.join("proc/sys/kernel/osrelease"), "7.0.0-38-generic");
    let coretemp = root.join("sys/class/hwmon/hwmon0");
    w(coretemp.join("name"), "coretemp");
    w(coretemp.join("temp1_input"), "55000");
}

/// Install a per-process `powerprofilesctl`-compatible shim and point
/// OMEN_PPDCTL at it (shared state file under /tmp for this process).
#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::fans::FanMode;
    use omen_core::machine::MachineClass;
    use omen_core::profile::ProfileKind;

    #[test]
    fn victus_fb_fixture_matches_reference_behavior() {
        let _g = PPD_LOCK.lock().unwrap();
        let m = MockBackend::new(MockModel::VictusFb);
        let info = m.detect();
        assert_eq!(info.class, MachineClass::Victus);
        assert_eq!(info.board_name, "8C30");
        let caps = m.capabilities();
        assert!(caps.fan_monitoring.is_available());
        assert!(
            !caps.fan_control.is_available(),
            "8C30 must not expose manual fan control"
        );
        assert!(!caps.fan_curve.is_available());
        // Profiles via PPD shim.
        assert!(caps.performance_profiles.is_available());
        assert_eq!(
            caps.profile_plane,
            omen_core::capability::ControlPlane::PowerProfilesDaemon
        );
        // Honest absences.
        assert!(!caps.lighting_zones.is_available());
        assert!(!caps.charge_threshold.is_available());
        assert!(!caps.graphics_mux.is_available());
        // Fan writes must be refused.
        assert!(m.set_fan_mode(FanMode::Manual { duty_pct: 60 }).is_err());
        // But PPD profile switch works end-to-end.
        m.set_profile(ProfileKind::Performance).unwrap();
        assert_eq!(m.profile().unwrap(), ProfileKind::Performance);
        // Sensors contain real fixture values.
        let s = m.read_sensors().unwrap();
        assert!(s
            .iter()
            .any(|x| x.id.ends_with("k10temp/temp1_input") && x.value == Some(61.8)));
        assert!(s
            .iter()
            .any(|x| x.id.ends_with("hp/fan1_input") && x.value == Some(2400.0)));
        // Fan telemetry readable.
        let f = m.fan_state().unwrap();
        assert_eq!(f.cpu_rpm, Some(2400));
        // Battery.
        let b = m.battery().unwrap();
        assert_eq!(b.capacity_pct, Some(100));
        assert!(m.ac().unwrap().online);
    }

    #[test]
    fn fan_control_omen_fixture_supports_manual() {
        let _g = PPD_LOCK.lock().unwrap();
        let m = MockBackend::new(MockModel::FanControlOmen);
        let caps = m.capabilities();
        assert!(caps.fan_control.is_available());
        assert!(caps.fan_curve.is_available());
        assert_eq!(
            caps.profile_plane,
            omen_core::capability::ControlPlane::PlatformProfile
        );
        assert_eq!(
            caps.profile_choices,
            vec![
                ProfileKind::Quiet,
                ProfileKind::Balanced,
                ProfileKind::Performance
            ]
        );
        m.set_fan_mode(FanMode::Manual { duty_pct: 70 }).unwrap();
        let st = m.fan_state().unwrap();
        assert!(
            matches!(st.mode, Some(FanMode::Manual { duty_pct }) if (65..=75).contains(&duty_pct))
        );
        m.set_profile(ProfileKind::Performance).unwrap();
        assert_eq!(m.profile().unwrap(), ProfileKind::Performance);
        // Floor enforcement through the whole stack.
        m.set_fan_mode(FanMode::Manual { duty_pct: 0 }).unwrap();
        let st = m.fan_state().unwrap();
        assert!(
            matches!(st.mode, Some(FanMode::Manual { duty_pct }) if duty_pct >= omen_core::MIN_FAN_DUTY_PCT)
        );
    }

    #[test]
    fn plain_laptop_is_monitoring_only() {
        let _g = PPD_LOCK.lock().unwrap();
        let m = MockBackend::new(MockModel::PlainLaptop);
        let caps = m.capabilities();
        assert_eq!(caps.machine.class, MachineClass::NonHp);
        assert!(!caps.fan_monitoring.is_available());
        assert!(matches!(
            m.fan_state(),
            Err(omen_core::OmenError::Unsupported(_))
        ));
        // PPD profiles still available (generic, safe).
        assert!(caps.performance_profiles.is_available());
        let s = m.read_sensors().unwrap();
        assert!(s.iter().any(|x| x.id.ends_with("coretemp/temp1_input")));
    }

    #[test]
    fn live_fixture_mutation_visible_through_backend() {
        let _g = PPD_LOCK.lock().unwrap();
        let m = MockBackend::new(MockModel::VictusFb);
        // k10temp lives at hwmon1 in the Victus fixture.
        m.set_hwmon_dir("hwmon1", "temp1_input", "90000");
        let s = m.read_sensors().unwrap();
        assert!(s
            .iter()
            .any(|x| x.id.ends_with("k10temp/temp1_input") && x.value == Some(90.0)));
    }
}
