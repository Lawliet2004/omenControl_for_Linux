//! Keyboard lighting integration.
//!
//! omen-linux never drives lighting hardware with hand-rolled protocol
//! writes. It integrates two kernel surfaces when present:
//!
//! 1. The `omen-rgb-keyboard` community module
//!    (`/sys/devices/platform/omen-rgb-keyboard/rgb_zones/{zone00..zoneNN,all,
//!    brightness,animation_mode}`) — zone RGB.
//! 2. LED class `*::kbd_backlight*` — plain backlight brightness.
//!
//! Both are detectable, documented interfaces; absence means the feature is
//! honestly unavailable.

use crate::fsutil;
use crate::SysRoot;
use omen_core::lighting::{LightingState, RgbColor};
use omen_core::{OmenError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum LightingInterface {
    OmenRgbModule { base: PathBuf, zones: u8 },
    LedBacklight(PathBuf),
    None,
}

impl LightingInterface {
    pub fn detect(root: &SysRoot) -> LightingInterface {
        let base = root.join("sys/devices/platform/omen-rgb-keyboard/rgb_zones");
        if base.exists() {
            let mut zones = 0u8;
            for i in 0..8 {
                if base.join(format!("zone{i:02}")).exists() {
                    zones += 1;
                }
            }
            if zones > 0 {
                return LightingInterface::OmenRgbModule { base, zones };
            }
        }
        // LED class backlight.
        let leds = root.sysfs().join("class/leds");
        if let Ok(entries) = std::fs::read_dir(&leds) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .collect();
            names.sort();
            for n in names {
                if n.contains("kbd_backlight") {
                    let p = leds.join(&n);
                    if p.join("brightness").exists() {
                        return LightingInterface::LedBacklight(p);
                    }
                }
            }
        }
        LightingInterface::None
    }

    pub fn read(&self) -> Result<LightingState> {
        match self {
            LightingInterface::OmenRgbModule { base, zones } => {
                let mut colors = Vec::new();
                for i in 0..*zones {
                    let hex = fsutil::read_trim(&base.join(format!("zone{i:02}")));
                    colors.push(hex.as_deref().and_then(RgbColor::from_hex).ok_or_else(|| {
                        OmenError::Io(format!("zone{i:02} unreadable or invalid"))
                    })?);
                }
                let brightness = fsutil::read_u64(&base.join("brightness"))
                    .map(|b| b.clamp(0, 100) as u8)
                    .unwrap_or(100);
                let animation = fsutil::read_trim(&base.join("animation_mode"));
                Ok(LightingState {
                    zones: *zones,
                    colors,
                    brightness_pct: brightness,
                    animation,
                })
            }
            LightingInterface::LedBacklight(p) => {
                let cur = fsutil::read_u64(&p.join("brightness")).unwrap_or(0);
                let max = fsutil::read_u64(&p.join("max_brightness"))
                    .unwrap_or(1)
                    .max(1);
                Ok(LightingState {
                    zones: 1,
                    colors: vec![RgbColor::WHITE],
                    brightness_pct: ((cur * 100) / max).clamp(0, 100) as u8,
                    animation: None,
                })
            }
            LightingInterface::None => Err(OmenError::Unsupported(
                "no keyboard-lighting interface on this machine".into(),
            )),
        }
    }

    pub fn write(&self, st: &LightingState) -> Result<()> {
        st.validate()?;
        match self {
            LightingInterface::OmenRgbModule { base, zones } => {
                if st.zones != *zones || st.colors.len() != *zones as usize {
                    return Err(OmenError::InvalidArgument(format!(
                        "expected {zones} zone colors, got {}",
                        st.colors.len()
                    )));
                }
                if st
                    .animation
                    .as_deref()
                    .is_some_and(|a| a != "static" && a != "off")
                {
                    return Err(OmenError::Unsupported(
                        "animation writes are not validated for this adapter".into(),
                    ));
                }
                for (i, c) in st.colors.iter().enumerate() {
                    let p = base.join(format!("zone{i:02}"));
                    fsutil::write(&p, &c.to_hex())
                        .map_err(|e| OmenError::Io(format!("zone{i:02}: {e}")))?;
                }
                fsutil::write(&base.join("brightness"), &st.brightness_pct.to_string())
                    .map_err(|e| OmenError::Io(format!("brightness: {e}")))?;
                // Readback.
                let now = self.read()?;
                if now.colors != st.colors || now.brightness_pct != st.brightness_pct {
                    return Err(OmenError::StateMismatch(
                        "lighting state readback differs from request".into(),
                    ));
                }
                Ok(())
            }
            LightingInterface::LedBacklight(p) => {
                let max = fsutil::read_u64(&p.join("max_brightness"))
                    .ok_or_else(|| OmenError::Io("max_brightness unreadable".into()))?
                    .max(1);
                let target = ((st.brightness_pct as u64) * max) / 100;
                fsutil::write(&p.join("brightness"), &target.to_string())
                    .map_err(|e| OmenError::Io(format!("brightness: {e}")))?;
                let got = fsutil::read_u64(&p.join("brightness")).unwrap_or(0);
                if got != target {
                    return Err(OmenError::StateMismatch(format!(
                        "brightness readback {got} != {target}"
                    )));
                }
                Ok(())
            }
            LightingInterface::None => Err(OmenError::Unsupported(
                "no keyboard-lighting interface on this machine".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_fixture(zones: usize) -> (tempfile::TempDir, SysRoot) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir
            .path()
            .join("sys/devices/platform/omen-rgb-keyboard/rgb_zones");
        std::fs::create_dir_all(&base).unwrap();
        for i in 0..zones {
            std::fs::write(base.join(format!("zone{i:02}")), "FF0000\n").unwrap();
        }
        std::fs::write(base.join("brightness"), "80\n").unwrap();
        let root = SysRoot::new(dir.path().to_path_buf());
        (dir, root)
    }

    #[test]
    fn detects_module_and_reads() {
        let (_d, root) = rgb_fixture(4);
        let li = LightingInterface::detect(&root);
        match &li {
            LightingInterface::OmenRgbModule { zones, .. } => assert_eq!(*zones, 4),
            other => panic!("expected module, got {other:?}"),
        }
        let st = li.read().unwrap();
        assert_eq!(st.zones, 4);
        assert_eq!(st.colors[0], RgbColor { r: 255, g: 0, b: 0 });
        assert_eq!(st.brightness_pct, 80);
    }

    #[test]
    fn writes_and_verifies() {
        let (_d, root) = rgb_fixture(4);
        let li = LightingInterface::detect(&root);
        let st = LightingState {
            zones: 4,
            colors: vec![
                RgbColor { r: 0, g: 255, b: 0 },
                RgbColor { r: 0, g: 255, b: 0 },
                RgbColor { r: 0, g: 0, b: 255 },
                RgbColor {
                    r: 255,
                    g: 255,
                    b: 0,
                },
            ],
            brightness_pct: 50,
            animation: None,
        };
        li.write(&st).unwrap();
        let back = li.read().unwrap();
        assert_eq!(back.colors, st.colors);
        assert_eq!(back.brightness_pct, 50);
    }

    #[test]
    fn wrong_zone_count_rejected() {
        let (_d, root) = rgb_fixture(4);
        let li = LightingInterface::detect(&root);
        let st = LightingState {
            zones: 1,
            colors: vec![RgbColor::WHITE],
            brightness_pct: 100,
            animation: None,
        };
        assert!(matches!(li.write(&st), Err(OmenError::InvalidArgument(_))));
    }

    #[test]
    fn none_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::new(dir.path());
        let li = LightingInterface::detect(&root);
        assert!(matches!(li, LightingInterface::None));
        assert!(matches!(li.read(), Err(OmenError::Unsupported(_))));
    }

    #[test]
    fn led_backlight_detected_and_written() {
        let dir = tempfile::tempdir().unwrap();
        let led = dir.path().join("sys/class/leds/platform::kbd_backlight");
        std::fs::create_dir_all(&led).unwrap();
        std::fs::write(led.join("brightness"), "1\n").unwrap();
        std::fs::write(led.join("max_brightness"), "4\n").unwrap();
        let root = SysRoot::new(dir.path());
        let li = LightingInterface::detect(&root);
        assert!(matches!(li, LightingInterface::LedBacklight(_)));
        let st = li.read().unwrap();
        assert_eq!(st.brightness_pct, 25);
        let want = LightingState {
            zones: 1,
            colors: vec![RgbColor::WHITE],
            brightness_pct: 100,
            animation: None,
        };
        li.write(&want).unwrap();
        assert_eq!(
            fsutil::read_trim(&led.join("brightness")).as_deref(),
            Some("4")
        );
    }
}
