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
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::time::{Duration, Instant};

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

// The socketpair is created by the original sandbox wrapper before fork.
// Linux credentials and SO_PEERPIDFD identify that creator across PID namespaces;
// no caller-supplied PID or /proc scan is used to select a process.
struct Handoff {
    socket: File,
    caller: Process,
}

impl Handoff {
    fn open(uid: u32, descriptor: i32) -> Result<Self> {
        if descriptor < 3 {
            return Err(invalid("a private handoff socket is required"));
        }
        let fd = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let socket = unsafe { File::from_raw_fd(fd) };
        let mut kind: i32 = 0;
        let mut len = std::mem::size_of_val(&kind) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                (&mut kind as *mut i32).cast(),
                &mut len,
            )
        } != 0
            || kind != libc::SOCK_SEQPACKET
        {
            return Err(invalid("the handoff needs a Unix packet socket"));
        }
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        len = std::mem::size_of_val(&credentials) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut len,
            )
        } != 0
            || credentials.uid != uid
            || credentials.pid <= 0
        {
            return Err(invalid("the handoff peer is not the desktop user"));
        }
        let mut peer_fd: i32 = -1;
        len = std::mem::size_of_val(&peer_fd) as libc::socklen_t;
        // SO_PEERPIDFD (Linux 6.5). Refuse kernels without this identity proof.
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                77,
                (&mut peer_fd as *mut i32).cast(),
                &mut len,
            )
        } != 0
            || peer_fd < 0
        {
            return Err(OmenError::Unsupported(
                "Flatpak handoff requires socket peer process descriptors".into(),
            ));
        }
        let peer = unsafe { File::from_raw_fd(peer_fd) };
        let mut poll = libc::pollfd {
            fd: peer.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 0) } != 0 {
            return Err(invalid("the original sandbox caller has exited"));
        }
        let mut caller = Process::open(uid, credentials.pid as u32)?;
        // The socket peer descriptor was live before /proc was opened, so the
        // PID could not have been reused. Keep that original descriptor pinned.
        caller.pidfd = peer;
        caller.alive()?;
        Ok(Self { socket, caller })
    }

    fn send(&self, packet: &[u8], descriptor: Option<&File>) -> Result<()> {
        self.caller.alive()?;
        let mut iov = libc::iovec {
            iov_base: packet.as_ptr().cast_mut().cast(),
            iov_len: packet.len(),
        };
        // usize alignment is sufficient for cmsghdr on supported Linux ABIs.
        let mut control = [0usize; 8];
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        if let Some(descriptor) = descriptor {
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen =
                unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
            let header = unsafe { libc::CMSG_FIRSTHDR(&message) };
            unsafe {
                (*header).cmsg_level = libc::SOL_SOCKET;
                (*header).cmsg_type = libc::SCM_RIGHTS;
                (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as usize;
                std::ptr::write_unaligned(
                    libc::CMSG_DATA(header).cast::<i32>(),
                    descriptor.as_raw_fd(),
                );
            }
        }
        if unsafe { libc::sendmsg(self.socket.as_raw_fd(), &message, libc::MSG_NOSIGNAL) }
            != packet.len() as isize
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn acknowledge(&self) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            self.caller.alive()?;
            let mut fds = [
                libc::pollfd {
                    fd: self.socket.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.caller.pidfd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let result = unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err(std::io::Error::last_os_error().into());
            }
            if fds[0].revents & libc::POLLIN != 0 {
                let mut packet = [0u8; 2];
                if unsafe {
                    libc::recv(
                        self.socket.as_raw_fd(),
                        packet.as_mut_ptr().cast(),
                        packet.len(),
                        libc::MSG_TRUNC,
                    )
                } == 1
                    && packet[0] == b'm'
                {
                    self.caller.alive()?;
                    return Ok(());
                }
                return Err(invalid("the sandbox rejected its nested game group"));
            }
            if fds.iter().any(|fd| fd.revents != 0) || Instant::now() >= until {
                return Err(invalid("the sandbox handoff closed or timed out"));
            }
        }
    }
}

fn caller(uid: u32, instance: &str, id: &str, process: &Process) -> Result<()> {
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
    if namespace == Process::open(uid, std::process::id())?.namespace()?
        || namespace != process.namespace()?
    {
        return Err(invalid("the caller is not in this Flatpak PID namespace"));
    }
    for info in [root.info()?, process.info()?] {
        if ini(&info, "Instance", "instance-id")? != instance {
            return Err(invalid("Flatpak instance identity differs"));
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
    }
    root.alive()?;
    process.alive()
}

fn cgroup_parent(uid: u32, path: &str) -> Result<File> {
    let prefix = format!("/user.slice/user-{uid}.slice/user@{uid}.service/");
    let tail = path
        .strip_prefix(&prefix)
        .ok_or_else(|| invalid("Flatpak is outside the desktop user manager"))?;
    let parts: Vec<_> = tail.split('/').collect();
    let app = parts
        .iter()
        .position(|part| part.starts_with("app-flatpak-") && part.ends_with(".scope"));
    if parts.len() > 14
        || app.is_none()
        || parts
            .iter()
            .any(|part| part.is_empty() || part.len() > 255 || matches!(*part, "." | ".."))
        || parts[..app.unwrap()]
            .iter()
            .any(|part| !part.ends_with(".slice"))
    {
        return Err(invalid(
            "the original Flatpak cgroup cannot contain a game group",
        ));
    }
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/sys/fs/cgroup")?;
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut fs) } != 0
        || fs.f_type as u64 != 0x6367_7270
    {
        return Err(invalid(
            "the game group requires the real cgroup v2 filesystem",
        ));
    }
    for part in path.trim_start_matches('/').split('/') {
        let part = std::ffi::CString::new(part).map_err(|_| invalid("invalid cgroup component"))?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    if directory.metadata()?.uid() != uid {
        return Err(invalid(
            "the Flatpak cgroup is not delegated to the desktop user",
        ));
    }
    Ok(directory)
}

fn scope(uid: u32, token: &str, handoff: &Handoff) -> Result<()> {
    handoff.caller.alive()?;
    let original = handoff.caller.group()?;
    let parent = cgroup_parent(uid, &original)?;
    // Trim only empty OMEN groups from this same pinned, user-owned parent.
    // Do not change delegation, controllers or any parent resource policy.
    let mut count = 0;
    for (index, entry) in
        std::fs::read_dir(format!("/proc/self/fd/{}", parent.as_raw_fd()))?.enumerate()
    {
        if index >= 4096 {
            return Err(invalid("too many entries in the Flatpak cgroup"));
        }
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(token) = name
            .strip_prefix("omen-game-")
            .and_then(|s| s.strip_suffix(".scope"))
        else {
            continue;
        };
        if token.len() != 32
            || !token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            continue;
        }
        let name = std::ffi::CString::new(name).map_err(|_| invalid("invalid cgroup name"))?;
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
            count += 1;
        }
    }
    if count >= 128 {
        return Err(invalid("too many active game groups in this Flatpak"));
    }
    let unit = format!("omen-game-{token}.scope");
    let name = std::ffi::CString::new(unit.as_str()).map_err(|_| invalid("invalid game group"))?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| -> Result<()> {
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let child = unsafe { File::from_raw_fd(fd) };
        let fd = unsafe {
            libc::openat(
                child.as_raw_fd(),
                c"cgroup.procs".as_ptr(),
                libc::O_WRONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let membership = unsafe { File::from_raw_fd(fd) };
        handoff.caller.alive()?;
        if handoff.caller.group()? != original {
            return Err(invalid("Flatpak caller moved before handoff"));
        }
        let mut packet = vec![b'M'];
        packet.extend_from_slice(token.as_bytes());
        handoff.send(&packet, Some(&membership))?;
        handoff.acknowledge()?;
        if handoff.caller.group()? != format!("{original}/{unit}") {
            return Err(invalid("the sandbox did not enter its nested game group"));
        }
        // A write of zero migrates only this host helper, without a numeric PID
        // race. The sandbox wrapper already moved itself through the same FD.
        if unsafe { libc::write(membership.as_raw_fd(), b"0\n".as_ptr().cast(), 2) } != 2 {
            return Err(std::io::Error::last_os_error().into());
        }
        if Process::open(uid, std::process::id())?.group()? != handoff.caller.group()? {
            return Err(invalid("host and sandbox game groups differ"));
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
        }
    }
    result
}

/// Approve only after the waiting caller shares the scope and a verified hold.
/// The caller remains inside its original Flatpak namespaces and seccomp rules.
pub fn prepare(id: &str, instance: &str, handoff_fd: i32, socket: Option<&str>) -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 || unsafe { libc::getuid() } != uid {
        return Err(OmenError::PermissionDenied(
            "Flatpak game preparation must run as the desktop user".into(),
        ));
    }
    if !omen_core::games::valid_game_id(id)
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
    let handoff = Handoff::open(uid, handoff_fd).inspect_err(|e| report(Stage::Scope, e))?;
    caller(uid, instance, id, &handoff.caller).inspect_err(|e| report(Stage::Scope, e))?;
    let config = OmenConfig::load().inspect_err(|e| report(Stage::Configuration, e))?;
    if !config.games.managed_game_ids.contains(id) {
        return handoff.send(b"O", None);
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
    scope(uid, &token, &handoff).inspect_err(|e| report(Stage::Scope, e))?;
    let daemon = socket
        .map(IpcClient::new)
        .unwrap_or_else(IpcClient::default_path);
    let registered = (|| -> Result<()> {
        handoff.caller.alive()?;
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
        handoff.caller.alive()?;
        if handoff.caller.group()? != Process::open(uid, std::process::id())?.group()? {
            return Err(invalid("sandbox caller left its approved game scope"));
        }
        handoff.send(b"R", None)
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
