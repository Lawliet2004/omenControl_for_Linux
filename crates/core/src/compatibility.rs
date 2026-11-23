//! Historical evidence is separate from runtime capability and authorization.
use crate::machine::MachineInfo;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompatibilityEvidence {
    pub classification: &'static str,
    pub observations: &'static str,
    pub profile_validation: &'static str,
    pub hardware_control_validation: &'static str,
    pub reference: &'static str,
}

pub fn unknown_origin() -> CompatibilityEvidence {
    CompatibilityEvidence {
        classification: "unverified_source",
        observations: "Source origin is unavailable; no historical physical result is attributed to these readings",
        profile_validation: "Physical validation cannot be inferred without verified source identity",
        hardware_control_validation: "No physical hardware control validation is established",
        reference: "HARDWARE_COMPATIBILITY.md",
    }
}

/// An exact historical target match may cite that observation. It never changes
/// a capability, authorizes a write, or transfers results to another firmware.
pub fn evidence(machine: &MachineInfo, fixture: bool) -> CompatibilityEvidence {
    let (classification, observations, profile_validation) = if fixture {
        (
            "fixture",
            "Development fixture; no physical observations established",
            "Fixture behavior only; no physical profile validation",
        )
    } else if machine.sys_vendor == "HP"
        && machine.product_name == "Victus by HP 15.6 inch Gaming Laptop PC-fb1002AX"
        && machine.board_name == "8C30"
        && machine.bios_version == "F.10"
        && machine.kernel_version == "7.0.0-38-generic"
    {
        ("documented_exact_target", "Read-only CPU/GPU, fan, memory/storage, battery/AC and network observations documented for this exact target",
            "One historical PPD Performance → Balanced → Performance transaction and independent readback passed; broader transitions, thermal behavior and current-session writes remain unverified")
    } else {
        ("runtime_discovery_only", "Runtime readings and interfaces do not establish physical validation for this model/BIOS/kernel combination",
            "No exact-target physical profile transaction is recorded in this application's evidence table")
    };
    CompatibilityEvidence {
        classification,
        observations,
        profile_validation,
        hardware_control_validation: "Fan/RGB/charging/MUX control and physical recovery are not validated by this evidence table",
        reference: "HARDWARE_COMPATIBILITY.md; PROFILE_VALIDATION_PLAN.md; evidence/live-profile-test.json",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> MachineInfo {
        MachineInfo {
            sys_vendor: "HP".into(),
            product_name: "Victus by HP 15.6 inch Gaming Laptop PC-fb1002AX".into(),
            board_name: "8C30".into(),
            bios_version: "F.10".into(),
            kernel_version: "7.0.0-38-generic".into(),
            ..Default::default()
        }
    }
    #[test]
    fn fixtures_and_nearby_firmware_never_inherit_physical_results() {
        let machine = reference();
        assert_eq!(evidence(&machine, true).classification, "fixture");
        assert_eq!(
            evidence(&machine, false).classification,
            "documented_exact_target"
        );
        for field in ["model", "board", "bios", "kernel", "vendor"] {
            let mut changed = machine.clone();
            match field {
                "model" => changed.product_name.push('X'),
                "board" => changed.board_name = "8C31".into(),
                "bios" => changed.bios_version = "F.11".into(),
                "kernel" => changed.kernel_version.push('X'),
                _ => changed.sys_vendor = "Other".into(),
            }
            assert_eq!(
                evidence(&changed, false).classification,
                "runtime_discovery_only"
            );
        }
    }
}
