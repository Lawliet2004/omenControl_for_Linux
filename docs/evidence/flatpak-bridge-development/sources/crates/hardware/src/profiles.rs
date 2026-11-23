//! Standard profile interfaces. Prefer the active desktop power manager;
//! do not write platform_profile behind its back. No raw firmware values.
use crate::{fsutil, SysRoot};
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use std::path::PathBuf;
use std::time::Duration;

pub mod holds;

#[derive(Debug, Clone)]
pub enum ProfileBackend {
    PlatformProfile {
        sysfs: PathBuf,
        choices: Vec<ProfileKind>,
    },
    Ppd {
        choices: Vec<ProfileKind>,
        service: String,
    },
    Fixture {
        current: PathBuf,
        choices: Vec<ProfileKind>,
    },
    None,
}

fn parse_platform_choice(name: &str) -> Option<ProfileKind> {
    match name.trim() {
        "quiet" | "low-power" => Some(ProfileKind::Quiet),
        "balanced" => Some(ProfileKind::Balanced),
        "cool" => Some(ProfileKind::Cool),
        "performance" => Some(ProfileKind::Performance),
        _ => None,
    }
}

fn ppd_path(service: &str) -> &'static str {
    if service == "org.freedesktop.UPower.PowerProfiles" {
        "/org/freedesktop/UPower/PowerProfiles"
    } else {
        "/net/hadess/PowerProfiles"
    }
}

fn ppd_property(service: &str, property: &str) -> Result<serde_json::Value> {
    let out = crate::command::output(
        std::process::Command::new("/usr/bin/busctl").args([
            "--system",
            "--timeout=2",
            "--json=short",
            "get-property",
            service,
            ppd_path(service),
            service,
            property,
        ]),
        Duration::from_secs(3),
    )?;
    if !out.status.success() {
        return Err(OmenError::Io("PPD D-Bus property read failed".into()));
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| OmenError::Io(format!("PPD property reply: {e}")))?;
    value
        .get("data")
        .cloned()
        .ok_or_else(|| OmenError::Io("PPD property reply has no data".into()))
}

fn advertised_profiles(value: &serde_json::Value) -> Vec<ProfileKind> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("Profile")?.get("data")?.as_str().and_then(map_ppd))
        .collect()
}

fn ppd_backend() -> Option<ProfileBackend> {
    for service in [
        "org.freedesktop.UPower.PowerProfiles",
        "net.hadess.PowerProfiles",
    ] {
        if let Ok(value) = ppd_property(service, "Profiles") {
            let choices = advertised_profiles(&value);
            if !choices.is_empty() {
                return Some(ProfileBackend::Ppd {
                    choices,
                    service: service.into(),
                });
            }
        }
    }
    None
}

impl ProfileBackend {
    fn reject_conflicting_managers(&self) -> Result<()> {
        // Avoid known independent managers. Presence detection is a heuristic,
        // not proof that no other writer exists; record this limitation.
        if !matches!(self, Self::Fixture { .. }) {
            if let Ok(processes) = std::fs::read_dir("/proc") {
                for process in processes.flatten() {
                    if let Some(name) = fsutil::read_trim(&process.path().join("comm")) {
                        if ["tlp", "tuned", "auto-cpufreq"].contains(&name.as_str()) {
                            return Err(OmenError::Unsupported(format!(
                                "conflicting power manager '{name}' is running"
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn hold(&self, profile: ProfileKind, reason: &str) -> Result<holds::ProfileHold> {
        match self {
            Self::Ppd { service, choices } if choices.contains(&profile) => {
                self.reject_conflicting_managers()?;
                holds::ProfileHold::system(service, profile, reason)
            }
            _ => Err(OmenError::Unsupported(
                "managed profiles require Power Profiles Daemon and an advertised holdable profile"
                    .into(),
            )),
        }
    }
    pub fn detect(root: &SysRoot) -> ProfileBackend {
        let live = root.path() == std::path::Path::new("/");
        if live {
            if let Some(ppd) = ppd_backend() {
                return ppd;
            }
        }
        let sysfs = root.join("sys/firmware/acpi/platform_profile");
        let choices_f = root.join("sys/firmware/acpi/platform_profile_choices");
        if sysfs.exists() {
            let choices = fsutil::read_trim(&choices_f)
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(parse_platform_choice)
                .collect::<Vec<_>>();
            if !choices.is_empty() {
                return Self::PlatformProfile { sysfs, choices };
            }
        }
        if !live {
            let current = root.join("run/omen-fixture/ppd-current");
            if current.exists() {
                let choices = fsutil::read_trim(&root.join("run/omen-fixture/ppd-choices"))
                    .unwrap_or_default()
                    .split_whitespace()
                    .filter_map(map_ppd)
                    .collect();
                return Self::Fixture { current, choices };
            }
        }
        Self::None
    }

    pub fn choices(&self) -> Vec<ProfileKind> {
        match self {
            Self::PlatformProfile { choices, .. }
            | Self::Ppd { choices, .. }
            | Self::Fixture { choices, .. } => choices.clone(),
            Self::None => Vec::new(),
        }
    }

    pub fn get(&self) -> Result<ProfileKind> {
        match self {
            Self::PlatformProfile { sysfs, .. } => fsutil::read_trim(sysfs)
                .as_deref()
                .and_then(parse_platform_choice)
                .ok_or_else(|| OmenError::Io("platform_profile unreadable or unknown".into())),
            Self::Ppd { service, .. } => {
                let value = ppd_property(service, "ActiveProfile")?;
                value
                    .as_str()
                    .and_then(map_ppd)
                    .ok_or_else(|| OmenError::Io("unknown PPD profile".into()))
            }
            Self::Fixture { current, .. } => fsutil::read_trim(current)
                .as_deref()
                .and_then(map_ppd)
                .ok_or_else(|| OmenError::Io("fixture profile unreadable".into())),
            Self::None => Err(OmenError::Unsupported(
                "no profile interface detected".into(),
            )),
        }
    }

    pub fn set(&self, profile: ProfileKind) -> Result<()> {
        self.reject_conflicting_managers()?;
        if !self.choices().contains(&profile) {
            return Err(OmenError::Unsupported(format!(
                "profile {profile} is not advertised by the selected backend"
            )));
        }
        match self {
            Self::PlatformProfile { sysfs, .. } => {
                if sysfs.starts_with("/sys/") && ppd_backend().is_some() {
                    return Err(OmenError::Unsupported(
                        "power-profiles-daemon became active; rediscover profile backend".into(),
                    ));
                }
                let raw_choices =
                    fsutil::read_trim(&sysfs.with_file_name("platform_profile_choices"))
                        .unwrap_or_default();
                let raw = raw_choices
                    .split_whitespace()
                    .find(|raw| parse_platform_choice(raw) == Some(profile))
                    .ok_or_else(|| OmenError::Unsupported("profile choices changed".into()))?;
                fsutil::write(sysfs, raw)?;
            }
            Self::Ppd { service, .. } => {
                let holds = ppd_property(service, "ActiveProfileHolds")?;
                if !holds.as_array().is_some_and(|v| v.is_empty()) {
                    return Err(OmenError::Unsupported(
                        "another application holds a profile, or hold state is unreadable".into(),
                    ));
                }
                let name = profile.ppd_name().ok_or_else(|| {
                    OmenError::Unsupported("profile has no PPD equivalent".into())
                })?;
                let out = crate::command::output(
                    std::process::Command::new("/usr/bin/busctl").args([
                        "--system",
                        "--timeout=2",
                        "set-property",
                        service,
                        ppd_path(service),
                        service,
                        "ActiveProfile",
                        "s",
                        name,
                    ]),
                    Duration::from_secs(3),
                )?;
                if !out.status.success() {
                    return Err(OmenError::Io("PPD rejected profile write".into()));
                }
            }
            Self::Fixture { current, .. } => {
                fsutil::write(current, profile.ppd_name().unwrap_or("balanced"))?
            }
            Self::None => return Err(OmenError::Unsupported("no profile backend".into())),
        }
        if self.get()? != profile {
            return Err(OmenError::StateMismatch(
                "profile readback differs from request".into(),
            ));
        }
        Ok(())
    }
}

fn map_ppd(name: &str) -> Option<ProfileKind> {
    match name.trim() {
        "power-saver" => Some(ProfileKind::Quiet),
        "balanced" => Some(ProfileKind::Balanced),
        "performance" => Some(ProfileKind::Performance),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppd_choices_are_advertised_not_invented() {
        let value = serde_json::json!([
            {"Profile": {"type": "s", "data": "balanced"}},
            {"Profile": {"type": "s", "data": "power-saver"}},
            {"Profile": {"type": "s", "data": "vendor-unknown"}}
        ]);
        assert_eq!(
            advertised_profiles(&value),
            vec![ProfileKind::Balanced, ProfileKind::Quiet]
        );
    }

    fn fixture_pp(root: &std::path::Path, choices: &str, current: &str) {
        let acpi = root.join("sys/firmware/acpi");
        std::fs::create_dir_all(&acpi).unwrap();
        std::fs::write(acpi.join("platform_profile"), current).unwrap();
        std::fs::write(acpi.join("platform_profile_choices"), choices).unwrap();
    }

    #[test]
    fn detects_and_reads_platform_profile() {
        let dir = tempfile::tempdir().unwrap();
        fixture_pp(dir.path(), "quiet balanced performance", "balanced\n");
        let root = SysRoot::new(dir.path());
        let be = ProfileBackend::detect(&root);
        assert!(matches!(be, ProfileBackend::PlatformProfile { .. }));
        assert_eq!(be.get().unwrap(), ProfileKind::Balanced);
        assert_eq!(be.choices().len(), 3);
    }

    #[test]
    fn set_validates_against_choices() {
        let dir = tempfile::tempdir().unwrap();
        fixture_pp(dir.path(), "balanced performance", "balanced\n");
        let root = SysRoot::new(dir.path());
        let be = ProfileBackend::detect(&root);
        assert!(matches!(
            be.set(ProfileKind::Quiet),
            Err(OmenError::Unsupported(_))
        ));
    }

    #[test]
    fn set_writes_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        fixture_pp(dir.path(), "quiet balanced performance", "balanced\n");
        let root = SysRoot::new(dir.path());
        let be = ProfileBackend::detect(&root);
        be.set(ProfileKind::Performance).unwrap();
        assert_eq!(
            fsutil::read_trim(&dir.path().join("sys/firmware/acpi/platform_profile")).as_deref(),
            Some("performance")
        );
    }

    #[test]
    fn no_backend_is_unsupported() {
        let _g = crate::mock::PPD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // Ensure PPD fallback is not picked up in tests: point at a
        // nonexistent ctl binary.
        std::env::set_var("OMEN_PPDCTL", "/nonexistent/powerprofilesctl");
        let root = SysRoot::new(dir.path());
        let be = ProfileBackend::detect(&root);
        assert!(matches!(be, ProfileBackend::None));
        assert!(matches!(be.get(), Err(OmenError::Unsupported(_))));
        std::env::remove_var("OMEN_PPDCTL");
    }
}
