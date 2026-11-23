//! Settings IO is independent of connection fallback: desktop data always uses
//! the user file; service policies require the authenticated IPC endpoint.
use crate::{IpcClient, Op};
use omen_core::config::OmenConfig;
use omen_core::profile::ProfileKind;
use omen_core::settings::{SettingsBackup, SettingsScope, UserSettings};
use omen_core::{OmenError, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

static USER_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const MAX_BACKUP_BYTES: u64 = 1_048_576;

#[derive(Clone)]
pub struct SettingsController {
    daemon: Option<IpcClient>,
    user_path: PathBuf,
}

impl SettingsController {
    pub fn new(daemon: Option<IpcClient>, user_path: PathBuf) -> Self {
        Self { daemon, user_path }
    }
    pub fn load(&self, scope: SettingsScope) -> Result<SettingsBackup> {
        match scope {
            SettingsScope::User => Ok(SettingsBackup::User(UserSettings::from(
                &OmenConfig::load_from(&self.user_path)?,
            ))),
            SettingsScope::Service => Ok(SettingsBackup::Service(
                self.daemon()?.call(Op::GetServiceSettings)?.decode()?,
            )),
        }
    }
    fn daemon(&self) -> Result<&IpcClient> {
        self.daemon.as_ref().ok_or_else(|| {
            OmenError::DaemonUnavailable(
                "service settings require a connected, current omen-daemon".into(),
            )
        })
    }
    pub fn apply(&self, backup: SettingsBackup) -> Result<()> {
        backup.validate()?;
        match backup {
            SettingsBackup::User(user) => self.edit_user(move |current| {
                *current = user;
            }),
            SettingsBackup::Service(settings) => {
                let response = self.daemon()?.call(Op::SetServiceSettings { settings })?;
                if response.ok {
                    Ok(())
                } else {
                    Err(response.error.unwrap_or_else(|| {
                        OmenError::Internal("service settings were not saved".into())
                    }))
                }
            }
        }
    }
    fn edit_user(&self, edit: impl FnOnce(&mut UserSettings)) -> Result<()> {
        let _guard = USER_WRITE
            .lock()
            .map_err(|_| OmenError::Internal("settings writer unavailable".into()))?;
        let current = OmenConfig::load_from(&self.user_path)?;
        let mut user = UserSettings::from(&current);
        edit(&mut user);
        user.apply_to(&current)?.save_to(&self.user_path)
    }
    pub fn set_start_minimized(&self, enabled: bool) -> Result<()> {
        self.edit_user(|user| user.general.start_minimized = enabled)
    }
    pub fn set_global_shortcut_enabled(&self, enabled: bool) -> Result<()> {
        self.edit_user(|user| user.general.global_shortcut_enabled = enabled)
    }
    pub fn set_overlay_enabled(&self, enabled: bool) -> Result<()> {
        self.edit_user(|user| user.general.overlay_enabled = enabled)
    }
    pub fn set_onboarding_dismissed(&self, dismissed: bool) -> Result<()> {
        self.edit_user(|user| user.general.onboarding_dismissed = dismissed)
    }
    pub fn set_alerts(&self, alerts: omen_core::config::AlertConfig) -> Result<()> {
        self.edit_user(|user| user.general.alerts = alerts)
    }
    pub fn set_dashboard_metrics(
        &self,
        metrics: Vec<omen_core::config::DashboardMetric>,
    ) -> Result<()> {
        self.edit_user(|user| user.general.dashboard_metrics = metrics)
    }
    pub fn set_game_profile(&self, id: &str, profile: Option<ProfileKind>) -> Result<()> {
        self.edit_user(|user| {
            if !matches!(profile, Some(ProfileKind::Performance | ProfileKind::Quiet)) {
                user.games.managed_game_ids.remove(id);
            }
            match profile {
                Some(profile) => {
                    user.games.per_game_profiles.insert(id.into(), profile);
                }
                None => {
                    user.games.per_game_profiles.remove(id);
                }
            }
        })
    }
    pub fn set_game_managed(&self, id: &str, enabled: bool) -> Result<()> {
        self.edit_user(|user| {
            if enabled {
                user.games.managed_game_ids.insert(id.into());
            } else {
                user.games.managed_game_ids.remove(id);
            }
        })
    }
    pub fn set_library_locations(
        &self,
        locations: Vec<omen_core::games::LibraryLocation>,
    ) -> Result<()> {
        self.edit_user(|user| user.games.library_locations = locations)
    }
    pub fn import_file(&self, path: &Path, scope: SettingsScope) -> Result<()> {
        self.apply(Self::read_file(path, scope)?)
    }
    pub fn read_file(path: &Path, scope: SettingsScope) -> Result<SettingsBackup> {
        let mut raw = String::new();
        std::fs::File::open(path)?
            .take(MAX_BACKUP_BYTES + 1)
            .read_to_string(&mut raw)?;
        if raw.len() as u64 > MAX_BACKUP_BYTES {
            return Err(OmenError::InvalidArgument(
                "settings backup exceeds 1 MiB".into(),
            ));
        }
        SettingsBackup::import_json(&raw, scope)
    }
    pub fn export_file(&self, path: &Path, scope: SettingsScope) -> Result<()> {
        self.write_json_document(path, &self.load(scope)?.export_json()?)
    }
    /// Private atomic JSON export, also used for diagnostic reports. Replacing
    /// a selected symlink never follows it to an unrelated target file.
    pub fn write_json_document(&self, path: &Path, raw: &str) -> Result<()> {
        for active in [&self.user_path, &OmenConfig::system_path()] {
            if path == active
                || (path.canonicalize().ok().is_some()
                    && path.canonicalize().ok() == active.canonicalize().ok())
            {
                return Err(OmenError::InvalidArgument(
                    "choose a backup file instead of an active TOML configuration".into(),
                ));
            }
        }
        if raw.len() as u64 > MAX_BACKUP_BYTES {
            return Err(OmenError::InvalidArgument(
                "settings backup exceeds 1 MiB".into(),
            ));
        }
        serde_json::from_str::<serde_json::Value>(raw)
            .map_err(|e| OmenError::InvalidArgument(format!("export is not valid JSON: {e}")))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(raw.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(path)
            .map_err(|e| OmenError::Io(format!("export {}: {e}", path.display())))?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_library_locations_are_user_scoped_validated_and_backed_up() {
        use omen_core::games::{GameSource, LibraryLocation};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        let locations = vec![
            LibraryLocation {
                source: GameSource::Steam,
                path: temp.path().join("Steam"),
                flatpak: false,
                snap: false,
            },
            LibraryLocation {
                source: GameSource::Steam,
                path: temp.path().join("Snap Steam"),
                flatpak: false,
                snap: true,
            },
            LibraryLocation {
                source: GameSource::Legendary,
                path: temp.path().join("Legendary"),
                flatpak: false,
                snap: false,
            },
        ];
        controller.set_library_locations(locations.clone()).unwrap();
        let saved = OmenConfig::load_from(&path).unwrap();
        assert_eq!(saved.games.library_locations, locations);
        let user = controller
            .load(SettingsScope::User)
            .unwrap()
            .export_json()
            .unwrap();
        let restored = SettingsBackup::import_json(&user, SettingsScope::User).unwrap();
        let SettingsBackup::User(restored) = restored else {
            panic!("user scope");
        };
        assert_eq!(restored.games.library_locations, locations);
        assert!(
            !SettingsBackup::Service(omen_core::settings::ServiceSettings::from(&saved))
                .export_json()
                .unwrap()
                .contains("library_locations")
        );
        let before = std::fs::read(&path).unwrap();
        assert!(controller
            .set_library_locations(vec![LibraryLocation {
                source: GameSource::Steam,
                path: "relative".into(),
                flatpak: false,
                snap: false,
            }])
            .is_err());
        assert!(controller
            .set_library_locations(vec![LibraryLocation {
                source: GameSource::Custom,
                path: temp.path().into(),
                flatpak: false,
                snap: false,
            }])
            .is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
    }
    #[test]
    fn onboarding_preference_is_user_scoped_and_preserves_other_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        assert!(
            !OmenConfig::load_from(&path)
                .unwrap()
                .general
                .onboarding_dismissed
        );
        controller.set_start_minimized(true).unwrap();
        controller.set_onboarding_dismissed(true).unwrap();
        let saved = OmenConfig::load_from(&path).unwrap();
        assert!(saved.general.onboarding_dismissed);
        assert!(saved.general.start_minimized);
        let user = controller
            .load(SettingsScope::User)
            .unwrap()
            .export_json()
            .unwrap();
        assert!(user.contains("onboarding_dismissed"));
        let service = SettingsBackup::Service(omen_core::settings::ServiceSettings::from(&saved))
            .export_json()
            .unwrap();
        assert!(!service.contains("onboarding_dismissed"));
        std::fs::write(&path, "invalid {").unwrap();
        assert!(controller.set_onboarding_dismissed(true).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid {");
    }

    #[test]
    fn older_user_configs_do_not_show_first_run_welcome() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        for raw in [
            "version = 1\n",
            "version = 1\n[general]\nstart_minimized = true\n",
        ] {
            std::fs::write(&path, raw).unwrap();
            assert!(
                OmenConfig::load_from(&path)
                    .unwrap()
                    .general
                    .onboarding_dismissed
            );
        }
    }
    #[test]
    fn dashboard_selection_round_trips_and_rejects_duplicates() {
        use omen_core::config::DashboardMetric;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        let selected = vec![DashboardMetric::Battery, DashboardMetric::CpuFan];
        controller.set_dashboard_metrics(selected.clone()).unwrap();
        assert_eq!(
            OmenConfig::load_from(&path)
                .unwrap()
                .general
                .dashboard_metrics,
            selected
        );
        assert!(controller
            .set_dashboard_metrics(vec![DashboardMetric::CpuFan, DashboardMetric::CpuFan])
            .is_err());
        assert_eq!(
            OmenConfig::load_from(&path)
                .unwrap()
                .general
                .dashboard_metrics,
            selected
        );
        let backup = controller.load(SettingsScope::User).unwrap();
        controller.set_dashboard_metrics(vec![]).unwrap();
        controller.apply(backup).unwrap();
        assert_eq!(
            OmenConfig::load_from(&path)
                .unwrap()
                .general
                .dashboard_metrics,
            selected
        );
    }
    #[test]
    fn connected_mode_never_uses_daemon_for_desktop_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("user/config.toml");
        let controller = SettingsController::new(
            Some(IpcClient::new(temp.path().join("missing.sock"))),
            path.clone(),
        );
        controller.set_start_minimized(true).unwrap();
        controller.set_global_shortcut_enabled(true).unwrap();
        controller.set_overlay_enabled(true).unwrap();
        controller
            .set_game_profile("steam/620", Some(ProfileKind::Quiet))
            .unwrap();
        let backup = temp.path().join("desktop.json");
        controller
            .export_file(&backup, SettingsScope::User)
            .unwrap();
        controller.set_start_minimized(false).unwrap();
        controller.set_global_shortcut_enabled(false).unwrap();
        controller.set_overlay_enabled(false).unwrap();
        controller
            .import_file(&backup, SettingsScope::User)
            .unwrap();
        let cfg = OmenConfig::load_from(&path).unwrap();
        assert!(cfg.general.start_minimized);
        assert!(cfg.general.global_shortcut_enabled);
        assert!(cfg.general.overlay_enabled);
        assert_eq!(cfg.games.per_game_profiles["steam/620"], ProfileKind::Quiet);
        let before = std::fs::read(&path).unwrap();
        assert!(controller
            .export_file(&temp.path().join("service.json"), SettingsScope::Service)
            .is_err());
        assert!(controller
            .apply(SettingsBackup::Service(
                omen_core::settings::ServiceSettings::from(&OmenConfig::default())
            ))
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn corrupt_user_file_is_not_replaced_when_editing_bindings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "invalid {").unwrap();
        let controller = SettingsController::new(None, path.clone());
        assert!(controller
            .set_game_profile("steam/620", Some(ProfileKind::Balanced))
            .is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid {");
    }
    #[test]
    fn managed_game_modes_validate_targets_and_survive_scoped_backups() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("user/config.toml");
        let controller = SettingsController::new(None, path.clone());
        controller
            .set_game_profile("custom/example", Some(ProfileKind::Balanced))
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(controller.set_game_managed("custom/example", true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        controller
            .set_game_profile("custom/example", Some(ProfileKind::Performance))
            .unwrap();
        controller.set_game_managed("custom/example", true).unwrap();
        let backup = controller.load(SettingsScope::User).unwrap();
        controller
            .set_game_profile("custom/example", Some(ProfileKind::Balanced))
            .unwrap();
        assert!(OmenConfig::load_from(&path)
            .unwrap()
            .games
            .managed_game_ids
            .is_empty());
        controller.apply(backup).unwrap();
        let restored = OmenConfig::load_from(&path).unwrap();
        assert!(restored.games.managed_game_ids.contains("custom/example"));
        assert_eq!(
            restored.games.per_game_profiles["custom/example"],
            ProfileKind::Performance
        );
    }
    #[test]
    fn alert_preferences_are_user_scoped_and_validated() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        let alerts = omen_core::config::AlertConfig {
            enabled: true,
            temperature_c: 95,
            hysteresis_c: 8,
            cooldown_seconds: 120,
        };
        controller.set_alerts(alerts.clone()).unwrap();
        assert_eq!(OmenConfig::load_from(&path).unwrap().general.alerts, alerts);
        let backup = controller
            .load(SettingsScope::User)
            .unwrap()
            .export_json()
            .unwrap();
        assert!(backup.contains("temperature_c"));
        let service = SettingsBackup::Service(omen_core::settings::ServiceSettings::from(
            &OmenConfig::default(),
        ))
        .export_json()
        .unwrap();
        assert!(!service.contains("temperature_c"));
        let before = std::fs::read(&path).unwrap();
        assert!(controller
            .set_alerts(omen_core::config::AlertConfig {
                cooldown_seconds: 0,
                ..alerts
            })
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn scope_mismatch_never_changes_user_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        controller.set_start_minimized(true).unwrap();
        let before = std::fs::read(&path).unwrap();
        let backup = temp.path().join("service.json");
        std::fs::write(
            &backup,
            SettingsBackup::Service(omen_core::settings::ServiceSettings::from(
                &OmenConfig::default(),
            ))
            .export_json()
            .unwrap(),
        )
        .unwrap();
        assert!(controller
            .import_file(&backup, SettingsScope::User)
            .is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn export_is_private_and_refuses_active_config_path() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let controller = SettingsController::new(None, path.clone());
        controller.set_start_minimized(true).unwrap();
        assert!(controller.export_file(&path, SettingsScope::User).is_err());
        let alias = temp.path().join("alias.json");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(controller.write_json_document(&alias, "{}").is_err());
        let backup = temp.path().join("user.json");
        controller
            .export_file(&backup, SettingsScope::User)
            .unwrap();
        assert_eq!(
            std::fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
