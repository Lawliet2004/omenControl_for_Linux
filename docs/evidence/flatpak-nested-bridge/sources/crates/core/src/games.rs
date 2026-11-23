//! Game library vocabulary: detected games and per-game profile bindings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameSource {
    Steam,
    Heroic,
    Lutris,
    Legendary,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Game {
    /// Stable id, e.g. `steam/1234567`.
    pub id: String,
    pub name: String,
    pub source: GameSource,
    /// User-authored shell command for Custom entries. Store launchers ignore it.
    pub launch_cmd: String,
    /// Discovery provenance for configuration-specific launchers/Steam shortcuts.
    /// Never interpreted as a shell command or sent as profile authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_root: Option<std::path::PathBuf>,
}

/// An explicitly selected desktop-user library location. For Steam this is
/// the directory containing steamapps; for Heroic its data/config directory;
/// for Lutris the directory containing pga.db; for Legendary the configuration
/// directory containing installed.json. It never grants service access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LibraryLocation {
    pub source: GameSource,
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub flatpak: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub snap: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl LibraryLocation {
    pub fn valid(&self) -> bool {
        self.path.is_absolute()
            && self.source != GameSource::Custom
            && !(self.flatpak && self.snap)
            && (!self.snap || self.source == GameSource::Steam)
            && (self.source != GameSource::Legendary || !self.flatpak)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Starting,
    Tracking,
    Ended,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameSession {
    pub token: String,
    pub game_id: String,
    pub requested_profile: crate::profile::ProfileKind,
    pub profile_held: bool,
    pub phase: SessionPhase,
    pub detail: String,
    pub started_unix: u64,
    pub updated_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSessionReport {
    pub sessions: Vec<GameSession>,
    pub recovery_error: Option<String>,
    pub mock: bool,
}

pub fn valid_game_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .split_once('/')
            .is_some_and(|(source, item)| !source.is_empty() && !item.is_empty())
        && !id.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileKind;

    #[test]
    fn existing_game_and_location_documents_keep_their_legacy_shape() {
        let text = r#"{"id":"steam/620","name":"Fixture","source":"steam","launch_cmd":""}"#;
        let game: Game = serde_json::from_str(text).unwrap();
        assert!(game.library_root.is_none());
        assert_eq!(
            serde_json::to_value(game).unwrap(),
            serde_json::from_str::<serde_json::Value>(text).unwrap()
        );
        let location: LibraryLocation =
            serde_json::from_str(r#"{"source":"steam","path":"/fixture","flatpak":false}"#)
                .unwrap();
        assert!(location.valid() && !location.snap);
        assert!(!serde_json::to_value(location)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("snap"));
    }

    #[test]
    fn library_installations_reject_unsupported_or_conflicting_routes() {
        let mut location = LibraryLocation {
            source: GameSource::Steam,
            path: "/fixture".into(),
            flatpak: false,
            snap: true,
        };
        assert!(location.valid());
        location.flatpak = true;
        assert!(!location.valid());
        location.flatpak = false;
        location.source = GameSource::Legendary;
        assert!(!location.valid());
        location.snap = false;
        assert!(location.valid());
        location.flatpak = true;
        assert!(!location.valid());
        location.flatpak = false;
        location.path = "relative".into();
        assert!(!location.valid());
    }

    #[test]
    fn game_serializes_with_source() {
        let g = Game {
            id: "steam/1091500".into(),
            name: "Cyberpunk 2077".into(),
            source: GameSource::Steam,
            launch_cmd: "steam steam://rungameid/1091500".into(),
            library_root: None,
        };
        let s = serde_json::to_string(&g).unwrap();
        assert!(s.contains("\"steam\""));
    }

    /// Compile-time check that per-game profiles reference the shared enum.
    #[test]
    fn per_game_profile_type_exists() {
        let p: Option<ProfileKind> = Some(ProfileKind::Performance);
        assert_eq!(
            p.map(|kind| kind.to_string()),
            Some("Performance".to_owned())
        );
    }
}
