//! Page builders for the nine main screens. Every hardware control checks
//! `Capabilities` first; unsupported features render disabled with the
//! honest reason — never as a working control.

use crate::charts::{chart, push, Chart, History, Series};
use crate::data::{DataSource, Snapshot};
use crate::i18n::tr;
use gtk::prelude::*;
use omen_core::capability::Capabilities;

use omen_core::profile::ProfileKind;
use omen_core::sensors::SensorKind;
use std::cell::RefCell;
use std::rc::Rc;

#[allow(dead_code)]
pub const ACCENT: (f64, f64, f64) = (0.90, 0.28, 0.30);
pub const CPU_COLOR: (f64, f64, f64) = (0.90, 0.28, 0.30);
pub const GPU_COLOR: (f64, f64, f64) = (0.30, 0.72, 0.47);
pub const FAN1_COLOR: (f64, f64, f64) = (0.36, 0.58, 0.92);
pub const FAN2_COLOR: (f64, f64, f64) = (0.94, 0.69, 0.25);

struct SnapshotUpdater {
    page: gtk::glib::WeakRef<gtk::Widget>,
    update: Rc<dyn Fn(&Snapshot)>,
}

pub struct Ui {
    pub source: DataSource,
    pub source_label: &'static str,
    pub(crate) fixture: Option<bool>,
    pub latest_caps: RefCell<Capabilities>,
    pub history: History,
    updaters: RefCell<Vec<SnapshotUpdater>>,
    pub charts: RefCell<Vec<Chart>>,
    pub status_label: gtk::Label,
    pub connection_label: gtk::Label,
    pub tray_status: gtk::Label,
    pub latest_profile: std::cell::Cell<Option<ProfileKind>>,
    latest_snapshot: Rc<RefCell<Option<Snapshot>>>,
    snapshot_expired: std::cell::Cell<bool>,
    alerts: RefCell<crate::alerts::AlertMonitor>,
}

impl Ui {
    /// Authorization and hardware calls can take seconds. Keep GTK's main
    /// thread responsive and serialize actions so successive clicks cannot race.
    pub(crate) fn run_action(
        &self,
        label: String,
        action: impl FnOnce(DataSource) -> omen_core::Result<()> + Send + 'static,
    ) {
        self.run_task(label, move |source| {
            action(source).map(|()| "complete".to_owned())
        });
    }

    /// Hardware operations require a recent capability report, including at startup.
    pub(crate) fn run_hardware_action(
        &self,
        label: String,
        action: impl FnOnce(DataSource) -> omen_core::Result<()> + Send + 'static,
    ) {
        if self.snapshot_expired.get() {
            self.status_label.set_text(STALE_REASON);
            return;
        }
        self.run_action(label, action);
    }

    pub(crate) fn run_hardware_task(
        &self,
        label: String,
        action: impl FnOnce(DataSource) -> omen_core::Result<String> + Send + 'static,
    ) {
        if self.snapshot_expired.get() {
            self.status_label.set_text(STALE_REASON);
            return;
        }
        self.run_task(label, action);
    }

    pub(crate) fn run_task(
        &self,
        label: String,
        action: impl FnOnce(DataSource) -> omen_core::Result<String> + Send + 'static,
    ) {
        static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
            self.status_label
                .set_text(&tr("Another action is still running."));
            return;
        }
        self.status_label.set_text(&format!("{label}…"));
        let source = self.source.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(source)))
                .unwrap_or_else(|_| {
                    Err(omen_core::OmenError::Internal(
                        "action failed unexpectedly".into(),
                    ))
                });
            let _ = tx.send(result);
            BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
        });
        let status = self.status_label.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            match rx.try_recv() {
                Ok(result) => {
                    status.set_text(&match result {
                        Ok(message) => format!("{label}: {message}"),
                        Err(e) => format!("{label}: {e}"),
                    });
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
                Err(_) => gtk::glib::ControlFlow::Continue,
            }
        });
    }
    pub fn new(source: DataSource) -> Ui {
        let caps = source.capabilities();
        let fixture = source.fixture().ok();
        let source_label = match (&source, fixture) {
            (DataSource::Daemon(_), Some(true)) => "MOCK — fixture data, no real hardware",
            (DataSource::Daemon(_), Some(false)) => "omen-daemon + local read-only GPU fallback",
            (DataSource::Daemon(_), None) => "service source unverified",
            (DataSource::Direct(_), _) => source.label(),
        };
        Ui {
            source,
            source_label,
            fixture,
            latest_caps: RefCell::new(expired_capabilities(&caps)),
            history: History::new(
                omen_core::config::OmenConfig::load()
                    .map(|config| config.general.history_samples)
                    .unwrap_or(300),
                omen_core::config::OmenConfig::load()
                    .map(|config| config.general.poll_interval_ms)
                    .unwrap_or(1000),
            ),
            updaters: RefCell::new(Vec::new()),
            charts: RefCell::new(Vec::new()),
            status_label: gtk::Label::builder()
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .halign(gtk::Align::Start)
                .build(),
            connection_label: gtk::Label::builder()
                .wrap(true)
                .halign(gtk::Align::Start)
                .build(),
            tray_status: gtk::Label::builder()
                .label(tr("Connecting to system tray…"))
                .wrap(true)
                .xalign(0.0)
                .build(),
            latest_profile: std::cell::Cell::new(None),
            latest_snapshot: Rc::new(RefCell::new(None)),
            snapshot_expired: std::cell::Cell::new(true),
            alerts: RefCell::new(crate::alerts::AlertMonitor::new(
                omen_core::config::OmenConfig::load()
                    .map(|c| c.general.alerts)
                    .unwrap_or_default(),
                omen_core::config::OmenConfig::load()
                    .map(|c| c.general.poll_interval_ms)
                    .unwrap_or(1000),
            )),
        }
    }

    pub(crate) fn register(&self, page: &impl IsA<gtk::Widget>, f: impl Fn(&Snapshot) + 'static) {
        let page = page.as_ref();
        let f: Rc<dyn Fn(&Snapshot)> = Rc::new(f);
        let snapshot = self.latest_snapshot.clone();
        let on_map = f.clone();
        page.connect_map(move |_| {
            if let Some(snapshot) = snapshot.borrow().as_ref() {
                on_map(snapshot);
            }
        });
        let mut updaters = self.updaters.borrow_mut();
        // Recycling can continue through a prolonged polling outage. Reclaim
        // dead registrations without requiring another snapshot to arrive.
        if updaters.len().is_multiple_of(64) {
            updaters.retain(|entry| entry.page.upgrade().is_some());
        }
        updaters.push(SnapshotUpdater {
            page: page.downgrade(),
            update: f,
        });
    }

    fn update_pages(&self, snapshot: &Snapshot) {
        // Library rescans replace rows. Prune their callbacks instead of
        // retaining every historical scan for the rest of the application.
        let updates: Vec<_> = {
            let mut registered = self.updaters.borrow_mut();
            registered.retain(|entry| entry.page.upgrade().is_some());
            registered
                .iter()
                .filter(|entry| entry.page.upgrade().is_some_and(|page| page.is_mapped()))
                .map(|entry| entry.update.clone())
                .collect()
        };
        for update in updates {
            update(snapshot);
        }
    }

    fn add_chart(
        &self,
        title: &str,
        series: Vec<Series>,
        min: Option<f64>,
        max: Option<f64>,
    ) -> gtk::Box {
        let chart = chart(title, self.history.clone(), series, min, max);
        let widget = chart.widget.clone();
        self.charts.borrow_mut().push(chart);
        widget
    }

    pub fn refresh_charts(&self) {
        for chart in self.charts.borrow().iter() {
            chart.refresh();
        }
    }

    /// Invalidate displayed readings when the polling worker has stopped making
    /// progress. This deliberately does not append a sample to history.
    /// Returns true only when the state changes, so the caller can update the
    /// tray and connection status once per outage.
    pub fn expire_snapshot(&self) -> bool {
        if self.snapshot_expired.replace(true) {
            return false;
        }
        let Some(snapshot) = self.latest_snapshot.borrow().as_ref().map(expired_snapshot) else {
            return false;
        };
        *self.latest_caps.borrow_mut() = snapshot.capabilities.clone();
        *self.latest_snapshot.borrow_mut() = Some(snapshot.clone());
        self.latest_profile.set(None);
        self.update_pages(&snapshot);
        true
    }

    pub fn dispatch(&self, snap: &Snapshot) {
        self.alerts
            .borrow_mut()
            .dispatch(snap, crate::overlay::now_ms(), self.source_label);
        self.snapshot_expired.set(false);
        *self.latest_snapshot.borrow_mut() = Some(snap.clone());
        *self.latest_caps.borrow_mut() = snap.capabilities.clone();
        self.latest_profile.set(snap.profile);
        let frame = crate::overlay::now_ms();
        let sensor_reading = |sensor: &omen_core::sensors::SensorSample| {
            sensor
                .value
                .map(|value| (sensor.measured_unix_ms, value, sensor.source.to_string()))
        };
        let gpu = select_gpu_temp(snap, frame, sensor_max_age());
        let cpu = select_cpu_temp(snap, frame, sensor_max_age());
        push(
            &self.history,
            "gpu_temp",
            frame,
            gpu.and_then(sensor_reading),
        );
        push(
            &self.history,
            "cpu_temp",
            frame,
            cpu.and_then(sensor_reading),
        );
        let snapshot_reading = |value: Option<f64>| {
            value.map(|v| (snap.started_unix_ms, v, "snapshot time".to_string()))
        };
        push(
            &self.history,
            "fan1",
            frame,
            snapshot_reading(snap.fan.as_ref().and_then(|f| f.cpu_rpm).map(|v| v as f64)),
        );
        push(
            &self.history,
            "fan2",
            frame,
            snapshot_reading(snap.fan.as_ref().and_then(|f| f.gpu_rpm).map(|v| v as f64)),
        );
        push(
            &self.history,
            "duty",
            frame,
            snapshot_reading(snap.fan.as_ref().and_then(|f| f.duty_pct).map(f64::from)),
        );
        push(
            &self.history,
            "batt",
            frame,
            snapshot_reading(
                snap.battery
                    .as_ref()
                    .filter(|b| b.present)
                    .and_then(|b| b.capacity_pct)
                    .map(f64::from),
            ),
        );
        self.update_pages(snap);
        self.refresh_charts();
    }
}

const STALE_REASON: &str = "Monitoring data expired; waiting for a new update";

fn expired_capabilities(caps: &Capabilities) -> Capabilities {
    use omen_core::capability::Capability;
    let mut expired = caps.clone();
    let unavailable = || Capability::unavailable(STALE_REASON);
    expired.fan_monitoring = unavailable();
    expired.fan_modes = unavailable();
    expired.fan_control = unavailable();
    expired.fan_curve = unavailable();
    expired.performance_profiles = unavailable();
    expired.profile_choices.clear();
    expired.lighting_zones = unavailable();
    expired.keyboard_backlight = unavailable();
    expired.graphics_mux = unavailable();
    expired.charge_threshold = unavailable();
    expired.battery_aware_profiles = unavailable();
    expired.nvidia_telemetry = unavailable();
    expired.amd_telemetry = unavailable();
    expired
}

fn expired_snapshot(last: &Snapshot) -> Snapshot {
    Snapshot {
        controls: Default::default(),
        started_unix_ms: last.started_unix_ms,
        network: None,
        network_error: Some(STALE_REASON.into()),
        capabilities: expired_capabilities(&last.capabilities),
        sensors: Vec::new(),
        fan: None,
        fan_err: Some(STALE_REASON.into()),
        profile: None,
        battery: None,
        ac_online: None,
        error: Some(STALE_REASON.into()),
    }
}

fn lookup(snap: &Snapshot, id: &str) -> Option<f64> {
    snap.sensors
        .iter()
        .find(|s| s.id == id && sensor_is_fresh(s, crate::overlay::now_ms(), sensor_max_age()))
        .and_then(|s| s.value)
}

fn sensor_max_age() -> u64 {
    static MAX_AGE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *MAX_AGE.get_or_init(|| {
        omen_core::config::OmenConfig::load()
            .map(|c| c.general.poll_interval_ms)
            .unwrap_or(1000)
            .saturating_mul(3)
            .clamp(7_000, 30_000)
    })
}

fn sensor_is_fresh(sensor: &omen_core::sensors::SensorSample, now: u64, max_age: u64) -> bool {
    sensor.measured_unix_ms != 0
        && now
            .checked_sub(sensor.measured_unix_ms)
            .is_some_and(|age| age <= max_age)
        && sensor.value.is_some_and(f64::is_finite)
}

fn select_cpu_temp(
    snap: &Snapshot,
    now: u64,
    max_age: u64,
) -> Option<&omen_core::sensors::SensorSample> {
    snap.sensors
        .iter()
        .filter(|s| s.kind == SensorKind::Temperature && sensor_is_fresh(s, now, max_age))
        .filter(|s| {
            s.label.contains("CPU")
                || s.label.contains("Tctl")
                || s.label.contains("k10temp")
                || s.label.contains("Package")
        })
        .max_by(|a, b| a.value.unwrap().total_cmp(&b.value.unwrap()))
}

fn select_gpu_temp(
    snap: &Snapshot,
    now: u64,
    max_age: u64,
) -> Option<&omen_core::sensors::SensorSample> {
    snap.sensors
        .iter()
        .find(|s| {
            s.id == "nvidia/temp"
                && s.kind == SensorKind::Temperature
                && sensor_is_fresh(s, now, max_age)
        })
        .or_else(|| {
            snap.sensors.iter().find(|s| {
                s.kind == SensorKind::Temperature
                    && s.label.contains("GPU (AMD)")
                    && sensor_is_fresh(s, now, max_age)
            })
        })
}

fn cpu_freq(snap: &Snapshot) -> Option<f64> {
    lookup(snap, "cpu/avg_freq")
}

// ------------------------------------------------------------- widgets --

fn section(title: &str) -> gtk::Box {
    let v = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    let lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["section-title"])
        .build();
    v.append(&lbl);
    v
}

/// Reflows metric cards as the content width changes, including in split views.
fn metric_flow(max_columns: u32) -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(max_columns)
        .column_spacing(12)
        .row_spacing(12)
        .homogeneous(true)
        .hexpand(true)
        .build()
}

fn value_card(title: &str) -> (gtk::Widget, gtk::Label) {
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .css_classes(["card"])
        .hexpand(true)
        .build();
    let t = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["card-title"])
        .build();
    let v = gtk::Label::builder()
        .label(tr("—"))
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["card-value"])
        .build();
    vbox.append(&t);
    vbox.append(&v);
    (vbox.upcast(), v)
}

// --------------------------------------------------------------- pages --

use omen_core::config::DashboardMetric;

const DASHBOARD_METRICS: [(DashboardMetric, &str); 6] = [
    (DashboardMetric::CpuTemperature, "CPU temperature"),
    (DashboardMetric::GpuTemperature, "GPU temperature"),
    (DashboardMetric::CpuFan, "CPU fan"),
    (DashboardMetric::GpuFan, "GPU fan"),
    (DashboardMetric::GpuPower, "GPU power"),
    (DashboardMetric::Battery, "Battery"),
];

fn dashboard_selection(
    current: &[DashboardMetric],
    metric: DashboardMetric,
    enabled: bool,
) -> Vec<DashboardMetric> {
    DASHBOARD_METRICS
        .iter()
        .map(|(candidate, _)| *candidate)
        .filter(|candidate| {
            if *candidate == metric {
                enabled
            } else {
                current.contains(candidate)
            }
        })
        .collect()
}

fn dashboard_sensor(
    snap: &Snapshot,
    metric: DashboardMetric,
    now: u64,
) -> Option<&omen_core::sensors::SensorSample> {
    let fresh = |s: &&omen_core::sensors::SensorSample| sensor_is_fresh(s, now, sensor_max_age());
    match metric {
        DashboardMetric::CpuTemperature => {
            select_cpu_temp(snap, now, sensor_max_age()).or_else(|| {
                snap.sensors.iter().find(|s| {
                    s.kind == SensorKind::Temperature
                        && (s.label.contains("CPU")
                            || s.label.contains("Tctl")
                            || s.label.contains("k10temp")
                            || s.label.contains("Package"))
                })
            })
        }
        DashboardMetric::GpuTemperature => {
            select_gpu_temp(snap, now, sensor_max_age()).or_else(|| {
                snap.sensors.iter().find(|s| {
                    s.kind == SensorKind::Temperature
                        && (s.id == "nvidia/temp" || s.label.contains("GPU (AMD)"))
                })
            })
        }
        DashboardMetric::GpuPower => snap
            .sensors
            .iter()
            .filter(|s| {
                s.kind == SensorKind::Power
                    && (s.id == "nvidia/power" || s.label.contains("GPU (AMD)"))
            })
            .find(fresh)
            .or_else(|| {
                snap.sensors.iter().find(|s| {
                    s.kind == SensorKind::Power
                        && (s.id == "nvidia/power" || s.label.contains("GPU (AMD)"))
                })
            }),
        _ => None,
    }
}

fn dashboard_reading(snap: &Snapshot, metric: DashboardMetric, now: u64) -> (String, String) {
    if snap.error.as_deref() == Some(STALE_REASON) {
        return ("Unavailable".into(), "Monitoring update expired".into());
    }
    if matches!(
        metric,
        DashboardMetric::CpuTemperature
            | DashboardMetric::GpuTemperature
            | DashboardMetric::GpuPower
    ) {
        return match dashboard_sensor(snap, metric, now) {
            Some(sensor) if sensor_is_fresh(sensor, now, sensor_max_age()) => {
                let unit = if metric == DashboardMetric::GpuPower {
                    "W"
                } else {
                    "°C"
                };
                let age = now.saturating_sub(sensor.measured_unix_ms) / 1000;
                (
                    format!("{:.1} {unit}", sensor.value.unwrap()),
                    format!("{} · {age}s ago", sensor.source),
                )
            }
            Some(sensor) if sensor.value.is_some_and(f64::is_finite) => (
                "Stale".into(),
                format!("{} · reading expired", sensor.source),
            ),
            Some(sensor) => (
                "Unavailable".into(),
                format!("{} · no reading", sensor.source),
            ),
            None => ("Unavailable".into(), "No sensor reading".into()),
        };
    }
    let age = now
        .checked_sub(snap.started_unix_ms)
        .map(|v| format!("{}s ago", v / 1000))
        .unwrap_or_else(|| "time unavailable".into());
    match metric {
        DashboardMetric::CpuFan | DashboardMetric::GpuFan => {
            let rpm = snap.fan.as_ref().and_then(|fan| {
                if metric == DashboardMetric::CpuFan {
                    fan.cpu_rpm
                } else {
                    fan.gpu_rpm
                }
            });
            match rpm {
                Some(value) => (format!("{value} RPM"), format!("Fan report · {age}")),
                None => (
                    "Unavailable".into(),
                    if snap.fan_err.is_some() {
                        "Fan report failed".into()
                    } else {
                        "No fan reading".into()
                    },
                ),
            }
        }
        DashboardMetric::Battery => match snap
            .battery
            .as_ref()
            .filter(|b| b.present)
            .and_then(|b| b.capacity_pct)
        {
            Some(value) => {
                let power = match snap.ac_online {
                    Some(true) => "AC power",
                    Some(false) => "Battery power",
                    None => "Power source unknown",
                };
                (format!("{value}%"), format!("{power} · {age}"))
            }
            None => ("Unavailable".into(), "No battery reading".into()),
        },
        _ => unreachable!(),
    }
}

pub fn dashboard(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .build();
    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["card"])
        .build();
    let profile = gtk::Label::builder()
        .label(tr("Current profile: waiting for an update"))
        .xalign(0.0)
        .wrap(true)
        .build();
    let update = gtk::Label::builder()
        .label(tr("Waiting for the first monitoring update"))
        .xalign(0.0)
        .wrap(true)
        .build();
    let source = gtk::Label::builder()
        .label(format!("Source: {}", ui.source_label))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim"])
        .build();
    summary.append(&profile);
    summary.append(&update);
    summary.append(&source);
    page.append(&summary);

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::builder()
        .label(tr("Metrics"))
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["section-title"])
        .build();
    let menu = gtk::MenuButton::builder()
        .label(tr("Choose metrics"))
        .build();
    let popover = gtk::Popover::new();
    let choices = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    popover.set_child(Some(&choices));
    menu.set_popover(Some(&popover));
    heading.append(&title);
    heading.append(&menu);
    page.append(&heading);

    let grid = metric_flow(3);
    let selected = Rc::new(RefCell::new(
        omen_core::config::OmenConfig::load()
            .map(|c| c.general.dashboard_metrics)
            .unwrap_or_else(|_| omen_core::config::GeneralConfig::default().dashboard_metrics),
    ));
    let empty = gtk::Label::builder()
        .label(tr(
            "No metrics selected. Choose metrics to show readings here.",
        ))
        .xalign(0.0)
        .wrap(true)
        .build();
    let mut labels = Vec::new();
    for (metric, title) in DASHBOARD_METRICS {
        let (widget, value) = value_card(title);
        widget.set_size_request(175, -1);
        let detail = gtk::Label::builder()
            .label(tr("Waiting for an update"))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim"])
            .build();
        widget.downcast_ref::<gtk::Box>().unwrap().append(&detail);
        grid.append(&widget);
        let child = widget.parent().expect("FlowBox wraps metric");
        child.set_visible(selected.borrow().contains(&metric));
        let check = gtk::CheckButton::with_label(title);
        check.set_active(selected.borrow().contains(&metric));
        let selected = selected.clone();
        let status = ui.status_label.clone();
        let empty = empty.clone();
        let reverting = Rc::new(std::cell::Cell::new(false));
        check.connect_toggled(move |check| {
            if reverting.get() {
                return;
            }
            let next = dashboard_selection(&selected.borrow(), metric, check.is_active());
            match omen_ipc::settings::SettingsController::new(
                None,
                omen_core::config::OmenConfig::path(),
            )
            .set_dashboard_metrics(next.clone())
            {
                Ok(()) => {
                    child.set_visible(check.is_active());
                    empty.set_visible(next.is_empty());
                    *selected.borrow_mut() = next;
                }
                Err(error) => {
                    status.set_text(&format!("Could not save dashboard metrics: {error}"));
                    reverting.set(true);
                    check.set_active(!check.is_active());
                    reverting.set(false);
                }
            }
        });
        choices.append(&check);
        labels.push((metric, value, detail));
    }
    empty.set_visible(selected.borrow().is_empty());
    page.append(&grid);
    page.append(&empty);

    let s1 = section(&tr("Temperature history (°C)"));
    s1.append(&ui.add_chart(
        "Temperature history (°C)",
        vec![
            Series {
                key: "cpu_temp",
                label: "CPU",
                rgb: CPU_COLOR,
            },
            Series {
                key: "gpu_temp",
                label: "GPU",
                rgb: GPU_COLOR,
            },
        ],
        Some(0.0),
        Some(105.0),
    ));
    page.append(&s1);
    let s2 = section(&tr("Fan history (RPM)"));
    s2.append(&ui.add_chart(
        "Fan history (RPM)",
        vec![
            Series {
                key: "fan1",
                label: "CPU fan",
                rgb: FAN1_COLOR,
            },
            Series {
                key: "fan2",
                label: "GPU fan",
                rgb: FAN2_COLOR,
            },
        ],
        Some(0.0),
        None,
    ));
    page.append(&s2);

    let update_snapshot = update.clone();
    ui.register(&page, move |snap| {
        let now = crate::overlay::now_ms();
        profile.set_text(&match snap.profile {
            Some(value) => format!("Current profile: {value}"),
            None => "Current profile: unavailable".into(),
        });
        update_snapshot.set_text(&health_update_text(Some(snap), now));
        for (metric, value, detail) in &labels {
            let (reading, context) = dashboard_reading(snap, *metric, now);
            value.set_text(&reading);
            detail.set_text(&context);
        }
    });
    let weak_page = page.downgrade();
    let latest = ui.latest_snapshot.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let Some(page) = weak_page.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        if page.is_mapped() {
            update.set_text(&health_update_text(
                latest.borrow().as_ref(),
                crate::overlay::now_ms(),
            ));
        }
        gtk::glib::ControlFlow::Continue
    });
    page_scroll(page)
}

pub fn vitals(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let grid = metric_flow(4);
    let (fr_w, fr_l) = value_card(&tr("CPU clock"));
    let (ld_w, ld_l) = value_card(&tr("CPU utilization"));
    let (mu_w, mu_l) = value_card(&tr("Memory used"));
    let (th_w, th_l) = value_card(&tr("Storage used (/)"));
    for w in [fr_w, ld_w, mu_w, th_w] {
        w.set_size_request(175, -1);
        grid.append(&w);
    }
    page.append(&grid);

    let network = section(&tr("Network interfaces"));
    network.append(&gtk::Label::builder()
        .label(tr("Per-interface rates; VPNs and bridges may count the same traffic. Values are not added. Errors and drops are cumulative; RX drops include missed packets."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    let network_status = gtk::Label::builder()
        .label(tr("Collecting network counters…"))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim"])
        .build();
    network.append(&network_status);
    let network_list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    network.append(&network_list);
    network_view::bind(ui, &page, network_list, network_status);
    page.append(&network);

    let s = section(&tr("All sensors"));
    let search = gtk::SearchEntry::builder()
        .placeholder_text(tr("Search sensor names, identifiers, devices or sources"))
        .hexpand(true)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search sensors")]);
    s.append(&search);
    let sensor_status = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim"])
        .label(tr("Collecting sensors…"))
        .build();
    s.append(&sensor_status);
    let temps = section(&tr("Temperatures"));
    let temps_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    temps.append(&temps_list);
    s.append(&temps);
    let others = section(&tr("Other readings"));
    let others_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    others.append(&others_list);
    s.append(&others);
    page.append(&s);

    // Keep the last snapshot so a search updates immediately, even while the
    // next sensor poll is pending.
    let search_snapshot: Rc<RefCell<Option<Snapshot>>> = Rc::new(RefCell::new(None));
    {
        let snapshot = search_snapshot.clone();
        let temps = temps.clone();
        let others = others.clone();
        let temps_list = temps_list.clone();
        let others_list = others_list.clone();
        let status = sensor_status.clone();
        search.connect_search_changed(move |entry| {
            if let Some(snapshot) = snapshot.borrow().as_ref() {
                update_sensor_lists(
                    snapshot,
                    &entry.text(),
                    &temps,
                    &temps_list,
                    &others,
                    &others_list,
                    &status,
                );
            }
        });
    }

    let search_for_action = search.clone();
    ui.register(&page, move |snap| {
        fr_l.set_text(
            &cpu_freq(snap)
                .map(|v| format!("{v:.0} MHz"))
                .unwrap_or_else(|| "—".into()),
        );
        ld_l.set_text(
            &lookup(snap, "cpu/utilization")
                .map(|v| format!("{v:.1} %"))
                .unwrap_or_else(|| "Collecting…".into()),
        );
        mu_l.set_text(
            &lookup(snap, "mem/used_pct")
                .map(|v| format!("{v:.1} %"))
                .unwrap_or_else(|| "—".into()),
        );
        th_l.set_text(
            &lookup(snap, "storage/root_used_pct")
                .map(|v| format!("{v:.1} %"))
                .unwrap_or_else(|| "—".into()),
        );
        *search_snapshot.borrow_mut() = Some(snap.clone());
        update_sensor_lists(
            snap,
            &search.text(),
            &temps,
            &temps_list,
            &others,
            &others_list,
            &sensor_status,
        );
    });

    let page = page_scroll(page);
    crate::widgets::search_action(&page, &search_for_action);
    page
}

fn sensor_matches(sensor: &omen_core::sensors::SensorSample, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {:?}",
        sensor.label,
        sensor.id,
        sensor.source,
        sensor.device_id.as_deref().unwrap_or(""),
        sensor.kind
    )
    .to_lowercase();
    query.split_whitespace().all(|term| haystack.contains(term))
}

fn update_sensor_lists(
    snap: &Snapshot,
    query: &str,
    temps: &gtk::Box,
    temps_list: &gtk::Box,
    others: &gtk::Box,
    others_list: &gtk::Box,
    status: &gtk::Label,
) {
    let query = query.to_lowercase();
    let mut matches: Vec<_> = snap
        .sensors
        .iter()
        .filter(|s| sensor_matches(s, &query))
        .collect();
    matches.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    let (temperature, other): (Vec<_>, Vec<_>) = matches
        .into_iter()
        .partition(|s| s.kind == SensorKind::Temperature);
    let temp_count = temperature.len();
    let other_count = other.len();
    let now = crate::overlay::now_ms();
    update_sensor_rows(temps_list, &temperature, now);
    update_sensor_rows(others_list, &other, now);
    temps.set_visible(temp_count > 0);
    others.set_visible(other_count > 0);
    let count = temp_count + other_count;
    status.set_text(if count == 0 {
        if snap.sensors.is_empty() {
            "No sensor readings available."
        } else {
            "No sensors match this search."
        }
    } else {
        ""
    });
}

/// Reuse rows across polling updates so keyboard focus and selected diagnostic
/// identifiers are not discarded on every sample.
fn sensor_age_text(measured: u64, now: u64, max_age: u64) -> String {
    if measured == 0 || measured > now {
        "Stale · time unavailable".to_string()
    } else {
        let age = now - measured;
        format!(
            "{}{}s old",
            if age > max_age { "Stale · " } else { "" },
            age / 1000
        )
    }
}

fn update_sensor_rows(list: &gtk::Box, sensors: &[&omen_core::sensors::SensorSample], now: u64) {
    let mut next = list.first_child();
    for sensor in sensors {
        let row = if let Some(widget) = next.take() {
            next = widget.next_sibling();
            widget.downcast::<gtk::Box>().expect("sensor row")
        } else {
            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .css_classes(["sensor-row"])
                .spacing(3)
                .build();
            let name = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .selectable(true)
                .build();
            let detail = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .selectable(true)
                .css_classes(["dim", "small"])
                .build();
            row.append(&name);
            row.append(&detail);
            list.append(&row);
            row
        };
        let value = sensor
            .value
            .map(|v| format!("{v:.1} {}", sensor.kind.unit()).trim().to_string())
            .or_else(|| sensor.text.clone())
            .unwrap_or_else(|| "Unavailable".into());
        let age = sensor_age_text(sensor.measured_unix_ms, now, sensor_max_age());
        let name = row
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .expect("sensor name");
        let detail = name
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .expect("sensor detail");
        let name_text = format!("{}: {}", sensor.label, value);
        let detail_text = format!("{} · {} · {}", sensor.source, sensor.id, age);
        if name.text() != name_text {
            name.set_text(&name_text);
        }
        if detail.text() != detail_text {
            detail.set_text(&detail_text);
        }
    }
    while let Some(widget) = next {
        next = widget.next_sibling();
        list.remove(&widget);
    }
}

#[path = "control_pages.rs"]
mod control_pages;

pub use control_pages::{battery, fan_control, graphics, lighting, performance};

fn game_profile_options(
    available: &[ProfileKind],
    bound: Option<ProfileKind>,
) -> Vec<(String, Option<ProfileKind>)> {
    let mut options = vec![("(no profile)".into(), None)];
    for profile in available {
        if !options.iter().any(|(_, value)| *value == Some(*profile)) {
            options.push((profile.to_string(), Some(*profile)));
        }
    }
    if let Some(profile) = bound {
        if !available.contains(&profile) {
            options.push((format!("{profile} (unavailable here)"), Some(profile)));
        }
    }
    options
}

#[path = "network_view.rs"]
mod network_view;

#[path = "game_library.rs"]
mod game_library;

pub fn game_library(ui: &Rc<Ui>) -> gtk::Widget {
    game_library::build(ui)
}

#[derive(Clone)]
struct GameRecovery {
    widget: gtk::Box,
    label: gtk::Label,
    button: gtk::Button,
    target: Rc<std::cell::Cell<Option<(ProfileKind, ProfileKind)>>>,
}

impl GameRecovery {
    fn new(ui: &Rc<Ui>) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 8);
        widget.add_css_class("card");
        widget.set_visible(false);
        let label = gtk::Label::builder().xalign(0.0).wrap(true).build();
        let button = gtk::Button::with_label(&tr("Restore previous profile"));
        button.set_halign(gtk::Align::Start);
        button.set_sensitive(false);
        widget.append(&label);
        widget.append(&button);
        let target = Rc::new(std::cell::Cell::new(None::<(ProfileKind, ProfileKind)>));
        {
            let target = target.clone();
            let button = button.downgrade();
            ui.register(&widget, move |snapshot| {
                if let Some(button) = button.upgrade() {
                    let enabled = target.get().is_some_and(|(_, expected)| snapshot.profile == Some(expected))
                        && snapshot.capabilities.performance_profiles.is_available();
                    button.set_sensitive(enabled);
                    button.set_tooltip_text(Some(if enabled {
                        "Restore only if the current profile still matches this launch"
                    } else {
                        "The profile changed or control is unavailable; open Performance Control to review it"
                    }));
                }
            });
        }
        {
            let ui = ui.clone();
            let target = target.clone();
            button.connect_clicked(move |_| {
                let Some((original, expected)) = target.get() else {
                    return;
                };
                ui.run_hardware_task("Restore game profile".into(), move |source| {
                    source.restore_game_profile(expected, original)
                });
            });
        }
        Self {
            widget,
            label,
            button,
            target,
        }
    }
}

fn refresh_library_locations(ui: &Rc<Ui>, list: &gtk::Box, rescan: &gtk::Button) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let config = match omen_core::config::OmenConfig::load() {
        Ok(config) => config,
        Err(error) => {
            ui.status_label
                .set_text(&format!("Could not read library locations: {error}"));
            return;
        }
    };
    if config.games.library_locations.is_empty() {
        list.append(
            &gtk::Label::builder()
                .label(tr("Default library locations only."))
                .xalign(0.0)
                .build(),
        );
    }
    for location in config.games.library_locations {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::builder()
            .label(format!(
                "{:?} ({})\n{}",
                location.source,
                if location.flatpak {
                    "Flatpak"
                } else if location.snap {
                    "Snap"
                } else {
                    "Native"
                },
                location.path.display()
            ))
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .selectable(true)
            .hexpand(true)
            .build();
        row.append(&label);
        let remove = gtk::Button::with_label(&tr("Remove"));
        remove.set_valign(gtk::Align::Center);
        remove.set_tooltip_text(Some(&format!(
            "Remove {} from library scans",
            location.path.display()
        )));
        let ui = ui.clone();
        let list_for_remove = list.clone();
        let rescan = rescan.clone();
        remove.connect_clicked(move |_| {
            let controller = omen_ipc::settings::SettingsController::new(
                None,
                omen_core::config::OmenConfig::path(),
            );
            let result = omen_core::config::OmenConfig::load().and_then(|mut config| {
                config
                    .games
                    .library_locations
                    .retain(|entry| entry != &location);
                controller.set_library_locations(config.games.library_locations)
            });
            match result {
                Ok(()) => {
                    refresh_library_locations(&ui, &list_for_remove, &rescan);
                    rescan.emit_clicked();
                }
                Err(error) => ui
                    .status_label
                    .set_text(&format!("Could not remove library location: {error}")),
            }
        });
        row.append(&remove);
        list.append(&row);
    }
}

fn library_locations(ui: &Rc<Ui>, rescan: &gtk::Button) -> gtk::Expander {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.append(&gtk::Label::builder()
        .label(tr("Choose an installation and add its library folder. Adding a location does not verify game files."))
        .xalign(0.0).wrap(true).build());
    let source = gtk::DropDown::from_strings(&[
        "Steam · Native",
        "Steam · Flatpak",
        "Heroic · Native",
        "Heroic · Flatpak",
        "Lutris · Native",
        "Lutris · Flatpak",
        "Steam · Snap",
        "Legendary · Native",
    ]);
    source.update_property(&[gtk::accessible::Property::Label(
        "Library launcher installation",
    )]);
    let folder_hint = |selected| {
        match selected {
        0 | 1 | 6 => "Choose the Steam folder containing steamapps. Launch uses this installation's active configuration.",
        2 | 3 => "Choose the Heroic configuration folder containing its store records. Launch uses Heroic's active configuration.",
        4 | 5 => "Choose the Lutris data folder containing pga.db. Launch uses Lutris's active configuration.",
        7 => "Choose the Legendary configuration folder containing installed.json. Launch uses this selected configuration.",
        _ => "Select a supported launcher installation.",
    }
    };
    let hint = gtk::Label::builder()
        .label(folder_hint(source.selected()))
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    source.update_relation(&[gtk::accessible::Relation::DescribedBy(&[hint.upcast_ref()])]);
    source.connect_selected_notify({
        let hint = hint.clone();
        move |source| hint.set_label(folder_hint(source.selected()))
    });
    let add = gtk::Button::with_label(&tr("Add library location…"));
    content.append(&source);
    content.append(&hint);
    content.append(&add);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    refresh_library_locations(ui, &list, rescan);
    content.append(&list);
    let ui = ui.clone();
    let rescan = rescan.clone();
    add.connect_clicked(move |button| {
        use omen_core::games::{GameSource, LibraryLocation};
        let selected = source.selected();
        let installation = match selected {
            0 | 1 | 6 => GameSource::Steam,
            2 | 3 => GameSource::Heroic,
            4 | 5 => GameSource::Lutris,
            7 => GameSource::Legendary,
            _ => return,
        };
        let ui = ui.clone();
        let list = list.clone();
        let rescan = rescan.clone();
        let parent = button.root().and_downcast::<gtk::Window>();
        gtk::FileDialog::builder()
            .title(tr("Add library location"))
            .modal(true)
            .build()
            .select_folder(
                parent.as_ref(),
                None::<&gtk::gio::Cancellable>,
                move |selection| {
                    let file = match selection {
                        Ok(file) => file,
                        Err(error)
                            if error.matches(gtk::DialogError::Dismissed)
                                || error.matches(gtk::DialogError::Cancelled) =>
                        {
                            return
                        }
                        Err(error) => {
                            ui.status_label
                                .set_text(&format!("Could not select library location: {error}"));
                            return;
                        }
                    };
                    let Some(path) = file.path() else {
                        ui.status_label
                            .set_text(&tr("Choose a local library directory."));
                        return;
                    };
                    let location = LibraryLocation {
                        source: installation,
                        path,
                        flatpak: matches!(selected, 1 | 3 | 5),
                        snap: selected == 6,
                    };
                    let result = omen_core::config::OmenConfig::load().and_then(|mut config| {
                        if !config.games.library_locations.contains(&location) {
                            config.games.library_locations.push(location);
                        }
                        omen_ipc::settings::SettingsController::new(
                            None,
                            omen_core::config::OmenConfig::path(),
                        )
                        .set_library_locations(config.games.library_locations)
                    });
                    match result {
                        Ok(()) => {
                            refresh_library_locations(&ui, &list, &rescan);
                            rescan.emit_clicked();
                        }
                        Err(error) => ui
                            .status_label
                            .set_text(&format!("Could not save library location: {error}")),
                    }
                },
            );
    });
    let expander = gtk::Expander::builder()
        .label(tr("Library locations"))
        .child(&content)
        .build();
    crate::widgets::disclosure(&expander);
    expander
}

// The capability report is an observation of the selected backend, not proof
// that a particular device or firmware has been physically validated.
fn capability_text(cap: &omen_core::capability::Capability) -> String {
    use omen_core::capability::Capability;
    let state = match cap {
        Capability::Available { .. } => "Available",
        Capability::ReadOnly { .. } => "Read-only",
        Capability::Unavailable { .. } => "Unavailable",
    };
    format!("{state} — {}", cap.detail())
}

fn display_identity(value: &str) -> &str {
    if value.trim().is_empty() {
        "Not reported"
    } else {
        value
    }
}

fn health_update_text(snapshot: Option<&Snapshot>, now_ms: u64) -> String {
    let Some(snapshot) = snapshot else {
        return "Waiting for the first monitoring update".into();
    };
    let age = if snapshot.started_unix_ms == 0 || snapshot.started_unix_ms > now_ms {
        "update time unavailable".to_string()
    } else {
        let seconds = (now_ms - snapshot.started_unix_ms) / 1000;
        format!("last update {seconds} seconds ago")
    };
    match snapshot.error.as_deref() {
        Some(STALE_REASON) => format!("Monitoring data expired — {age}"),
        Some(error) => format!("{error} — {age}"),
        None => format!("Monitoring update received — {age}"),
    }
}

fn health_center(ui: &Rc<Ui>) -> gtk::Box {
    let section = section(&tr("Device and service health"));
    let source = gtk::Label::builder()
        .label(format!("Source: {}", ui.source_label))
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    section.append(&source);
    let update = gtk::Label::builder()
        .label(tr("Waiting for the first monitoring update"))
        .wrap(true)
        .xalign(0.0)
        .build();
    section.append(&update);
    let machine = gtk::Label::builder()
        .label(tr("Device identity: waiting for an update"))
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    section.append(&machine);
    section.append(&gtk::Label::builder()
        .label(tr("Availability is reported by the current source and does not by itself establish physical validation. Hardware writes remain subject to their safety checks and authorization."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());

    let details = gtk::Expander::builder()
        .label(tr("Capability details"))
        .build();
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 8);
    rows.set_margin_top(8);
    let names = [
        "Fan monitoring",
        "Automatic / maximum fan modes",
        "Manual fan control",
        "Fan curves",
        "Performance profiles",
        "Keyboard lighting zones",
        "Keyboard backlight",
        "Graphics switching",
        "Charge threshold",
        "Battery-aware profiles",
        "NVIDIA telemetry",
        "AMD telemetry",
    ];
    let labels: Vec<_> = names
        .into_iter()
        .map(|name| {
            let label = gtk::Label::builder()
                .label(format!("{name}: Waiting for an update"))
                .wrap(true)
                .xalign(0.0)
                .selectable(true)
                .build();
            rows.append(&label);
            label
        })
        .collect();
    details.set_child(Some(&rows));
    crate::widgets::disclosure(&details);
    section.append(&details);
    let evidence = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    let evidence_expander = gtk::Expander::builder()
        .label(tr("Compatibility evidence"))
        .child(&evidence)
        .build();
    crate::widgets::disclosure(&evidence_expander);
    section.append(&evidence_expander);

    let update_for_snapshot = update.clone();
    let fixture = ui.fixture;
    ui.register(&section, move |snapshot| {
        update_for_snapshot.set_text(&health_update_text(
            Some(snapshot),
            crate::overlay::now_ms(),
        ));
        let caps = &snapshot.capabilities;
        let report = match fixture {
            Some(fixture) => omen_core::compatibility::evidence(&caps.machine, fixture),
            None => omen_core::compatibility::unknown_origin(),
        };
        evidence.set_text(&format!(
            "{}\n\n{}\n\n{}\n\nReference: {}",
            report.observations,
            report.profile_validation,
            report.hardware_control_validation,
            report.reference
        ));
        let id = &caps.machine;
        machine.set_text(&format!(
            "Model: {}\nBoard: {}\nBIOS: {}\nKernel: {}",
            display_identity(&id.product_name),
            display_identity(&id.board_name),
            display_identity(&id.bios_version),
            display_identity(&id.kernel_version)
        ));
        let capabilities = [
            &caps.fan_monitoring,
            &caps.fan_modes,
            &caps.fan_control,
            &caps.fan_curve,
            &caps.performance_profiles,
            &caps.lighting_zones,
            &caps.keyboard_backlight,
            &caps.graphics_mux,
            &caps.charge_threshold,
            &caps.battery_aware_profiles,
            &caps.nvidia_telemetry,
            &caps.amd_telemetry,
        ];
        for ((label, name), cap) in labels.iter().zip(names).zip(capabilities) {
            label.set_text(&format!("{name}: {}", capability_text(cap)));
        }
    });

    // Age the visible report even when polling stops. The main window expires
    // the snapshot separately and publishes the revoked capability report.
    let weak_section = section.downgrade();
    let latest = ui.latest_snapshot.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let Some(section) = weak_section.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        if section.is_mapped() {
            update.set_text(&health_update_text(
                latest.borrow().as_ref(),
                crate::overlay::now_ms(),
            ));
        }
        gtk::glib::ControlFlow::Continue
    });
    section
}

pub fn settings(ui: &Rc<Ui>, global_shortcuts: &gtk::Box, overlay: &gtk::Box) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);

    page.append(&health_center(ui));

    let desktop = section(&tr("Desktop integration"));
    desktop.append(&ui.tray_status);
    let startup = gtk::CheckButton::with_label(&tr("Start in system tray"));
    crate::widgets::wrap_check(&startup);
    startup.set_active(
        omen_core::config::OmenConfig::load()
            .map(|c| c.general.start_minimized)
            .unwrap_or(false),
    );
    startup.set_tooltip_text(Some(
        "Applies next launch. If no tray host is available, the window opens normally.",
    ));
    let saving = Rc::new(std::cell::Cell::new(false));
    {
        let ui = ui.clone();
        startup.connect_toggled(move |button| {
            if saving.replace(true) {
                return;
            }
            let enabled = button.is_active();
            let result = omen_ipc::settings::SettingsController::new(
                ui.source.daemon().cloned(),
                omen_core::config::OmenConfig::path(),
            )
            .set_start_minimized(enabled);
            match result {
                Ok(()) => ui
                    .status_label
                    .set_text(&tr("Startup preference saved; applies next launch")),
                Err(e) => {
                    button.set_active(!enabled);
                    ui.status_label
                        .set_text(&format!("Startup preference was not saved: {e}"));
                }
            }
            saving.set(false);
        });
    }
    desktop.append(&startup);
    desktop.append(&gtk::Label::builder()
        .label(tr("Use Hide to tray to keep monitoring. Closing the window quits. Ctrl+H hides; Ctrl+Q quits. These two shortcuts apply while this window is focused."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    desktop.append(&gtk::Label::builder()
        .label("F6 and Shift+F6 move between the header, navigation and page. Use arrows to choose a page, then Tab to enter it. Ctrl+1 through Ctrl+9 open the nine pages in sidebar order. Ctrl+F focuses sensor or game search. Escape closes a menu or file chooser.")
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    desktop.append(global_shortcuts);
    desktop.append(overlay);
    page.append(&desktop);

    let alerts = section(&tr("Monitoring notifications"));
    alerts.append(&gtk::Label::builder()
        .label(tr("Optional temperature notifications are informational, not thermal protection. They do not change hardware settings and work only while the application is running. Desktop notification settings also apply."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    let config = omen_core::config::OmenConfig::load()
        .map(|c| c.general.alerts)
        .unwrap_or_default();
    let enabled =
        gtk::CheckButton::with_label(&tr("Notify when a temperature reaches the threshold"));
    crate::widgets::wrap_check(&enabled);
    enabled.set_active(config.enabled);
    alerts.append(&enabled);
    let temperature = gtk::SpinButton::with_range(40.0, 120.0, 1.0);
    temperature.set_value(f64::from(config.temperature_c));
    let hysteresis = gtk::SpinButton::with_range(1.0, 30.0, 1.0);
    hysteresis.set_value(f64::from(config.hysteresis_c));
    let cooldown = gtk::SpinButton::with_range(30.0, 3600.0, 30.0);
    cooldown.set_value(f64::from(config.cooldown_seconds));
    for (title, spin) in [
        ("Temperature threshold (°C)", &temperature),
        ("Rearm after cooling by (°C)", &hysteresis),
        ("Minimum time between notifications (seconds)", &cooldown),
    ] {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .build();
        label.set_mnemonic_widget(Some(spin));
        row.append(&label);
        row.append(spin);
        alerts.append(&row);
    }
    let save_alerts = gtk::Button::with_label(&tr("Save notification preferences"));
    {
        let ui = ui.clone();
        save_alerts.connect_clicked(move |_| {
            let config = omen_core::config::AlertConfig {
                enabled: enabled.is_active(),
                temperature_c: temperature.value_as_int() as u32,
                hysteresis_c: hysteresis.value_as_int() as u32,
                cooldown_seconds: cooldown.value_as_int() as u32,
            };
            match omen_ipc::settings::SettingsController::new(
                ui.source.daemon().cloned(),
                omen_core::config::OmenConfig::path(),
            )
            .set_alerts(config.clone())
            {
                Ok(()) => {
                    ui.alerts.borrow_mut().configure(config);
                    ui.status_label
                        .set_text(&tr("Notification preferences saved"));
                }
                Err(error) => ui
                    .status_label
                    .set_text(&format!("Notification preferences were not saved: {error}")),
            }
        });
    }
    alerts.append(&save_alerts);
    page.append(&alerts);

    let backups = section(&tr("Settings backups"));
    backups.append(&gtk::Label::builder()
        .label(tr("User settings contain desktop preferences, dashboard metrics, notification preferences, custom games and profile bindings. Review a backup before applying it; restart to refresh the app."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    backups.append(&settings_buttons(
        ui,
        omen_core::settings::SettingsScope::User,
    ));
    backups.append(&gtk::Label::builder()
        .label(tr("Service policies contain fan curves, lighting presets and battery automation. Applying them requires administrator authorization and may activate automation for all users."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    backups.append(&settings_buttons(
        ui,
        omen_core::settings::SettingsScope::Service,
    ));
    page.append(&backups);

    let s2 = section(&tr("Diagnostics"));
    s2.append(&gtk::Label::builder()
        .label(tr("The report can include machine and BIOS details, sensor readings, network information, device state and service configuration. Review it before sharing; it is saved to the local file you choose."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build());
    let export = gtk::Button::with_label(&tr("Export diagnostics (JSON)"));
    {
        let ui2 = ui.clone();
        export.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
            button.set_sensitive(false);
            ui2.status_label
                .set_text(&tr("Preparing diagnostics for review…"));
            let source = ui2.source.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(source.diagnostics_json());
            });
            let ui = ui2.clone();
            let button = button.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let result = match rx.try_recv() {
                    Ok(result) => result,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return gtk::glib::ControlFlow::Continue
                    }
                    Err(_) => Err(omen_core::OmenError::Internal(
                        "diagnostic worker stopped unexpectedly".into(),
                    )),
                };
                button.set_sensitive(true);
                match result {
                    Ok(json) if json.len() <= 1024 * 1024 => {
                        ui.status_label.set_text("");
                        review_diagnostics(&ui, parent.as_ref(), json);
                    }
                    Ok(_) => ui
                        .status_label
                        .set_text(&tr("Diagnostic report exceeds the 1 MiB export limit.")),
                    Err(error) => ui
                        .status_label
                        .set_text(&format!("Could not prepare diagnostics: {error}")),
                }
                gtk::glib::ControlFlow::Break
            });
        });
    }
    s2.append(&export);
    page.append(&s2);

    let s3 = section(&tr("About"));
    s3.append(&gtk::Label::builder()
        .label(tr("omen-linux — a native, honest HP OMEN Gaming Hub alternative.\nControls only what your hardware and kernel actually support; every disabled control shows why."))
        .wrap(true)
        .halign(gtk::Align::Start)
        .css_classes(["dim"])
        .build());
    page.append(&s3);

    page_scroll(page)
}

fn review_diagnostics(ui: &Rc<Ui>, parent: Option<&gtk::Window>, json: String) {
    let window = gtk::Window::builder()
        .title(tr("Review diagnostics"))
        .default_width(760)
        .default_height(600)
        .modal(true)
        .build();
    window.set_transient_for(parent);
    if let Some(parent) = parent {
        window.set_application(parent.application().as_ref());
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.append(&gtk::Label::builder().label(tr("Review the exact report below. It may contain machine identifiers, network interface names and service configuration. Save it locally only if you want to keep or share these details."))
        .xalign(0.0).wrap(true).build());
    let report = gtk::TextView::builder()
        .editable(false)
        .accepts_tab(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    report.update_property(&[gtk::accessible::Property::Label(&tr(
        "Diagnostics report JSON",
    ))]);
    report.buffer().set_text(&json);
    content.append(
        &gtk::ScrolledWindow::builder()
            .child(&report)
            .vexpand(true)
            .hexpand(true)
            .build(),
    );
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::End);
    let close = gtk::Button::with_label(&tr("Close preview"));
    let save = gtk::Button::with_label(&tr("Save report…"));
    save.add_css_class("suggested-action");
    actions.append(&close);
    actions.append(&save);
    content.append(&actions);
    window.set_child(Some(&content));
    {
        let window = window.downgrade();
        close.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
    }
    {
        let ui = ui.clone();
        let window = window.downgrade();
        save.connect_clicked(move |_| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let ui = ui.clone();
            let json = json.clone();
            let close_after = window.downgrade();
            gtk::FileDialog::builder()
                .title(tr("Export diagnostics"))
                .initial_name("omen-diagnostics.json")
                .modal(true)
                .build()
                .save(
                    Some(&window),
                    None::<&gtk::gio::Cancellable>,
                    move |result| match result {
                        Ok(file) => {
                            let Some(path) = file.path() else {
                                ui.status_label
                                    .set_text(&tr("Choose a local file for diagnostics"));
                                return;
                            };
                            ui.run_task("Export diagnostics".into(), move |source| {
                                omen_ipc::settings::SettingsController::new(
                                    source.daemon().cloned(),
                                    omen_core::config::OmenConfig::path(),
                                )
                                .write_json_document(&path, &json)?;
                                Ok(format!("saved {}", path.display()))
                            });
                            if let Some(window) = close_after.upgrade() {
                                window.close();
                            }
                        }
                        Err(error)
                            if error.matches(gtk::DialogError::Dismissed)
                                || error.matches(gtk::DialogError::Cancelled) => {}
                        Err(error) => ui
                            .status_label
                            .set_text(&format!("File selection failed: {error}")),
                    },
                );
        });
    }
    window.present();
}

fn settings_buttons(ui: &Rc<Ui>, scope: omen_core::settings::SettingsScope) -> gtk::Widget {
    use omen_core::settings::SettingsScope;
    let row = crate::widgets::action_row();
    let name = if scope == SettingsScope::User {
        "user settings"
    } else {
        "service policies"
    };
    for import in [false, true] {
        let label = format!("{} {name}…", if import { "Import" } else { "Export" });
        let button = gtk::Button::with_label(&label);
        if scope == SettingsScope::Service && ui.source.daemon().is_none() {
            button.set_sensitive(false);
            button.set_tooltip_text(Some(&tr(
                "Connect to omen-daemon to manage service policies",
            )));
        }
        let ui = ui.clone();
        button.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
            let dialog = gtk::FileDialog::builder().title(&label).modal(true).build();
            if !import {
                dialog.set_initial_name(Some(if scope == SettingsScope::User {
                    "omen-user-settings.json"
                } else {
                    "omen-service-policies.json"
                }));
            }
            let ui = ui.clone();
            let action = label.trim_end_matches('…').to_owned();
            let callback = move |result: Result<gtk::gio::File, gtk::glib::Error>| {
                let file = match result {
                    Ok(file) => file,
                    Err(error)
                        if error.matches(gtk::DialogError::Dismissed)
                            || error.matches(gtk::DialogError::Cancelled) =>
                    {
                        return
                    }
                    Err(error) => {
                        ui.status_label
                            .set_text(&format!("File selection failed: {error}"));
                        return;
                    }
                };
                let Some(path) = file.path() else {
                    ui.status_label.set_text(&tr("Choose a local backup file"));
                    return;
                };
                if import && scope == SettingsScope::Service {
                    preview_service_import(&ui, path);
                    return;
                }
                if import && scope == SettingsScope::User {
                    preview_user_import(&ui, path);
                    return;
                }
                ui.run_task(action.clone(), move |source| {
                    let controller = omen_ipc::settings::SettingsController::new(
                        source.daemon().cloned(),
                        omen_core::config::OmenConfig::path(),
                    );
                    if import {
                        controller.import_file(&path, scope)?;
                        Ok("saved; restart the app to refresh desktop and game views".into())
                    } else {
                        controller.export_file(&path, scope)?;
                        Ok(format!("saved {}", path.display()))
                    }
                });
            };
            if import {
                dialog.open(parent.as_ref(), None::<&gtk::gio::Cancellable>, callback);
            } else {
                dialog.save(parent.as_ref(), None::<&gtk::gio::Cancellable>, callback);
            }
        });
        crate::widgets::append_action(&row, &button);
    }
    row.widget.upcast()
}

fn preview_user_import(ui: &Rc<Ui>, path: std::path::PathBuf) {
    use omen_core::settings::{SettingsBackup, SettingsScope};
    let (tx, rx) = std::sync::mpsc::channel();
    ui.status_label
        .set_text(&tr("Reading user settings backup…"));
    std::thread::spawn(move || {
        let _ = tx.send(omen_ipc::settings::SettingsController::read_file(
            &path,
            SettingsScope::User,
        ));
    });
    let ui = ui.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok(Ok(SettingsBackup::User(settings))) => {
                let alerts = &settings.general.alerts;
                let metrics = DASHBOARD_METRICS
                    .iter()
                    .filter(|(metric, _)| settings.general.dashboard_metrics.contains(metric))
                    .map(|(_, name)| *name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let metrics = if metrics.is_empty() { "None" } else { &metrics };
                let detail = format!("Dashboard metrics: {metrics}\n\nTemperature notifications: {}\nThreshold: {} °C\nRearm after cooling: {} °C\nMinimum interval: {} seconds\n\nAutomatic profile restoration: {} configured games. Their launcher wrappers must be configured on this installation.\n\nDesktop preferences, custom games and profile bindings will also be replaced. Restart to apply dashboard and desktop preferences and notifications.",
                    if alerts.enabled { "enabled" } else { "disabled" }, alerts.temperature_c, alerts.hysteresis_c, alerts.cooldown_seconds, settings.games.managed_game_ids.len());
                let dialog = gtk::AlertDialog::builder()
                    .message("Apply user settings?")
                    .detail(detail)
                    .buttons(["Cancel", "Apply settings"])
                    .cancel_button(0)
                    .default_button(0)
                    .modal(true)
                    .build();
                let parent = ui.status_label.root().and_downcast::<gtk::Window>();
                let ui = ui.clone();
                dialog.choose(parent.as_ref(), None::<&gtk::gio::Cancellable>, move |response| {
                    if matches!(response, Ok(1)) {
                        ui.run_task("Import user settings".into(), move |source| {
                            omen_ipc::settings::SettingsController::new(source.daemon().cloned(), omen_core::config::OmenConfig::path())
                                .apply(SettingsBackup::User(settings))?;
                            Ok("saved; restart the app to apply dashboard and desktop preferences and notifications".into())
                        });
                    }
                });
                gtk::glib::ControlFlow::Break
            }
            Ok(Ok(_)) => {
                ui.status_label
                    .set_text(&tr("The backup is not user settings"));
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                ui.status_label
                    .set_text(&format!("Could not read user settings: {error}"));
                gtk::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                ui.status_label
                    .set_text(&tr("Could not read user settings"));
                gtk::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
        }
    });
}

fn preview_service_import(ui: &Rc<Ui>, path: std::path::PathBuf) {
    use omen_core::settings::{SettingsBackup, SettingsScope};
    let (tx, rx) = std::sync::mpsc::channel();
    ui.status_label
        .set_text(&tr("Reading service policy backup…"));
    std::thread::spawn(move || {
        let _ = tx.send(omen_ipc::settings::SettingsController::read_file(
            &path,
            SettingsScope::Service,
        ));
    });
    let ui = ui.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok(Ok(SettingsBackup::Service(settings))) => {
                let detail = format!("Battery automation: {}\nBattery profile: {}\nAC profile: {}\n\nThe saved fan curve and lighting presets will also be replaced. Enabled automation may change power profiles. These service policies affect all users; hardware capability checks still apply.",
                    if settings.power.battery_aware { "enabled" } else { "disabled" },
                    settings.power.on_battery_profile,
                    settings.power.on_ac_profile.map(|p| p.to_string()).unwrap_or_else(|| "leave unchanged".into()));
                let dialog = gtk::AlertDialog::builder()
                    .message("Apply service policies?")
                    .detail(detail)
                    .buttons(["Cancel", "Apply policies"])
                    .cancel_button(0)
                    .default_button(0)
                    .modal(true)
                    .build();
                let parent = ui.status_label.root().and_downcast::<gtk::Window>();
                let ui = ui.clone();
                dialog.choose(
                    parent.as_ref(),
                    None::<&gtk::gio::Cancellable>,
                    move |response| {
                        if matches!(response, Ok(1)) {
                            ui.run_hardware_action(
                                "Import service policies".into(),
                                move |source| {
                                    omen_ipc::settings::SettingsController::new(
                                        source.daemon().cloned(),
                                        omen_core::config::OmenConfig::path(),
                                    )
                                    .apply(SettingsBackup::Service(settings))
                                },
                            );
                        } else {
                            ui.status_label
                                .set_text(&tr("Service policy import cancelled"));
                        }
                    },
                );
                gtk::glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                ui.status_label.set_text(&format!("Import failed: {error}"));
                gtk::glib::ControlFlow::Break
            }
            Ok(Ok(_)) => unreachable!("scope checked by backup parser"),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                ui.status_label
                    .set_text(&tr("Could not read service policy backup"));
                gtk::glib::ControlFlow::Break
            }
            Err(_) => gtk::glib::ControlFlow::Continue,
        }
    });
}

fn page_scroll(page: gtk::Box) -> gtk::Widget {
    let sc = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&page)
        .vexpand(true)
        .build();
    sc.upcast()
}

#[cfg(test)]
mod profile_binding_tests {
    use super::*;
    #[test]
    fn unavailable_saved_profile_stays_visible_without_inventing_other_choices() {
        let options = game_profile_options(&[ProfileKind::Balanced], Some(ProfileKind::Cool));
        assert_eq!(
            options,
            vec![
                ("(no profile)".into(), None),
                ("Balanced".into(), Some(ProfileKind::Balanced)),
                ("Cool (unavailable here)".into(), Some(ProfileKind::Cool))
            ]
        );
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;
    use omen_core::capability::Capability;
    use omen_core::sensors::{SensorSample, SensorSource};

    #[test]
    fn dashboard_selection_keeps_consistent_order_and_allows_empty() {
        let one = dashboard_selection(&[], DashboardMetric::Battery, true);
        assert_eq!(one, vec![DashboardMetric::Battery]);
        let both = dashboard_selection(&one, DashboardMetric::CpuFan, true);
        assert_eq!(
            both,
            vec![DashboardMetric::CpuFan, DashboardMetric::Battery]
        );
        assert_eq!(
            dashboard_selection(&both, DashboardMetric::CpuFan, true),
            both
        );
        assert!(dashboard_selection(&one, DashboardMetric::Battery, false).is_empty());
    }

    #[test]
    fn dashboard_distinguishes_fresh_stale_and_missing_metrics() {
        let mut snapshot = Snapshot {
            controls: Default::default(),
            started_unix_ms: 20_000,
            network: None,
            network_error: None,
            capabilities: crate::data::fallback_caps(),
            sensors: Vec::new(),
            fan: None,
            fan_err: None,
            profile: None,
            battery: None,
            ac_online: None,
            error: None,
        };
        let mut sample = SensorSample::numeric(
            "cpu",
            "CPU package",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            55.0,
        );
        sample.measured_unix_ms = 19_000;
        snapshot.sensors.push(sample);
        assert_eq!(
            dashboard_reading(&snapshot, DashboardMetric::CpuTemperature, 20_000),
            ("55.0 °C".into(), "hwmon · 1s ago".into())
        );
        assert_eq!(
            dashboard_reading(&snapshot, DashboardMetric::CpuTemperature, 100_000).0,
            "Stale"
        );
        assert_eq!(
            dashboard_reading(&snapshot, DashboardMetric::GpuTemperature, 20_000).0,
            "Unavailable"
        );
        assert_eq!(
            dashboard_reading(&snapshot, DashboardMetric::CpuFan, 20_000).0,
            "Unavailable"
        );
        assert_eq!(
            dashboard_reading(
                &expired_snapshot(&snapshot),
                DashboardMetric::CpuTemperature,
                100_000
            ),
            ("Unavailable".into(), "Monitoring update expired".into())
        );
    }

    #[test]
    fn sensor_age_marks_expired_missing_and_future_times_stale() {
        assert_eq!(sensor_age_text(19_000, 20_000, 7_000), "1s old");
        assert_eq!(sensor_age_text(12_000, 20_000, 7_000), "Stale · 8s old");
        assert_eq!(
            sensor_age_text(0, 20_000, 7_000),
            "Stale · time unavailable"
        );
        assert_eq!(
            sensor_age_text(20_001, 20_000, 7_000),
            "Stale · time unavailable"
        );
    }

    #[test]
    fn dashboard_and_history_select_fresh_cpu_and_gpu_before_preference_or_maximum() {
        let mut snapshot = Snapshot {
            controls: Default::default(),
            started_unix_ms: 20_000,
            network: None,
            network_error: None,
            capabilities: crate::data::fallback_caps(),
            sensors: Vec::new(),
            fan: None,
            fan_err: None,
            profile: None,
            battery: None,
            ac_online: None,
            error: None,
        };
        for (id, label, value, time) in [
            ("old", "CPU package", 99.0, 1_000),
            ("future", "CPU package", 98.0, 20_001),
            ("cpu", "CPU package", 55.0, 19_000),
            ("nvidia/temp", "NVIDIA GPU", 100.0, 1_000),
            ("amd", "GPU (AMD)", 60.0, 19_000),
        ] {
            let mut sample = SensorSample::numeric(
                id,
                label,
                SensorKind::Temperature,
                SensorSource::Mock,
                value,
            );
            sample.measured_unix_ms = time;
            snapshot.sensors.push(sample);
        }
        assert_eq!(select_cpu_temp(&snapshot, 20_000, 7_000).unwrap().id, "cpu");
        assert_eq!(select_gpu_temp(&snapshot, 20_000, 7_000).unwrap().id, "amd");
        assert!(select_cpu_temp(&snapshot, 30_000, 7_000).is_none());
        assert!(select_gpu_temp(&snapshot, 30_000, 7_000).is_none());
    }

    #[test]
    fn sensor_search_matches_labels_identifiers_sources_and_device() {
        let mut sensor = SensorSample::unavailable(
            "hwmon6:k10temp/temp1_input",
            "CPU package",
            SensorKind::Temperature,
            SensorSource::Hwmon,
        );
        sensor.device_id = Some("0000:03:00.0".into());
        assert!(sensor_matches(&sensor, "cpu package"));
        assert!(sensor_matches(&sensor, "hwmon temp1"));
        assert!(sensor_matches(&sensor, "0000:03 temperature"));
        assert!(!sensor_matches(&sensor, "gpu"));
        assert!(sensor_matches(&sensor, ""));
    }

    #[test]
    fn health_report_describes_capability_state_and_missing_identity() {
        assert_eq!(
            capability_text(&Capability::available("fixture")),
            "Available — fixture"
        );
        assert_eq!(
            capability_text(&Capability::read_only("writes disabled")),
            "Read-only — writes disabled"
        );
        assert_eq!(
            capability_text(&Capability::unavailable("no interface")),
            "Unavailable — no interface"
        );
        assert_eq!(display_identity("  "), "Not reported");
        assert_eq!(display_identity("F.10"), "F.10");
    }

    #[test]
    fn health_report_ages_and_explains_expired_and_failed_snapshots() {
        assert_eq!(
            health_update_text(None, 10_000),
            "Waiting for the first monitoring update"
        );
        let mut snapshot = Snapshot {
            controls: Default::default(),
            started_unix_ms: 1_000,
            network: None,
            network_error: None,
            capabilities: crate::data::fallback_caps(),
            sensors: Vec::new(),
            fan: None,
            fan_err: None,
            profile: None,
            battery: None,
            ac_online: None,
            error: None,
        };
        assert_eq!(
            health_update_text(Some(&snapshot), 10_000),
            "Monitoring update received — last update 9 seconds ago"
        );
        snapshot.error = Some("Sensor update failed: disconnected".into());
        assert!(health_update_text(Some(&snapshot), 10_000)
            .contains("Sensor update failed: disconnected"));
        let expired = expired_snapshot(&snapshot);
        assert_eq!(
            health_update_text(Some(&expired), 10_000),
            "Monitoring data expired — last update 9 seconds ago"
        );
        snapshot.started_unix_ms = 20_000;
        assert!(health_update_text(Some(&snapshot), 10_000).contains("update time unavailable"));
    }

    #[test]
    fn expired_snapshot_clears_readings_and_revokes_all_control_capabilities() {
        let mut caps = crate::data::fallback_caps();
        let available = || Capability::available("fixture");
        caps.fan_monitoring = available();
        caps.fan_modes = available();
        caps.fan_control = available();
        caps.fan_curve = available();
        caps.performance_profiles = available();
        caps.profile_choices = vec![ProfileKind::Balanced];
        caps.lighting_zones = available();
        caps.keyboard_backlight = available();
        caps.graphics_mux = available();
        caps.charge_threshold = available();
        caps.battery_aware_profiles = available();
        caps.nvidia_telemetry = available();
        caps.amd_telemetry = available();
        let mut sensor =
            SensorSample::unavailable("cpu", "CPU", SensorKind::Temperature, SensorSource::Mock);
        sensor.value = Some(70.0);
        let original = Snapshot {
            controls: crate::data::ControlReadings {
                fan_curve: Some(Ok(Default::default())),
                lighting: Some(Ok(omen_core::lighting::LightingState {
                    zones: 1,
                    colors: vec![omen_core::lighting::RgbColor::WHITE],
                    brightness_pct: 70,
                    animation: None,
                })),
                charge_threshold: Some(Ok(Some(80))),
                graphics: Some(Err("fixture inventory unavailable".into())),
            },
            started_unix_ms: 123,
            network: None,
            network_error: None,
            capabilities: caps,
            sensors: vec![sensor],
            fan: Some(omen_core::fans::FanState {
                cpu_rpm: Some(2000),
                ..Default::default()
            }),
            fan_err: None,
            profile: Some(ProfileKind::Balanced),
            battery: Some(omen_core::power::BatteryInfo {
                present: true,
                capacity_pct: Some(80),
                ..Default::default()
            }),
            ac_online: Some(true),
            error: None,
        };
        let expired = expired_snapshot(&original);
        assert_eq!(expired.started_unix_ms, 123);
        assert!(expired.sensors.is_empty());
        assert!(expired.fan.is_none());
        assert!(expired.profile.is_none());
        assert!(expired.battery.is_none());
        assert!(expired.ac_online.is_none());
        assert!(expired.controls.fan_curve.is_none());
        assert!(expired.controls.lighting.is_none());
        assert!(expired.controls.charge_threshold.is_none());
        assert!(expired.controls.graphics.is_none());
        assert_eq!(expired.error.as_deref(), Some(STALE_REASON));
        let c = &expired.capabilities;
        for capability in [
            &c.fan_monitoring,
            &c.fan_modes,
            &c.fan_control,
            &c.fan_curve,
            &c.performance_profiles,
            &c.lighting_zones,
            &c.keyboard_backlight,
            &c.graphics_mux,
            &c.charge_threshold,
            &c.battery_aware_profiles,
            &c.nvidia_telemetry,
            &c.amd_telemetry,
        ] {
            assert!(!capability.is_available());
            assert_eq!(capability.detail(), STALE_REASON);
        }
        assert!(c.profile_choices.is_empty());
        assert_eq!(original.sensors[0].value, Some(70.0));
        assert!(original.capabilities.performance_profiles.is_available());
    }
}
