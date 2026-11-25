//! Opt-in compact status window. Receives the existing poller's snapshots;
//! never opens hardware, spawns a helper or requests compositor privileges.
use crate::data::Snapshot;
use crate::i18n::{number, tr};
use gtk::prelude::*;
use omen_core::sensors::{SensorKind, SensorSample, SensorSource};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn names() -> [String; 9] {
    [
        tr("CPU temperature (highest)"),
        tr("CPU utilization"),
        tr("GPU temperature (highest)"),
        tr("GPU utilization (highest)"),
        tr("Memory used"),
        tr("CPU fan"),
        tr("GPU fan"),
        tr("Power profile"),
        tr("Battery / power source"),
    ]
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .unwrap_or(0)
}

fn fresh(time: u64, now: u64, max_age: u64) -> bool {
    time != 0 && now.checked_sub(time).is_some_and(|age| age <= max_age)
}

fn numeric<'a>(
    samples: impl Iterator<Item = &'a SensorSample>,
    now: u64,
    max_age: u64,
    unit: &str,
) -> String {
    let samples: Vec<_> = samples.collect();
    if samples.is_empty() {
        return tr("Unavailable");
    }
    if samples.iter().any(|s| {
        s.value
            .is_none_or(|v| !v.is_finite() || v < 0.0 || (unit == "%" && v > 100.0))
    }) {
        return tr("Unavailable");
    }
    if samples
        .iter()
        .any(|s| !fresh(s.measured_unix_ms, now, max_age))
    {
        return tr("Stale");
    }
    let highest = samples
        .iter()
        .filter_map(|s| s.value)
        .reduce(f64::max)
        .unwrap();
    format!("{} {unit}", number(highest, 0))
}

fn values(snapshot: &Snapshot, now: u64, max_age: u64) -> Vec<String> {
    if !fresh(snapshot.started_unix_ms, now, max_age) {
        return vec![tr("Stale"); names().len()];
    }
    let sensors = &snapshot.sensors;
    let cpu_temp = numeric(
        sensors.iter().filter(|s| {
            s.kind == SensorKind::Temperature
                && s.source == SensorSource::Hwmon
                && (s.id.contains("/k10temp/") || s.id.contains("/coretemp/"))
        }),
        now,
        max_age,
        "°C",
    );
    let cpu_usage = numeric(
        sensors
            .iter()
            .filter(|s| s.id == "cpu/utilization" && s.kind == SensorKind::Percent),
        now,
        max_age,
        "%",
    );
    let gpu_temp = numeric(
        sensors.iter().filter(|s| {
            s.kind == SensorKind::Temperature
                && ((s.id == "nvidia/temp" && s.source == SensorSource::NvidiaSmi)
                    || (s.id.contains("/amdgpu/") && s.source == SensorSource::Hwmon))
        }),
        now,
        max_age,
        "°C",
    );
    let gpu_usage = numeric(
        sensors.iter().filter(|s| {
            s.kind == SensorKind::Percent
                && ((s.id == "nvidia/util" && s.source == SensorSource::NvidiaSmi)
                    || (s.id.ends_with("/amd/utilization") && s.source == SensorSource::Amdgpu))
        }),
        now,
        max_age,
        "%",
    );
    let memory = numeric(
        sensors
            .iter()
            .filter(|s| s.id == "mem/used_pct" && s.kind == SensorKind::Percent),
        now,
        max_age,
        "%",
    );
    let rpm = |value: Option<u64>| {
        value
            .map(|v| format!("{v} RPM"))
            .unwrap_or_else(|| tr("Unavailable"))
    };
    let profile = snapshot
        .profile
        .map(|p| {
            if snapshot.capabilities.performance_profiles.is_available() {
                p.to_string()
            } else {
                tr("{profile} · read-only").replace("{profile}", &p.to_string())
            }
        })
        .unwrap_or_else(|| tr("Unavailable"));
    let power = match snapshot.ac_online {
        Some(true) => tr("AC"),
        Some(false) => tr("Battery"),
        None => tr("Source unavailable"),
    };
    let battery = snapshot
        .battery
        .as_ref()
        .filter(|b| b.present)
        .and_then(|b| b.capacity_pct)
        .map(|pct| {
            tr("{percent}% · {power}")
                .replace("{percent}", &pct.to_string())
                .replace("{power}", &power)
        })
        .unwrap_or_else(|| tr("Battery unavailable · {power}").replace("{power}", &power));
    vec![
        cpu_temp,
        cpu_usage,
        gpu_temp,
        gpu_usage,
        memory,
        rpm(snapshot.fan.as_ref().and_then(|f| f.cpu_rpm)),
        rpm(snapshot.fan.as_ref().and_then(|f| f.gpu_rpm)),
        profile,
        battery,
    ]
}

pub struct Overlay {
    pub widget: gtk::Box,
    window: RefCell<Option<gtk::Window>>,
    app: gtk::glib::WeakRef<libadwaita::Application>,
    content: gtk::Box,
    header: gtk::HeaderBar,
    toggle: gtk::CheckButton,
    status: gtk::Label,
    health: gtk::Label,
    origin: gtk::Label,
    labels: Vec<gtk::Label>,
    snapshot: RefCell<Option<Snapshot>>,
    last_second: Cell<u64>,
    max_age: Cell<u64>,
    syncing: Cell<bool>,
}

impl Overlay {
    pub fn new(
        app: &libadwaita::Application,
        main: &gtk::Window,
        poll_ms: u64,
        source: &str,
    ) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let toggle = gtk::CheckButton::with_label(&tr("Show status overlay"));
        crate::widgets::wrap_check(&toggle);
        toggle.set_tooltip_text(Some(&tr(
            "Show a compact monitoring window; saves your preference for next launch (Ctrl+O)",
        )));
        let status = gtk::Label::builder().wrap(true).xalign(0.0).build();
        widget.append(&toggle);
        widget.append(&status);
        widget.append(&gtk::Label::builder().label(tr("Compact monitoring window. Move it with your desktop's window controls. Placement above games and visibility in full screen depend on the compositor."))
            .wrap(true).xalign(0.0).build());
        let header = gtk::HeaderBar::new();
        let title = gtk::Label::new(Some(&tr("OMEN Status")));
        title.add_css_class("heading");
        header.set_title_widget(Some(&title));
        // An explicit text action remains accessible even without WM decorations.
        let close = gtk::Button::with_label(&tr("Close overlay"));
        header.pack_end(&close);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_width_request(400);
        content.set_margin_start(14);
        content.set_margin_end(14);
        content.set_margin_top(14);
        content.set_margin_bottom(14);
        let grid = gtk::Grid::builder()
            .column_spacing(18)
            .row_spacing(10)
            .build();
        let mut labels = Vec::new();
        for (row, name) in names().iter().enumerate() {
            let label = gtk::Label::builder()
                .label(name)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(24)
                .hexpand(true)
                .build();
            let value = gtk::Label::builder()
                .label(tr("Waiting…"))
                .xalign(1.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(22)
                .build();
            value.add_css_class("heading");
            grid.attach(&label, 0, row as i32, 1, 1);
            grid.attach(&value, 1, row as i32, 1, 1);
            labels.push(value);
        }
        content.append(&grid);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let health = gtk::Label::builder()
            .label(tr("Waiting for monitoring data…"))
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();
        content.append(&health);
        let origin = gtk::Label::builder()
            .label(source)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();
        origin.set_tooltip_text(Some(source));
        content.append(&origin);
        let open = gtk::Button::with_label(&tr("Open OMEN Control"));
        let main = main.downgrade();
        open.connect_clicked(move |_| {
            if let Some(main) = main.upgrade() {
                main.present();
            }
        });
        content.append(&open);
        let this = Rc::new(Self {
            widget,
            window: RefCell::new(None),
            app: app.downgrade(),
            content,
            header,
            toggle,
            status,
            health,
            origin,
            labels,
            snapshot: RefCell::new(None),
            last_second: Cell::new(0),
            max_age: Cell::new((poll_ms.saturating_mul(3)).clamp(7_000, 30_000)),
            syncing: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.toggle.connect_toggled(move |button| {
            if let Some(this) = weak.upgrade().filter(|s| !s.syncing.get()) {
                this.set_enabled(button.is_active());
            }
        });
        let weak = Rc::downgrade(&this);
        close.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.set_enabled(false);
            }
        });
        let action = gtk::gio::SimpleAction::new("toggle-overlay", None);
        let weak = Rc::downgrade(&this);
        action.connect_activate(move |_, _| {
            if let Some(this) = weak.upgrade() {
                this.toggle();
            }
        });
        app.add_action(&action);
        app.set_accels_for_action("app.toggle-overlay", &["<Control>o"]);
        match omen_core::config::OmenConfig::load() {
            Ok(config) => {
                this.show(config.general.overlay_enabled);
                this.status
                    .set_text(&tr("Overlay preference is saved when toggled"));
            }
            Err(error) => this.status.set_text(
                &tr("Overlay preference could not be read: {error}")
                    .replace("{error}", &error.to_string()),
            ),
        }
        this
    }

    pub fn set_poll_ms(&self, poll_ms: u64) {
        self.max_age
            .set(poll_ms.saturating_mul(3).clamp(7_000, 30_000));
    }
    pub fn window(&self) -> Option<gtk::Window> {
        self.window.borrow().clone()
    }
    pub fn enabled(&self) -> bool {
        self.window
            .borrow()
            .as_ref()
            .is_some_and(|w| w.is_visible())
    }
    pub fn toggle(self: &Rc<Self>) {
        self.set_enabled(!self.enabled());
    }
    fn show(self: &Rc<Self>, enabled: bool) {
        self.syncing.set(true);
        self.toggle.set_active(enabled);
        self.syncing.set(false);
        // set_visible maps without requesting activation/focus via present().
        if enabled {
            if self.window.borrow().is_none() {
                let Some(app) = self.app.upgrade() else {
                    return;
                };
                let window: gtk::Window = gtk::ApplicationWindow::builder()
                    .application(&app)
                    .title(tr("OMEN Status Overlay"))
                    .default_width(440)
                    .default_height(460)
                    .resizable(true)
                    .build()
                    .upcast();
                window.set_titlebar(Some(&self.header));
                window.set_child(Some(&self.content));
                let weak = Rc::downgrade(self);
                window.connect_close_request(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.set_enabled(false);
                    }
                    gtk::glib::Propagation::Stop
                });
                let hide_action = gtk::gio::SimpleAction::new("close-overlay", None);
                let weak = Rc::downgrade(self);
                hide_action.connect_activate(move |_, _| {
                    if let Some(this) = weak.upgrade() {
                        this.set_enabled(false);
                    }
                });
                let actions = gtk::gio::SimpleActionGroup::new();
                actions.add_action(&hide_action);
                window.insert_action_group("overlay", Some(&actions));
                let keys = gtk::ShortcutController::new();
                keys.add_shortcut(gtk::Shortcut::new(
                    gtk::ShortcutTrigger::parse_string("Escape"),
                    Some(gtk::NamedAction::new("overlay.close-overlay")),
                ));
                window.add_controller(keys);

                if std::env::var_os("OMEN_REPORT_READY").is_some() {
                    window.connect_visible_notify(|w| {
                        eprintln!("OMEN_OVERLAY_VISIBLE {}", w.is_visible())
                    });
                }
                *self.window.borrow_mut() = Some(window);
            }
            if let Some(window) = self.window.borrow().as_ref() {
                window.set_visible(true);
            }
            self.last_second.set(0);
            self.refresh();
        } else {
            // Remove closed windows from GTK's toplevel inventory. Besides
            // releasing the surface, this avoids retaining an unrealized
            // accessible root during concurrent AT-SPI tree enumeration.
            let window = self.window.borrow_mut().take();
            if let Some(window) = window {
                window.set_child(None::<&gtk::Widget>);
                window.set_titlebar(None::<&gtk::Widget>);
                window.destroy();
            }
        }
    }
    fn set_enabled(self: &Rc<Self>, enabled: bool) {
        let saved = omen_ipc::settings::SettingsController::new(
            None,
            omen_core::config::OmenConfig::path(),
        )
        .set_overlay_enabled(enabled);
        match saved {
            Ok(()) => {
                self.show(enabled);
                self.status.set_text(&if enabled {
                    tr("Overlay enabled; preference saved")
                } else {
                    tr("Overlay disabled; preference saved")
                });
            }
            Err(error) => {
                // Closing must always work, even if preferences became unwritable.
                self.show(enabled && self.enabled());
                self.status.set_text(
                    &tr("Overlay preference was not saved: {error}")
                        .replace("{error}", &error.to_string()),
                );
            }
        }
    }
    pub fn accept(&self, snapshot: &Snapshot) {
        self.origin.set_text(snapshot.origin.label);
        self.origin.set_tooltip_text(Some(snapshot.origin.label));
        *self.snapshot.borrow_mut() = Some(snapshot.clone());
        self.last_second.set(0);
        self.refresh();
    }
    pub fn refresh(&self) {
        if !self.enabled() {
            return;
        }
        let now = now_ms();
        if self.last_second.replace(now / 1000) == now / 1000 {
            return;
        }
        if let Some(snapshot) = self.snapshot.borrow().as_ref() {
            let max_age = self.max_age.get();
            let values = values(snapshot, now, max_age);
            for (label, value) in self.labels.iter().zip(values) {
                label.set_text(&value);
                label.set_tooltip_text(Some(&value));
            }
            let state = if !fresh(snapshot.started_unix_ms, now, max_age) {
                tr("Monitoring delayed · values are stale")
            } else if snapshot.error.is_some() {
                tr("Sensor data unavailable · open OMEN Control")
            } else {
                tr("Monitoring · missing metrics are unavailable")
            };
            self.health.set_text(&state);
            self.health.set_tooltip_text(snapshot.error.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_and_values_fail_visibly_instead_of_looking_current() {
        let mut sample = SensorSample::numeric(
            "cpu/utilization",
            "CPU",
            SensorKind::Percent,
            SensorSource::Procfs,
            45.0,
        );
        sample.measured_unix_ms = 1000;
        assert_eq!(numeric([&sample].into_iter(), 2000, 7000, "%"), "45 %");
        assert_eq!(numeric([&sample].into_iter(), 9000, 7000, "%"), "Stale");
        assert_eq!(numeric([&sample].into_iter(), 500, 7000, "%"), "Stale");
        sample.value = Some(f64::NAN);
        assert_eq!(
            numeric([&sample].into_iter(), 2000, 7000, "%"),
            "Unavailable"
        );
        assert_eq!(numeric([].into_iter(), 2000, 7000, "%"), "Unavailable");
    }
    #[test]
    fn a_cached_or_missing_channel_cannot_hide_behind_a_fresh_channel() {
        let mut a = SensorSample::numeric(
            "a",
            "GPU",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            55.0,
        );
        a.measured_unix_ms = 2000;
        let mut b = a.clone();
        b.value = Some(90.0);
        b.measured_unix_ms = 1;
        assert_eq!(numeric([&a, &b].into_iter(), 9000, 7000, "°C"), "Stale");
        b.value = None;
        assert_eq!(
            numeric([&a, &b].into_iter(), 9000, 7000, "°C"),
            "Unavailable"
        );
    }
    fn sample_frame() -> Snapshot {
        use omen_hardware::HardwareBackend;
        let backend =
            omen_hardware::mock::MockBackend::new(omen_hardware::mock::MockModel::VictusFb);
        Snapshot {
            origin: Default::default(),
            route: crate::data::DataSource::Offline,
            controls: Default::default(),
            started_unix_ms: 1000,
            capabilities: backend.capabilities(),
            sensors: vec![],
            top_processes: vec![],
            fan: None,
            fan_err: None,
            profile: None,
            battery: None,
            ac_online: None,
            error: None,
            network: None,
            network_error: None,
        }
    }
    #[test]
    fn overlay_uses_provider_identity_and_reports_the_highest_gpu_reading() {
        let mut frame = sample_frame();
        let mut fake = SensorSample::numeric(
            "hwmon0/acpitz/temp1_input",
            "CPU Package",
            SensorKind::Temperature,
            SensorSource::Hwmon,
            99.0,
        );
        fake.measured_unix_ms = 1000;
        frame.sensors.push(fake);
        assert_eq!(values(&frame, 2000, 7000)[0], "Unavailable");
        for (id, source, value) in [
            ("hwmon1/k10temp/temp1_input", SensorSource::Hwmon, 60.0),
            ("hwmon2/amdgpu/temp1_input", SensorSource::Hwmon, 45.0),
            ("nvidia/temp", SensorSource::NvidiaSmi, 70.0),
        ] {
            let mut sample =
                SensorSample::numeric(id, "temperature", SensorKind::Temperature, source, value);
            sample.measured_unix_ms = 1000;
            frame.sensors.push(sample);
        }
        let shown = values(&frame, 2000, 7000);
        assert_eq!(shown[0], "60 °C");
        assert_eq!(shown[2], "70 °C");
        assert_eq!(shown[5], "Unavailable");
        assert_eq!(shown[7], "Unavailable");
    }
    #[test]
    fn stale_whole_frames_cannot_leave_profile_fans_or_battery_looking_current() {
        let mut frame = sample_frame();
        frame.profile = Some(omen_core::profile::ProfileKind::Balanced);
        frame.fan = Some(omen_core::fans::FanState {
            cpu_rpm: Some(3000),
            ..Default::default()
        });
        assert!(values(&frame, 9000, 7000)
            .iter()
            .all(|value| value == "Stale"));
        assert!(values(&frame, 500, 7000)
            .iter()
            .all(|value| value == "Stale"));
        assert_eq!(values(&frame, 2000, 7000)[5], "3000 RPM");
    }
}
