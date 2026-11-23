//! Board allowlists mirroring `drivers/platform/x86/hp/hp-wmi.c` (GPL-2.0+,
//! upstream Linux). Facts only — board IDs and their firmware capabilities.
//!
//! Attribution: these lists are transcribed from the upstream kernel source
//! to explain *why* a given sysfs interface does or does not exist. The lists
//! are used for diagnostics and classification only; actual control gating
//! always defers to live sysfs presence, which is the authoritative signal.

/// Boards using the OMEN-specific thermal-profile firmware path.
pub const OMEN_THERMAL_BOARDS: &[&str] = &[
    "84DA", "84DB", "84DC", "8572", "8573", "8574", "8575", "8600", "8601", "8602", "8603", "8604",
    "8605", "8606", "8607", "860A", "8746", "8747", "8748", "8749", "874A", "8786", "8787", "8788",
    "878A", "878B", "878C", "87B5", "886B", "886C", "88C8", "88CB", "88D1", "88D2", "88F4", "88F5",
    "88F6", "88F7", "88FD", "88FE", "88FF", "8900", "8901", "8902", "8912", "8917", "8918", "8949",
    "894A", "89EB", "8A15", "8A42", "8A43", "8BAD", "8C58", "8E41",
];

/// Boards forced to the OMEN v0 thermal protocol.
pub const OMEN_FORCE_V0_BOARDS: &[&str] = &["8607", "8746", "8747", "8748", "8749", "874A"];

/// Boards whose EC thermal-profile timer resets to balanced when it expires.
pub const OMEN_TIMED_BOARDS: &[&str] = &["8A15", "8A42", "8BAD"];

/// Victus 16-d boards using the Victus thermal-profile path.
pub const VICTUS_THERMAL_BOARDS: &[&str] = &["88F8", "8A25"];

/// Boards with full thermal+fan parameter sets (thermal-profile capable).
pub const THERMAL_PARAM_BOARDS: &[&str] = &[
    // omen_v1_legacy
    "8902", "8A44", "8A4D", "8D26", "8E35", // victus_s
    "8A3D", "8B2F", "8BBE", "8BD4", "8BD5", "8C99", "8C9C", // omen_v1
    "8BAA", "8BA9", "8BAB", "8BC2", "8BCA", "8BCD", "8C76", "8C77", "8C78",
    // omen_v1_no_ec
    "8BB3", "8D41", "8D87", "8D88", "8DD6",
];

/// Boards for which the hp-wmi driver registers manual fan control
/// (`get_fan_speed` callback — the Victus S fan profile family).
pub const FAN_CONTROL_BOARDS: &[&str] = &["8A3D", "8B2F", "8BBE", "8BD4", "8BD5", "8C99", "8C9C"];

/// What the kernel driver is expected to provide for a board ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFanCapability {
    /// Board is allowlisted for manual fan control (`pwm1`/`pwm2` present).
    Manual,
    /// Board gets thermal profiles but not manual fan control.
    ThermalOnly,
    /// Board is unknown to the allowlists.
    Unknown,
}

pub fn board_fan_capability(board: &str) -> BoardFanCapability {
    if FAN_CONTROL_BOARDS.contains(&board) {
        BoardFanCapability::Manual
    } else if THERMAL_PARAM_BOARDS.contains(&board)
        || OMEN_THERMAL_BOARDS.contains(&board)
        || VICTUS_THERMAL_BOARDS.contains(&board)
    {
        BoardFanCapability::ThermalOnly
    } else {
        BoardFanCapability::Unknown
    }
}

/// True when the board is on any hp-wmi thermal-profile allowlist.
pub fn board_has_thermal_profile(board: &str) -> bool {
    board_fan_capability(board) != BoardFanCapability::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_victus_8c30_is_unknown() {
        // The reference laptop for this project: monitoring only.
        assert_eq!(board_fan_capability("8C30"), BoardFanCapability::Unknown);
    }

    #[test]
    fn victus_s_boards_are_manual() {
        for b in ["8A3D", "8C99"] {
            assert_eq!(board_fan_capability(b), BoardFanCapability::Manual);
        }
    }

    #[test]
    fn omen_boards_are_thermal_at_least() {
        assert!(board_has_thermal_profile("84DA"));
        assert!(board_has_thermal_profile("8BAA"));
        assert!(board_has_thermal_profile("88F8"));
        assert_eq!(
            board_fan_capability("8BAA"),
            BoardFanCapability::ThermalOnly
        );
    }

    #[test]
    fn tables_are_sorted_unique_where_relevant() {
        let mut sorted = OMEN_THERMAL_BOARDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), OMEN_THERMAL_BOARDS.len());
    }
}
