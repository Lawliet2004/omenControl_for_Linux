//! Keyboard lighting vocabulary (zone RGB + brightness).
//!
//! omen-linux never drives lighting hardware directly; it integrates with
//! kernel interfaces (the `omen-rgb-keyboard` module sysfs or LED-class
//! backlight devices) when they exist, and reports the feature unavailable
//! otherwise.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const OFF: RgbColor = RgbColor { r: 0, g: 0, b: 0 };
    pub const WHITE: RgbColor = RgbColor {
        r: 255,
        g: 255,
        b: 255,
    };

    /// `RRGGBB` hex as consumed by the omen-rgb-keyboard sysfs interface.
    pub fn to_hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn from_hex(s: &str) -> Option<RgbColor> {
        let s = s.trim_start_matches('#');
        if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(RgbColor {
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: (v & 0xFF) as u8,
        })
    }
}

/// Snapshot of the lighting subsystem state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LightingState {
    /// Number of controllable zones (1 for single-zone, 4 for four-zone).
    pub zones: u8,
    pub colors: Vec<RgbColor>,
    /// 0..=100
    pub brightness_pct: u8,
    /// Animation mode name when the interface exposes one.
    pub animation: Option<String>,
}

impl LightingState {
    pub fn validate(&self) -> crate::Result<()> {
        if !(1..=4).contains(&self.zones)
            || self.colors.len() != self.zones as usize
            || self.brightness_pct > 100
        {
            return Err(crate::OmenError::InvalidArgument(
                "lighting needs 1–4 matching zone colors and brightness 0–100".into(),
            ));
        }
        if self.animation.as_ref().is_some_and(|a| {
            !["off", "static", "breathing", "rainbow", "chase"].contains(&a.as_str())
        }) {
            return Err(crate::OmenError::Unsupported(
                "unknown lighting animation".into(),
            ));
        }
        Ok(())
    }
}

/// A named, persisted lighting preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LightingPreset {
    pub name: String,
    pub state: LightingState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = RgbColor {
            r: 0x12,
            g: 0xAB,
            b: 0xFF,
        };
        assert_eq!(c.to_hex(), "12ABFF");
        assert_eq!(RgbColor::from_hex("12ABFF"), Some(c));
        assert_eq!(RgbColor::from_hex("#12abff"), Some(c));
        assert_eq!(RgbColor::from_hex("XYZ"), None);
        assert_eq!(RgbColor::from_hex("12345"), None);
    }
}
