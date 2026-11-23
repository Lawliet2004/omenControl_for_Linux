//! Shared native behavior for disclosure, reflow and action feedback.
use gtk::{glib::translate::ToGlibPtr, prelude::*};
use libadwaita::prelude::BreakpointBinExt;

fn disclosure_label(expander: &gtk::Expander) {
    if let Some(label) = expander.label_widget().and_downcast::<gtk::Label>() {
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        // The expanded content is a separate reading surface, not the name of
        // the disclosure button. Keep its name stable as children become visible.
        expander.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    } else {
        expander.update_relation(&[gtk::accessible::Relation::LabelledBy(&[])]);
    }
}

pub fn disclosure(expander: &gtk::Expander) {
    // GtkExpander relies on ancestor activation shortcuts. A surrounding
    // ListView handles those first, so a focused disclosure otherwise ignores
    // Space/Enter. Give the focused expander its own local native activation.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = expander.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if matches!(
            key,
            gtk::gdk::Key::space | gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter
        ) && !modifiers.intersects(
            gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::SUPER_MASK,
        ) {
            if let Some(expander) = weak.upgrade().filter(|e| e.has_focus()) {
                expander.activate();
                return gtk::glib::Propagation::Stop;
            }
        }
        gtk::glib::Propagation::Proceed
    });
    expander.add_controller(keys);
    if let Some(child) = expander.child() {
        // GTK 4.22 detaches collapsed content but can still attempt to focus
        // its selectable children. Bind visibility as well, so hidden content
        // never consumes Tab or steals focus from an unrooted widget.
        expander
            .bind_property("expanded", &child, "visible")
            .sync_create()
            .build();
    }
    disclosure_label(expander);
    expander.connect_notify_local(Some("label-widget"), |expander, _| {
        disclosure_label(expander)
    });
}

pub struct ActionRow {
    pub widget: libadwaita::BreakpointBin,
    content: gtk::Box,
    measure: std::rc::Rc<dyn Fn()>,
    refresh: std::rc::Rc<dyn Fn()>,
}

pub fn action_row() -> ActionRow {
    // Use actual child requisitions, including the configured font and DPI.
    // A fixed pixel or SP cutoff misses large user-selected font sizes.
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let bin = libadwaita::BreakpointBin::builder()
        .child(&content)
        .width_request(1)
        .height_request(1)
        .build();
    let breakpoint = libadwaita::Breakpoint::new(
        libadwaita::BreakpointCondition::parse("max-width: 1px").expect("static breakpoint"),
    );
    breakpoint.add_setter(
        &content,
        "orientation",
        Some(&gtk::Orientation::Vertical.to_value()),
    );
    bin.add_breakpoint(breakpoint.clone());
    let (weak_bin, weak_content, weak_breakpoint) =
        (bin.downgrade(), content.downgrade(), breakpoint.downgrade());
    let last_width = Cell::new(-1);
    let update: Rc<dyn Fn()> = Rc::new(move || {
        let (Some(bin), Some(content), Some(breakpoint)) = (
            weak_bin.upgrade(),
            weak_content.upgrade(),
            weak_breakpoint.upgrade(),
        ) else {
            return;
        };
        let mut widths = Vec::new();
        let mut child = content.first_child();
        while let Some(widget) = child {
            if widget.is_visible() {
                widths.push(widget.measure(gtk::Orientation::Horizontal, -1).0);
            }
            child = widget.next_sibling();
        }
        if widths.is_empty() {
            return;
        }
        let minimum = widths.iter().sum::<i32>() + content.spacing() * (widths.len() as i32 - 1);
        if last_width.replace(minimum) != minimum {
            let condition = libadwaita::BreakpointCondition::new_length(
                libadwaita::BreakpointConditionLengthType::MaxWidth,
                f64::from(minimum + 1),
                libadwaita::LengthUnit::Px,
            );
            breakpoint.set_condition(Some(&condition));
        }
        let minimum_width = if content.orientation() == gtk::Orientation::Horizontal {
            minimum
        } else {
            *widths.iter().max().unwrap()
        };
        let height = content
            .measure(gtk::Orientation::Vertical, bin.width().max(minimum_width))
            .0
            .max(1);
        if bin.height_request() != height {
            bin.set_height_request(height);
        }
    });
    let measure = update.clone();
    let queued = Rc::new(Cell::new(false));
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        if queued.replace(true) {
            return;
        }
        let update = update.clone();
        let queued = queued.clone();
        gtk::glib::idle_add_local_once(move || {
            queued.set(false);
            update();
        });
    });
    for apply in [true, false] {
        let refresh = refresh.clone();
        if apply {
            breakpoint.connect_apply(move |_| refresh());
        } else {
            breakpoint.connect_unapply(move |_| refresh());
        }
    }
    {
        let refresh = refresh.clone();
        bin.connect_map(move |_| refresh());
    }
    if let Some(settings) = gtk::Settings::default() {
        let refresh = refresh.clone();
        let handler = RefCell::new(Some(settings.connect_notify_local(
            None,
            move |_, property| {
                if matches!(property.name(), "gtk-font-name" | "gtk-xft-dpi") {
                    refresh();
                }
            },
        )));
        let settings = settings.downgrade();
        bin.connect_destroy(move |_| {
            if let (Some(settings), Some(handler)) =
                (settings.upgrade(), handler.borrow_mut().take())
            {
                settings.disconnect(handler);
            }
        });
    }
    ActionRow {
        widget: bin,
        content,
        measure,
        refresh,
    }
}

pub fn append_action(row: &ActionRow, widget: &impl IsA<gtk::Widget>) {
    row.content.append(widget);
    (row.measure)();
    let refresh = row.refresh.clone();
    widget
        .as_ref()
        .connect_notify_local(None, move |_, _| refresh());
    (row.refresh)();
}

pub fn wrap_check(button: &gtk::CheckButton) {
    let mut child = button.first_child();
    while let Some(widget) = child {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_xalign(0.0);
        }
        child = widget.next_sibling();
    }
}

/// GTK 4.14 introduced announcements. Resolve the optional API at runtime to
/// retain the application's GTK 4.12 minimum. Older GTK still exposes the
/// selectable feedback text; automatic announcement requires the newer API.
pub fn announce_changes(label: &gtk::Label) {
    type Announce =
        unsafe extern "C" fn(*mut gtk::ffi::GtkAccessible, *const libc::c_char, libc::c_int);
    static ANNOUNCE: std::sync::OnceLock<Option<Announce>> = std::sync::OnceLock::new();
    let announce = *ANNOUNCE.get_or_init(|| unsafe {
        let symbol = libc::dlsym(libc::RTLD_DEFAULT, c"gtk_accessible_announce".as_ptr());
        if symbol.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut libc::c_void, Announce>(symbol))
        }
    });
    if let Some(announce) = announce {
        label.connect_label_notify(move |label| {
            let message = label.text();
            if std::env::var_os("OMEN_REPORT_READY").is_some() {
                eprintln!(
                    "OMEN_A11Y_FEEDBACK_CHANGED mapped={} empty={}",
                    label.is_mapped(),
                    message.is_empty()
                );
            }
            if !message.is_empty() {
                let label = label.downgrade();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(100),
                    move || {
                        let Some(label) = label
                            .upgrade()
                            .filter(|label| label.is_mapped() && label.text() == message)
                        else {
                            return;
                        };
                        // Newly shown labels may not yet have a registered AT-SPI
                        // context. The window's existing context is stable.
                        let Some(window) = label.root().and_downcast::<gtk::Window>() else {
                            return;
                        };
                        unsafe {
                            if std::env::var_os("OMEN_REPORT_READY").is_some() {
                                eprintln!("OMEN_A11Y_ANNOUNCEMENT_REQUESTED");
                            }
                            // LOW queues the update after current speech. Only action
                            // feedback uses this helper, never continuously polled data.
                            announce(
                                window.upcast_ref::<gtk::Accessible>().to_glib_none().0,
                                message.as_ptr(),
                                0,
                            );
                        }
                    },
                );
            }
        });
    } else if std::env::var_os("OMEN_REPORT_READY").is_some() {
        eprintln!("OMEN_A11Y_ANNOUNCEMENT_API_UNAVAILABLE");
    }
}

pub fn search_action(page: &gtk::Widget, search: &gtk::SearchEntry) {
    let group = gtk::gio::SimpleActionGroup::new();
    let action = gtk::gio::SimpleAction::new("focus-search", None);
    let page_weak = page.downgrade();
    let search_weak = search.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(page) = page_weak.upgrade().filter(|page| page.is_mapped()) {
            if let Some(scroller) = page.downcast_ref::<gtk::ScrolledWindow>() {
                scroller
                    .vadjustment()
                    .set_value(scroller.vadjustment().lower());
            }
            let page = page.downgrade();
            let search = search_weak.clone();
            // Virtualized library controls rebind on the next layout frame.
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                if page.upgrade().is_some_and(|page| page.is_mapped()) {
                    if let Some(search) = search.upgrade().filter(|search| search.is_mapped()) {
                        search.grab_focus();
                    }
                }
            });
        }
    });
    group.add_action(&action);
    page.insert_action_group("page", Some(&group));
}
