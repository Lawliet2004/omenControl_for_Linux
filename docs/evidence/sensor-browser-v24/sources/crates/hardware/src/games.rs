//! Installed-library discovery and desktop launch preparation.
//! Launch happens only in an unprivileged desktop process.

use omen_core::games::{Game, GameSource};
use omen_core::{OmenError, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Home directory override for tests ($OMEN_HOME), else $HOME.
fn home() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEN_HOME")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| ".".into())),
    )
}

mod binary_vdf;
mod discovery;
mod vdf;
pub use discovery::{detect_heroic, detect_legendary, detect_lutris, detect_steam};

#[derive(Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LauncherState {
    RequestCompleted,
    Running,
}

#[derive(Debug, serde::Serialize, PartialEq)]
pub struct LaunchReceipt {
    pub process_id: u32,
    pub launcher_state: LauncherState,
    pub startup_window_ms: u64,
    pub game_execution_verified: bool,
}

/// Validate inputs and resolve the launcher before a profile can be changed.
pub enum LateLaunchFailure {
    Exited(std::process::ExitStatus),
    Observation,
}

pub struct PreparedLaunch {
    command: Command,
    observer: Option<Box<dyn FnOnce(LateLaunchFailure) + Send>>,
}

fn desktop_user_only() -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(OmenError::PermissionDenied(
            "games must launch as the desktop user, never as root".into(),
        ));
    }
    Ok(())
}

fn executable(name: &str, search: &std::ffi::OsStr) -> Result<PathBuf> {
    std::env::split_paths(search)
        .map(|directory| directory.join(name))
        .find(|path| {
            path.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .map(|path| {
            if path.is_absolute() {
                Ok(path)
            } else {
                Ok(std::env::current_dir()?.join(path))
            }
        })
        .unwrap_or_else(|| {
            Err(OmenError::NotFound(format!(
                "launcher '{name}' is not executable in PATH"
            )))
        })
}

fn store_id<'a>(game: &'a Game, prefix: &str) -> Result<(bool, &'a str)> {
    let id = game
        .id
        .strip_prefix(prefix)
        .ok_or_else(|| OmenError::InvalidArgument("invalid store game ID".into()))?;
    Ok(match id.strip_prefix("flatpak/") {
        Some(id) => (true, id),
        None => (false, id),
    })
}

fn store_command(
    name: &str,
    application: &str,
    flatpak: bool,
    uri: String,
    search: &std::ffi::OsStr,
) -> Result<Command> {
    let mut command = Command::new(executable(if flatpak { "flatpak" } else { name }, search)?);
    if flatpak {
        command.args(["run", application]);
    }
    command.arg(uri);
    Ok(command)
}

fn legendary_configuration_supported(output: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(output) else {
        return false;
    };
    text.lines().any(|line| {
        let Some(version) = line
            .strip_prefix("legendary version \"")
            .and_then(|s| s.split_once('"').map(|(v, _)| v))
        else {
            return false;
        };
        let mut parts = version.split('.');
        let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) = (
            parts.next().map(str::parse::<u32>),
            parts.next().map(str::parse::<u32>),
            parts.next().map(str::parse::<u32>),
            parts.next(),
        ) else {
            return false;
        };
        // LEGENDARY_CONFIG_PATH first shipped in 0.20.34. Unknown major/minor
        // series need review before assuming they preserve this routing contract.
        major == 0 && (minor == 20 && patch >= 34 || minor == 21)
    })
}

impl PreparedLaunch {
    /// A desktop-owned observer. Its ID is diagnostic metadata, not daemon
    /// authority. Store URI forwarding may not carry it into an existing client.
    pub fn observe_failure(
        mut self,
        request_id: Option<&str>,
        observer: impl FnOnce(LateLaunchFailure) + Send + 'static,
    ) -> Self {
        if let Some(request_id) = request_id {
            self.command.env("OMEN_LAUNCH_REQUEST_ID", request_id);
        }
        self.observer = Some(Box::new(observer));
        self
    }

    /// Use only for an actual custom command, never a store URI submission.
    pub fn wrap_actual_command(self, cli: &std::path::Path, id: &str) -> Result<Self> {
        if !cli.is_absolute()
            || !cli
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            return Err(OmenError::NotFound("omen-cli must be installed beside the application for automatic custom-game profiles".into()));
        }
        let mut command = Command::new(cli);
        for (name, value) in self.command.get_envs() {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
        command
            .args(["game-run", id, "--"])
            .arg(self.command.get_program())
            .args(self.command.get_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        Ok(Self {
            command,
            observer: self.observer,
        })
    }
    pub fn prepare(game: &Game) -> Result<Self> {
        desktop_user_only()?;
        Self::prepare_with_path(game, &std::env::var_os("PATH").unwrap_or_default())
    }

    fn prepare_with_path(game: &Game, search: &std::ffi::OsStr) -> Result<Self> {
        let mut command = match game.source {
            GameSource::Steam => {
                use discovery::SteamInstallation;
                let invalid = || {
                    OmenError::InvalidArgument("invalid Steam application or shortcut ID".into())
                };
                let id = game.id.strip_prefix("steam/").ok_or_else(invalid)?;
                let (route, id) = if let Some(id) = id.strip_prefix("flatpak/") {
                    (SteamInstallation::Flatpak, id)
                } else if let Some(id) = id.strip_prefix("snap/") {
                    (SteamInstallation::Snap, id)
                } else {
                    (SteamInstallation::Native, id)
                };
                let launch_id = if let Some(shortcut) = id.strip_prefix("shortcut/") {
                    let (account, appid) = shortcut.split_once('/').ok_or_else(invalid)?;
                    let account = account
                        .parse::<u32>()
                        .ok()
                        .filter(|id| *id != 0)
                        .ok_or_else(invalid)?;
                    let appid = appid
                        .parse::<u32>()
                        .ok()
                        .filter(|id| *id >= 0x80001388)
                        .ok_or_else(invalid)?;
                    let root = game
                        .library_root
                        .as_ref()
                        .filter(|p| p.is_absolute())
                        .ok_or_else(invalid)?;
                    if discovery::active_steam_account(root) != Some(account) {
                        return Err(OmenError::LaunchFailed("Steam's most recent account changed or is unavailable. Rescan the library before launching this shortcut.".into()));
                    }
                    (u64::from(appid) << 32) | 0x02000000
                } else {
                    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(invalid());
                    }
                    u64::from(
                        id.parse::<u32>()
                            .ok()
                            .filter(|id| *id != 0)
                            .ok_or_else(invalid)?,
                    )
                };
                let uri = format!("steam://rungameid/{launch_id}");
                if route == SteamInstallation::Snap {
                    let mut command = Command::new(executable("snap", search)?);
                    command.args(["run", "steam", &uri]);
                    command
                } else {
                    store_command(
                        "steam",
                        "com.valvesoftware.Steam",
                        route == SteamInstallation::Flatpak,
                        uri,
                        search,
                    )?
                }
            }
            GameSource::Heroic => {
                let (flatpak, id) = store_id(game, "heroic/")?;
                let (runner, id) = id
                    .split_once('/')
                    .filter(|(runner, id)| {
                        matches!(*runner, "legendary" | "gog" | "nile")
                            && discovery::safe_component(id)
                    })
                    .ok_or_else(|| {
                        OmenError::InvalidArgument("invalid Heroic runner or game ID".into())
                    })?;
                store_command(
                    "heroic",
                    "com.heroicgameslauncher.hgl",
                    flatpak,
                    format!("heroic://launch/{runner}/{id}"),
                    search,
                )?
            }
            GameSource::Lutris => {
                let (flatpak, id) = store_id(game, "lutris/")?;
                if id.parse::<i64>().ok().filter(|id| *id > 0).is_none()
                    || !id.bytes().all(|c| c.is_ascii_digit())
                {
                    return Err(OmenError::InvalidArgument(
                        "invalid Lutris database game ID".into(),
                    ));
                }
                store_command(
                    "lutris",
                    "net.lutris.Lutris",
                    flatpak,
                    format!("lutris:rungameid/{id}"),
                    search,
                )?
            }
            GameSource::Custom => {
                if !game.id.starts_with("custom/")
                    || game.id.len() <= 7
                    || game.launch_cmd.trim().is_empty()
                {
                    return Err(OmenError::InvalidArgument(
                        "custom games need a custom/ ID and nonempty command".into(),
                    ));
                }
                if !std::fs::metadata("/bin/sh")
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                {
                    return Err(OmenError::NotFound("/bin/sh is not executable".into()));
                }
                let mut c = Command::new("/bin/sh");
                c.args(["-c", &game.launch_cmd]);
                c
            }
            GameSource::Legendary => {
                let id = game
                    .id
                    .strip_prefix("legendary/")
                    .filter(|id| discovery::safe_component(id))
                    .ok_or_else(|| {
                        OmenError::InvalidArgument("invalid Legendary application ID".into())
                    })?;
                let root=game.library_root.as_ref().filter(|p|p.is_absolute() && p.is_dir()).ok_or_else(||OmenError::InvalidArgument("Legendary requires its discovered configuration directory; rescan the library".into()))?;
                let program = executable("legendary", search)?;
                let supported = crate::command::output(
                    Command::new(&program)
                        .arg("--version")
                        .env("LEGENDARY_CONFIG_PATH", root)
                        .stdin(Stdio::null()),
                    Duration::from_secs(3),
                )
                .is_ok_and(|output| {
                    output.status.success() && legendary_configuration_supported(&output.stdout)
                });
                if !supported {
                    return Err(OmenError::Unsupported("Legendary configuration routing requires version 0.20.34 or later in the 0.20/0.21 series. Update Legendary or use an explicit Custom command.".into()));
                }
                let mut command = Command::new(program);
                command
                    .args(["launch", "--", id])
                    .env("LEGENDARY_CONFIG_PATH", root);
                command
            }
        };
        // Diagnostic wrapper metadata must not become a persistent launcher's
        // environment and accidentally correlate later unrelated games.
        command.env_remove("OMEN_LAUNCH_REQUEST_ID");
        // Keep machine-readable CLI stdout clean and do not attach GUI games
        // to the app's input. Errors remain available on the app's stderr.
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        Ok(Self {
            command,
            observer: None,
        })
    }

    pub fn start(mut self) -> Result<LaunchReceipt> {
        desktop_user_only()?;
        let mut child = self
            .command
            .spawn()
            .map_err(|e| OmenError::LaunchFailed(format!("could not start launcher: {e}")))?;
        let process_id = child.id();
        let observation = Duration::from_millis(300);
        let deadline = Instant::now() + observation;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    return Ok(LaunchReceipt {
                        process_id,
                        launcher_state: LauncherState::RequestCompleted,
                        startup_window_ms: 300,
                        game_execution_verified: false,
                    })
                }
                Ok(Some(status)) => {
                    return Err(OmenError::LaunchFailed(format!(
                        "launcher exited with {status}"
                    )))
                }
                Ok(None) if Instant::now() >= deadline => {
                    let observer = self.observer.take();
                    std::thread::spawn(move || match child.wait() {
                        Ok(status) if !status.success() => {
                            eprintln!("launch process {process_id} ended later with {status}; game execution remains unverified; review sessions and the current profile");
                            if let Some(observer) = observer {
                                observer(LateLaunchFailure::Exited(status));
                            }
                        }
                        Err(error) => {
                            eprintln!("cannot reap launch process {process_id}: {error}");
                            if let Some(observer) = observer {
                                observer(LateLaunchFailure::Observation);
                            }
                        }
                        _ => {}
                    });
                    return Ok(LaunchReceipt {
                        process_id,
                        launcher_state: LauncherState::Running,
                        startup_window_ms: 300,
                        game_execution_verified: false,
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(OmenError::LaunchFailed(format!(
                        "launcher status could not be observed: {error}"
                    )));
                }
            }
        }
    }
}

pub fn detect_all() -> Vec<Game> {
    detect_with_locations(&[])
}

pub fn detect_with_locations(locations: &[omen_core::games::LibraryLocation]) -> Vec<Game> {
    let h = home();
    let mut g = detect_steam(&h);
    g.extend(detect_heroic(&h));
    g.extend(detect_lutris(&h));
    g.extend(detect_legendary(&h));
    g.extend(discovery::additional_libraries(&h, locations));
    let mut ids = std::collections::BTreeSet::new();
    g.retain(|game| ids.insert(game.id.clone()));
    g.sort_by_key(|game| game.name.to_lowercase());
    g.truncate(10_000);
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legendary_versions_must_preserve_selected_configuration_routing() {
        assert!(legendary_configuration_supported(
            b"legendary version \"0.20.34\", codename \"fixture\"\n"
        ));
        assert!(legendary_configuration_supported(
            b"legendary version \"0.21.1\", codename \"fixture\"\n"
        ));
        for text in [
            "legendary version \"0.20.33\"",
            "legendary version \"0.22.0\"",
            "legendary version \"1.0.0\"",
            "legendary version \"0.21.1rc1\"",
            "python 3.21.1",
            "legendary version \"0.21.1.2\"",
        ] {
            assert!(!legendary_configuration_supported(text.as_bytes()));
        }
    }
    #[test]
    fn snap_and_legendary_routes_keep_arguments_and_configuration_separate() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["snap", "legendary"] {
            let path = temp.path().join(name);
            std::fs::write(
                &path,
                if name == "legendary" {
                    "#!/bin/sh\nprintf 'legendary version \"0.21.1\"\\n'\n"
                } else {
                    "fixture"
                },
            )
            .unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let game = Game {
            id: "steam/snap/620".into(),
            name: "Snap".into(),
            source: GameSource::Steam,
            launch_cmd: "untrusted; shell".into(),
            library_root: None,
        };
        let prepared = PreparedLaunch::prepare_with_path(&game, temp.path().as_os_str()).unwrap();
        assert_eq!(prepared.command.get_program(), temp.path().join("snap"));
        assert_eq!(
            prepared.command.get_args().collect::<Vec<_>>(),
            ["run", "steam", "steam://rungameid/620"]
        );
        let game = Game {
            id: "legendary/--literal".into(),
            name: "Legendary".into(),
            source: GameSource::Legendary,
            launch_cmd: "untrusted; shell".into(),
            library_root: Some(temp.path().into()),
        };
        let prepared = PreparedLaunch::prepare_with_path(&game, temp.path().as_os_str()).unwrap();
        assert_eq!(
            prepared.command.get_args().collect::<Vec<_>>(),
            ["launch", "--", "--literal"]
        );
        assert!(prepared
            .command
            .get_envs()
            .any(|(name, value)| name == "LEGENDARY_CONFIG_PATH"
                && value == Some(temp.path().as_os_str())));
        for id in ["legendary/bad?arg=x", "legendary/../invalid"] {
            let invalid = Game {
                id: id.into(),
                ..game.clone()
            };
            assert!(PreparedLaunch::prepare_with_path(&invalid, temp.path().as_os_str()).is_err());
        }
        let invalid = Game {
            library_root: None,
            ..game
        };
        assert!(PreparedLaunch::prepare_with_path(&invalid, temp.path().as_os_str()).is_err());
    }
    #[test]
    fn shortcut_launch_id_matches_valve_layout_and_account_changes_fail_preparation() {
        let temp = tempfile::tempdir().unwrap();
        let steam = temp.path().join("steam");
        std::fs::write(&steam, "fixture").unwrap();
        std::fs::set_permissions(&steam, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = temp.path().join("config");
        std::fs::create_dir(&config).unwrap();
        let login = config.join("loginusers.vdf");
        std::fs::write(
            &login,
            "\"users\"{\"76561197960265735\"{\"MostRecent\"\"1\"}}",
        )
        .unwrap();
        let game = Game {
            id: "steam/shortcut/7/4045620583".into(),
            name: "Shortcut".into(),
            source: GameSource::Steam,
            launch_cmd: "/never execute".into(),
            library_root: Some(temp.path().into()),
        };
        let prepared = PreparedLaunch::prepare_with_path(&game, temp.path().as_os_str()).unwrap();
        // Valve CGameID stores shortcut type 2 at bit 24 and the unsigned ID
        // in the upper word. This literal also exercises IDs above signed i32.
        assert_eq!(
            prepared.command.get_args().collect::<Vec<_>>(),
            ["steam://rungameid/17375808096043008000"]
        );
        std::fs::write(
            &login,
            "\"users\"{\"76561197960265736\"{\"MostRecent\"\"1\"}}",
        )
        .unwrap();
        assert!(matches!(
            PreparedLaunch::prepare_with_path(&game, temp.path().as_os_str()),
            Err(OmenError::LaunchFailed(_))
        ));
        for id in [
            "steam/0",
            "steam/4294967296",
            "steam/shortcut/7/2147483648",
            "steam/snap/not-a-number",
        ] {
            let invalid = Game {
                id: id.into(),
                ..game.clone()
            };
            assert!(PreparedLaunch::prepare_with_path(&invalid, temp.path().as_os_str()).is_err());
        }
    }

    #[test]
    fn steam_launcher_uses_one_uri_argument_and_ignores_stored_shell_text() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("steam");
        std::fs::write(&binary, "fixture").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let game = Game {
            id: "steam/620".into(),
            name: "fixture".into(),
            source: GameSource::Steam,
            launch_cmd: "untrusted; shell text".into(),
            library_root: None,
        };
        let prepared = PreparedLaunch::prepare_with_path(&game, dir.path().as_os_str()).unwrap();
        assert_eq!(prepared.command.get_program(), binary);
        assert_eq!(
            prepared.command.get_args().collect::<Vec<_>>(),
            vec!["steam://rungameid/620"]
        );
    }
    #[test]
    fn malformed_steam_id_is_rejected_before_resolving_or_spawning() {
        let game = Game {
            id: "steam/620;bad".into(),
            name: "fixture".into(),
            source: GameSource::Steam,
            launch_cmd: "".into(),
            library_root: None,
        };
        assert!(matches!(
            PreparedLaunch::prepare_with_path(&game, std::ffi::OsStr::new("")),
            Err(OmenError::InvalidArgument(_))
        ));
    }

    #[test]
    fn store_routes_use_fixed_programs_and_arguments() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["steam", "heroic", "lutris", "flatpak"] {
            let path = dir.path().join(name);
            std::fs::write(&path, "fixture").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        for (source, id, binary, arguments) in [
            (
                GameSource::Steam,
                "steam/flatpak/620",
                "flatpak",
                vec!["run", "com.valvesoftware.Steam", "steam://rungameid/620"],
            ),
            (
                GameSource::Heroic,
                "heroic/legendary/Quail",
                "heroic",
                vec!["heroic://launch/legendary/Quail"],
            ),
            (
                GameSource::Heroic,
                "heroic/flatpak/gog/123",
                "flatpak",
                vec![
                    "run",
                    "com.heroicgameslauncher.hgl",
                    "heroic://launch/gog/123",
                ],
            ),
            (
                GameSource::Heroic,
                "heroic/nile/amzn1.adg.fixture",
                "heroic",
                vec!["heroic://launch/nile/amzn1.adg.fixture"],
            ),
            (
                GameSource::Lutris,
                "lutris/9",
                "lutris",
                vec!["lutris:rungameid/9"],
            ),
            (
                GameSource::Lutris,
                "lutris/flatpak/9",
                "flatpak",
                vec!["run", "net.lutris.Lutris", "lutris:rungameid/9"],
            ),
        ] {
            let game = Game {
                id: id.into(),
                name: "fixture".into(),
                source,
                launch_cmd: "untrusted shell text".into(),
                library_root: None,
            };
            let prepared =
                PreparedLaunch::prepare_with_path(&game, dir.path().as_os_str()).unwrap();
            assert_eq!(prepared.command.get_program(), dir.path().join(binary));
            assert_eq!(prepared.command.get_args().collect::<Vec<_>>(), arguments);
        }
    }
    #[test]
    fn invalid_store_components_fail_before_executable_lookup() {
        for (source, id) in [
            (GameSource::Heroic, "heroic/legendary/game?arg=x"),
            (GameSource::Heroic, "heroic/legendary/game/extra"),
            (GameSource::Heroic, "heroic/unknown/game"),
            (GameSource::Lutris, "lutris/0"),
            (GameSource::Lutris, "lutris/flatpak/-1"),
            (GameSource::Steam, "steam/flatpak/620&arg"),
        ] {
            let game = Game {
                id: id.into(),
                name: "fixture".into(),
                source,
                launch_cmd: String::new(),
                library_root: None,
            };
            assert!(matches!(
                PreparedLaunch::prepare_with_path(&game, std::ffi::OsStr::new("")),
                Err(OmenError::InvalidArgument(_))
            ));
        }
    }

    fn steam_fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join(".steam/steam/steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("appmanifest_1091500.acf"),
            "\"appid\"		\"1091500\"\n\"name\"		\"Cyberpunk 2077\"\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("appmanifest_620.acf"),
            "\"appid\"		\"620\"\n\"name\"		\"Portal 2\"\n",
        )
        .unwrap();
        std::fs::write(apps.join("appmanifest_broken.acf"), "garbage").unwrap();
        (dir, apps)
    }

    #[test]
    fn detects_steam_games_sorted() {
        let (dir, _apps) = steam_fixture();
        let games = detect_steam(dir.path());
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Cyberpunk 2077");
        assert_eq!(games[1].name, "Portal 2");
        assert!(games[0].launch_cmd.is_empty());
    }

    #[test]
    fn no_steam_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_steam(dir.path()).is_empty());
    }
}
