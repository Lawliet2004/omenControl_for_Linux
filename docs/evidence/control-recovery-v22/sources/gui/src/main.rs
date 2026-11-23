//! `omen-gui` — GTK4/libadwaita desktop application.

mod alerts;
mod charts;
mod data;
mod game_sessions;
mod i18n;
mod keyboard;
mod launch_issues;
mod onboarding;
mod overlay;
mod pages;
mod shortcuts;
mod tray;
mod ui_report;
mod victus;
mod widgets;

use crate::i18n::tr;
use data::{DataSource, Snapshot};
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use pages::Ui;
use std::rc::Rc;
use std::time::{Duration, Instant};

const APP_ID: &str = "com.omenlinux.Gui";

fn main() {
    gtk::gio::resources_register_include!("omen.gresource")
        .expect("bundled OMEN icon resources must be valid");
    let args: Vec<String> = std::env::args().collect();
    let app = adw::Application::builder().application_id(APP_ID).build();
    if std::env::var_os("OMEN_CAPTURE_PATH").is_some()
        || args.iter().any(|arg| arg == "--new-instance")
    {
        app.set_flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
    }
    app.connect_startup(|app| {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display).add_resource_path("/com/omenlinux/Gui/icons");
        }
        if let Some(connection) = app.dbus_connection() {
            connection.set_exit_on_close(false);
        }
    });
    // --page <id> opens a specific page (used by tests/screenshot tooling).
    let initial_page = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .cloned();
    app.connect_activate(move |app| build_ui(app, initial_page.clone()));
    // GApplication rejects unknown options, so hand it a filtered argv.
    let filtered: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !(a.as_str() == "--page"
                || a.as_str() == "--new-instance"
                || (*i > 0 && args[i - 1].as_str() == "--page"))
        })
        .map(|(_, a)| a.clone())
        .collect();
    app.run_with_args(&filtered);
}

fn build_ui(app: &adw::Application, initial_page: Option<String>) {
    if let Some(window) = app
        .windows()
        .iter()
        .find(|w| w.widget_name() == "omen-main-window")
    {
        window.present();
        return;
    }
    load_css();

    let source = DataSource::detect();
    let poll_ms = omen_core::config::OmenConfig::load()
        .map(|c| c.general.poll_interval_ms)
        .unwrap_or(1000);
    let ui = Rc::new(Ui::new(source));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(tr("OMEN Control"))
        .default_width(capture_dimension("OMEN_CAPTURE_WIDTH", 1240))
        .default_height(capture_dimension("OMEN_CAPTURE_HEIGHT", 800))
        .icon_name("computer-symbolic")
        .build();
    window.set_widget_name("omen-main-window");
    let overlay = overlay::Overlay::new(app, window.upcast_ref(), poll_ms, ui.source_label);
    let global_shortcuts = shortcuts::GlobalShortcuts::new(window.upcast_ref(), &overlay);
    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        gtk::glib::Propagation::Stop
    });
    let shortcuts_for_shutdown = global_shortcuts.clone();
    app.connect_shutdown(move |_| shortcuts_for_shutdown.shutdown());

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();
    let split = adw::OverlaySplitView::new();
    split.set_min_sidebar_width(220.0);
    split.set_max_sidebar_width(260.0);
    split.set_sidebar_width_fraction(0.22);
    split.set_show_sidebar(true);
    split.set_vexpand(true);
    split.connect_collapsed_notify(|split| split.set_show_sidebar(!split.is_collapsed()));
    let navigation_button = gtk::Button::from_icon_name("sidebar-show-symbolic");
    navigation_button.set_tooltip_text(Some(&tr("Show or hide navigation")));
    navigation_button.update_property(&[gtk::accessible::Property::Label(&tr(
        "Show or hide navigation",
    ))]);
    {
        let split = split.clone();
        navigation_button.connect_clicked(move |_| split.set_show_sidebar(!split.shows_sidebar()));
    }
    header.pack_start(&navigation_button);
    let hide_to_tray = gtk::Button::from_icon_name("window-minimize-symbolic");
    hide_to_tray.update_property(&[gtk::accessible::Property::Label(&tr("Hide to tray"))]);
    hide_to_tray.set_sensitive(false);
    hide_to_tray.set_tooltip_text(Some(&tr("Waiting for a system tray host (Ctrl+H)")));
    let page_title = gtk::Label::builder()
        .label(tr("Dashboard"))
        .css_classes(["title"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(24)
        .build();
    header.set_title_widget(Some(&page_title));
    let health_button = gtk::Button::with_label(&tr("Connecting…"));
    health_button.set_tooltip_text(Some(&tr("Open Settings & Diagnostics")));
    header.pack_end(&health_button);
    let menu_button = gtk::Button::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text(tr("Application actions"))
        .build();
    menu_button.update_property(&[gtk::accessible::Property::Label(&tr("Application actions"))]);
    let action_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let action_popover = gtk::Popover::builder().child(&action_list).build();
    action_popover.set_parent(&menu_button);
    for (title, action) in [
        (tr("Settings & Diagnostics"), "app.diagnostics"),
        (tr("Show welcome"), "app.welcome"),
        (tr("Toggle status overlay"), "app.toggle-overlay"),
        (tr("Hide to tray"), "app.hide"),
        (tr("Quit"), "app.quit"),
    ] {
        let button = gtk::Button::with_label(&title);
        button.add_css_class("flat");
        button.set_action_name(Some(action));
        let popover = action_popover.downgrade();
        button.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        });
        action_list.append(&button);
    }
    {
        let popover = action_popover.clone();
        menu_button.connect_clicked(move |_| {
            if std::env::var_os("OMEN_REPORT_READY").is_some() {
                eprintln!(
                    "OMEN_ACTIONS_OPEN visible={} mapped={}",
                    popover.is_visible(),
                    popover.is_mapped()
                );
            }
            popover.popup();
        });
    }
    if std::env::var_os("OMEN_REPORT_READY").is_some() {
        action_popover.connect_map(|_| eprintln!("OMEN_ACTIONS_MAPPED"));
        action_popover.connect_unmap(|_| eprintln!("OMEN_ACTIONS_UNMAPPED"));
    }
    menu_button.connect_destroy(move |_| action_popover.unparent());
    header.pack_end(&menu_button);
    root.append(&header);

    // Sidebar
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sidebar.set_width_request(235);
    sidebar.add_css_class("sidebar");
    sidebar.append(
        &gtk::Label::builder()
            .label(tr("OMEN Control"))
            .halign(gtk::Align::Start)
            .margin_top(10)
            .margin_start(12)
            .css_classes(["section-title"])
            .build(),
    );
    let nav = gtk::ListBox::new();
    nav.add_css_class("nav");
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);

    let ui_handle = ui.clone();
    let pages: Vec<(&str, &str, &str)> = vec![
        ("dashboard", "Dashboard", "view-grid-symbolic"),
        ("vitals", "System Vitals", "applications-system-symbolic"),
        ("performance", "Performance Control", "speedometer-symbolic"),
        ("fan", "Fan & Thermal", "view-refresh-symbolic"),
        (
            "lighting",
            "Lighting Studio",
            "weather-clear-night-symbolic",
        ),
        ("graphics", "Graphics", "video-display-symbolic"),
        ("battery", "Battery & Power", "battery-symbolic"),
        ("games", "Game Library", "applications-games-symbolic"),
        (
            "settings",
            "Settings & Diagnostics",
            "preferences-system-symbolic",
        ),
    ];
    nav.set_header_func(|row, _previous| {
        let heading = match row.index() {
            0 => Some("Monitor"),
            2 => Some("Controls"),
            7 => Some("Library"),
            8 => Some("Application"),
            _ => None,
        };
        if let Some(heading) = heading {
            let label = gtk::Label::builder()
                .label(heading)
                .xalign(0.0)
                .margin_start(14)
                .margin_top(10)
                .margin_bottom(3)
                .css_classes(["navigation-heading"])
                .build();
            row.set_header(Some(&label));
        } else {
            row.set_header(gtk::Widget::NONE);
        }
    });
    for (id, title, icon) in &pages {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .margin_start(12)
            .margin_end(10)
            .margin_top(7)
            .margin_bottom(7)
            .build();
        row.append(&gtk::Image::from_icon_name(icon));
        row.append(
            &gtk::Label::builder()
                .label(*title)
                .xalign(0.0)
                .wrap(true)
                .hexpand(true)
                .build(),
        );
        nav.append(&row);
        let page = match *id {
            "dashboard" => pages::dashboard(&ui_handle),
            "vitals" => pages::vitals(&ui_handle),
            "performance" => pages::performance(&ui_handle),
            "fan" => pages::fan_control(&ui_handle),
            "lighting" => pages::lighting(&ui_handle),
            "graphics" => pages::graphics(&ui_handle),
            "games" => pages::game_library(&ui_handle),
            "battery" => pages::battery(&ui_handle),
            _ => pages::settings(&ui_handle, &global_shortcuts.widget, &overlay.widget),
        };
        stack.add_titled(&page, Some(id), title);
    }
    let open_diagnostics = {
        let nav = nav.clone();
        let stack = stack.clone();
        move || select_page(&stack, &nav, "settings")
    };
    let welcome = onboarding::welcome(open_diagnostics.clone());
    let dashboard_content = stack
        .child_by_name("dashboard")
        .and_downcast::<gtk::ScrolledWindow>()
        .and_then(|scrolled| scrolled.child());
    let dashboard_content = match dashboard_content {
        Some(child) if child.is::<gtk::Viewport>() => child
            .downcast::<gtk::Viewport>()
            .ok()
            .and_then(|viewport| viewport.child()),
        child => child,
    };
    if let Some(page) = dashboard_content.and_downcast::<gtk::Box>() {
        page.prepend(&welcome);
    }
    let diagnostics_action = gtk::gio::SimpleAction::new("diagnostics", None);
    diagnostics_action.connect_activate(move |_, _| open_diagnostics());
    app.add_action(&diagnostics_action);
    let welcome_action = gtk::gio::SimpleAction::new("welcome", None);
    {
        let stack = stack.clone();
        let nav = nav.clone();
        welcome_action.connect_activate(move |_, _| {
            select_page(&stack, &nav, "dashboard");
            welcome.set_visible(true);
        });
    }
    app.add_action(&welcome_action);
    // Remember stack children by index for navigation.
    let page_widgets: Vec<gtk::Widget> = pages_vec(&stack);
    let stack_for_nav = stack.clone();
    let page_widgets_for_nav = page_widgets.clone();
    let title_for_nav = page_title.clone();
    let titles: Vec<String> = pages
        .iter()
        .map(|(_, title, _)| (*title).to_string())
        .collect();
    nav.connect_row_selected(move |_l, row| {
        if let Some(row) = row {
            let idx = row.index();
            if idx >= 0 {
                if let Some(child) = page_widgets_for_nav.get(idx as usize) {
                    stack_for_nav.set_visible_child(child);
                    if let Some(title) = titles.get(idx as usize) {
                        title_for_nav.set_label(title);
                    }
                }
            }
        }
    });
    {
        let nav = nav.clone();
        let stack = stack.clone();
        health_button.connect_clicked(move |_| {
            if let Some(child) = stack.child_by_name("settings") {
                if let Some(index) = pages_vec(&stack).iter().position(|p| p == &child) {
                    nav.select_row(nav.row_at_index(index as i32).as_ref());
                }
            }
        });
    }
    if let Some(first) = nav.first_child() {
        if let Some(row) = first.downcast_ref::<gtk::ListBoxRow>() {
            nav.select_row(Some(row));
        }
    }
    if let Some(page_id) = initial_page {
        if let Some(child) = stack.child_by_name(&page_id) {
            stack.set_visible_child(&child);
            // keep sidebar selection in sync
            let idx = page_widgets.iter().position(|w| w == &child).unwrap_or(0);
            if let Some(row) = nav.row_at_index(idx as i32) {
                nav.select_row(Some(&row));
            }
        }
    }
    sidebar.append(&nav);

    // Status footer inside sidebar
    let footer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    footer.set_margin_bottom(8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.append(&ui.connection_label);
    footer.append(
        &gtk::Label::builder()
            .label(format!("source: {}", ui.source_label))
            .wrap(true)
            .max_width_chars(26)
            .xalign(0.0)
            .css_classes(["dim", "small"])
            .halign(gtk::Align::Start)
            .build(),
    );
    sidebar.append(&footer);

    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(235)
        .child(&sidebar)
        .build();
    split.set_sidebar(Some(&sidebar_scroll));

    let content_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content_area.set_hexpand(true);
    content_area.set_vexpand(true);
    let margin = gtk::Box::new(gtk::Orientation::Vertical, 0);
    margin.set_margin_top(14);
    margin.set_margin_bottom(14);
    margin.set_margin_start(18);
    margin.set_margin_end(18);
    margin.append(&stack);
    content_area.append(&margin);
    // Action feedback must remain reachable with folded navigation, including
    // failed imports and unavailable controls. It uses a native text label so
    // assistive technology can read and select the complete message.
    let feedback = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    feedback.set_margin_start(18);
    feedback.set_margin_end(18);
    feedback.set_margin_bottom(10);
    feedback.add_css_class("card");
    ui.status_label.set_hexpand(true);
    ui.status_label.set_xalign(0.0);
    ui.status_label.set_selectable(true);
    feedback.append(&ui.status_label);
    let clear_feedback = gtk::Button::from_icon_name("window-close-symbolic");
    clear_feedback.set_tooltip_text(Some(&tr("Dismiss message")));
    clear_feedback.update_property(&[gtk::accessible::Property::Label(&tr("Dismiss message"))]);
    {
        let status = ui.status_label.clone();
        clear_feedback.connect_clicked(move |_| status.set_text(""));
    }
    feedback.append(&clear_feedback);
    feedback.set_visible(!ui.status_label.text().is_empty());
    {
        let feedback = feedback.downgrade();
        ui.status_label.connect_label_notify(move |label| {
            if let Some(feedback) = feedback.upgrade() {
                feedback.set_visible(!label.text().is_empty());
            }
        });
    }
    content_area.append(&feedback);
    split.set_content(Some(&content_area));
    root.append(&split);
    window.set_content(Some(&root));
    widgets::announce_changes(&ui.status_label);
    keyboard::install(
        &window,
        app,
        (&header, &navigation_button),
        &nav,
        &stack,
        &split,
        &pages,
    );

    let tray_updates = install_tray(app, &window, &ui, &hide_to_tray, &stack, &nav, &overlay);

    // Polling thread → mpsc → timeout-drained UI updates (glib 0.20 has no
    // MainContext::channel; draining a std channel from the main loop is the
    // documented pattern).
    let (tx, rx) = std::sync::mpsc::sync_channel::<Snapshot>(1);
    let rx = Rc::new(rx);
    {
        let source = ui.source.clone();
        std::thread::spawn(move || loop {
            let mut snap = source.snapshot();
            if snap.sensors.is_empty() && snap.error.is_none() {
                snap.error = Some("no sensor data (is the daemon running?)".into());
            }
            if tx.send(snap).is_err() {
                break; // window gone
            }
            std::thread::sleep(Duration::from_millis(poll_ms.max(250)));
        });
    }
    {
        let ui = ui.clone();
        let window = window.clone();
        let overlay = overlay.clone();
        let health_button = health_button.clone();
        let split = split.clone();
        // Bound freshness by acquisition time, not merely channel arrival: a
        // poll blocked halfway through can eventually deliver an old snapshot.
        let freshness = Duration::from_millis(poll_ms.saturating_mul(3).clamp(7_000, 30_000));
        let mut last_acquisition = Instant::now();
        gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
            let collapsed = window.width() < 850;
            if split.is_collapsed() != collapsed {
                split.set_collapsed(collapsed);
            }
            // Drain pending snapshots, keep only the newest to avoid lag.
            let mut newest: Option<Snapshot> = None;
            while let Ok(s) = rx.try_recv() {
                newest = Some(s);
            }
            if let Some(snap) = newest {
                let age = crate::overlay::now_ms()
                    .checked_sub(snap.started_unix_ms)
                    .map(Duration::from_millis);
                if let Some(age) = age.filter(|age| *age <= freshness) {
                    last_acquisition = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
                    let _ = tray_updates.try_send(tray::Update::State {
                        profile: snap.profile,
                        visible: window.is_visible(),
                        choices: snap.capabilities.profile_choices.clone(),
                        writable: snap.capabilities.performance_profiles.is_available(),
                        overlay: overlay.enabled(),
                    });
                    if let Some(e) = &snap.error {
                        ui.connection_label.set_text(e);
                        health_button.set_label(&tr("Attention needed"));
                        health_button.set_tooltip_text(Some(e));
                    } else {
                        ui.connection_label.set_text("");
                        health_button.set_label(&match (&ui.source, ui.fixture) {
                            (_, Some(true)) => tr("Fixture"),
                            (DataSource::Daemon(_), None) => tr("Source unverified"),
                            (DataSource::Daemon(_), _) => tr("Service"),
                            (DataSource::Direct(_), _) => tr("Direct"),
                        });
                        health_button.set_tooltip_text(Some(match &ui.source {
                            DataSource::Daemon(_) => {
                                "Service connected — open Settings & Diagnostics"
                            }
                            DataSource::Direct(_) => {
                                "Direct monitoring — open Settings & Diagnostics"
                            }
                        }));
                    }
                    ui.dispatch(&snap);
                    overlay.accept(&snap);
                }
            }
            if last_acquisition.elapsed() > freshness {
                if ui.expire_snapshot() {
                    let _ = tray_updates.try_send(tray::Update::State {
                        profile: None,
                        visible: window.is_visible(),
                        choices: Vec::new(),
                        writable: false,
                        overlay: overlay.enabled(),
                    });
                }
                health_button.set_label(&tr("Data stale"));
                health_button.set_tooltip_text(Some(
                    "Monitoring has stopped updating — open Settings & Diagnostics",
                ));
                ui.connection_label
                    .set_text(&tr("Monitoring data is stale; waiting for an update."));
            }
            ui.refresh_charts();
            overlay.refresh();
            gtk::glib::ControlFlow::Continue
        });
    }

    if std::env::var_os("OMEN_REPORT_READY").is_some() {
        eprintln!(
            "OMEN_UI_BACKEND {}",
            gtk::prelude::WidgetExt::display(&window).type_().name()
        );
        window.connect_map(|_| eprintln!("OMEN_UI_MAPPED"));
        window.connect_visible_notify(|w| eprintln!("OMEN_UI_VISIBLE {}", w.is_visible()));
        stack.connect_visible_child_name_notify(|s| {
            eprintln!(
                "OMEN_UI_PAGE {}",
                s.visible_child_name().unwrap_or_default()
            )
        });
    }
    window.present();
    // Capture only our own GTK widget tree for repeatable visual checks. This
    // never reads other windows or the desktop framebuffer.
    if let Some(path) = std::env::var_os("OMEN_CAPTURE_PATH") {
        let app = app.clone();
        let window = window.clone();
        let overlay = overlay.clone();
        gtk::glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if let Some(path) = std::env::var_os("OMEN_UI_REPORT_PATH") {
                if let Err(error) =
                    ui_report::save(window.upcast_ref(), std::path::Path::new(&path))
                {
                    eprintln!("Could not save requested UI geometry report: {error}");
                }
            }
            capture_widget(window.upcast_ref(), std::path::Path::new(&path));
            if let Some(path) = std::env::var_os("OMEN_CAPTURE_OVERLAY_PATH") {
                capture_widget(
                    &overlay.window().expect("enabled overlay"),
                    std::path::Path::new(&path),
                );
            }
            app.quit();
        });
    }
}

// Capture sizing is only active for the existing application-owned screenshot
// path; normal users retain the standard initial window size.
fn capture_dimension(key: &str, default: i32) -> i32 {
    if std::env::var_os("OMEN_CAPTURE_PATH").is_none() {
        return default;
    }
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| (320..=4096).contains(value))
        .unwrap_or(default)
}

fn capture_widget(window: &gtk::Window, path: &std::path::Path) {
    assert!(window.is_mapped(), "capture target is not mapped");
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
    let node = snapshot.to_node().expect("rendered window node");
    window
        .renderer()
        .expect("window renderer")
        .render_texture(&node, None)
        .save_to_png(path)
        .expect("save application screenshot");
}

fn install_tray(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    ui: &Rc<Ui>,
    hide: &gtk::Button,
    stack: &gtk::Stack,
    nav: &gtk::ListBox,
    overlay: &Rc<overlay::Overlay>,
) -> std::sync::mpsc::SyncSender<tray::Update> {
    let (updates, events) = tray::start(
        ui.source_label.into(),
        ui.latest_caps.borrow().profile_choices.clone(),
        ui.latest_caps.borrow().performance_profiles.is_available(),
    );
    let online = Rc::new(std::cell::Cell::new(false));
    {
        let online = online.clone();
        let window = window.clone();
        hide.connect_clicked(move |_| {
            if online.get() {
                window.set_visible(false);
            }
        });
    }
    let quit_action = gtk::gio::SimpleAction::new("quit", None);
    let app_weak = app.downgrade();
    quit_action.connect_activate(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit_action);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    let hide_action = gtk::gio::SimpleAction::new("hide", None);
    hide_action.set_enabled(hide.is_sensitive());
    {
        let action = hide_action.downgrade();
        hide.connect_sensitive_notify(move |button| {
            if let Some(action) = action.upgrade() {
                action.set_enabled(button.is_sensitive());
            }
        });
    }
    let hide_button = hide.clone();
    hide_action.connect_activate(move |_, _| {
        if hide_button.is_sensitive() {
            hide_button.emit_clicked();
        }
    });
    app.add_action(&hide_action);
    app.set_accels_for_action("app.hide", &["<Control>h"]);
    let shutdown = updates.clone();
    app.connect_shutdown(move |_| {
        let _ = shutdown.try_send(tray::Update::Shutdown);
    });
    let mut first_connection = true;
    let start_minimized = omen_core::config::OmenConfig::load()
        .map(|c| c.general.start_minimized)
        .unwrap_or(false)
        && std::env::var_os("OMEN_CAPTURE_PATH").is_none();
    let window = window.clone();
    let app = app.clone();
    let ui = ui.clone();
    let hide = hide.clone();
    let stack = stack.clone();
    let nav = nav.clone();
    let state_updates = updates.clone();
    let overlay = overlay.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        while let Ok(event) = events.try_recv() {
            match event {
                tray::Event::Online => {
                    online.set(true);
                    ui.tray_status.set_text(&tr("System tray connected"));
                    hide.set_sensitive(true);
                    hide.set_tooltip_text(Some(&tr("Keep monitoring in the system tray (Ctrl+H)")));
                    if first_connection && start_minimized {
                        window.set_visible(false);
                    }
                    first_connection = false;
                    if std::env::var_os("OMEN_REPORT_READY").is_some() {
                        eprintln!("OMEN_TRAY_ONLINE");
                    }
                }
                tray::Event::Offline(reason) => {
                    online.set(false);
                    first_connection = false;
                    hide.set_sensitive(false);
                    hide.set_tooltip_text(Some(&tr("No system tray host is available")));
                    ui.tray_status
                        .set_text(&tr("System tray unavailable — the window stays open"));
                    ui.tray_status.set_tooltip_text(Some(&reason));
                    window.present();
                    if std::env::var_os("OMEN_REPORT_READY").is_some() {
                        eprintln!("OMEN_TRAY_OFFLINE {reason}");
                    }
                }
                tray::Event::Action(action) => match action {
                    tray::Action::Show(token) => {
                        if let Some(token) = token {
                            window.set_startup_id(&token);
                        }
                        window.present();
                    }
                    tray::Action::Hide => {
                        if online.get() {
                            window.set_visible(false);
                        }
                    }
                    tray::Action::Quit => app.quit(),
                    tray::Action::ToggleOverlay => overlay.toggle(),
                    tray::Action::Page(page) => {
                        if let Some(child) = stack.child_by_name(page) {
                            if let Some(index) = pages_vec(&stack).iter().position(|p| p == &child)
                            {
                                nav.select_row(nav.row_at_index(index as i32).as_ref());
                            }
                        }
                        window.present();
                    }
                    tray::Action::Profile(profile) => {
                        let allowed = {
                            let caps = ui.latest_caps.borrow();
                            caps.performance_profiles.is_available()
                                && caps.profile_choices.contains(&profile)
                        };
                        if allowed {
                            ui.run_hardware_action(
                                format!("Set profile {profile}"),
                                move |source| source.set_profile(profile),
                            );
                        } else {
                            ui.status_label.set_text(&tr("Power profile control is unavailable; open Performance Control for details"));
                            window.present();
                        }
                    }
                },
            }
            // Do not wait for the next hardware poll to update the Hide item.
            let _ = state_updates.try_send(tray::Update::State {
                profile: ui.latest_profile.get(),
                visible: window.is_visible(),
                choices: ui.latest_caps.borrow().profile_choices.clone(),
                writable: ui.latest_caps.borrow().performance_profiles.is_available(),
                overlay: overlay.enabled(),
            });
        }
        gtk::glib::ControlFlow::Continue
    });
    updates
}

/// Collect the stack's children in page order (StackSelectionModel's
/// `child()` returns Result; project it cleanly).
fn pages_vec(stack: &gtk::Stack) -> Vec<gtk::Widget> {
    let n = stack.pages().n_items();
    (0..n)
        .filter_map(|i| {
            stack
                .pages()
                .item(i)
                .and_then(|o| o.downcast::<gtk::StackPage>().ok())
                .map(|p| p.child())
        })
        .collect()
}

fn select_page(stack: &gtk::Stack, nav: &gtk::ListBox, name: &str) {
    if let Some(child) = stack.child_by_name(name) {
        if let Some(index) = pages_vec(stack).iter().position(|page| page == &child) {
            nav.select_row(nav.row_at_index(index as i32).as_ref());
        }
    }
}

fn load_css() {
    let css = r#"
    .sidebar { background: @window_bg_color; }
    .nav row { border-radius: 8px; margin: 1px 6px; }
    .nav row:selected { background: alpha(@accent_color, 0.25); }
    .section-title { font-weight: 700; font-size: 105%; opacity: 0.85; }
    .navigation-heading { font-weight: 700; font-size: 85%; opacity: 0.8; }
    .col-title { font-weight: 600; opacity: 0.8; }
    .card { background: @card_bg_color; color: @card_fg_color; border-radius: 12px; padding: 12px 14px; }
    .card-title { opacity: 0.8; font-size: 90%; }
    .card-value { font-size: 140%; font-weight: 700; }
    .pill { border-radius: 999px; padding: 6px 16px; }
    .suggested-action { background-color: shade(@accent_bg_color, 0.8); color: @accent_fg_color; }
    .unavailable { background: alpha(@warning_color, 0.13); color: @window_fg_color; border-radius: 10px; padding: 10px 14px; }
    .dim { opacity: 0.8; }
    .small { font-size: 85%; }
    .game-library { background: transparent; }
    .game-library > row { padding: 0; }
    .sensor-row { background: alpha(@window_fg_color, 0.04); border-radius: 8px; padding: 4px 10px; }
    .sensor-val { font-weight: 600; }
    .chart { background: @card_bg_color; border-radius: 12px; }
    .title { font-size: 110%; }
    "#;
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
