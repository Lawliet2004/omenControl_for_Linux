use super::vdf::{self, Value};
use omen_core::games::{Game, GameSource};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_GAMES: usize = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SteamInstallation {
    Native,
    Flatpak,
    Snap,
}
impl SteamInstallation {
    fn prefix(self) -> &'static str {
        match self {
            Self::Native => "steam",
            Self::Flatpak => "steam/flatpak",
            Self::Snap => "steam/snap",
        }
    }
}

fn read_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_FILE).then_some(bytes)
}
fn read(path: &Path) -> Option<String> {
    String::from_utf8(read_bytes(path)?).ok()
}
fn json(path: &Path) -> Option<Json> {
    serde_json::from_str(&read(path)?).ok()
}
fn sorted(games: BTreeMap<String, Game>) -> Vec<Game> {
    let mut games: Vec<_> = games.into_values().collect();
    games.sort_by_key(|game| game.name.to_lowercase());
    games
}
fn xdg(home: &Path, variable: &str, fallback: &str) -> PathBuf {
    // Test homes must never discover the real user's libraries via inherited XDG variables.
    if std::env::var_os("OMEN_HOME").is_none()
        && std::env::var_os("HOME").is_some_and(|value| Path::new(&value) == home)
    {
        if let Some(value) = std::env::var_os(variable)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return value;
        }
    }
    home.join(fallback)
}

pub fn detect_steam(home: &Path) -> Vec<Game> {
    use SteamInstallation::*;
    let snap = home.join("snap/steam/common/.local/share/Steam");
    let snap_common = home
        .canonicalize()
        .unwrap_or_else(|_| home.to_path_buf())
        .join("snap/steam/common");
    let native = |path: PathBuf| {
        let installation = if path
            .canonicalize()
            .is_ok_and(|resolved| resolved.starts_with(&snap_common))
        {
            Snap
        } else {
            Native
        };
        (path, installation)
    };
    steam_at(
        home,
        &[
            native(home.join(".steam/steam")),
            native(home.join(".steam/root")),
            native(xdg(home, "XDG_DATA_HOME", ".local/share").join("Steam")),
            (
                home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
                Flatpak,
            ),
            (snap, Snap),
        ],
    )
}

fn steam_at(home: &Path, candidates: &[(PathBuf, SteamInstallation)]) -> Vec<Game> {
    let mut games = BTreeMap::new();
    let mut scanned = BTreeSet::new();
    for (installation, route) in candidates {
        let route = *route;
        let sandboxed = route != SteamInstallation::Native;
        shortcuts_at(installation, route, &mut games);
        let apps = installation.join("steamapps");
        let mut roots = vec![apps.clone()];
        if let Some(entries) =
            read(&apps.join("libraryfolders.vdf")).and_then(|text| vdf::parse(&text))
        {
            let entries = vdf::object(&entries, "libraryfolders").unwrap_or(&entries);
            for (key, value) in entries {
                if key.is_empty() || !key.bytes().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let path = match value {
                    Value::Text(path) => Some(path.as_str()),
                    Value::Object(values) => vdf::text(values, "path"),
                };
                if let Some(path) = path.map(Path::new).filter(|p| p.is_absolute()) {
                    let path = if sandboxed && path == home.join(".local/share/Steam") {
                        installation.as_path()
                    } else {
                        path
                    };
                    roots.push(path.join("steamapps"));
                }
            }
        }
        for root in roots {
            let Ok(canonical) = root.canonicalize() else {
                continue;
            };
            if !scanned.insert((route, canonical)) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten().take(MAX_GAMES) {
                let filename = entry.file_name();
                let Some(id) = filename
                    .to_str()
                    .and_then(|name| name.strip_prefix("appmanifest_"))
                    .and_then(|name| name.strip_suffix(".acf"))
                else {
                    continue;
                };
                if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let Some(values) = read(&entry.path()).and_then(|text| vdf::parse(&text)) else {
                    continue;
                };
                let state = vdf::object(&values, "AppState").unwrap_or(&values);
                if vdf::text(state, "appid") != Some(id) {
                    continue;
                }
                let Some(name) = vdf::text(state, "name").filter(|name| !name.trim().is_empty())
                else {
                    continue;
                };
                // When supplied, Steam's installed flag must be set. Old/minimal records
                // without this field are inventory only, not proof the files are intact.
                if vdf::text(state, "StateFlags")
                    .is_some_and(|value| value.parse::<u32>().map_or(true, |flags| flags & 4 == 0))
                {
                    continue;
                }
                let prefix = route.prefix();
                let id = format!("{prefix}/{id}");
                games.entry(id.clone()).or_insert_with(|| Game {
                    id,
                    name: name.into(),
                    source: GameSource::Steam,
                    launch_cmd: String::new(),
                    library_root: None,
                });
            }
        }
    }
    sorted(games)
}

/// Steam stores the most recently selected account separately from credentials.
/// Never scan all accounts and silently launch a different account's shortcut.
pub(super) fn active_steam_account(root: &Path) -> Option<u32> {
    let parsed = vdf::parse(&read(&root.join("config/loginusers.vdf"))?)?;
    let mut roots = parsed
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("users"));
    let (_, Value::Object(users)) = roots.next()? else {
        return None;
    };
    if roots.next().is_some() {
        return None;
    }
    let mut found = None;
    for (id, value) in users {
        let Value::Object(fields) = value else {
            continue;
        };
        let mut flags = fields
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("MostRecent"));
        let flag = flags.next();
        if flags.next().is_some() {
            return None;
        }
        match flag {
            None => continue,
            Some((_, Value::Text(value))) if value == "0" => continue,
            Some((_, Value::Text(value))) if value == "1" => {}
            _ => return None,
        }
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let number = id.parse::<u64>().ok()?.checked_sub(76561197960265728)?;
        let account = u32::try_from(number).ok().filter(|id| *id != 0)?;
        if found.replace(account).is_some() {
            return None;
        }
    }
    found
}

fn shortcuts_at(root: &Path, route: SteamInstallation, games: &mut BTreeMap<String, Game>) {
    use super::binary_vdf::{self, Value as Binary};
    let Some(account) = active_steam_account(root) else {
        return;
    };
    let path = root.join(format!("userdata/{account}/config/shortcuts.vdf"));
    let Some(entries) = read_bytes(&path).and_then(|data| binary_vdf::parse(&data)) else {
        return;
    };
    let Some(Binary::Object(shortcuts)) = binary_vdf::unique(&entries, "shortcuts") else {
        return;
    };
    let Ok(root) = root.canonicalize() else {
        return;
    };
    for (key, value) in shortcuts.iter().take(MAX_GAMES) {
        if games.len() >= MAX_GAMES {
            break;
        }
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Binary::Object(fields) = value else {
            continue;
        };
        let Some(Binary::Int(appid)) = binary_vdf::unique(fields, "appid") else {
            continue;
        };
        if *appid < 0x80001388 {
            continue;
        }
        let Some(Binary::Text(name)) = binary_vdf::unique(fields, "AppName") else {
            continue;
        };
        if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
            continue;
        }
        let id = format!("{}/shortcut/{account}/{appid}", route.prefix());
        games.entry(id.clone()).or_insert_with(|| Game {
            id,
            name: name.clone(),
            source: GameSource::Steam,
            launch_cmd: String::new(),
            library_root: Some(root.clone()),
        });
    }
}

pub fn detect_legendary(home: &Path) -> Vec<Game> {
    let configured = (std::env::var_os("OMEN_HOME").is_none()
        && std::env::var_os("HOME").is_some_and(|p| Path::new(&p) == home))
    .then(|| std::env::var_os("LEGENDARY_CONFIG_PATH"))
    .flatten()
    .filter(|p| !p.is_empty());
    let root = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg(home, "XDG_CONFIG_HOME", ".config").join("legendary"));
    legendary_at(&[root])
}
fn legendary_at(roots: &[PathBuf]) -> Vec<Game> {
    let mut games = BTreeMap::new();
    for root in roots {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let Some(Json::Object(entries)) = json(&root.join("installed.json")) else {
            continue;
        };
        for (app, record) in entries.into_iter().take(MAX_GAMES) {
            if !safe_component(&app)
                || app.len() > 240
                || (record.get("is_dlc").and_then(Json::as_bool) == Some(true)
                    && record
                        .get("executable")
                        .and_then(Json::as_str)
                        .is_none_or(|s| s.is_empty()))
            {
                continue;
            }
            if record
                .get("app_name")
                .and_then(Json::as_str)
                .is_some_and(|id| id != app)
            {
                continue;
            }
            let Some(name) = record.get("title").and_then(Json::as_str).filter(|s| {
                !s.trim().is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)
            }) else {
                continue;
            };
            let Some(path) = record
                .get("install_path")
                .and_then(Json::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if !Path::new(path).is_absolute() {
                continue;
            }
            let id = format!("legendary/{app}");
            games.entry(id.clone()).or_insert_with(|| Game {
                id,
                name: name.into(),
                source: GameSource::Legendary,
                launch_cmd: String::new(),
                library_root: Some(root.clone()),
            });
        }
    }
    sorted(games)
}

pub(super) fn additional_libraries(
    home: &Path,
    locations: &[omen_core::games::LibraryLocation],
) -> Vec<Game> {
    let mut games = Vec::new();
    for location in locations.iter().take(32).filter(|l| l.valid()) {
        if games.len() >= MAX_GAMES {
            break;
        }
        let root = [(location.path.clone(), location.flatpak)];
        games.extend(match location.source {
            GameSource::Steam => steam_at(
                home,
                &[(
                    location.path.clone(),
                    if location.snap {
                        SteamInstallation::Snap
                    } else if location.flatpak {
                        SteamInstallation::Flatpak
                    } else {
                        SteamInstallation::Native
                    },
                )],
            ),
            GameSource::Heroic => heroic_at(&root),
            GameSource::Lutris => lutris_at(&[(location.path.join("pga.db"), location.flatpak)]),
            GameSource::Legendary => legendary_at(std::slice::from_ref(&location.path)),
            GameSource::Custom => Vec::new(),
        });
    }
    games.truncate(MAX_GAMES);
    games
}

pub(super) fn safe_component(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 512
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
}

pub fn detect_heroic(home: &Path) -> Vec<Game> {
    heroic_at(&[
        (
            xdg(home, "XDG_CONFIG_HOME", ".config").join("heroic"),
            false,
        ),
        (
            home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"),
            true,
        ),
    ])
}
fn heroic_at(roots: &[(PathBuf, bool)]) -> Vec<Game> {
    let mut games = BTreeMap::new();
    for (root, sandboxed) in roots {
        let mut add = |runner: &str, id: &str, name: &str| {
            if !safe_component(id) {
                return;
            }
            let prefix = if *sandboxed {
                "heroic/flatpak"
            } else {
                "heroic"
            };
            let id = format!("{prefix}/{runner}/{id}");
            games.entry(id.clone()).or_insert_with(|| Game {
                id,
                name: name.into(),
                source: GameSource::Heroic,
                launch_cmd: String::new(),
                library_root: None,
            });
        };
        if let Some(Json::Object(entries)) =
            json(&root.join("legendaryConfig/legendary/installed.json"))
        {
            for (id, record) in entries.into_iter().take(MAX_GAMES) {
                if record.get("is_dlc").and_then(Json::as_bool) == Some(true) {
                    continue;
                }
                if let Some(name) = record
                    .get("title")
                    .and_then(Json::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    add("legendary", &id, name);
                }
            }
        }
        if let Some(entries) = json(&root.join("gog_store/installed.json"))
            .and_then(|value| value.get("installed").and_then(Json::as_array).cloned())
        {
            for record in entries.into_iter().take(MAX_GAMES) {
                if record.get("is_dlc").and_then(Json::as_bool) == Some(true) {
                    continue;
                }
                if let Some(id) = record.get("appName").and_then(Json::as_str) {
                    add("gog", id, &format!("GOG {id}"));
                }
            }
        }
        let library = json(&root.join("nile_config/nile/library.json")).unwrap_or(Json::Null);
        let titles: BTreeMap<&str, &str> = library
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|record| {
                let product = record.get("product")?;
                Some((
                    product.get("id")?.as_str()?,
                    product.get("title")?.as_str()?,
                ))
            })
            .collect();
        if let Some(Json::Array(entries)) = json(&root.join("nile_config/nile/installed.json")) {
            for record in entries.into_iter().take(MAX_GAMES) {
                let Some(id) = record.get("id").and_then(Json::as_str) else {
                    continue;
                };
                add("nile", id, titles.get(id).copied().unwrap_or(id));
            }
        }
    }
    sorted(games)
}

pub fn detect_lutris(home: &Path) -> Vec<Game> {
    lutris_at(&[
        (
            xdg(home, "XDG_DATA_HOME", ".local/share").join("lutris/pga.db"),
            false,
        ),
        (
            home.join(".var/app/net.lutris.Lutris/data/lutris/pga.db"),
            true,
        ),
    ])
}
fn lutris_at(databases: &[(PathBuf, bool)]) -> Vec<Game> {
    let mut games = BTreeMap::new();
    for (path, sandboxed) in databases {
        // READ_ONLY preserves the launcher's database and observes its WAL. Never
        // create a missing DB or use immutable=1 (which would hide live WAL rows).
        let Ok(db) = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        let _ = db.busy_timeout(std::time::Duration::from_millis(100));
        let Ok(mut query) = db.prepare(
            "SELECT id, name FROM games WHERE installed = 1 AND id > 0 ORDER BY id LIMIT 10000",
        ) else {
            continue;
        };
        let Ok(rows) = query.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) else {
            continue;
        };
        for (id, name) in rows.flatten() {
            if name.trim().is_empty() {
                continue;
            }
            let prefix = if *sandboxed {
                "lutris/flatpak"
            } else {
                "lutris"
            };
            let id = format!("{prefix}/{id}");
            games.insert(
                id.clone(),
                Game {
                    id,
                    name,
                    source: GameSource::Lutris,
                    launch_cmd: String::new(),
                    library_root: None,
                },
            );
        }
    }
    sorted(games)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn login(root: &Path, account: u32) {
        put(
            &root.join("config/loginusers.vdf"),
            &format!(
                "\"users\"{{\"{}\"{{\"MostRecent\"\"1\"}}}}",
                76561197960265728u64 + u64::from(account)
            ),
        );
    }
    fn shortcut(root: &Path, account: u32, appid: u32, name: &str) {
        let path = root.join(format!("userdata/{account}/config/shortcuts.vdf"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = b"\x00shortcuts\0\x000\0\x02appid\0".to_vec();
        bytes.extend(appid.to_le_bytes());
        bytes.extend(b"\x01AppName\0");
        bytes.extend(name.as_bytes());
        bytes.push(0);
        // Discovery never executes a shortcut's command/launch-option metadata.
        bytes.extend(b"\x01exe\0/untrusted; shell $(text)\0\x00tags\0\x010\0tag\0\x08\x08\x08\x08");
        std::fs::write(path, bytes).unwrap();
    }
    #[test]
    fn binary_shortcuts_use_only_recent_account_and_distinct_installations() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        for (path, prefix) in [
            (root.join(".steam/steam"), "steam"),
            (
                root.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
                "steam/flatpak",
            ),
            (
                root.join("snap/steam/common/.local/share/Steam"),
                "steam/snap",
            ),
        ] {
            login(&path, 7);
            shortcut(&path, 7, 0xf1234567, "Café 二");
            shortcut(&path, 8, 0xf7654321, "Other account");
            let games = detect_steam(root);
            let id = format!("{prefix}/shortcut/7/4045620583");
            let game = games.iter().find(|g| g.id == id).unwrap();
            assert_eq!(game.name, "Café 二");
            assert!(game.launch_cmd.is_empty());
            assert_eq!(game.library_root, Some(path.canonicalize().unwrap()));
            assert!(games.iter().all(|g| !g.name.contains("Other account")));
        }
        assert_eq!(detect_steam(root).len(), 3);
        let native = root.join(".steam/steam");
        login(&native, 8);
        assert!(detect_steam(root)
            .iter()
            .any(|g| g.id == "steam/shortcut/8/4150608673"));
        put(&native.join("config/loginusers.vdf"),"\"users\"{\"76561197960265735\"{\"MostRecent\"\"1\"}\"76561197960265736\"{\"MostRecent\"\"1\"}}");
        assert!(active_steam_account(&native).is_none());
    }
    #[test]
    fn snap_aliases_do_not_turn_shared_native_data_into_the_wrong_route() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let snap = root.join("snap/steam/common/.local/share/Steam");
        manifest(&snap.join("steamapps"), "620", "Snap game");
        std::fs::create_dir_all(root.join(".steam")).unwrap();
        let native = root.join(".steam/steam");
        std::os::unix::fs::symlink(&snap, &native).unwrap();
        assert_eq!(
            detect_steam(root)
                .iter()
                .map(|g| g.id.as_str())
                .collect::<Vec<_>>(),
            ["steam/snap/620"]
        );
        std::fs::remove_file(&native).unwrap();
        std::fs::remove_dir_all(&snap).unwrap();
        manifest(&native.join("steamapps"), "621", "Shared game");
        std::os::unix::fs::symlink(&native, &snap).unwrap();
        assert_eq!(
            detect_steam(root)
                .iter()
                .map(|g| g.id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["steam/621", "steam/snap/621"])
        );
    }
    #[test]
    fn malformed_shortcut_file_isolated_from_store_inventory_and_never_blocks() {
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join(".steam/steam");
        manifest(&steam.join("steamapps"), "620", "Store game");
        login(&steam, 7);
        shortcut(&steam, 7, 0xf1234567, "Shortcut");
        let path = steam.join("userdata/7/config/shortcuts.vdf");
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert_eq!(detect_steam(home.path()).len(), 1);
        std::fs::remove_file(&path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let fifo = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(detect_steam(home.path()).len(), 1);
    }
    #[test]
    fn legendary_reads_installed_entries_and_keeps_selected_configuration() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".config/legendary");
        put(
            &root.join("installed.json"),
            r#"{"Quail":{"app_name":"Quail","title":"Native fixture","install_path":"/fixture"},"DLC":{"title":"Launchable DLC","install_path":"/fixture","is_dlc":true,"executable":"dlc.exe"},"OnlyDLC":{"title":"Content","install_path":"/fixture","is_dlc":true},"Mismatch":{"app_name":"Other","title":"Wrong","install_path":"/fixture"},"--literal":{"title":"Literal option-like ID","install_path":"/fixture"},"NoPath":{"title":"Not installed"}}"#,
        );
        let games = detect_legendary(home.path());
        assert_eq!(games.len(), 3);
        assert!(games.iter().all(|g| g.source == GameSource::Legendary
            && g.launch_cmd.is_empty()
            && g.library_root == Some(root.clone())));
        let extra = home.path().join("Selected configuration");
        put(
            &extra.join("installed.json"),
            r#"{"Quail":{"title":"Selected fixture","install_path":"/fixture"}}"#,
        );
        let found = additional_libraries(
            home.path(),
            &[omen_core::games::LibraryLocation {
                source: GameSource::Legendary,
                path: extra.clone(),
                flatpak: false,
                snap: false,
            }],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].library_root, Some(extra));
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn manifest(apps: &Path, id: &str, name: &str) {
        put(
            &apps.join(format!("appmanifest_{id}.acf")),
            &format!(
                "\"AppState\" {{ \"appid\" \"{id}\" \"name\" {} \"StateFlags\" \"4\" }}",
                serde_json::to_string(name).unwrap()
            ),
        );
    }
    #[test]
    fn explicit_locations_read_known_formats_and_keep_launcher_installations_distinct() {
        use omen_core::games::LibraryLocation;
        let home = tempfile::tempdir().unwrap();
        let steam = home.path().join("external-steam");
        let heroic = home.path().join("external-heroic");
        let lutris = home.path().join("external-lutris");
        manifest(&steam.join("steamapps"), "620", "External Steam");
        put(
            &heroic.join("legendaryConfig/legendary/installed.json"),
            r#"{"Quail":{"title":"External Heroic"}}"#,
        );
        std::fs::create_dir_all(&lutris).unwrap();
        let database = lutris.join("pga.db");
        let db = rusqlite::Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE games(id INTEGER, name TEXT, installed INTEGER); INSERT INTO games VALUES(7, 'External Lutris', 1);").unwrap();
        drop(db);
        let before = std::fs::read(&database).unwrap();
        let mut locations = vec![
            LibraryLocation {
                source: GameSource::Steam,
                path: steam.clone(),
                flatpak: false,
                snap: false,
            },
            LibraryLocation {
                source: GameSource::Steam,
                path: steam,
                flatpak: true,
                snap: false,
            },
            LibraryLocation {
                source: GameSource::Heroic,
                path: heroic,
                flatpak: false,
                snap: false,
            },
            LibraryLocation {
                source: GameSource::Lutris,
                path: lutris,
                flatpak: true,
                snap: false,
            },
        ];
        let games = additional_libraries(home.path(), &locations);
        let ids: BTreeSet<_> = games.iter().map(|game| game.id.as_str()).collect();
        assert_eq!(
            ids,
            BTreeSet::from([
                "steam/620",
                "steam/flatpak/620",
                "heroic/legendary/Quail",
                "lutris/flatpak/7"
            ])
        );
        assert_eq!(before, std::fs::read(database).unwrap());
        locations.push(LibraryLocation {
            source: GameSource::Steam,
            path: "relative".into(),
            flatpak: false,
            snap: false,
        });
        locations.push(LibraryLocation {
            source: GameSource::Lutris,
            path: home.path().join("absent"),
            flatpak: false,
            snap: false,
        });
        assert_eq!(additional_libraries(home.path(), &locations), games);
        assert!(!home.path().join("absent/pga.db").exists());
    }
    #[test]
    fn steam_unions_native_flatpak_legacy_and_modern_libraries() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let native = root.join(".steam/steam/steamapps");
        let modern = root.join("modern \\\"二/steamapps");
        let legacy = root.join("legacy/steamapps");
        let second = root.join(".local/share/Steam/steamapps");
        let flatpak = root.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps");
        manifest(&native, "1", "Primary");
        manifest(&second, "2", "Second installation");
        manifest(&modern, "3", "Café \"二\"\\path");
        manifest(&legacy, "4", "Legacy library");
        manifest(&flatpak, "1", "Sandbox primary");
        let quote = |p: &Path| serde_json::to_string(&p.to_str().unwrap()).unwrap();
        put(
            &native.join("libraryfolders.vdf"),
            &format!(
                "\"libraryfolders\" {{ \"1\" {{ \"path\" {} }} \"2\" {} }}",
                quote(modern.parent().unwrap()),
                quote(legacy.parent().unwrap())
            ),
        );
        // Steam stores its sandbox's home path here; it must map to the Flatpak
        // root on the host, not accidentally discover the native installation.
        put(
            &flatpak.join("libraryfolders.vdf"),
            &format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" {} }} }}",
                quote(second.parent().unwrap())
            ),
        );
        std::os::unix::fs::symlink(root.join(".steam/steam"), root.join(".steam/root")).unwrap();
        let games = detect_steam(root);
        assert_eq!(games.len(), 5);
        assert_eq!(
            games.iter().find(|g| g.id == "steam/3").unwrap().name,
            "Café \"二\"\\path"
        );
        assert_eq!(
            games
                .iter()
                .filter(|g| g.id.starts_with("steam/flatpak/"))
                .count(),
            1
        );
    }
    #[test]
    fn steam_skips_mismatched_incomplete_and_truncated_records() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join(".steam/steam/steamapps");
        manifest(&apps, "1", "Valid");
        put(
            &apps.join("appmanifest_2.acf"),
            "\"AppState\" {\"appid\" \"2\" \"name\" \"Downloading\" \"StateFlags\" \"2\"}",
        );
        put(
            &apps.join("appmanifest_3.acf"),
            "\"AppState\" {\"appid\" \"99\" \"name\" \"Mismatch\"}",
        );
        put(
            &apps.join("appmanifest_4.acf"),
            "\"AppState\" {\"appid\" \"4\" \"name\" \"Truncated\"",
        );
        assert_eq!(
            detect_steam(home.path())
                .iter()
                .map(|g| g.id.as_str())
                .collect::<Vec<_>>(),
            ["steam/1"]
        );
    }
    #[test]
    fn heroic_reads_only_installed_records_and_keeps_installations_distinct() {
        let home = tempfile::tempdir().unwrap();
        let native = home.path().join("native");
        let flatpak = home.path().join("flatpak");
        let epic = r#"{"Quail":{"title":"Epic fixture","is_dlc":false},"DLC":{"title":"Expansion","is_dlc":true},"bad?arg=x":{"title":"Invalid URI"}}"#;
        put(
            &native.join("legendaryConfig/legendary/installed.json"),
            epic,
        );
        put(
            &flatpak.join("legendaryConfig/legendary/installed.json"),
            epic,
        );
        put(
            &native.join("gog_store/installed.json"),
            r#"{"installed":[{"appName":"123","install_path":"/fixture"}]}"#,
        );
        put(
            &native.join("nile_config/nile/installed.json"),
            r#"[{"id":"amzn1.adg.fixture","path":"/fixture"}]"#,
        );
        put(
            &native.join("nile_config/nile/library.json"),
            r#"[{"product":{"id":"amzn1.adg.fixture","title":"Amazon fixture"}},{"product":{"id":"not-installed","title":"Owned only"}}]"#,
        );
        let games = heroic_at(&[(native, false), (flatpak, true)]);
        assert_eq!(games.len(), 4);
        assert!(games
            .iter()
            .any(|g| g.id == "heroic/flatpak/legendary/Quail"));
        assert_eq!(
            games
                .iter()
                .find(|g| g.id == "heroic/nile/amzn1.adg.fixture")
                .unwrap()
                .name,
            "Amazon fixture"
        );
        assert!(games.iter().all(|g| g.launch_cmd.is_empty()));
    }
    #[test]
    fn absent_or_corrupt_launchers_do_not_create_databases_or_hide_other_games() {
        let home = tempfile::tempdir().unwrap();
        let missing = home.path().join("missing.db");
        let corrupt = home.path().join("corrupt.db");
        put(&corrupt, "not sqlite");
        assert!(lutris_at(&[(missing.clone(), false), (corrupt.clone(), true)]).is_empty());
        assert!(!missing.exists());
        assert_eq!(std::fs::read_to_string(corrupt).unwrap(), "not sqlite");
        put(
            &home.path().join("legendaryConfig/legendary/installed.json"),
            "{corrupt",
        );
        put(
            &home.path().join("gog_store/installed.json"),
            r#"{"installed":[{"appName":"123"}]}"#,
        );
        assert_eq!(heroic_at(&[(home.path().into(), false)]).len(), 1);
    }
    #[test]
    fn lutris_observes_committed_wal_rows_without_modifying_database() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("pga.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE games (id INTEGER, name TEXT, installed INTEGER); INSERT INTO games VALUES (1,'Installed',1),(2,'Owned only',0),(3,'二',1);").unwrap();
        let before = std::fs::read(&path).unwrap();
        let wal_before = std::fs::read(path.with_extension("db-wal")).unwrap();
        let games = lutris_at(&[(path.clone(), false), (path.clone(), true)]);
        assert_eq!(games.len(), 4);
        assert!(games
            .iter()
            .any(|g| g.id == "lutris/flatpak/3" && g.name == "二"));
        assert_eq!(before, std::fs::read(&path).unwrap());
        assert_eq!(
            wal_before,
            std::fs::read(path.with_extension("db-wal")).unwrap()
        );
    }
}
