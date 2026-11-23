//! Desktop-owned failure reports, never profile authority. Stored values contain
//! no command, arguments, captured output, provider cookies or reusable leases.
use omen_core::{games::valid_game_id, OmenError, Result};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const REQUEST_ENV: &str = "OMEN_LAUNCH_REQUEST_ID";
const MAX_RECORDS: usize = 64;
const MAX_BYTES: u64 = 262_144;

pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Configuration,
    Preparation,
    Scope,
    Registration,
    Execution,
    Submission,
}
impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Configuration => "Game settings",
            Self::Preparation => "Launch preparation",
            Self::Scope => "User game scope",
            Self::Registration => "Profile request",
            Self::Execution => "Wrapped command",
            Self::Submission => "Launch process",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Reason {
    Configuration,
    Authorization,
    ServiceUnavailable,
    PolicyOrLimit,
    Unsupported,
    Verification,
    Dependency,
    ScopeSetup,
    Execution,
    Submission,
    Observation,
    ExitCode(i32),
    Signal(i32),
}
impl Reason {
    pub fn description(self) -> String {
        match self {
            Self::Configuration => "Game settings could not be read or validated. Review user settings and retry.".into(),
            Self::Authorization => "Authorization was refused. Review daemon access in Settings and approve the request when prompted.".into(),
            Self::ServiceUnavailable => "The OMEN service could not be reached. Check Settings and Diagnostics, then retry.".into(),
            Self::PolicyOrLimit => "A profile policy, session limit or recovery check prevented startup. Review game sessions and Settings and Diagnostics.".into(),
            Self::Unsupported => "The required profile hold or process tracking is unavailable. Review the requirements in Profile setup.".into(),
            Self::Verification => "The requested profile hold was not confirmed. Review game sessions and command output before retrying.".into(),
            Self::Dependency => "A required game or wrapper dependency could not be found. Review Profile setup and the command output.".into(),
            Self::ScopeSetup => "The dedicated user game scope could not be created or verified. Check the user systemd session and Profile setup.".into(),
            Self::Execution => "The wrapped command could not be executed. Check its executable, permissions and command output.".into(),
            Self::Submission => "The launch request failed before a successful receipt. Review Profile setup and command output before retrying.".into(),
            Self::Observation => "The launch process could not be observed to completion. A game may still be running; review sessions and the current profile.".into(),
            Self::ExitCode(code) => format!("The launch process exited with code {code}. This does not prove that a game failed to start or has stopped. Review sessions, the current profile and command output."),
            Self::Signal(signal) => format!("The launch process ended after signal {signal}. A game may still be running; review sessions and the current profile."),
        }
    }
    pub fn from_status(status: std::process::ExitStatus) -> Option<Self> {
        use std::os::unix::process::ExitStatusExt;
        if status.success() {
            None
        } else if let Some(code) = status.code() {
            Some(Self::ExitCode(code))
        } else if let Some(signal) = status.signal() {
            Some(Self::Signal(signal))
        } else {
            Some(Self::Observation)
        }
    }
    pub fn from_error(stage: Stage, error: &OmenError) -> Self {
        // Parser and backend error strings may contain source lines or paths.
        // Persist a closed reason code, never an arbitrary Display message.
        if stage == Stage::Configuration {
            return Self::Configuration;
        }
        if stage == Stage::Execution {
            return Self::Execution;
        }
        match error {
            OmenError::PermissionDenied(_) => Self::Authorization,
            OmenError::DaemonUnavailable(_) => Self::ServiceUnavailable,
            OmenError::SafetyLockout(_) => Self::PolicyOrLimit,
            OmenError::Unsupported(_) | OmenError::NotImplemented(_) => Self::Unsupported,
            OmenError::StateMismatch(_) => Self::Verification,
            OmenError::NotFound(_) => Self::Dependency,
            OmenError::LaunchFailed(_) => Self::Submission,
            _ if stage == Stage::Scope => Self::ScopeSetup,
            _ if stage == Stage::Registration => Self::Verification,
            _ if stage == Stage::Preparation => Self::Configuration,
            _ => Self::Observation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub id: String,
    pub game_id: String,
    pub scope_token: Option<String>,
    pub stage: Stage,
    pub reason: Reason,
    pub occurred_unix_ms: u64,
}
impl Issue {
    pub fn new(
        id: String,
        game_id: String,
        scope_token: Option<String>,
        stage: Stage,
        reason: Reason,
    ) -> Self {
        Self {
            id,
            game_id,
            scope_token,
            stage,
            reason,
            occurred_unix_ms: now_ms(),
        }
    }
    fn valid(&self) -> bool {
        valid_id(&self.id)
            && valid_game_id(&self.game_id)
            && self.scope_token.as_ref().is_none_or(|v| valid_id(v))
            && match self.reason {
                Reason::ExitCode(v) => (1..=255).contains(&v),
                Reason::Signal(v) => (1..=64).contains(&v),
                _ => true,
            }
    }
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u32,
    uid: u32,
    issues: Vec<Issue>,
}
#[derive(Clone)]
pub struct History {
    directory: PathBuf,
}
fn invalid(message: &str) -> OmenError {
    OmenError::Io(format!("desktop launch history: {message}"))
}

fn child(directory: &File, name: &str, flags: i32) -> std::io::Result<File> {
    let name = CString::new(name).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn private_file(file: &File) -> Result<()> {
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o600 {
        return Err(invalid(
            "files must be private 0600 regular files owned by the desktop user",
        ));
    }
    Ok(())
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl History {
    pub fn for_config(config: &Path) -> Self {
        Self {
            directory: config
                .parent()
                .unwrap_or(Path::new("."))
                .join("launch-issues"),
        }
    }
    fn directory(&self, create: bool) -> Result<Option<File>> {
        if unsafe { libc::geteuid() } == 0 {
            return Err(OmenError::PermissionDenied(
                "desktop launch history is not a root/service operation".into(),
            ));
        }
        if create {
            if let Some(parent) = self.directory.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::DirBuilder::new().mode(0o700).create(&self.directory) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.directory)
        {
            Ok(file) => file,
            Err(e) if !create && e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let m = file.metadata()?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err(invalid(
                "directory must be owned by the desktop user with mode 0700",
            ));
        }
        Ok(Some(file))
    }
    fn lock(directory: &File) -> Result<Lock> {
        let file = child(directory, ".lock", libc::O_RDWR | libc::O_CREAT)?;
        private_file(&file)?;
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            let op = libc::LOCK_EX;
            if unsafe { libc::flock(file.as_raw_fd(), op | libc::LOCK_NB) } == 0 {
                return Ok(Lock(file));
            }
            let e = std::io::Error::last_os_error();
            if !matches!(
                e.raw_os_error(),
                Some(libc::EWOULDBLOCK) | Some(libc::EINTR)
            ) {
                return Err(e.into());
            }
            if Instant::now() >= deadline {
                return Err(invalid("busy; retry shortly"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn load(directory: &File) -> Result<Vec<Issue>> {
        let mut file = match child(directory, "issues.json", libc::O_RDONLY) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        private_file(&file)?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("file exceeds 256 KiB"));
        }
        let data: Stored = serde_json::from_slice(&bytes).map_err(|_| {
            invalid("invalid data; clear desktop launch issues to reset this report history")
        })?;
        let mut ids = std::collections::BTreeSet::new();
        if data.version != 1
            || data.uid != unsafe { libc::geteuid() }
            || data.issues.len() > MAX_RECORDS
            || data.issues.iter().any(|i| !i.valid() || !ids.insert(&i.id))
        {
            return Err(invalid("invalid version, owner or records; clear desktop launch issues to reset this report history"));
        }
        Ok(data.issues)
    }
    fn save(directory: &File, issues: Vec<Issue>) -> Result<()> {
        let name = CString::new(".pending").unwrap();
        match child(directory, ".pending", libc::O_RDONLY) {
            Ok(file) => {
                private_file(&file)?;
                if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut file = child(
            directory,
            ".pending",
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        private_file(&file)?;
        let bytes = serde_json::to_vec(&Stored {
            version: 1,
            uid: unsafe { libc::geteuid() },
            issues,
        })
        .map_err(|e| invalid(&e.to_string()))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("too many report bytes"));
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        let dest = CString::new("issues.json").unwrap();
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                name.as_ptr(),
                directory.as_raw_fd(),
                dest.as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        directory.sync_all()?;
        Ok(())
    }
    pub fn read(&self) -> Result<Vec<Issue>> {
        let Some(directory) = self.directory(false)? else {
            return Ok(Vec::new());
        };
        // Writers replace complete snapshots atomically. Reading the pinned
        // old or new regular file needs no writable lock file.
        Self::load(&directory)
    }
    /// First detailed failure wins over a later generic observer for this ID.
    pub fn record(&self, issue: Issue) -> Result<()> {
        if !issue.valid() {
            return Err(invalid("invalid issue"));
        }
        let directory = self.directory(true)?.unwrap();
        let _lock = Self::lock(&directory)?;
        let mut issues = Self::load(&directory)?;
        if issues.iter().any(|i| i.id == issue.id) {
            return Ok(());
        }
        issues.push(issue);
        if issues.len() > MAX_RECORDS {
            issues.drain(..issues.len() - MAX_RECORDS);
        }
        Self::save(&directory, issues)
    }
    /// Explicitly resets only this user's report file, even if its JSON is bad.
    pub fn clear(&self) -> Result<()> {
        let Some(directory) = self.directory(false)? else {
            return Ok(());
        };
        let _lock = Self::lock(&directory)?;
        match child(&directory, "issues.json", libc::O_RDONLY) {
            Ok(file) => private_file(&file)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Self::save(&directory, Vec::new())
    }
}

// Live reports are a bounded fallback for this desktop process when its
// private report file cannot be written. They contain the same closed data.
fn live_queue() -> &'static std::sync::Mutex<std::collections::VecDeque<(PathBuf, Issue)>> {
    static QUEUE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::VecDeque<(PathBuf, Issue)>>,
    > = std::sync::OnceLock::new();
    QUEUE.get_or_init(Default::default)
}
impl History {
    pub fn live(&self) -> Vec<Issue> {
        live_queue()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(directory, _)| directory == &self.directory)
            .map(|(_, issue)| issue.clone())
            .collect()
    }
    pub fn clear_live(&self) {
        live_queue()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(directory, _)| directory != &self.directory);
    }
}
pub fn best_effort(history: &History, issue: Issue) {
    if !issue.valid() || unsafe { libc::geteuid() } == 0 {
        return;
    }
    {
        let mut queue = live_queue().lock().unwrap_or_else(|e| e.into_inner());
        if !queue
            .iter()
            .any(|(directory, old)| directory == &history.directory && old.id == issue.id)
        {
            queue.push_back((history.directory.clone(), issue.clone()));
            while queue.len() > MAX_RECORDS {
                queue.pop_front();
            }
        }
    }
    match history.record(issue.clone()) {
        Ok(()) => {
            live_queue()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(directory, old)| directory != &history.directory || old.id != issue.id);
        }
        Err(error) => eprintln!("Could not save desktop launch issue: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn issue(index: usize, reason: Reason) -> Issue {
        Issue::new(
            format!("{index:032x}"),
            "custom/privacy".into(),
            None,
            Stage::Registration,
            reason,
        )
    }
    fn fixture() -> (tempfile::TempDir, History) {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        fs::write(&config, "user configuration stays unchanged").unwrap();
        let history = History::for_config(&config);
        (directory, history)
    }
    #[test]
    fn reports_are_private_bounded_and_do_not_persist_arbitrary_error_text() {
        let (directory, history) = fixture();
        let error = OmenError::Io("SECRET password=credentials launch_cmd=private-command".into());
        history
            .record(issue(0, Reason::from_error(Stage::Configuration, &error)))
            .unwrap();
        history.record(issue(0, Reason::ExitCode(1))).unwrap();
        assert_eq!(history.read().unwrap()[0].reason, Reason::Configuration);
        assert!(!fs::read_to_string(history.directory.join("issues.json"))
            .unwrap()
            .contains("SECRET"));
        for index in 1..=70 {
            history.record(issue(index, Reason::Authorization)).unwrap();
        }
        let records = history.read().unwrap();
        assert_eq!(records.len(), 64);
        assert_eq!(records[0].id, format!("{:032x}", 7));
        assert_eq!(
            fs::metadata(&history.directory).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(history.directory.join("issues.json"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml")).unwrap(),
            "user configuration stays unchanged"
        );
    }
    #[test]
    fn concurrent_reports_keep_distinct_ids() {
        let (_directory, history) = fixture();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|id| {
                let history = history.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    history.record(issue(id, Reason::Authorization)).unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(history.read().unwrap().len(), 8);
    }
    #[test]
    fn invalid_data_is_preserved_until_explicit_reset() {
        let (_directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let path = history.directory.join("issues.json");
        fs::write(&path, "broken report data").unwrap();
        assert!(history.read().is_err());
        assert!(history.record(issue(1, Reason::Execution)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken report data");
        history.clear().unwrap();
        assert!(history.read().unwrap().is_empty());
    }
    #[test]
    fn symlinks_and_nonprivate_files_never_replace_other_user_data() {
        let (directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let path = history.directory.join("issues.json");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(directory.path().join("config.toml"), &path).unwrap();
        assert!(history.read().is_err());
        assert!(history.clear().is_err());
        assert!(history.record(issue(1, Reason::Execution)).is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml")).unwrap(),
            "user configuration stays unchanged"
        );
        fs::remove_file(&path).unwrap();
        history.record(issue(2, Reason::Authorization)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(history.read().is_err());
        assert!(history.clear().is_err());
    }
    #[test]
    fn an_occupied_lock_has_a_bounded_wait() {
        let (_directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let directory = history.directory(false).unwrap().unwrap();
        let _held = History::lock(&directory).unwrap();
        let started = Instant::now();
        let failure = history.record(issue(1, Reason::Execution)).unwrap_err();
        assert!(failure.to_string().contains("busy"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn a_reader_does_not_wait_for_a_writer_lock() {
        let (_directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let directory = history.directory(false).unwrap().unwrap();
        let _held = History::lock(&directory).unwrap();
        assert_eq!(history.read().unwrap()[0].reason, Reason::Authorization);
    }
    #[test]
    fn failed_persistence_keeps_a_private_process_local_report_until_reset() {
        let (_directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let path = history.directory.join("issues.json");
        fs::write(&path, "broken report data").unwrap();
        best_effort(&history, issue(1, Reason::Execution));
        best_effort(&history, issue(1, Reason::ExitCode(1)));
        assert_eq!(history.live().len(), 1);
        assert_eq!(history.live()[0].reason, Reason::Execution);
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken report data");
        history.clear().unwrap();
        history.clear_live();
        assert!(history.read().unwrap().is_empty());
        assert!(history.live().is_empty());
    }
    #[test]
    fn schema_and_owner_confusion_are_rejected() {
        let (_directory, history) = fixture();
        history.record(issue(0, Reason::Authorization)).unwrap();
        let path = history.directory.join("issues.json");
        let original = fs::read(&path).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["uid"] = serde_json::json!(0);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(history.read().is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["issues"][0]["command"] = serde_json::json!("unexpected field");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(history.read().is_err());
    }
}

#[cfg(test)]
mod special_file_tests {
    use super::*;
    #[test]
    fn named_pipes_are_rejected_without_waiting_for_a_writer() {
        let root = tempfile::tempdir().unwrap();
        let history = History::for_config(&root.path().join("config.toml"));
        let directory = history.directory(true).unwrap().unwrap();
        let path = CString::new("issues.json").unwrap();
        assert_eq!(
            unsafe { libc::mkfifoat(directory.as_raw_fd(), path.as_ptr(), 0o600) },
            0
        );
        let started = Instant::now();
        assert!(history.read().is_err());
        assert!(history.clear().is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
