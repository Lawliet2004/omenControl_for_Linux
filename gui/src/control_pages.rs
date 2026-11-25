//! Controls follow fresh capabilities and readbacks without rebuilding editors.
use super::{lookup, metric_flow, page_scroll, section, value_card, Ui};
use crate::i18n::tr;
use gtk::prelude::*;
use omen_core::{capability::Capability, fans::FanMode, profile::ProfileKind};
use std::{cell::Cell, cell::RefCell, rc::Rc};

fn text(value: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(value)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .build()
}

fn reason(label: &gtk::Label, cap: &Capability, name: &str) {
    label.set_visible(!cap.is_available());
    label.set_text(&format!(
        "{name} unavailable: {}. Review the device report in Settings & Diagnostics.",
        cap.detail()
    ));
}

pub fn performance(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let group = section(&tr("Performance profile"));
    let current = text("Current profile: waiting for an update");
    group.append(&current);
    let actions = crate::widgets::action_row();
    let buttons: Vec<_> = ProfileKind::all()
        .into_iter()
        .map(|profile| {
            let button = gtk::ToggleButton::with_label(&profile.to_string());
            button.set_sensitive(false);
            button.set_visible(false);
            button.add_css_class("pill");
            crate::widgets::append_action(&actions, &button);
            let ui = ui.clone();
            button.connect_clicked(move |button| {
                if button.is_active() {
                    ui.run_hardware_action(format!("Set profile to {profile}"), move |source| {
                        source.set_profile(profile)
                    });
                }
            });
            (profile, button)
        })
        .collect();
    group.append(&actions.widget);
    let unavailable = text("Waiting for a fresh capability report");
    unavailable.add_css_class("unavailable");
    group.append(&unavailable);
    let backend = text("");
    backend.add_css_class("dim");
    group.append(&backend);
    group.append(&text("Profiles are offered by the selected Linux backend. Their effects depend on the driver, AC power and firmware."));
    ui.register(&page, move |snapshot| {
        let caps = &snapshot.capabilities;
        current.set_text(&format!(
            "Current profile: {}",
            snapshot
                .profile
                .map(|p| p.to_string())
                .unwrap_or_else(|| "Unavailable".into())
        ));
        reason(
            &unavailable,
            &caps.performance_profiles,
            "Performance profiles",
        );
        backend.set_text(&format!(
            "Backend: {} — {}",
            match caps.profile_plane {
                omen_core::capability::ControlPlane::PlatformProfile => "platform_profile sysfs",
                omen_core::capability::ControlPlane::PowerProfilesDaemon => "power-profiles-daemon",
                omen_core::capability::ControlPlane::Mock => "fixture",
                _ => "other",
            },
            caps.performance_profiles.detail()
        ));
        for (profile, button) in &buttons {
            let advertised = caps.profile_choices.contains(profile);
            // Keep known choices in place, disabled, through an outage. A fresh
            // available provider can add/remove any of the normalized profiles.
            button.set_visible(
                advertised || (!caps.performance_profiles.is_available() && button.is_visible()),
            );
            button.set_sensitive(caps.performance_profiles.is_available() && advertised);
            button.set_active(snapshot.profile == Some(*profile));
        }
    });
    page.append(&group);
    page_scroll(page)
}

pub fn fan_control(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let group = section(&tr("Fan and thermal control"));
    let grid = metric_flow(3);
    let (cpu, cpu_value) = value_card("CPU fan");
    let (gpu, gpu_value) = value_card("GPU fan");
    let (mode, mode_value) = value_card("Mode");
    for card in [cpu, gpu, mode] {
        card.set_size_request(175, -1);
        grid.append(&card);
    }
    group.append(&grid);
    let telemetry = text("Waiting for fan telemetry");
    telemetry.add_css_class("unavailable");
    group.append(&telemetry);
    page.append(&group);
    let modes = section("Firmware fan mode");
    let actions = crate::widgets::action_row();
    let automatic = gtk::Button::with_label("Automatic");
    let maximum = gtk::Button::with_label("Maximum");
    for (button, mode, name) in [
        (&automatic, FanMode::Auto, "Set fans to Automatic"),
        (&maximum, FanMode::Max, "Set fans to Maximum"),
    ] {
        button.set_sensitive(false);
        crate::widgets::append_action(&actions, button);
        let ui = ui.clone();
        button.connect_clicked(move |_| {
            ui.run_hardware_action(name.into(), move |source| source.set_fan_mode(mode))
        });
    }
    modes.append(&actions.widget);
    let mode_reason = text("Waiting for a fresh capability report");
    modes.append(&mode_reason);
    modes.append(&text("Automatic lets firmware manage cooling. Maximum requests the firmware's maximum fan mode. Neither provides adjustable percentages."));
    page.append(&modes);

    let manual = section("Manual fan speed");
    let manual_reason = text("Waiting for a fresh capability report");
    manual_reason.add_css_class("unavailable");
    manual.append(&manual_reason);
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 8);
    editor.set_visible(false);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 20.0, 100.0, 5.0);
    scale.set_value(50.0);
    scale.update_property(&[gtk::accessible::Property::Label("Manual fan duty (%)")]);
    let requested = text("Requested manual duty: 50 %");
    editor.append(&requested);
    scale.connect_value_changed(move |scale| {
        requested.set_text(&format!("Requested manual duty: {:.0} %", scale.value()))
    });
    // The readout is observed policy/RPM; this slider is an explicit request.
    let hint = text("Adjust the requested duty, then apply it. The observed mode is shown above.");
    editor.append(&hint);
    editor.append(&scale);
    let apply = gtk::Button::with_label("Apply manual speed");
    apply.set_sensitive(false);
    apply.set_halign(gtk::Align::Start);
    apply.add_css_class("suggested-action");
    {
        let ui = ui.clone();
        let scale = scale.clone();
        apply.connect_clicked(move |_| {
            let mode = FanMode::Manual {
                duty_pct: scale.value() as u8,
            };
            ui.run_hardware_action("Apply fan mode".into(), move |source| {
                source.set_fan_mode(mode)
            });
        });
    }
    editor.append(&apply);
    manual.append(&editor);
    manual.append(&text("Percentage and curve controls need manual PWM targets. Automatic/Maximum availability is shown separately above."));
    page.append(&manual);
    let curve = curve_editor(ui, &page);
    page.append(&curve);
    let mut_origin = RefCell::new(crate::data::SourceTracker::default());
    ui.register(&page, move |snapshot| {
        if mut_origin.borrow_mut().changed(&snapshot.origin) {
            scale.set_value(50.0);
        }
        let caps = &snapshot.capabilities;
        reason(&telemetry, &caps.fan_monitoring, "Fan telemetry");
        let enabled = caps.fan_modes.is_available() || caps.fan_control.is_available();
        automatic.set_sensitive(enabled);
        maximum.set_sensitive(enabled);
        mode_reason.set_text(caps.fan_modes.detail());
        reason(&manual_reason, &caps.fan_control, "Manual fan control");
        if caps.fan_control.is_available() {
            editor.set_visible(true);
        }
        editor.set_sensitive(caps.fan_control.is_available());
        apply.set_sensitive(caps.fan_control.is_available());
        if let Some(fan) = &snapshot.fan {
            cpu_value.set_text(
                &fan.cpu_rpm
                    .map(|r| format!("{r} RPM"))
                    .unwrap_or_else(|| "—".into()),
            );
            gpu_value.set_text(
                &fan.gpu_rpm
                    .map(|r| format!("{r} RPM"))
                    .unwrap_or_else(|| "—".into()),
            );
            mode_value.set_text(&match fan.mode {
                Some(FanMode::Auto) => "Automatic (firmware)".into(),
                Some(FanMode::Max) => "Maximum".into(),
                Some(FanMode::Manual { duty_pct }) => format!("Manual {duty_pct}%"),
                Some(FanMode::Curve) => "Custom curve".into(),
                None => "Unknown (not reported)".into(),
            });
        } else {
            cpu_value.set_text("—");
            gpu_value.set_text("—");
            mode_value.set_text(snapshot.fan_err.as_deref().unwrap_or("Unavailable"));
        }
    });
    page_scroll(page)
}

fn curve_editor(ui: &Rc<Ui>, page: &gtk::Box) -> gtk::Box {
    let group = section("Custom fan curve");
    let current = text("Saved curve: waiting for an update");
    group.append(&current);
    let unavailable = text("Waiting for a fresh capability report");
    unavailable.add_css_class("unavailable");
    group.append(&unavailable);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.append(&rows);
    let actions = crate::widgets::action_row();
    let save = gtk::Button::with_label("Save curve");
    let activate = gtk::Button::with_label("Activate curve mode");
    for button in [&save, &activate] {
        button.set_sensitive(false);
        crate::widgets::append_action(&actions, button);
    }
    content.append(&actions.widget);
    let hint = text("Save your changes before activating the stored curve. Live custom curves remain subject to backend safety checks.");
    content.append(&hint);
    content.append(&ui.add_chart(
        "Manual fan duty history (%)",
        vec![crate::charts::Series {
            key: "duty",
            label: "Duty %",
            rgb: super::FAN1_COLOR,
        }],
        Some(0.0),
        Some(100.0),
    ));
    let disclosure = gtk::Expander::builder()
        .label("Edit fan curve")
        .child(&content)
        .build();
    crate::widgets::disclosure(&disclosure);
    disclosure.set_visible(false);
    group.append(&disclosure);
    let initialized = Rc::new(Cell::new(false));
    {
        let rows = rows.clone();
        let ui = ui.clone();
        save.connect_clicked(move |_| {
            let curve = curve_from_ui(&rows);
            ui.run_hardware_action("Save fan curve".into(), move |source| {
                curve.validate().and_then(|()| source.set_fan_curve(curve))
            });
        });
    }
    {
        let ui = ui.clone();
        let rows = rows.clone();
        activate.connect_clicked(move |_| {
            let requested = curve_from_ui(&rows);
            ui.run_hardware_action("Activate fan curve".into(), move |source| {
                if source.fan_curve()? != requested {
                    return Err(omen_core::OmenError::InvalidArgument(
                        "Save the edited fan curve before activating it".into(),
                    ));
                }
                source.set_fan_mode(FanMode::Curve)
            })
        });
    }
    // Snapshot refreshes update status, never a loaded editor's spin controls.
    let mut_origin = RefCell::new(crate::data::SourceTracker::default());
    ui.register(page, move |snapshot| {
        if mut_origin.borrow_mut().changed(&snapshot.origin) {
            initialized.set(false);
            while let Some(child) = rows.first_child() {
                rows.remove(&child);
            }
            disclosure.set_visible(false);
        }
        reason(&unavailable, &snapshot.capabilities.fan_curve, "Fan curves");
        let stored = snapshot.controls.fan_curve.as_ref();
        let ready = match stored {
            Some(Ok(curve)) if curve.validate().is_ok() => {
                current.set_text(&format!("Saved curve: {} points", curve.points.len()));
                if !initialized.replace(true) {
                    curve_rows(&rows, curve, &activate);
                    disclosure.set_visible(true);
                }
                true
            }
            Some(Err(error)) => {
                current.set_text(&format!("Saved curve unavailable: {error}"));
                false
            }
            _ => {
                current.set_text("Saved curve: unavailable");
                false
            }
        };
        let enabled = ready && snapshot.capabilities.fan_curve.is_available();
        rows.set_sensitive(enabled);
        save.set_sensitive(enabled);
        activate.set_sensitive(
            enabled
                && stored.is_some_and(|stored| {
                    stored
                        .as_ref()
                        .is_ok_and(|curve| *curve == curve_from_ui(&rows))
                }),
        );
    });
    group
}

fn curve_rows(rows: &gtk::Box, curve: &omen_core::fans::FanCurve, activate: &gtk::Button) {
    for (index, point) in curve.points.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
        for (title, value, minimum, maximum, step) in [
            ("Temperature (°C)", point.temp_c as f64, 0.0, 110.0, 1.0),
            ("Fan duty (%)", point.duty_pct as f64, 0.0, 100.0, 5.0),
        ] {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = text(title);
            label.set_hexpand(true);
            let spin = gtk::SpinButton::with_range(minimum, maximum, step);
            spin.set_value(value);
            let activate = activate.clone();
            spin.connect_value_changed(move |_| activate.set_sensitive(false));
            spin.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} for curve point {}",
                title,
                index + 1
            ))]);
            line.append(&label);
            line.append(&spin);
            row.append(&line);
        }
        rows.append(&row);
    }
}

fn curve_from_ui(rows: &gtk::Box) -> omen_core::fans::FanCurve {
    fn values(widget: &gtk::Widget, result: &mut Vec<f64>) {
        if let Some(spin) = widget.downcast_ref::<gtk::SpinButton>() {
            result.push(spin.value());
        } else {
            let mut child = widget.first_child();
            while let Some(widget) = child {
                values(&widget, result);
                child = widget.next_sibling();
            }
        }
    }
    let mut points = Vec::new();
    let mut child = rows.first_child();
    while let Some(row) = child {
        let mut fields = Vec::new();
        values(&row, &mut fields);
        if fields.len() == 2 {
            points.push(omen_core::fans::CurvePoint {
                temp_c: fields[0] as i32,
                duty_pct: fields[1] as u8,
            });
        }
        child = row.next_sibling();
    }
    // Preserve authored point order so validation can explain a crossing.
    omen_core::fans::FanCurve { points }
}

pub fn lighting(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let group = section("Lighting studio");
    let current = text("Current lighting: waiting for an update");
    group.append(&current);
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 10);
    editor.set_visible(false);
    let colors: Vec<_> = (0..4)
        .map(|zone| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let label = text(&format!("Zone {}", zone + 1));
            label.set_hexpand(true);
            let button = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "Keyboard zone {} color",
                zone + 1
            ))]);
            row.append(&label);
            row.append(&button);
            row.set_visible(false);
            editor.append(&row);
            (row, button)
        })
        .collect();
    editor.append(&text("Requested brightness"));
    let brightness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 5.0);
    brightness.set_draw_value(true);
    brightness.update_property(&[gtk::accessible::Property::Label("Keyboard brightness (%)")]);
    editor.append(&brightness);
    let apply = gtk::Button::with_label("Apply keyboard lighting");
    apply.set_halign(gtk::Align::Start);
    apply.update_property(&[gtk::accessible::Property::Label("Apply keyboard lighting")]);
    apply.add_css_class("suggested-action");
    apply.set_sensitive(false);
    editor.append(&apply);
    group.append(&editor);
    let layout_notice =
        text("Lighting zone layout changed. Latest values loaded; review them before applying.");
    layout_notice.set_visible(false);
    group.append(&layout_notice);
    let unavailable = text("Waiting for a fresh capability report");
    unavailable.add_css_class("unavailable");
    group.append(&unavailable);
    group.append(&text("Lighting availability is reported by the current source. Four-zone and per-key protocols need separate validation; a missing interface does not prove that the keyboard lacks lighting hardware."));
    let initialized = Rc::new(Cell::new(0u8));
    let observed = Rc::new(RefCell::new(None::<omen_core::lighting::LightingState>));
    {
        let ui = ui.clone();
        let colors = colors.clone();
        let brightness = brightness.clone();
        let observed = observed.clone();
        let initialized = initialized.clone();
        apply.connect_clicked(move |_| {
            let Some(mut state) = observed.borrow().clone() else {
                return;
            };
            state.zones = initialized.get();
            state.colors = colors
                .iter()
                .take(state.zones as usize)
                .map(|(_, button)| {
                    let color = button.rgba();
                    omen_core::lighting::RgbColor {
                        r: (color.red() * 255.0).round() as u8,
                        g: (color.green() * 255.0).round() as u8,
                        b: (color.blue() * 255.0).round() as u8,
                    }
                })
                .collect();
            state.brightness_pct = brightness.value() as u8;
            ui.run_hardware_action("Apply lighting".into(), move |source| {
                state.validate().and_then(|()| source.set_lighting(&state))
            });
        });
    }
    let mut_origin = RefCell::new(crate::data::SourceTracker::default());
    ui.register(&page, move |snapshot| {
        if mut_origin.borrow_mut().changed(&snapshot.origin) {
            initialized.set(0);
            *observed.borrow_mut() = None;
            editor.set_visible(false);
        }
        let cap = &snapshot.capabilities.lighting_zones;
        reason(&unavailable, cap, "Keyboard lighting control");
        let state = snapshot
            .controls
            .lighting
            .as_ref()
            .and_then(|state| state.as_ref().ok())
            .filter(|state| {
                state.validate().is_ok()
                    && Some(state.zones) == snapshot.capabilities.lighting_zone_count
            });
        let ready = if let Some(state) = state {
            current.set_text(&format!(
                "Current lighting: {} zones · {} % brightness",
                state.zones, state.brightness_pct
            ));
            if initialized.get() != state.zones {
                layout_notice.set_visible(initialized.get() != 0);
                initialized.set(state.zones);
                for (index, (row, button)) in colors.iter().enumerate() {
                    row.set_visible(index < state.zones as usize);
                    if let Some(color) = state.colors.get(index) {
                        button.set_rgba(&gtk::gdk::RGBA::new(
                            color.r as f32 / 255.0,
                            color.g as f32 / 255.0,
                            color.b as f32 / 255.0,
                            1.0,
                        ));
                    }
                }
                brightness.set_value(state.brightness_pct as f64);
                editor.set_visible(true);
            }
            *observed.borrow_mut() = Some(state.clone());
            true
        } else {
            *observed.borrow_mut() = None;
            current.set_text(&match snapshot.controls.lighting.as_ref() {
                Some(Err(error)) => format!("Current lighting unavailable: {error}"),
                _ => "Current lighting: unavailable".into(),
            });
            false
        };
        editor.set_sensitive(ready && cap.is_available());
        apply.set_sensitive(ready && cap.is_available());
    });
    page.append(&group);
    page_scroll(page)
}

pub fn graphics(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let group = section("Graphics / hybrid");
    let status = text("Graphics inventory: waiting for an update");
    group.append(&status);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    group.append(&list);
    page.append(&group);
    let mux = section("Mode switching (MUX)");
    let mux_status = text("Waiting for a fresh report");
    mux.append(&mux_status);
    page.append(&mux);
    let telemetry = section("NVIDIA telemetry");
    let (card, value) = value_card("GPU (NVIDIA)");
    telemetry.append(&card);
    let telemetry_reason = text("Waiting for a fresh report");
    telemetry.append(&telemetry_reason);
    page.append(&telemetry);
    let rows = RefCell::new(Vec::<(String, gtk::Label)>::new());
    ui.register(&page, move |snapshot| {
        let graphics = snapshot.controls.graphics.as_ref().and_then(|result| result.as_ref().ok());
        let identities: Vec<_> = graphics.map(|g| g.gpus.iter().map(|gpu| gpu.pci_address.clone()).collect()).unwrap_or_default();
        let mut rows = rows.borrow_mut();
        if rows.iter().map(|(id, _)| id).ne(identities.iter()) {
            while let Some(child) = list.first_child() { list.remove(&child); }
            rows.clear();
            for id in identities { let label = text(""); label.set_selectable(true); list.append(&label); rows.push((id, label)); }
        }
        if let Some(graphics) = graphics {
            status.set_text(if !graphics.inventory_complete { "Graphics inventory is incomplete; some controllers may be unavailable." }
                else if graphics.gpus.is_empty() { "No PCI display controllers found." } else { "Current display controllers" });
            for ((_, label), gpu) in rows.iter().zip(&graphics.gpus) {
                label.set_text(&format!("{} {} · {}\nDriver: {} · Runtime: {}", gpu.vendor,
                    if gpu.discrete { "(discrete)" } else { "(topology unverified)" }, gpu.pci_address,
                    gpu.driver.as_deref().unwrap_or("Not reported"), gpu.runtime_status.as_deref().unwrap_or("Not reported")));
            }
        } else {
            status.set_text(&match snapshot.controls.graphics.as_ref() { Some(Err(error)) => format!("Graphics inventory unavailable: {error}"), _ => "Graphics inventory: unavailable".into() });
        }
        mux_status.set_text(&format!("{}\nNo MUX switching action is implemented. Review compatibility in Settings & Diagnostics.", snapshot.capabilities.graphics_mux.detail()));
        let temperature = lookup(snapshot, "nvidia/temp").map(|v| format!("{v:.0} °C")).unwrap_or_else(|| "—".into());
        let power = lookup(snapshot, "nvidia/power").map(|v| format!("{v:.1} W")).unwrap_or_else(|| "—".into());
        value.set_text(&format!("{temperature}, {power}"));
        telemetry_reason.set_text(snapshot.capabilities.nvidia_telemetry.detail());
    });
    page_scroll(page)
}

pub fn battery(ui: &Rc<Ui>) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let group = section("Battery and power");
    let grid = metric_flow(4);
    let (state, state_value) = value_card("State");
    let (charge, charge_value) = value_card("Charge");
    let (health, health_value) = value_card("Health");
    let (power, power_value) = value_card("Power draw");
    let (cycles, cycles_value) = value_card("Cycle count");
    let (capacity, capacity_value) = value_card("Full / design capacity");
    for card in [state, charge, health, power, cycles, capacity] {
        card.set_size_request(175, -1);
        grid.append(&card);
    }
    group.append(&grid);
    page.append(&group);
    let threshold = section("Charge threshold");
    let current = text("Current charge threshold: waiting for an update");
    threshold.append(&current);
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 8);
    editor.set_visible(false);
    let requested = text("Requested charge threshold");
    editor.append(&requested);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 50.0, 100.0, 5.0);
    scale.update_property(&[gtk::accessible::Property::Label("Charge threshold (%)")]);
    scale.connect_value_changed(move |scale| {
        requested.set_text(&format!(
            "Requested charge threshold: {:.0} %",
            scale.value()
        ))
    });
    editor.append(&scale);
    let apply = gtk::Button::with_label("Apply threshold");
    apply.set_halign(gtk::Align::Start);
    apply.set_sensitive(false);
    editor.append(&apply);
    {
        let ui = ui.clone();
        let scale = scale.clone();
        apply.connect_clicked(move |_| {
            let pct = scale.value() as u8;
            ui.run_hardware_action("Set charge threshold".into(), move |source| {
                source.set_charge_threshold(pct)
            });
        });
    }
    threshold.append(&editor);
    let unavailable = text("Waiting for a fresh capability report");
    unavailable.add_css_class("unavailable");
    threshold.append(&unavailable);
    page.append(&threshold);
    let initialized = Cell::new(false);
    let mut_origin = RefCell::new(crate::data::SourceTracker::default());
    ui.register(&page, move |snapshot| {
        if mut_origin.borrow_mut().changed(&snapshot.origin) {
            initialized.set(false);
            editor.set_visible(false);
        }
        if let Some(battery) = snapshot.battery.as_ref().filter(|b| b.present) {
            state_value.set_text(&format!("{:?}", battery.state));
            charge_value.set_text(
                &battery
                    .capacity_pct
                    .map(|v| format!("{v} %"))
                    .unwrap_or_else(|| "—".into()),
            );
            health_value.set_text(
                &battery
                    .health_pct
                    .map(|v| format!("{v:.0} %"))
                    .unwrap_or_else(|| "—".into()),
            );
            power_value.set_text(
                &battery
                    .power_w
                    .map(|v| format!("{v:+.1} W"))
                    .unwrap_or_else(|| "—".into()),
            );
            cycles_value.set_text(
                &battery
                    .cycle_count
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "—".into()),
            );
            capacity_value.set_text(&match (battery.energy_wh_full, battery.energy_wh_design) {
                (Some(full), Some(design)) => format!("{full:.1} / {design:.1} Wh"),
                (Some(full), None) => format!("{full:.1} Wh"),
                (None, Some(design)) => format!("— / {design:.1} Wh"),
                (None, None) => "—".into(),
            });
        } else {
            state_value.set_text("Unavailable");
            charge_value.set_text("—");
            health_value.set_text("—");
            power_value.set_text("—");
            cycles_value.set_text("—");
            capacity_value.set_text("—");
        }
        reason(
            &unavailable,
            &snapshot.capabilities.charge_threshold,
            "Charge threshold control",
        );
        let ready = match snapshot.controls.charge_threshold.as_ref() {
            Some(Ok(Some(value))) if (50..=100).contains(value) => {
                current.set_text(&format!("Current charge threshold: {value} %"));
                if !initialized.replace(true) {
                    scale.set_value(*value as f64);
                    editor.set_visible(true);
                }
                true
            }
            Some(Err(error)) => {
                current.set_text(&format!("Current charge threshold unavailable: {error}"));
                false
            }
            _ => {
                current.set_text("Current charge threshold: unavailable");
                false
            }
        };
        let enabled = ready && snapshot.capabilities.charge_threshold.is_available();
        editor.set_sensitive(enabled);
        apply.set_sensitive(enabled);
    });
    page_scroll(page)
}
