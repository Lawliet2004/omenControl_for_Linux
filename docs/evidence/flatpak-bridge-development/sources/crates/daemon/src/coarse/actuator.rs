//! Exact-target mode actuator. Live access never accepts a caller-selected path.
use omen_core::{OmenError, Result};
use omen_hardware::SysRoot;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const TARGET: &str = "8C30:F.10:7.0.0-38-generic";
pub fn invalid(message: impl Into<String>) -> OmenError {
    OmenError::StateMismatch(message.into())
}
pub fn instance_valid(value: &str) -> bool {
    value
        .strip_prefix("hwmon")
        .is_some_and(|v| !v.is_empty() && v.len() <= 6 && v.bytes().all(|b| b.is_ascii_digit()))
}
pub fn machine_target(root: &SysRoot) -> String {
    let m = omen_hardware::detection::detect_machine(root);
    format!("{}:{}:{}", m.board_name, m.bios_version, m.kernel_version)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub path: PathBuf,
    mode: (u64, u64),
    name: (u64, u64),
    pub target: String,
}
pub struct Actuator {
    mode: File,
    name: File,
    pub identity: Identity,
    pub fixture: bool,
    root: SysRoot,
}
impl Actuator {
    pub fn open(instance: &str, fixture: Option<&Path>) -> Result<Self> {
        if !instance_valid(instance) {
            return Err(invalid("invalid hwmon instance"));
        }
        let root = fixture.map_or_else(SysRoot::default, |p| SysRoot::new(p.to_path_buf()));
        let directory = if fixture.is_some() {
            root.join("sys/class/hwmon").join(instance).canonicalize()?
        } else {
            let path = Path::new("/sys/devices/platform/hp-wmi/hwmon").join(instance);
            if path.canonicalize()? != path || machine_target(&root) != TARGET {
                return Err(invalid("unverified live hp-wmi target"));
            }
            path
        };
        let flags = libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let mode = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(flags)
            .open(directory.join("pwm1_enable"))?;
        let name = OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(directory.join("name"))?;
        for file in [&mode, &name] {
            let m = file.metadata()?;
            let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
            if !m.is_file()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o022 != 0
                || unsafe { libc::fstatfs(file.as_raw_fd(), fs.as_mut_ptr()) } != 0
            {
                return Err(invalid("actuator type/ownership check failed"));
            }
            let magic = unsafe { fs.assume_init() }.f_type as u64;
            if (fixture.is_none() && magic != 0x62656572)
                || (fixture.is_some()
                    && !matches!(
                        magic,
                        0xef53 | 0x01021994 | 0x794c7630 | 0x58465342 | 0x9123683e
                    ))
            {
                return Err(invalid(
                    "actuator filesystem does not match live/fixture selection",
                ));
            }
        }
        let m = mode.metadata()?;
        let n = name.metadata()?;
        let identity = Identity {
            path: directory,
            mode: (m.dev(), m.ino()),
            name: (n.dev(), n.ino()),
            target: machine_target(&root),
        };
        let mut value = Self {
            mode,
            name,
            identity,
            fixture: fixture.is_some(),
            root,
        };
        value.validate()?;
        if !matches!(Self::read(&mut value.mode)?.as_str(), "0" | "1" | "2") {
            return Err(invalid("unrecognized hp-wmi mode"));
        }
        Ok(value)
    }
    fn read(file: &mut File) -> Result<String> {
        file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        file.take(64).read_to_string(&mut text)?;
        Ok(text.trim().into())
    }
    pub fn validate(&mut self) -> Result<()> {
        if self.identity.path.canonicalize()? != self.identity.path
            || machine_target(&self.root) != self.identity.target
        {
            return Err(invalid("pinned controller or machine identity changed"));
        }
        for (file, name, id) in [
            (&self.mode, "pwm1_enable", self.identity.mode),
            (&self.name, "name", self.identity.name),
        ] {
            let m = file.metadata()?;
            let path = std::fs::symlink_metadata(self.identity.path.join(name))?;
            if !path.is_file()
                || (path.dev(), path.ino()) != id
                || (m.dev(), m.ino()) != id
                || m.nlink() == 0
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o022 != 0
            {
                return Err(invalid(
                    "pinned actuator was removed, replaced or made writable by others",
                ));
            }
        }
        if Self::read(&mut self.name)? != "hp" {
            return Err(invalid("pinned controller no longer identifies as hp"));
        }
        Ok(())
    }
    fn check_competing(&self) -> Result<()> {
        if !self.fixture && omen_hardware::detection::competing_fan_daemon(&self.root).is_some() {
            return Err(invalid("competing fan controller detected"));
        }
        Ok(())
    }
    pub fn verify_maximum(&mut self) -> Result<()> {
        self.check_competing()?;
        self.validate()?;
        if Self::read(&mut self.mode)? != "0" {
            return Err(invalid("Maximum mode changed outside its owner"));
        }
        Ok(())
    }
    pub fn apply(&mut self, maximum: bool) -> Result<()> {
        if maximum {
            self.check_competing()?;
        }
        self.validate()?;
        // sysfs attributes must never be truncated. Ordinary fixture files need it.
        if self.fixture {
            self.mode.set_len(0)?;
        }
        self.mode.seek(SeekFrom::Start(0))?;
        self.mode.write_all(if maximum { b"0\n" } else { b"2\n" })?;
        if Self::read(&mut self.mode)? != if maximum { "0" } else { "2" } {
            return Err(invalid("coarse mode readback mismatch"));
        }
        Ok(())
    }
}
