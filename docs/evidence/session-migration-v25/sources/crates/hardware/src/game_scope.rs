//! Read-only identity and lifetime of an explicitly wrapped game process tree.
//! A launcher PID or a matching executable name is never treated as game life.
use omen_core::{OmenError, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeState {
    Populated,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeIdentity {
    pub relative_path: String,
    pub device: u64,
    pub inode: u64,
}

pub struct GameScope {
    directory: File,
    identity: ScopeIdentity,
    peer: File,
    peer_start: u64,
}

fn fail(message: impl Into<String>) -> OmenError {
    OmenError::InvalidArgument(message.into())
}

fn read_child(directory: &File, name: &str) -> std::io::Result<File> {
    let name = std::ffi::CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid filename"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn text(mut file: File) -> std::io::Result<String> {
    let mut value = String::new();
    file.by_ref().take(16_385).read_to_string(&mut value)?;
    if value.len() > 16_384 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "process identity exceeds limit",
        ));
    }
    Ok(value)
}

fn scope_path(value: &str, uid: u32, token: &str) -> Result<String> {
    if uid == 0
        || token.len() != 32
        || !token
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(fail(
            "managed game needs a desktop UID and a random 32-digit lowercase hex token",
        ));
    }
    let prefix = format!("/user.slice/user-{uid}.slice/user@{uid}.service/");
    let tail = value
        .strip_prefix(&prefix)
        .ok_or_else(|| fail("game is outside the desktop user's systemd manager"))?;
    let parts: Vec<_> = tail.split('/').collect();
    let expected = format!("omen-game-{token}.scope");
    if parts.last().copied() != Some(expected.as_str())
        || parts.len() > 16
        || parts[..parts.len().saturating_sub(1)]
            .iter()
            .any(|p| !p.ends_with(".slice") || p.len() > 255)
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || value.contains("//")
    {
        return Err(fail("game must run in its dedicated omen-game scope"));
    }
    Ok(value.trim_start_matches('/').to_owned())
}

fn start_time(stat: &str) -> Result<u64> {
    stat.rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| fail("process start identity unavailable"))
}

fn populated(value: &str) -> Result<ScopeState> {
    let states: Vec<_> = value
        .lines()
        .filter_map(|line| line.strip_prefix("populated "))
        .collect();
    match states.as_slice() {
        ["0"] => Ok(ScopeState::Empty),
        ["1"] => Ok(ScopeState::Populated),
        _ => Err(fail("cgroup lifetime is unreadable or invalid")),
    }
}

impl GameScope {
    /// Derive membership from the connecting process, never from a caller's
    /// supplied filesystem path. The process directory and cgroup are pinned.
    pub fn for_process(uid: u32, pid: u32, token: &str) -> Result<Self> {
        let process = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(format!("/proc/{pid}"))?;
        let before = start_time(&text(read_child(&process, "stat")?)?)?;
        let status = text(read_child(&process, "status")?)?;
        let owners: Vec<u32> = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .ok_or_else(|| fail("process UID unavailable"))?
            .split_whitespace()
            .map(|v| v.parse().map_err(|_| fail("invalid process UID")))
            .collect::<Result<_>>()?;
        if owners.len() != 4 || owners.iter().any(|owner| *owner != uid) {
            return Err(fail("game process UID does not match the desktop peer"));
        }
        let membership = text(read_child(&process, "cgroup")?)?;
        let path = membership
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| fail("unified cgroups are required for managed sessions"))?;
        let relative_path = scope_path(path, uid, token)?;
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open("/sys/fs/cgroup")?;
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut fs) } != 0
            || fs.f_type as u64 != 0x6367_7270
        {
            return Err(fail(
                "the process group must be on the real cgroup v2 filesystem",
            ));
        }
        for part in relative_path.split('/') {
            let next = read_child(&directory, part)?;
            if !next.metadata()?.is_dir() {
                return Err(fail("invalid cgroup directory"));
            }
            directory = next;
        }
        read_child(&directory, "cgroup.events")?;
        let metadata = directory.metadata()?;
        let scope = Self {
            directory,
            peer: process,
            peer_start: before,
            identity: ScopeIdentity {
                relative_path,
                device: metadata.dev(),
                inode: metadata.ino(),
            },
        };
        // Reject a moved/exited/reused peer while its membership was inspected.
        if before != start_time(&text(read_child(&scope.peer, "stat")?)?)?
            || membership != text(read_child(&scope.peer, "cgroup")?)?
            || scope.state()? != ScopeState::Populated
        {
            return Err(fail("game process identity changed while registering"));
        }
        Ok(scope)
    }

    pub fn identity(&self) -> &ScopeIdentity {
        &self.identity
    }

    fn peer_text(&self, name: &str) -> Result<Option<String>> {
        // The pinned proc directory never refers to a replacement PID. Only
        // confirmed disappearance is normal exit; access/read failures must
        // interrupt observation rather than manufacture an ended session.
        match read_child(&self.peer, name).and_then(text) {
            Ok(value) => Ok(Some(value)),
            Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn peer_exited(&self, stat: &str) -> Result<bool> {
        if start_time(stat)? != self.peer_start {
            return Err(fail("registered game process identity changed"));
        }
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, tail)| tail.split_whitespace().next())
            .ok_or_else(|| fail("registered game process state is unavailable"))?;
        Ok(matches!(state, "Z" | "X" | "x"))
    }

    fn verify_peer_membership(&self) -> Result<()> {
        let Some(before) = self.peer_text("stat")? else {
            return Ok(());
        };
        if self.peer_exited(&before)? {
            return Ok(());
        }
        let Some(membership) = self.peer_text("cgroup")? else {
            return Ok(());
        };
        let Some(after) = self.peer_text("stat")? else {
            return Ok(());
        };
        if self.peer_exited(&after)? {
            return Ok(());
        }
        let path = membership
            .lines()
            .find_map(|line| line.strip_prefix("0::/"))
            .ok_or_else(|| fail("registered game process cgroup is unavailable"))?;
        // A nested cgroup remains part of the recursively tracked scope.
        // Matching text prefixes without a component boundary is insufficient.
        if path != self.identity.relative_path
            && !path
                .strip_prefix(&self.identity.relative_path)
                .is_some_and(|tail| tail.starts_with('/'))
        {
            return Err(OmenError::StateMismatch(
                "the wrapped command moved outside its tracked scope; a game may still be running. Review the game and current profile".into(),
            ));
        }
        Ok(())
    }

    pub fn state(&self) -> Result<ScopeState> {
        self.verify_peer_membership()?;
        // A removed cgroup cannot contain live members; the descriptor still
        // refers to the original inode rather than any replacement at its path.
        if self.directory.metadata()?.nlink() == 0 {
            return Ok(ScopeState::Empty);
        }
        // Reopen through the pinned directory for each observation. Repeated
        // pread(0) on one kernfs seqfile can replay its buffered old contents.
        let events = match read_child(&self.directory, "cgroup.events") {
            Ok(events) => events,
            Err(error) => {
                // kernfs may retain link counts after rmdir. Require both a
                // kernel-marked deleted inode and vanished virtual event file;
                // a rename or a generic read error is never proof of exit.
                let gone = |e: &std::io::Error| {
                    matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ENODEV))
                };
                let descriptor = format!("/proc/self/fd/{}", self.directory.as_raw_fd());
                if gone(&error)
                    && std::fs::read_link(descriptor)?
                        .as_os_str()
                        .as_bytes()
                        .ends_with(b" (deleted)")
                {
                    return Ok(ScopeState::Empty);
                }
                return Err(error.into());
            }
        };
        let mut bytes = [0u8; 4096];
        let n = events.read_at(&mut bytes, 0)?;
        let value =
            std::str::from_utf8(&bytes[..n]).map_err(|_| fail("invalid cgroup event data"))?;
        populated(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    #[test]
    fn scope_identity_rejects_other_users_and_ordinary_launcher_scopes() {
        let path = format!(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/omen-game-{TOKEN}.scope"
        );
        assert!(scope_path(&path, 1000, TOKEN).is_ok());
        assert!(scope_path(&path, 1001, TOKEN).is_err());
        assert!(scope_path(&path, 0, TOKEN).is_err());
        assert!(scope_path(&path.replace("omen-game-", "steam-"), 1000, TOKEN).is_err());
        assert!(scope_path(&path.replace("app.slice", "launcher.scope"), 1000, TOKEN).is_err());
        assert!(scope_path(&path.replace("app.slice", "../app.slice"), 1000, TOKEN).is_err());
        assert!(scope_path(&path, 1000, "bad").is_err());
    }
    #[test]
    fn malformed_lifetime_data_is_never_interpreted_as_game_exit() {
        assert_eq!(
            populated("populated 1\nfrozen 0\n").unwrap(),
            ScopeState::Populated
        );
        assert_eq!(
            populated("populated 0\nfrozen 0\n").unwrap(),
            ScopeState::Empty
        );
        for invalid in [
            "",
            "frozen 0\n",
            "populated 2\n",
            "populated 0\npopulated 1\n",
            "populated 0 garbage\n",
        ] {
            assert!(populated(invalid).is_err(), "{invalid:?}");
        }
    }
}
