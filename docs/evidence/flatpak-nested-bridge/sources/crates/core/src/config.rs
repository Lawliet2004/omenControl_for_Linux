//! Versioned TOML configuration with migrations, atomic saves and backups.
//!
//! Schema lives at `~/.config/omen-linux/config.toml` (override with
//! `$OMEN_CONFIG_DIR` for tests). Every write is atomic (tmp + rename) and
//! the previous file is kept as `config.toml.bak`.

use crate::fans::FanCurve;
use crate::games::Game;
use crate::lighting::LightingPreset;
use crate::profile::ProfileKind;
use crate::{OmenError, Result, CONFIG_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    /// Sensor polling interval in milliseconds (bounded 250..=10000).
    #[serde(default = "default_poll_ms")]
    pub poll_interval_ms: u64,
    /// History retention for graphs, in samples.
    #[serde(default = "default_history")]
    pub history_samples: usize,
    /// Start the GUI minimized to tray when desktop integration supports it.
    #[serde(default)]
    pub start_minimized: bool,
    /// Restore a user-approved desktop shortcut when the GUI starts.
    #[serde(default, skip_serializing_if = "is_false")]
    pub global_shortcut_enabled: bool,
    /// Show the unprivileged compact monitoring window on GUI startup.
    #[serde(default, skip_serializing_if = "is_false")]
    pub overlay_enabled: bool,
    /// Whether the desktop user dismissed the first-run welcome.
    /// Existing configurations predate the welcome and should not see it.
    #[serde(default = "default_onboarding_dismissed")]
    pub onboarding_dismissed: bool,
    /// Optional, informational temperature notifications for the desktop user.
    #[serde(default)]
    pub alerts: AlertConfig,
    /// Dashboard cards selected by the desktop user.
    #[serde(default = "default_dashboard_metrics")]
    pub dashboard_metrics: Vec<DashboardMetric>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardMetric {
    CpuTemperature,
    GpuTemperature,
    CpuFan,
    GpuFan,
    GpuPower,
    Battery,
}

fn default_dashboard_metrics() -> Vec<DashboardMetric> {
    use DashboardMetric::*;
    vec![
        CpuTemperature,
        GpuTemperature,
        CpuFan,
        GpuFan,
        GpuPower,
        Battery,
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AlertConfig {
    pub enabled: bool,
    pub temperature_c: u32,
    pub hysteresis_c: u32,
    pub cooldown_seconds: u32,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            temperature_c: 90,
            hysteresis_c: 5,
            cooldown_seconds: 300,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

fn default_onboarding_dismissed() -> bool {
    true
}

fn existing_general_config() -> GeneralConfig {
    GeneralConfig {
        onboarding_dismissed: true,
        ..GeneralConfig::default()
    }
}

fn default_poll_ms() -> u64 {
    1000
}
fn default_history() -> usize {
    300
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            poll_interval_ms: default_poll_ms(),
            history_samples: default_history(),
            start_minimized: false,
            global_shortcut_enabled: false,
            overlay_enabled: false,
            onboarding_dismissed: false,
            alerts: AlertConfig::default(),
            dashboard_metrics: default_dashboard_metrics(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FanConfig {
    /// Mode applied at daemon start when safe. `auto` is the fail-safe.
    #[serde(default)]
    pub startup_mode: StartupFanMode,
    #[serde(default = "FanCurve::default")]
    pub curve: FanCurve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StartupFanMode {
    /// Always restore firmware automatic control at startup.
    #[default]
    Auto,
    /// Restore the custom curve if (and only if) the hardware supports it.
    Curve,
}

impl Default for FanConfig {
    fn default() -> Self {
        FanConfig {
            startup_mode: StartupFanMode::Auto,
            curve: FanCurve::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct LightingConfig {
    #[serde(default)]
    pub presets: Vec<LightingPreset>,
    /// Index (or name) of the preset restored at startup, when supported.
    #[serde(default)]
    pub startup_preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PowerConfig {
    /// Automatically drop to a safe profile on battery.
    #[serde(default)]
    pub battery_aware: bool,
    #[serde(default = "default_battery_profile")]
    pub on_battery_profile: ProfileKind,
    #[serde(
        default = "default_ac_profile",
        serialize_with = "serialize_ac_profile",
        deserialize_with = "deserialize_ac_profile"
    )]
    pub on_ac_profile: Option<ProfileKind>,
}

impl Default for PowerConfig {
    fn default() -> Self {
        PowerConfig {
            battery_aware: false,
            on_battery_profile: ProfileKind::Quiet,
            on_ac_profile: Some(ProfileKind::Balanced),
        }
    }
}

fn default_battery_profile() -> ProfileKind {
    ProfileKind::Quiet
}
fn default_ac_profile() -> Option<ProfileKind> {
    Some(ProfileKind::Balanced)
}

// TOML cannot encode null. Persist None explicitly so reloading does not
// accidentally select the legacy missing-field default (Balanced).
fn serialize_ac_profile<S: serde::Serializer>(
    profile: &Option<ProfileKind>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match profile {
        Some(profile) => profile.serialize(serializer),
        None => serializer.serialize_str("unchanged"),
    }
}
fn deserialize_ac_profile<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<ProfileKind>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Profile(ProfileKind),
        Special(String),
    }
    match Option::<Stored>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Stored::Profile(profile)) => Ok(Some(profile)),
        Some(Stored::Special(value)) if value == "unchanged" => Ok(None),
        _ => Err(serde::de::Error::custom(
            "AC profile must be an advertised profile or unchanged",
        )),
    }
}

/// Per-game profile bindings keyed by game id (`steam/123`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    #[serde(default)]
    pub per_game_profiles: std::collections::BTreeMap<String, ProfileKind>,
    #[serde(default)]
    pub custom_games: Vec<Game>,
    #[serde(default)]
    pub library_locations: Vec<crate::games::LibraryLocation>,
    /// Defer profile ownership to the actual-command wrapper for these games.
    #[serde(default)]
    pub managed_game_ids: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OmenConfig {
    pub version: u32,
    #[serde(default = "existing_general_config")]
    pub general: GeneralConfig,
    #[serde(default)]
    pub fan: FanConfig,
    #[serde(default)]
    pub lighting: LightingConfig,
    #[serde(default)]
    pub power: PowerConfig,
    #[serde(default)]
    pub games: GameConfig,
}

impl Default for OmenConfig {
    fn default() -> Self {
        OmenConfig {
            version: CONFIG_VERSION,
            general: GeneralConfig::default(),
            fan: FanConfig::default(),
            lighting: LightingConfig::default(),
            power: PowerConfig::default(),
            games: GameConfig::default(),
        }
    }
}

impl OmenConfig {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            return Err(OmenError::InvalidArgument(
                "configuration schema version is unsupported".into(),
            ));
        }
        if !(250..=10_000).contains(&self.general.poll_interval_ms)
            || !(10..=10_000).contains(&self.general.history_samples)
        {
            return Err(OmenError::InvalidArgument(
                "poll interval or history length out of bounds".into(),
            ));
        }
        let alerts = &self.general.alerts;
        let mut metrics = std::collections::BTreeSet::new();
        if self
            .general
            .dashboard_metrics
            .iter()
            .any(|metric| !metrics.insert(metric))
        {
            return Err(OmenError::InvalidArgument(
                "dashboard metrics must be unique".into(),
            ));
        }
        if !(40..=120).contains(&alerts.temperature_c)
            || !(1..=30).contains(&alerts.hysteresis_c)
            || alerts.hysteresis_c >= alerts.temperature_c
            || !(30..=3600).contains(&alerts.cooldown_seconds)
        {
            return Err(OmenError::InvalidArgument(
                "alert threshold, hysteresis or cooldown out of bounds".into(),
            ));
        }
        self.fan.curve.validate()?;
        if self.games.managed_game_ids.len() > 10_000
            || self.games.managed_game_ids.iter().any(|id| {
                !crate::games::valid_game_id(id)
                    || !matches!(
                        self.games.per_game_profiles.get(id),
                        Some(ProfileKind::Performance | ProfileKind::Quiet)
                    )
            })
        {
            return Err(OmenError::InvalidArgument(
                "automatic game profiles require a valid game ID bound to Performance or Quiet"
                    .into(),
            ));
        }
        if self.games.library_locations.len() > 32
            || self
                .games
                .library_locations
                .iter()
                .any(|location| !location.valid())
        {
            return Err(OmenError::InvalidArgument(
                "library locations require absolute directories and supported installations: Steam native/Flatpak/Snap, Heroic or Lutris native/Flatpak, Legendary native (maximum 32)".into(),
            ));
        }
        for preset in &self.lighting.presets {
            preset.state.validate()?;
        }
        Ok(())
    }
    pub fn config_dir() -> PathBuf {
        if let Ok(d) = std::env::var("OMEN_CONFIG_DIR") {
            return PathBuf::from(d);
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config")
            });
        base.join("omen-linux")
    }

    pub fn path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// System-wide config used by the privileged daemon
    /// (`/etc/omen-linux/config.toml`; `$OMEN_SYSTEM_CONFIG_DIR` overrides
    /// for isolated development and tests).
    pub fn system_path() -> PathBuf {
        if let Ok(d) = std::env::var("OMEN_SYSTEM_CONFIG_DIR") {
            return PathBuf::from(d).join("config.toml");
        }
        PathBuf::from("/etc/omen-linux/config.toml")
    }

    /// Service configuration never falls back to a desktop user's file.
    /// Unprivileged development daemons need an explicit writable system-dir
    /// override to save settings; lack of permission must remain an error.
    pub fn daemon_path() -> PathBuf {
        Self::system_path()
    }

    /// Load config, migrating older schemas. A missing file yields defaults
    /// (which are then persisted lazily on first change).
    pub fn load() -> Result<OmenConfig> {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Result<OmenConfig> {
        if !path.exists() {
            return Ok(OmenConfig::default());
        }
        let raw = fs::read_to_string(path)
            .map_err(|e| OmenError::Io(format!("reading {}: {e}", path.display())))?;
        Self::from_str(&raw, path)
    }

    pub fn from_str(raw: &str, path: &Path) -> Result<OmenConfig> {
        let value: toml::Value = raw
            .parse()
            .map_err(|e| OmenError::InvalidArgument(format!("config parse error: {e}")))?;
        let version = value
            .get("version")
            .and_then(|v| v.as_integer())
            .unwrap_or(0) as u32;
        if version > CONFIG_VERSION {
            return Err(OmenError::InvalidArgument(format!(
                "config at {} was written by a newer version (schema {version} > {CONFIG_VERSION}); \
                 not downgrading",
                path.display()
            )));
        }
        let migrated = Self::migrate(value, version)?;
        let mut cfg: OmenConfig = migrated.try_into().map_err(|e: toml::de::Error| {
            OmenError::InvalidArgument(format!("config decode: {e}"))
        })?;
        cfg.general.poll_interval_ms = cfg.general.poll_interval_ms.clamp(250, 10_000);
        cfg.general.history_samples = cfg.general.history_samples.clamp(10, 10_000);
        cfg.validate()?;
        Ok(cfg)
    }

    /// Schema migrations. v0 (no version field) → v1 fills defaults through
    /// serde. Future steps chain here; each step must be pure and tested.
    fn migrate(mut value: toml::Value, from: u32) -> Result<toml::Value> {
        let mut v = from;
        if v == 0 {
            // v0 had poll_ms instead of poll_interval_ms.
            if let Some(table) = value.get_mut("general").and_then(|g| g.as_table_mut()) {
                if let Some(pm) = table.remove("poll_ms") {
                    table.insert("poll_interval_ms".into(), pm);
                }
            }
            v = 1;
        }
        if let Some(t) = value.as_table_mut() {
            t.insert("version".into(), toml::Value::from(v));
        }
        Ok(value)
    }

    /// Save atomically with a `.bak` of the previous file.
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| OmenError::Io(format!("mkdir {}: {e}", parent.display())))?;
        }
        let s = toml::to_string_pretty(self)
            .map_err(|e| OmenError::Internal(format!("config encode: {e}")))?;
        if path.exists() {
            let bak = path.with_extension("toml.bak");
            fs::copy(path, &bak)
                .map_err(|e| OmenError::Io(format!("backup to {}: {e}", bak.display())))?;
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(s.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)
            .map_err(|e| OmenError::Io(format!("save {}: {e}", path.display())))?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    /// Export as pretty JSON (Settings → export).
    pub fn export_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| OmenError::Internal(format!("export: {e}")))
    }

    /// Import legacy full-config JSON, preserving JSON null for optional fields.
    pub fn import_json(raw: &str) -> Result<OmenConfig> {
        let mut json: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| OmenError::InvalidArgument(format!("import parse: {e}")))?;
        let object = json
            .as_object_mut()
            .ok_or_else(|| OmenError::InvalidArgument("settings JSON must be an object".into()))?;
        let ver = match object.get("version") {
            None => 0,
            Some(value) => value.as_u64().ok_or_else(|| {
                OmenError::InvalidArgument("settings version must be a nonnegative integer".into())
            })?,
        };
        if ver > u64::from(CONFIG_VERSION) {
            return Err(OmenError::InvalidArgument(format!(
                "imported config uses newer schema {ver} > {CONFIG_VERSION}"
            )));
        }
        if ver == 0 {
            if let Some(general) = object.get_mut("general").and_then(|v| v.as_object_mut()) {
                if let Some(poll_ms) = general.remove("poll_ms") {
                    general.insert("poll_interval_ms".into(), poll_ms);
                }
            }
            object.insert("version".into(), serde_json::json!(1));
        }
        let mut config: Self = serde_json::from_value(json)
            .map_err(|e| OmenError::InvalidArgument(format!("settings decode: {e}")))?;
        config.general.poll_interval_ms = config.general.poll_interval_ms.clamp(250, 10_000);
        config.general.history_samples = config.general.history_samples.clamp(10, 10_000);
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fans::CurvePoint;

    #[test]
    fn legacy_json_round_trip_preserves_nullable_fields() {
        let mut config = OmenConfig::default();
        config.power.on_ac_profile = None;
        let restored = OmenConfig::import_json(&config.export_json().unwrap()).unwrap();
        assert_eq!(restored, config);
        assert!(OmenConfig::import_json(r#"{"version":"1"}"#).is_err());
        let mut nullable = serde_json::to_value(&config).unwrap();
        nullable["power"]["on_ac_profile"] = serde_json::Value::Null;
        assert_eq!(
            OmenConfig::import_json(&nullable.to_string()).unwrap(),
            config
        );
    }

    #[test]
    fn unchanged_ac_policy_survives_toml_save_and_reload() {
        let mut config = OmenConfig::default();
        config.power.on_ac_profile = None;
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert!(encoded.contains("on_ac_profile = \"unchanged\""));
        assert_eq!(
            OmenConfig::from_str(&encoded, Path::new("<test>")).unwrap(),
            config
        );
    }

    #[test]
    fn default_roundtrips_through_toml() {
        let cfg = OmenConfig::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back = OmenConfig::from_str(&s, Path::new("<test>")).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = OmenConfig::load_from(Path::new("/nonexistent/omen.toml")).unwrap();
        assert_eq!(cfg.version, CONFIG_VERSION);
    }

    #[test]
    fn v0_migrates_poll_ms_field() {
        let v0 = r#"
[general]
poll_ms = 500
"#;
        let cfg = OmenConfig::from_str(v0, Path::new("<v0>")).unwrap();
        assert_eq!(cfg.general.poll_interval_ms, 500);
        assert_eq!(cfg.version, CONFIG_VERSION);
    }

    #[test]
    fn rejects_future_schema() {
        let future = format!("version = {}", CONFIG_VERSION + 5);
        let err = OmenConfig::from_str(&future, Path::new("<future>")).unwrap_err();
        assert!(matches!(err, OmenError::InvalidArgument(_)));
        assert!(err.to_string().contains("newer version"));
    }

    #[test]
    fn rejects_invalid_curve() {
        let cfg = r#"
version = 1
[[fan.curve.points]]
temp_c = 50
duty_pct = 80
[[fan.curve.points]]
temp_c = 50
duty_pct = 40
"#;
        assert!(OmenConfig::from_str(cfg, Path::new("<bad>")).is_err());
    }

    #[test]
    fn save_is_atomic_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut cfg = OmenConfig::default();
        cfg.general.poll_interval_ms = 2500;
        cfg.save_to(&p).unwrap();
        assert!(p.exists());
        let first = fs::read_to_string(&p).unwrap();
        assert!(first.contains("2500"));
        cfg.general.poll_interval_ms = 500;
        cfg.save_to(&p).unwrap();
        assert!(p.with_extension("toml.bak").exists());
        assert!(fs::read_to_string(&p).unwrap().contains("500"));
        // no temp files left behind
        assert!(!p.with_extension("toml.tmp").exists());
    }

    #[test]
    fn poll_interval_clamped() {
        let raw = "version = 1\n[general]\npoll_interval_ms = 1\n";
        let cfg = OmenConfig::from_str(raw, Path::new("<clamp>")).unwrap();
        assert_eq!(cfg.general.poll_interval_ms, 250);
    }

    #[test]
    fn curve_points_serialize() {
        let c = FanCurve {
            points: vec![CurvePoint {
                temp_c: 45,
                duty_pct: 35,
            }],
        };
        let s = toml::to_string(&c).unwrap();
        assert!(s.contains("temp_c"));
    }
}
