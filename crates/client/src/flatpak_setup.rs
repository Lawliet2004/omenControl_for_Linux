//! Desktop-owned Flatpak helper installation. No launcher configuration or
//! sandbox permission is changed. Whole directories are published atomically.
use omen_core::{OmenError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LIMIT: u64 = 2 * 1024 * 1024;
const DIRECTORY: &str = "managed-v1";
const BINARY: &str = "omen-flatpak-game";
const MANIFEST: &str = "installation.json";

fn invalid(message: &str) -> OmenError {
    OmenError::InvalidArgument(message.into())
}

pub fn application(id: &str) -> Option<&'static str> {
    if !omen_core::games::valid_game_id(id) {
        return None;
    }
    if id.starts_with("steam/flatpak/") {
        Some("com.valvesoftware.Steam")
    } else if id.starts_with("heroic/flatpak/") {
        Some("com.heroicgameslauncher.hgl")
    } else if id.starts_with("lutris/flatpak/") {
        Some("net.lutris.Lutris")
    } else {
        None
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn prefix(id: &str) -> Result<String> {
    application(id).ok_or_else(|| invalid("this game is not a supported Flatpak installation"))?;
    // A literal absolute sandbox path works in launcher fields that do not
    // expand environment variables. Flatpak's app data is mounted here.
    let _canonical = home()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("the desktop home is unavailable"))?;
    let app = application(id).unwrap();
    let path = home.join(format!(
        ".var/app/{app}/data/omen-linux/{DIRECTORY}/{BINARY}"
    ));
    Ok(format!(
        "{} {} --",
        quote(
            path.to_str().ok_or_else(|| invalid(
                "the desktop home must be valid UTF-8 for launcher setup"
            ))?
        ),
        quote(id)
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub application: String,
    pub helper_installed: bool,
    pub host_permission: bool,
    pub ready: bool,
    pub detail: String,
    pub wrapper_command: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    application: String,
    sha256: String,
    size: u64,
}

fn home() -> Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 || uid != unsafe { libc::getuid() } {
        return Err(OmenError::PermissionDenied(
            "Flatpak setup must run as the desktop user".into(),
        ));
    }
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| invalid("an absolute desktop home is required"))?;
    let path = path.canonicalize()?;
    let metadata = path.metadata()?;
    if metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(invalid("the desktop home must be privately owned"));
    }
    Ok(path)
}

fn open_dir(parent: &File, name: &str, create: bool) -> Result<File> {
    let name = std::ffi::CString::new(name).map_err(|_| invalid("invalid helper directory"))?;
    if create && unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e.into());
        }
    }
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let directory = unsafe { File::from_raw_fd(fd) };
    let metadata = directory.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(invalid(
            "helper directories must be owned by you and not writable by other users",
        ));
    }
    Ok(directory)
}

fn base(home: &Path, app: &str, create: bool) -> Result<File> {
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(home)?;
    for name in [".var", "app", app, "data", "omen-linux"] {
        directory = open_dir(&directory, name, create)?;
    }
    Ok(directory)
}

fn read(directory: &File, name: &str, limit: u64) -> Result<Vec<u8>> {
    let name = std::ffi::CString::new(name).map_err(|_| invalid("invalid helper filename"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.len() > limit
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
        || (name.as_bytes() == BINARY.as_bytes() && metadata.mode() & 0o100 == 0)
    {
        return Err(invalid(
            "the installed helper is not a private regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("helper file exceeds its size limit"));
    }
    Ok(bytes)
}

fn verified(directory: &File, app: &str) -> Result<()> {
    let manifest: Manifest =
        serde_json::from_slice(&read(directory, MANIFEST, 4096)?).map_err(|_| {
            invalid("the helper installation record is invalid; preserve it for manual recovery")
        })?;
    let bytes = read(directory, BINARY, LIMIT)?;
    if manifest.version != 1
        || manifest.application != app
        || manifest.size != bytes.len() as u64
        || manifest.sha256 != format!("{:x}", Sha256::digest(&bytes))
    {
        return Err(invalid(
            "the helper was modified; preserve it and remove or recover it manually",
        ));
    }
    let entries: Vec<_> = std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
        .take(3)
        .collect::<std::io::Result<_>>()?;
    if entries.len() != 2
        || entries
            .iter()
            .any(|e| e.file_name() != BINARY && e.file_name() != MANIFEST)
    {
        return Err(invalid(
            "unrecognized files in the helper directory; preserve them for manual recovery",
        ));
    }
    Ok(())
}

fn write(directory: &File, name: &str, bytes: &[u8], mode: u32) -> Result<()> {
    let name = std::ffi::CString::new(name).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn erase(parent: &File, name: &str, directory: &File) -> Result<()> {
    for file in [BINARY, MANIFEST] {
        let file = std::ffi::CString::new(file).unwrap();
        if unsafe { libc::unlinkat(directory.as_raw_fd(), file.as_ptr(), 0) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.into());
            }
        }
    }
    let name = std::ffi::CString::new(name).unwrap();
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    parent.sync_all()?;
    Ok(())
}

fn payload() -> Result<PathBuf> {
    let sibling = std::env::current_exe()?.with_file_name(BINARY);
    let path = if sibling.is_file() {
        sibling
    } else {
        PathBuf::from("/usr/libexec/omen-linux/omen-flatpak-game")
    };
    if !path.is_file() {
        return Err(OmenError::NotFound(
            "the packaged Flatpak helper is missing; install the current OMEN package".into(),
        ));
    }
    Ok(path)
}

fn install_at(home: &Path, app: &str, source: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("the packaged helper is not a regular file"));
    }
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT || !bytes.starts_with(b"\x7fELF") {
        return Err(invalid("invalid packaged Flatpak helper"));
    }
    let parent = base(home, app, true)?;
    if unsafe { libc::flock(parent.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(invalid("another helper setup is running; retry"));
    }
    let existing = match open_dir(&parent, DIRECTORY, false) {
        Ok(directory) => {
            verified(&directory, app)?;
            Some(directory)
        }
        Err(OmenError::NotFound(_)) => None,
        Err(e) => return Err(e),
    };
    let temporary = format!(".setup-{}", crate::games::new_transaction_token()?);
    let staged = open_dir(&parent, &temporary, true)?;
    let mut published = false;
    let result = (|| {
        write(&staged, BINARY, &bytes, 0o700)?;
        let manifest = Manifest {
            version: 1,
            application: app.into(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        };
        write(
            &staged,
            MANIFEST,
            &serde_json::to_vec_pretty(&manifest).map_err(|e| invalid(&e.to_string()))?,
            0o600,
        )?;
        staged.sync_all()?;
        let from = std::ffi::CString::new(temporary.as_str()).unwrap();
        let to = std::ffi::CString::new(DIRECTORY).unwrap();
        let flags = if existing.is_some() {
            libc::RENAME_EXCHANGE
        } else {
            libc::RENAME_NOREPLACE
        };
        if unsafe {
            libc::renameat2(
                parent.as_raw_fd(),
                from.as_ptr(),
                parent.as_raw_fd(),
                to.as_ptr(),
                flags,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        published = true;
        parent.sync_all()?;
        if let Some(old) = existing {
            erase(&parent, &temporary, &old)?;
        }
        Ok(())
    })();
    if result.is_err() && !published {
        let _ = erase(&parent, &temporary, &staged);
    }
    result
}

fn permission(text: &str) -> bool {
    let mut session = false;
    let mut allowed = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            session = line == "[Session Bus Policy]";
        } else if session {
            if let Some((name, value)) = line.split_once('=') {
                if name == "org.freedesktop.Flatpak" {
                    allowed = matches!(value, "talk" | "own");
                }
            }
        }
    }
    allowed
}

pub fn status(id: &str) -> Result<Status> {
    let app = application(id).ok_or_else(|| invalid("unsupported Flatpak game installation"))?;
    let home = home()?;
    let helper = base(&home, app, false)
        .and_then(|d| open_dir(&d, DIRECTORY, false))
        .and_then(|d| verified(&d, app));
    let info = omen_hardware::command::output(
        std::process::Command::new("flatpak").args(["info", "--show-permissions", app]),
        Duration::from_secs(4),
    );
    let host_permission = info
        .as_ref()
        .is_ok_and(|o| o.status.success() && permission(&String::from_utf8_lossy(&o.stdout)));
    let installed = info.as_ref().is_ok_and(|o| o.status.success());
    let helper_installed = helper.is_ok();
    let ready = installed && host_permission && helper_installed;
    let detail = if !installed {
        "Flatpak or this launcher is unavailable. Install the launcher, then check again.".into()
    } else if !host_permission {
        format!("{app} cannot use flatpak-spawn --host. Review its org.freedesktop.Flatpak session-bus permission in your desktop's Flatpak settings, then restart the launcher. OMEN does not change sandbox permissions.")
    } else if helper
        .as_ref()
        .is_err_and(|e| !matches!(e, OmenError::NotFound(_)))
    {
        format!(
            "Helper recovery needed: {}. Preserve changed files; setup will not overwrite them.",
            helper.as_ref().unwrap_err()
        )
    } else if !helper_installed {
        "Install the launcher helper, then put the wrapper command below in the game's launcher settings. Kernel/cgroup support and profile authorization are checked on each launch.".into()
    } else {
        "Helper and host-command permission are ready. Configure the wrapper in the launcher. Each launch still verifies its sandbox, cgroup and authorized profile hold.".into()
    };
    Ok(Status {
        application: app.into(),
        helper_installed,
        host_permission,
        ready,
        detail,
        wrapper_command: prefix(id)?,
    })
}

pub fn install(id: &str) -> Result<Status> {
    let app = application(id).ok_or_else(|| invalid("unsupported Flatpak game installation"))?;
    install_at(&home()?, app, &payload()?)?;
    status(id)
}

pub fn remove(id: &str) -> Result<Status> {
    let app = application(id).ok_or_else(|| invalid("unsupported Flatpak game installation"))?;
    let parent = base(&home()?, app, false)?;
    if unsafe { libc::flock(parent.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(invalid("another helper setup is running; retry"));
    }
    let directory = open_dir(&parent, DIRECTORY, false)?;
    verified(&directory, app)?;
    let path = omen_core::config::OmenConfig::path();
    let prefix = match app {
        "com.valvesoftware.Steam" => "steam/flatpak/",
        "com.heroicgameslauncher.hgl" => "heroic/flatpak/",
        "net.lutris.Lutris" => "lutris/flatpak/",
        _ => return Err(invalid("unsupported launcher")),
    };
    omen_ipc::settings::SettingsController::new(None, path).clear_managed_game_prefix(prefix)?;
    let retired = format!(".remove-{}", crate::games::new_transaction_token()?);
    let from = std::ffi::CString::new(DIRECTORY).unwrap();
    let to = std::ffi::CString::new(retired.as_str()).unwrap();
    if unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    parent.sync_all()?;
    erase(&parent, &retired, &directory)?;
    status(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permissions_are_session_scoped_and_explicit_denial_wins() {
        assert!(permission(
            "[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n"
        ));
        assert!(!permission(
            "[System Bus Policy]\norg.freedesktop.Flatpak=talk\n"
        ));
        assert!(!permission(
            "[Session Bus Policy]\norg.freedesktop.Flatpak=talk\norg.freedesktop.Flatpak=none\n"
        ));
    }
    #[test]
    fn install_update_and_remove_preserve_other_files_and_refuse_changed_helpers() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("payload");
        std::fs::write(&source, b"\x7fELFfirst fixture").unwrap();
        install_at(home.path(), "net.lutris.Lutris", &source).unwrap();
        let parent = base(home.path(), "net.lutris.Lutris", false).unwrap();
        verified(
            &open_dir(&parent, DIRECTORY, false).unwrap(),
            "net.lutris.Lutris",
        )
        .unwrap();
        std::fs::write(&source, b"\x7fELFupdated fixture").unwrap();
        install_at(home.path(), "net.lutris.Lutris", &source).unwrap();
        let installed = home
            .path()
            .join(".var/app/net.lutris.Lutris/data/omen-linux/managed-v1");
        std::fs::write(installed.join(BINARY), b"user change").unwrap();
        assert!(install_at(home.path(), "net.lutris.Lutris", &source).is_err());
        assert_eq!(
            std::fs::read(installed.join(BINARY)).unwrap(),
            b"user change"
        );
        std::fs::remove_file(installed.join(BINARY)).unwrap();
        std::os::unix::fs::symlink(&source, installed.join(BINARY)).unwrap();
        assert!(install_at(home.path(), "net.lutris.Lutris", &source).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"\x7fELFupdated fixture");
    }
    #[test]
    fn ancestor_links_are_never_followed() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), home.path().join(".var")).unwrap();
        assert!(base(home.path(), "net.lutris.Lutris", true).is_err());
        assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
    }
}
