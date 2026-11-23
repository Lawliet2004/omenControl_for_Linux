//! Pinned ordinary-file actuator for supervision fixtures. Kernel/pseudo
//! filesystems and non-regular descriptors are rejected before any write.

use omen_core::fans::FanMode;
use omen_core::{OmenError, Result, MAX_FAN_DUTY_PCT, MIN_FAN_DUTY_PCT};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub struct Files {
    // Fixed roles: mode, CPU target, GPU target, controller name.
    pub handles: [File; 4],
    identities: [(u64, u64); 4],
    manual: bool,
}

fn invalid(message: &str) -> OmenError {
    OmenError::StateMismatch(message.into())
}

fn ordinary(file: &File) -> bool {
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: info is writable and fd is owned by file.
    if unsafe { libc::fstatfs(file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return false;
    }
    // Local ordinary filesystems from Linux uapi/linux/magic.h. No sysfs,
    // procfs, debugfs, device file or arbitrary pseudo-filesystem is accepted.
    let magic = unsafe { info.assume_init() }.f_type as u64;
    matches!(
        magic,
        0xef53 | 0x01021994 | 0x794c7630 | 0x58465342 | 0x9123683e
    )
}

impl Files {
    pub fn open(directory: &Path) -> Result<Self> {
        let mut opened = Vec::new();
        for (index, name) in ["pwm1_enable", "pwm1", "pwm2", "name"].iter().enumerate() {
            let missing_target = matches!(index, 1 | 2) && !directory.join(name).exists();
            opened.push(
                OpenOptions::new()
                    .read(true)
                    .write(index < 3 && !missing_target)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(directory.join(if missing_target { "name" } else { name }))?,
            );
        }
        Self::new(
            opened
                .try_into()
                .map_err(|_| invalid("incorrect actuator descriptor count"))?,
        )
    }

    pub fn inherited() -> Result<Self> {
        let mut handles = Vec::new();
        for fd in 4..8 {
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
                return Err(invalid("missing inherited fixture descriptor"));
            }
        }
        for fd in 4..8 {
            // SAFETY: this is the dedicated helper entry point; these fixed
            // descriptors are transferred once and owned only here.
            handles.push(unsafe { File::from_raw_fd(fd) });
        }
        Self::new(
            handles
                .try_into()
                .map_err(|_| invalid("missing actuator descriptors"))?,
        )
    }

    fn new(handles: [File; 4]) -> Result<Self> {
        let mut identities = [(0, 0); 4];
        for (i, file) in handles.iter().enumerate() {
            let m = file.metadata()?;
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o022 != 0
                || !ordinary(file)
            {
                return Err(invalid("fan guardian accepts only owned ordinary fixture files; live devices remain disabled"));
            }
            identities[i] = (m.dev(), m.ino());
        }
        // Missing manual targets use read-only name descriptors as sentinels.
        // Infer capability from pinned identities; never accept a wire claim.
        let manual = identities[1] != identities[3] && identities[2] != identities[3];
        let mut this = Self {
            handles,
            identities,
            manual,
        };
        this.validate()?;
        Ok(this)
    }

    pub fn duplicated(&self) -> Result<Vec<OwnedFd>> {
        self.handles
            .iter()
            .map(|file| {
                // SAFETY: fcntl duplicates a valid fd; high numbers avoid dup2
                // collisions with the helper's fixed descriptor roles.
                let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                Ok(unsafe { OwnedFd::from_raw_fd(fd) })
            })
            .collect()
    }

    fn text(&mut self, index: usize) -> Result<String> {
        let file = &mut self.handles[index];
        file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        file.take(64).read_to_string(&mut text)?;
        Ok(text.trim().into())
    }
    fn write(&mut self, index: usize, value: u64) -> Result<()> {
        let file = &mut self.handles[index];
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(format!("{value}\n").as_bytes())?;
        Ok(())
    }
    pub fn validate(&mut self) -> Result<()> {
        for (file, expected) in self.handles.iter().zip(&self.identities) {
            let m = file.metadata()?;
            if (m.dev(), m.ino()) != *expected || m.nlink() == 0 {
                return Err(invalid("pinned fan actuator identity was lost"));
            }
        }
        if self.text(3)? != "hp" {
            return Err(invalid("pinned controller no longer identifies as hp"));
        }
        Ok(())
    }
    pub fn apply(&mut self, mode: FanMode) -> Result<()> {
        self.validate()?;
        if matches!(mode, FanMode::Manual { .. }) && !self.manual {
            return Err(invalid("pinned controller exposes Automatic/Maximum only"));
        }
        let enable = match mode {
            FanMode::Auto => 2,
            FanMode::Max => 0,
            FanMode::Manual { .. } => 1,
            FanMode::Curve => return Err(invalid("curve intent cannot be sent to the actuator")),
        };
        self.write(0, enable)?;
        if let FanMode::Manual { duty_pct } = mode {
            let duty = duty_pct.clamp(MIN_FAN_DUTY_PCT, MAX_FAN_DUTY_PCT);
            let raw = omen_hardware::hp_wmi::duty_pct_to_raw(duty);
            for index in [1, 2] {
                self.write(index, raw)?;
                if self.text(index)?.parse::<u64>().ok() != Some(raw) {
                    return Err(invalid("fixture target readback mismatch"));
                }
            }
        }
        if self.text(0)?.parse::<u64>().ok() != Some(enable) {
            return Err(invalid("fixture policy readback mismatch"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn pseudo_filesystems_are_not_fixture_actuators() {
        assert!(!ordinary(&File::open("/proc/version").unwrap()));
        assert!(!ordinary(
            &File::open("/sys/class/dmi/id/board_name").unwrap()
        ));
    }
    #[test]
    fn mode_only_actuator_never_writes_name_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        for (name, value) in [("name", "hp"), ("pwm1_enable", "2")] {
            let path = dir.path().join(name);
            std::fs::write(&path, value).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut files = Files::open(dir.path()).unwrap();
        assert!(files.apply(FanMode::Manual { duty_pct: 50 }).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pwm1_enable")).unwrap(),
            "2"
        );
        files.apply(FanMode::Max).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pwm1_enable"))
                .unwrap()
                .trim(),
            "0"
        );
        files.apply(FanMode::Auto).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("name")).unwrap(),
            "hp"
        );
        assert!(!dir.path().join("pwm1").exists());
        assert!(!dir.path().join("pwm2").exists());
    }

    #[test]
    fn replacement_does_not_redirect_a_pinned_actuator() {
        let dir = tempfile::tempdir().unwrap();
        for (name, value) in [
            ("name", "hp"),
            ("pwm1_enable", "1"),
            ("pwm1", "128"),
            ("pwm2", "128"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, value).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut files = Files::open(dir.path()).unwrap();
        std::fs::remove_file(dir.path().join("pwm1_enable")).unwrap();
        std::fs::write(dir.path().join("pwm1_enable"), "0").unwrap();
        assert!(files.apply(FanMode::Auto).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pwm1_enable")).unwrap(),
            "0"
        );
    }
}
