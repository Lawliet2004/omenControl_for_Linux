//! Desktop integration with the separately installed Victus fan service.
//! This never invokes a shell or runs code from a user-writable location as root.
use omen_core::{fans::FanMode, OmenError, Result};
use omen_hardware::{detection, SysRoot};
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CONTROL: &str = "/usr/bin/victus-fanctl";

fn status_confirms(
    status: &serde_json::Value,
    requested: &str,
    expected: u64,
    started: f64,
) -> bool {
    status["ts"]
        .as_f64()
        .is_some_and(|timestamp| timestamp >= started)
        && status["override"].as_str() == Some(requested)
        && status["fan"]["pwm_enable"].as_u64() == Some(expected)
}

fn trusted_control() -> bool {
    std::fs::symlink_metadata(CONTROL).is_ok_and(|m| {
        m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0
    })
}

pub fn available() -> bool {
    let root = SysRoot::default();
    let machine = detection::detect_machine(&root);
    if machine.board_name != "8C30" || !trusted_control() {
        return false;
    }
    if detection::competing_fan_daemons(&root)
        .iter()
        .any(|name| name != "victus-fan.service")
    {
        return false;
    }
    omen_hardware::command::output(
        Command::new("/usr/bin/systemctl").args(["is-active", "--quiet", "victus-fan.service"]),
        Duration::from_millis(800),
    )
    .is_ok_and(|o| o.status.success())
}

pub fn set_mode(mode: FanMode) -> Result<()> {
    let (argument, expected) = match mode {
        FanMode::Auto => ("auto", 2),
        FanMode::Max => ("max", 0),
        _ => {
            return Err(OmenError::Unsupported(
                "Victus service supports Automatic and Maximum only".into(),
            ))
        }
    };
    if !available() {
        return Err(OmenError::SafetyLockout(
            "Victus fan service is unavailable or another fan controller is active".into(),
        ));
    }
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| OmenError::Io(e.to_string()))?
        .as_secs_f64();
    let result = omen_hardware::command::output(
        Command::new(CONTROL).args(["override", argument]),
        Duration::from_secs(5),
    )?;
    if !result.status.success() {
        return Err(OmenError::PermissionDenied(
            "Victus fan service rejected the request; check membership of the victusfan group"
                .into(),
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if !available() {
            return Err(OmenError::StateMismatch(
                "Victus fan service stopped or another controller appeared before confirmation"
                    .into(),
            ));
        }
        let status = std::fs::read("/run/victus-fan/status.json")
            .ok()
            .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok());
        let confirmed = status
            .as_ref()
            .is_some_and(|s| status_confirms(s, argument, expected, started));
        let readback = detection::find_hp_hwmon(&SysRoot::default()).and_then(|hp| hp.pwm_enable());
        if confirmed && readback == Some(expected as u8) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(OmenError::StateMismatch(
        "Mode was requested, but the service and hardware did not confirm it within eight seconds"
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_requires_fresh_matching_status_and_mode() {
        let status = serde_json::json!({"ts": 10.0, "override": "max", "fan": {"pwm_enable": 0}});
        assert!(status_confirms(&status, "max", 0, 9.0));
        assert!(!status_confirms(&status, "max", 0, 11.0));
        assert!(!status_confirms(&status, "auto", 0, 9.0));
        assert!(!status_confirms(&status, "max", 2, 9.0));
        assert!(!status_confirms(&serde_json::json!({}), "max", 0, 9.0));
    }
}
