//! Machine identification and classification from DMI/SMBIOS data.

use serde::{Deserialize, Serialize};

/// Broad hardware family classification used for capability tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineClass {
    /// HP OMEN with the OMEN thermal-profile firmware path (hp-wmi `omen_*` board lists).
    Omen,
    /// HP Victus with the Victus thermal-profile firmware path.
    Victus,
    /// Victus "S" boards — the family hp-wmi allowlists for
    /// manual fan control.
    VictusS,
    /// An HP machine not in any known allowlist. Read-only mode unless sysfs
    /// evidence proves otherwise.
    OtherHp,
    /// Not HP hardware at all: monitoring only.
    NonHp,
}

impl MachineClass {
    pub fn is_hp(self) -> bool {
        !matches!(self, MachineClass::NonHp)
    }
}

/// DMI identity of the running machine, mirrored from `/sys/class/dmi/id/*`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MachineInfo {
    pub sys_vendor: String,
    pub product_name: String,
    pub product_family: String,
    pub board_name: String,
    pub board_vendor: String,
    pub bios_version: String,
    pub bios_vendor: String,
    pub kernel_version: String,
    pub class: MachineClass,
}

impl MachineInfo {
    /// Classify a machine from its DMI strings.
    ///
    /// Evidence: HP product-family strings observed live are
    /// `103C_5335M7 HP Victus` (Victus) and OMEN families follow
    /// `... HP OMEN`/OMEN Transcend patterns. Board-level allowlists from
    /// drivers/platform/x86/hp/hp-wmi.c refine the class (see
    /// `omen-hardware::detection::board_tables`).
    pub fn classify(&self) -> MachineClass {
        let family = self.product_family.to_lowercase();
        let product = self.product_name.to_lowercase();
        if !self.sys_vendor.eq_ignore_ascii_case("HP")
            && !self.board_vendor.eq_ignore_ascii_case("HP")
        {
            return MachineClass::NonHp;
        }
        if family.contains("victus") || product.contains("victus") {
            MachineClass::Victus
        } else if family.contains("omen") || product.contains("omen") {
            MachineClass::Omen
        } else {
            MachineClass::OtherHp
        }
    }
}

impl Default for MachineInfo {
    fn default() -> Self {
        MachineInfo {
            sys_vendor: String::new(),
            product_name: String::new(),
            product_family: String::new(),
            board_name: String::new(),
            board_vendor: String::new(),
            bios_version: String::new(),
            bios_vendor: String::new(),
            kernel_version: String::new(),
            class: MachineClass::NonHp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn victus_fb() -> MachineInfo {
        MachineInfo {
            sys_vendor: "HP".into(),
            product_name: "Victus by HP 15.6 inch Gaming Laptop PC-fb1002AX".into(),
            product_family: "103C_5335M7 HP Victus".into(),
            board_name: "8C30".into(),
            board_vendor: "HP".into(),
            bios_version: "F.10".into(),
            bios_vendor: "AMI".into(),
            kernel_version: "7.0.0-38-generic".into(),
            class: MachineClass::Victus,
        }
    }

    #[test]
    fn classifies_reference_victus() {
        let m = victus_fb();
        assert_eq!(m.classify(), MachineClass::Victus);
    }

    #[test]
    fn classifies_omen_by_family() {
        let m = MachineInfo {
            product_family: "103C_5336NF HP OMEN".into(),
            sys_vendor: "HP".into(),
            ..MachineInfo::default()
        };
        assert_eq!(m.classify(), MachineClass::Omen);
    }

    #[test]
    fn classifies_non_hp() {
        let m = MachineInfo {
            sys_vendor: "Lenovo".into(),
            ..MachineInfo::default()
        };
        assert_eq!(m.classify(), MachineClass::NonHp);
    }

    #[test]
    fn classifies_other_hp() {
        let m = MachineInfo {
            sys_vendor: "HP".into(),
            product_name: "HP EliteBook 840".into(),
            ..MachineInfo::default()
        };
        assert_eq!(m.classify(), MachineClass::OtherHp);
    }
}
