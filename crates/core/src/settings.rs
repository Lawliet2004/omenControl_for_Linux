//! Explicit backup scopes over the existing versioned TOML schema. Desktop
//! preferences/games and service policies can never cross the import boundary.
use crate::config::{
    FanConfig, GameConfig, GeneralConfig, LightingConfig, OmenConfig, PowerConfig,
};
use crate::{OmenError, Result, CONFIG_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    User,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UserSettings {
    pub version: u32,
    pub general: GeneralConfig,
    pub games: GameConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServiceSettings {
    pub version: u32,
    pub fan: FanConfig,
    pub lighting: LightingConfig,
    pub power: PowerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "scope",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SettingsBackup {
    User(UserSettings),
    Service(ServiceSettings),
}

impl From<&OmenConfig> for UserSettings {
    fn from(config: &OmenConfig) -> Self {
        Self {
            version: config.version,
            general: config.general.clone(),
            games: config.games.clone(),
        }
    }
}
impl From<&OmenConfig> for ServiceSettings {
    fn from(config: &OmenConfig) -> Self {
        Self {
            version: config.version,
            fan: config.fan.clone(),
            lighting: config.lighting.clone(),
            power: config.power.clone(),
        }
    }
}

impl UserSettings {
    pub fn apply_to(&self, current: &OmenConfig) -> Result<OmenConfig> {
        let mut next = current.clone();
        next.version = self.version;
        next.general = self.general.clone();
        next.games = self.games.clone();
        next.validate()?;
        let mut ids = std::collections::BTreeSet::new();
        for game in &self.games.custom_games {
            if game.source != crate::games::GameSource::Custom
                || !game.id.starts_with("custom/")
                || game.id.len() <= 7
                || game.name.trim().is_empty()
                || game.launch_cmd.trim().is_empty()
                || game.id.contains(['\0', '\n'])
                || game.launch_cmd.contains('\0')
                || !ids.insert(&game.id)
            {
                return Err(OmenError::InvalidArgument(
                    "custom games need a unique custom/ ID, name, command and custom source".into(),
                ));
            }
        }
        Ok(next)
    }
}
impl ServiceSettings {
    pub fn apply_to(&self, current: &OmenConfig) -> Result<OmenConfig> {
        let mut next = current.clone();
        next.version = self.version;
        next.fan = self.fan.clone();
        next.lighting = self.lighting.clone();
        next.power = self.power.clone();
        next.validate()?;
        Ok(next)
    }
}

impl SettingsBackup {
    pub fn scope(&self) -> SettingsScope {
        match self {
            Self::User(_) => SettingsScope::User,
            Self::Service(_) => SettingsScope::Service,
        }
    }
    pub fn export_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|e| OmenError::Internal(format!("settings export: {e}")))
    }
    pub fn validate(&self) -> Result<()> {
        let defaults = OmenConfig::default();
        let config = match self {
            Self::User(s) => s.apply_to(&defaults)?,
            Self::Service(s) => s.apply_to(&defaults)?,
        };
        if config.version != CONFIG_VERSION {
            return Err(OmenError::InvalidArgument(
                "unsupported settings version".into(),
            ));
        }
        Ok(())
    }
    /// Untagged legacy exports migrate through OmenConfig, then retain only the
    /// explicitly chosen scope. A tagged backup for another scope is rejected.
    pub fn import_json(raw: &str, expected: SettingsScope) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| OmenError::InvalidArgument(format!("settings JSON: {e}")))?;
        let backup: Self = if value.get("scope").is_some() {
            serde_json::from_value(value)
                .map_err(|e| OmenError::InvalidArgument(format!("settings backup: {e}")))?
        } else {
            let legacy = OmenConfig::import_json(raw)?;
            match expected {
                SettingsScope::User => Self::User(UserSettings::from(&legacy)),
                SettingsScope::Service => Self::Service(ServiceSettings::from(&legacy)),
            }
        };
        if backup.scope() != expected {
            return Err(OmenError::InvalidArgument(
                "backup scope does not match the selected import action".into(),
            ));
        }
        backup.validate()?;
        Ok(backup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_scope_is_strict_and_service_has_no_desktop_data() {
        let mut config = OmenConfig::default();
        config.general.start_minimized = true;
        let raw = SettingsBackup::Service(ServiceSettings::from(&config))
            .export_json()
            .unwrap();
        assert!(!raw.contains("games") && !raw.contains("general"));
        assert!(SettingsBackup::import_json(&raw, SettingsScope::User).is_err());
        let bad = raw.replace("\"version\": 1", "\"version\": 1, \"games\": {}");
        assert!(SettingsBackup::import_json(&bad, SettingsScope::Service).is_err());
    }
    #[test]
    fn user_import_preserves_service_policy() {
        let mut existing = OmenConfig::default();
        existing.power.battery_aware = true;
        let mut incoming = UserSettings::from(&OmenConfig::default());
        incoming.general.start_minimized = true;
        let next = incoming.apply_to(&existing).unwrap();
        assert!(next.power.battery_aware && next.general.start_minimized);
    }
    #[test]
    fn service_import_preserves_desktop_data() {
        let mut existing = OmenConfig::default();
        existing.general.start_minimized = true;
        existing
            .games
            .per_game_profiles
            .insert("steam/620".into(), crate::profile::ProfileKind::Quiet);
        let mut incoming = ServiceSettings::from(&OmenConfig::default());
        incoming.power.battery_aware = true;
        let next = incoming.apply_to(&existing).unwrap();
        assert!(next.general.start_minimized && next.power.battery_aware);
        assert_eq!(next.games, existing.games);
    }
    #[test]
    fn legacy_user_import_migrates_without_service_fields() {
        let raw = r#"{"general":{"poll_ms":500},"power":{"battery_aware":true}}"#;
        let backup = SettingsBackup::import_json(raw, SettingsScope::User).unwrap();
        let next = if let SettingsBackup::User(user) = backup {
            user.apply_to(&OmenConfig::default()).unwrap()
        } else {
            panic!()
        };
        assert_eq!(next.general.poll_interval_ms, 500);
        assert!(!next.power.battery_aware);
    }
    #[test]
    fn future_scoped_backup_is_rejected() {
        let mut user = UserSettings::from(&OmenConfig::default());
        user.version = CONFIG_VERSION + 1;
        assert!(SettingsBackup::User(user).export_json().is_err());
    }
    #[test]
    fn user_backup_preserves_mangohud_overlay() {
        let mut config = OmenConfig::default();
        config.games.mangohud_overlay = true;
        let raw = SettingsBackup::User(UserSettings::from(&config))
            .export_json()
            .unwrap();
        let SettingsBackup::User(user) =
            SettingsBackup::import_json(&raw, SettingsScope::User).unwrap()
        else {
            panic!("expected user backup");
        };
        let next = user.apply_to(&OmenConfig::default()).unwrap();
        assert!(next.games.mangohud_overlay);
    }
}
