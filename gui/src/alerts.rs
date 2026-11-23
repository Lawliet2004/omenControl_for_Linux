//! User-opted informational notifications. This module never controls hardware.
use crate::data::Snapshot;
use gtk::prelude::*;
use omen_core::config::AlertConfig;
use omen_core::sensors::{SensorKind, SensorSample};
use std::collections::HashSet;

pub struct AlertMonitor {
    config: AlertConfig,
    max_age_ms: u64,
    alerted: HashSet<String>,
    last_notification_ms: Option<u64>,
}

impl AlertMonitor {
    pub fn new(config: AlertConfig, poll_ms: u64) -> Self {
        Self {
            config,
            max_age_ms: poll_ms.saturating_mul(3).clamp(7_000, 30_000),
            alerted: HashSet::new(),
            last_notification_ms: None,
        }
    }

    pub fn source_changed(&mut self) {
        self.alerted.clear();
    }

    pub fn configure(&mut self, config: AlertConfig) {
        if self.config != config {
            self.alerted.clear();
        }
        self.config = config;
    }

    pub fn set_poll_ms(&mut self, poll_ms: u64) {
        self.max_age_ms = poll_ms.saturating_mul(3).clamp(7_000, 30_000);
    }

    pub fn dispatch(&mut self, snapshot: &Snapshot, now: u64, source: &str) {
        if let Some((label, value)) =
            self.evaluate(snapshot.started_unix_ms, &snapshot.sensors, now)
        {
            if let Some(app) = gtk::gio::Application::default() {
                let notification = gtk::gio::Notification::new("Temperature notification");
                notification.set_body(Some(&format!(
                    "{source}\n{label}: {value:.0} °C. Informational only; not thermal protection."
                )));
                notification.set_default_action("app.diagnostics");
                app.send_notification(Some("temperature-alert"), &notification);
            }
        }
    }

    fn evaluate(
        &mut self,
        snapshot_time: u64,
        sensors: &[SensorSample],
        now: u64,
    ) -> Option<(String, f64)> {
        if !self.config.enabled || !fresh(snapshot_time, now, self.max_age_ms) {
            return None;
        }
        // First rearm sensors that have cooled, even during the global cooldown.
        for sensor in sensors {
            if sensor.kind != SensorKind::Temperature
                || !fresh(sensor.measured_unix_ms, now, self.max_age_ms)
            {
                continue;
            }
            if sensor.value.is_some_and(|value| {
                value.is_finite()
                    && value >= 0.0
                    && value
                        <= f64::from(
                            self.config
                                .temperature_c
                                .saturating_sub(self.config.hysteresis_c),
                        )
            }) {
                self.alerted.remove(&sensor.id);
            }
        }
        if self.last_notification_ms.is_some_and(|last| {
            now.checked_sub(last)
                .is_none_or(|age| age < u64::from(self.config.cooldown_seconds) * 1_000)
        }) {
            return None;
        }
        for sensor in sensors {
            let Some(value) = sensor.value else { continue };
            if sensor.kind == SensorKind::Temperature
                && fresh(sensor.measured_unix_ms, now, self.max_age_ms)
                && value.is_finite()
                && value >= f64::from(self.config.temperature_c)
                && !self.alerted.contains(&sensor.id)
            {
                self.alerted.insert(sensor.id.clone());
                self.last_notification_ms = Some(now);
                return Some((sensor.label.clone(), value));
            }
        }
        None
    }
}

fn fresh(time: u64, now: u64, max_age: u64) -> bool {
    time != 0 && now.checked_sub(time).is_some_and(|age| age <= max_age)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_core::sensors::SensorSource;
    fn sample(id: &str, value: Option<f64>, time: u64) -> SensorSample {
        let mut sensor =
            SensorSample::unavailable(id, id, SensorKind::Temperature, SensorSource::Mock);
        sensor.value = value;
        sensor.measured_unix_ms = time;
        sensor
    }
    fn monitor() -> AlertMonitor {
        AlertMonitor::new(
            AlertConfig {
                enabled: true,
                cooldown_seconds: 30,
                ..Default::default()
            },
            1000,
        )
    }
    #[test]
    fn stale_missing_future_and_invalid_samples_never_notify() {
        let mut alert = monitor();
        assert!(alert
            .evaluate(1, &[sample("a", Some(100.0), 10_000)], 10_000)
            .is_none());
        for sensor in [
            sample("a", None, 10_000),
            sample("a", Some(f64::NAN), 10_000),
            sample("a", Some(100.0), 1),
            sample("a", Some(100.0), 10_001),
        ] {
            assert!(alert.evaluate(10_000, &[sensor], 10_000).is_none());
        }
        assert!(alert
            .evaluate(10_000, &[sample("a", Some(100.0), 10_000)], 10_000)
            .is_some());
    }
    #[test]
    fn hysteresis_and_global_rate_limit_allow_a_pending_alert_after_cooldown() {
        let mut alert = monitor();
        assert!(alert
            .evaluate(1_000, &[sample("a", Some(90.0), 1_000)], 1_000)
            .is_some());
        assert!(alert
            .evaluate(
                2_000,
                &[
                    sample("a", Some(86.0), 2_000),
                    sample("b", Some(95.0), 2_000)
                ],
                2_000
            )
            .is_none());
        assert!(alert
            .evaluate(
                31_000,
                &[
                    sample("a", Some(86.0), 31_000),
                    sample("b", Some(95.0), 31_000)
                ],
                31_000
            )
            .is_some());
        assert!(alert
            .evaluate(61_000, &[sample("a", Some(90.0), 61_000)], 61_000)
            .is_none());
        assert!(alert
            .evaluate(62_000, &[sample("a", Some(85.0), 62_000)], 62_000)
            .is_none());
        assert!(alert
            .evaluate(63_000, &[sample("a", Some(90.0), 63_000)], 63_000)
            .is_some());
    }
    #[test]
    fn disabled_and_reconfigured_monitor_is_opt_in() {
        let mut alert = AlertMonitor::new(AlertConfig::default(), 1000);
        assert!(alert
            .evaluate(1_000, &[sample("a", Some(100.0), 1_000)], 1_000)
            .is_none());
        alert.configure(AlertConfig {
            enabled: true,
            ..Default::default()
        });
        assert!(alert
            .evaluate(1_000, &[sample("a", Some(100.0), 1_000)], 1_000)
            .is_some());
    }
}
