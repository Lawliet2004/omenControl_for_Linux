//! Desktop-user handoff for an existing Flatpak process. The waiting sandbox
//! wrapper executes its own unchanged argv after approval. No namespace entry,
//! environment reconstruction or privileged daemon PID/cgroup API is involved.
use crate::launch_issues::{best_effort, History, Issue, Reason, Stage};
use omen_core::config::OmenConfig;
use omen_core::games::{GameSession, SessionPhase};
use omen_core::{OmenError, Result};
use omen_ipc::{IpcClient, Op};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::time::{Duration, Instant};
use zbus::blocking::connection::Builder;
use zbus::zvariant::{Fd, OwnedObjectPath, Value};

fn invalid(message: &str) -> OmenError {
    OmenError::InvalidArgument(message.into())
}

fn read_at(directory: &File, path: &str, limit: usize) -> Result<String> {
    let path = std::ffi::CString::new(path).map_err(|_| invalid("invalid process path"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            path.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(invalid("process metadata is not a regular file"));
    }
    let mut text = String::new();
    file.take(limit as u64 + 1).read_to_string(&mut text)?;
    if text.len() > limit {
        return Err(invalid("process metadata exceeds the limit"));
    }
    Ok(text)
}

fn start(stat: &str) -> Result<u64> {
    stat.rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| invalid("process start identity unavailable"))
}

fn ini<'a>(text: &'a str, group: &str, key: &str) -> Result<&'a str> {
    let mut current = "";
    let mut found = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(value) = line.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            current = value;
        } else if current == group {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key && found.replace(value.trim()).is_some() {
                    return Err(invalid("ambiguous Flatpak instance metadata"));
                }
            }
        }
    }
    found
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Flatpak instance metadata is incomplete"))
}

struct Process {
    directory: File,
    pidfd: File,
    start: u64,
}

impl Process {
    fn open(uid: u32, pid: u32) -> Result<Self> {
        if pid == 0 || pid > i32::MAX as u32 {
            return Err(invalid("invalid sandbox process"));
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(format!("/proc/{pid}"))?;
        let before = start(&read_at(&directory, "stat", 16_384)?)?;
        let status = read_at(&directory, "status", 65_536)?;
        let owners: Vec<_> = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .ok_or_else(|| invalid("sandbox process UID unavailable"))?
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| invalid("invalid sandbox process UID"))?;
        if owners.len() != 4 || owners.iter().any(|owner| *owner != uid) {
            return Err(OmenError::PermissionDenied(
                "sandbox process belongs to another UID".into(),
            ));
        }
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(OmenError::Unsupported(
                "Flatpak handoff requires live process descriptors".into(),
            ));
        }
        let value = Self {
            directory,
            pidfd: unsafe { File::from_raw_fd(fd as i32) },
            start: before,
        };
        value.alive()?;
        Ok(value)
    }

    fn alive(&self) -> Result<()> {
        let stat = read_at(&self.directory, "stat", 16_384)?;
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, tail)| tail.split_whitespace().next());
        let mut poll = libc::pollfd {
            fd: self.pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if start(&stat)? != self.start
            || matches!(state, Some("Z" | "X" | "x"))
            || unsafe { libc::poll(&mut poll, 1, 0) } != 0
            || poll.revents != 0
        {
            return Err(invalid("the original sandbox caller exited or changed"));
        }
        Ok(())
    }

    fn namespace(&self) -> Result<(u64, u64)> {
        // ns/pid is a procfs magic link. Follow only this fixed kernel path,
        // through the pinned process directory, and verify its nsfs type.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                c"ns/pid".as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let fd = unsafe { File::from_raw_fd(fd) };
        if unsafe { libc::ioctl(fd.as_raw_fd(), 0xb703) } != libc::CLONE_NEWPID {
            return Err(invalid("sandbox PID namespace could not be verified"));
        }
        let metadata = fd.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    fn inner_pid(&self) -> Result<u32> {
        read_at(&self.directory, "status", 65_536)?
            .lines()
            .find_map(|line| line.strip_prefix("NSpid:"))
            .and_then(|line| line.split_whitespace().last())
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| invalid("sandbox PID mapping unavailable"))
    }

    fn info(&self) -> Result<String> {
        read_at(&self.directory, "root/.flatpak-info", 131_072)
    }

    fn group(&self) -> Result<String> {
        read_at(&self.directory, "cgroup", 16_384)?
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .map(str::to_owned)
            .ok_or_else(|| invalid("unified cgroup membership unavailable"))
    }
}

fn caller(uid: u32, instance: &str, inner_pid: u32, id: &str) -> Result<Process> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(format!("/run/user/{uid}/.flatpak/{instance}"))?;
    if directory.metadata()?.uid() != uid {
        return Err(invalid("Flatpak instance ownership differs"));
    }
    let state: serde_json::Value =
        serde_json::from_str(&read_at(&directory, "bwrapinfo.json", 65_536)?)
            .map_err(|_| invalid("Flatpak process metadata is invalid"))?;
    let root_pid = state["child-pid"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| invalid("Flatpak instance process unavailable"))?;
    let root = Process::open(uid, root_pid)?;
    let namespace = root.namespace()?;
    if namespace == Process::open(uid, std::process::id())?.namespace()? {
        return Err(invalid(
            "the caller is not in a separate sandbox PID namespace",
        ));
    }
    if ini(&root.info()?, "Instance", "instance-id")? != instance {
        return Err(invalid("Flatpak instance identity differs"));
    }
    let mut found = None;
    let began = Instant::now();
    for (index, entry) in std::fs::read_dir("/proc")?.enumerate() {
        if index >= 65_536 || began.elapsed() > Duration::from_secs(3) {
            return Err(invalid("sandbox process lookup exceeded its limit"));
        }
        let entry = entry?;
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        let Ok(pid) = entry.file_name().to_string_lossy().parse() else {
            continue;
        };
        let Ok(process) = Process::open(uid, pid) else {
            continue;
        };
        if process.namespace().ok() != Some(namespace)
            || process.inner_pid().ok() != Some(inner_pid)
        {
            continue;
        }
        let info = process.info()?;
        if ini(&info, "Instance", "instance-id")? != instance {
            return Err(invalid("sandbox caller identity differs"));
        }
        let expected = if id.starts_with("steam/flatpak/") {
            Some("com.valvesoftware.Steam")
        } else if id.starts_with("heroic/flatpak/") {
            Some("com.heroicgameslauncher.hgl")
        } else if id.starts_with("lutris/flatpak/") {
            Some("net.lutris.Lutris")
        } else {
            None
        };
        if expected.is_some_and(|app| ini(&info, "Application", "name").ok() != Some(app)) {
            return Err(invalid("game launcher and Flatpak application differ"));
        }
        process.alive()?;
        if found.replace(process).is_some() {
            return Err(invalid("sandbox caller is ambiguous"));
        }
    }
    root.alive()?;
    found.ok_or_else(|| invalid("the waiting sandbox caller could not be found"))
}

fn scope(uid: u32, token: &str, caller: &Process) -> Result<()> {
    caller.alive()?;
    let own = Process::open(uid, std::process::id())?;
    let address = format!("unix:path=/run/user/{uid}/bus");
    let bus = Builder::address(address.as_str())
        .map_err(|e| OmenError::Io(e.to_string()))?
        .method_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| OmenError::Io(e.to_string()))?;
    let unit = format!("omen-game-{token}.scope");
    let properties = vec![
        (
            "Description",
            Value::from("OMEN managed Flatpak game scope"),
        ),
        ("CollectMode", Value::from("inactive-or-failed")),
        (
            "PIDFDs",
            Value::new(vec![
                Fd::from(own.pidfd.as_fd()),
                Fd::from(caller.pidfd.as_fd()),
            ]),
        ),
    ];
    let auxiliary: Vec<(&str, Vec<(&str, Value<'_>)>)> = Vec::new();
    let reply = bus.call_method(Some("org.freedesktop.systemd1"), "/org/freedesktop/systemd1", Some("org.freedesktop.systemd1.Manager"),
        "StartTransientUnit", &(unit.as_str(), "fail", properties, auxiliary))
        .map_err(|e| OmenError::Unsupported(format!("the user manager could not create a scope using process descriptors; use manual restoration: {e}")))?;
    let _: OwnedObjectPath = reply
        .body()
        .deserialize()
        .map_err(|e| OmenError::Io(e.to_string()))?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        caller.alive()?;
        let group = own.group()?;
        let prefix = format!("/user.slice/user-{uid}.slice/user@{uid}.service/");
        if group.starts_with(&prefix)
            && group.ends_with(&format!("/{unit}"))
            && caller.group()? == group
        {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(invalid("sandbox caller did not enter the managed scope"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Approve only after the waiting caller shares the scope and a verified hold.
/// The caller remains inside its original Flatpak namespaces and seccomp rules.
pub fn prepare(id: &str, instance: &str, sandbox_pid: u32, socket: Option<&str>) -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 || unsafe { libc::getuid() } != uid {
        return Err(OmenError::PermissionDenied(
            "Flatpak game preparation must run as the desktop user".into(),
        ));
    }
    if !omen_core::games::valid_game_id(id)
        || sandbox_pid == 0
        || instance.is_empty()
        || instance.len() > 10
        || !instance.bytes().all(|value| value.is_ascii_digit())
        || instance.parse::<u32>().ok().is_none_or(|v| v == 0)
    {
        return Err(invalid(
            "a valid game, Flatpak instance and sandbox caller are required",
        ));
    }
    let token = crate::games::new_transaction_token()?;
    let history = History::for_config(&OmenConfig::path());
    let report = |stage, error: &OmenError| {
        best_effort(
            &history,
            Issue::new(
                token.clone(),
                id.into(),
                Some(token.clone()),
                stage,
                Reason::from_error(stage, error),
            ),
        )
    };
    let config = OmenConfig::load().inspect_err(|e| report(Stage::Configuration, e))?;
    if !config.games.managed_game_ids.contains(id) {
        return Ok(());
    }
    let profile = config
        .games
        .per_game_profiles
        .get(id)
        .copied()
        .ok_or_else(|| invalid("automatic game profile binding is missing"))
        .inspect_err(|e| report(Stage::Configuration, e))?;
    if !matches!(
        profile,
        omen_core::profile::ProfileKind::Performance | omen_core::profile::ProfileKind::Quiet
    ) {
        let error =
            OmenError::Unsupported("automatic game profiles require Performance or Quiet".into());
        report(Stage::Configuration, &error);
        return Err(error);
    }
    let caller =
        caller(uid, instance, sandbox_pid, id).inspect_err(|e| report(Stage::Preparation, e))?;
    scope(uid, &token, &caller).inspect_err(|e| report(Stage::Scope, e))?;
    let daemon = socket
        .map(IpcClient::new)
        .unwrap_or_else(IpcClient::default_path);
    let registered = (|| -> Result<()> {
        caller.alive()?;
        let session: GameSession = daemon.call_decode(Op::StartManagedGame {
            token: token.clone(),
            game_id: id.into(),
            profile,
        })?;
        if !session.profile_held || session.phase != SessionPhase::Tracking {
            return Err(OmenError::SafetyLockout(
                "the managed profile was released before sandbox approval".into(),
            ));
        }
        caller.alive()?;
        if caller.group()? != Process::open(uid, std::process::id())?.group()? {
            return Err(invalid("sandbox caller left its approved game scope"));
        }
        Ok(())
    })();
    if let Err(error) = &registered {
        let _ = daemon.call(Op::ReleaseGameSession {
            token: token.clone(),
        });
        report(Stage::Registration, error);
    }
    registered
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_identity_is_required_in_its_own_group_and_must_be_unambiguous() {
        assert_eq!(
            ini(
                "[Application]\nname=org.example.Game\n[Instance]\ninstance-id=42\n",
                "Instance",
                "instance-id"
            )
            .unwrap(),
            "42"
        );
        for text in [
            "[Application]\ninstance-id=42\n",
            "[Instance]\ninstance-id=\n",
            "[Instance]\ninstance-id=42\ninstance-id=43\n",
        ] {
            assert!(ini(text, "Instance", "instance-id").is_err());
        }
    }
}
