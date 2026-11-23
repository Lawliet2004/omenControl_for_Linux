//! Performance profile vocabulary shared between platform_profile,
//! power-profiles-daemon and the UI.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Normalized performance profile. Mapping to concrete backends:
///
/// * `platform_profile` sysfs: `quiet`/`low-power` → Quiet, `balanced` →
///   Balanced, `performance` → Performance, `cool` → Cool.
/// * power-profiles-daemon D-Bus: `power-saver` → Quiet, `balanced` →
///   Balanced, `performance` → Performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Quiet,
    Balanced,
    Cool,
    Performance,
}

impl ProfileKind {
    /// Canonical `platform_profile` name where applicable.
    pub fn platform_profile_name(self) -> Option<&'static str> {
        match self {
            ProfileKind::Quiet => Some("quiet"),
            ProfileKind::Balanced => Some("balanced"),
            ProfileKind::Cool => Some("cool"),
            ProfileKind::Performance => Some("performance"),
        }
    }

    /// Canonical power-profiles-daemon name where applicable.
    pub fn ppd_name(self) -> Option<&'static str> {
        match self {
            ProfileKind::Quiet => Some("power-saver"),
            ProfileKind::Balanced => Some("balanced"),
            ProfileKind::Performance => Some("performance"),
            ProfileKind::Cool => None,
        }
    }

    pub fn all() -> [ProfileKind; 4] {
        [
            ProfileKind::Quiet,
            ProfileKind::Balanced,
            ProfileKind::Cool,
            ProfileKind::Performance,
        ]
    }
}

impl fmt::Display for ProfileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ProfileKind::Quiet => "Quiet",
            ProfileKind::Balanced => "Balanced",
            ProfileKind::Cool => "Cool",
            ProfileKind::Performance => "Performance",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_backend_names() {
        assert_eq!(ProfileKind::Quiet.platform_profile_name(), Some("quiet"));
        assert_eq!(ProfileKind::Quiet.ppd_name(), Some("power-saver"));
        assert_eq!(ProfileKind::Cool.ppd_name(), None);
    }

    #[test]
    fn json_roundtrip() {
        let p = ProfileKind::Performance;
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"performance\"");
        let back: ProfileKind = serde_json::from_str("\"performance\"").unwrap();
        assert_eq!(back, p);
    }
}
