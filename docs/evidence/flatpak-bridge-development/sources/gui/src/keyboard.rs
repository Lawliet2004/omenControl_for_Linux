//! Window-local navigation. No global input grab or hardware action.
use gtk::prelude::*;
use libadwaita::{ApplicationWindow, HeaderBar, OverlaySplitView};

fn content(stack: &gtk::Stack, split: &OverlaySplitView) {
    if split.is_collapsed() {
        split.set_show_sidebar(false);
    }
    if let Some(page) = stack.visible_child() {
        if !page.child_focus(gtk::DirectionType::TabForward) {
            page.grab_focus();
        }
    }
}

pub fn install(
    window: &ApplicationWindow,
    app: &libadwaita::Application,
    header_controls: (&HeaderBar, &gtk::Button),
    nav: &gtk::ListBox,
    stack: &gtk::Stack,
    split: &OverlaySplitView,
    pages: &[(&str, &str, &str)],
) {
    let (header, first_header) = header_controls;
    nav.update_property(&[
        gtk::accessible::Property::Label("Pages"),
        gtk::accessible::Property::Description(
            "Use the arrow keys to choose a page, then Tab or F6 to enter its content.",
        ),
    ]);
    for (index, (id, _, _)) in pages.iter().enumerate() {
        let action = gtk::gio::SimpleAction::new(&format!("page-{id}"), None);
        let nav = nav.downgrade();
        let stack = stack.downgrade();
        let split = split.downgrade();
        action.connect_activate(move |_, _| {
            if let (Some(nav), Some(stack), Some(split)) =
                (nav.upgrade(), stack.upgrade(), split.upgrade())
            {
                nav.select_row(nav.row_at_index(index as i32).as_ref());
                content(&stack, &split);
            }
        });
        window.add_action(&action);
        app.set_accels_for_action(
            &format!("win.page-{id}"),
            &[&format!("<Control>{}", index + 1)],
        );
    }
    let find = gtk::gio::SimpleAction::new("find", None);
    {
        let stack = stack.downgrade();
        find.connect_activate(move |_, _| {
            if let Some(page) = stack.upgrade().and_then(|stack| stack.visible_child()) {
                let _ = page.activate_action("page.focus-search", None);
            }
        });
    }
    window.add_action(&find);
    app.set_accels_for_action("win.find", &["<Control>f"]);
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let stack = stack.downgrade();
        let split = split.downgrade();
        let nav = nav.downgrade();
        let header = header.downgrade();
        let first = first_header.downgrade();
        let window = window.downgrade();
        key.connect_key_pressed(move |_, key, _, modifiers| {
            let (Some(stack), Some(split), Some(nav), Some(header), Some(first), Some(window)) = (
                stack.upgrade(),
                split.upgrade(),
                nav.upgrade(),
                header.upgrade(),
                first.upgrade(),
                window.upgrade(),
            ) else {
                return gtk::glib::Propagation::Proceed;
            };
            let focus = gtk::prelude::RootExt::focus(&window);
            let within = |parent: &gtk::Widget| {
                focus
                    .as_ref()
                    .is_some_and(|focus| focus == parent || focus.is_ancestor(parent))
            };
            if key == gtk::gdk::Key::F6
                && !modifiers.intersects(
                    gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK,
                )
            {
                let current = if within(header.upcast_ref()) {
                    0
                } else if within(nav.upcast_ref()) {
                    1
                } else {
                    2
                };
                let next = (current
                    + if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                        2
                    } else {
                        1
                    })
                    % 3;
                match next {
                    0 => {
                        if split.is_collapsed() {
                            split.set_show_sidebar(false);
                        }
                        first.grab_focus();
                    }
                    1 => {
                        split.set_show_sidebar(true);
                        if let Some(row) = nav.selected_row().or_else(|| nav.row_at_index(0)) {
                            row.grab_focus();
                        }
                    }
                    _ => content(&stack, &split),
                }
                return gtk::glib::Propagation::Stop;
            }
            // GTK 4.12 lacks ListBox's later single-item Tab behavior. Treat
            // navigation as one stop; arrows select destinations inside it.
            if matches!(key, gtk::gdk::Key::Tab | gtk::gdk::Key::ISO_Left_Tab)
                && within(nav.upcast_ref())
                && !modifiers.intersects(
                    gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK,
                )
            {
                if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                    header.child_focus(gtk::DirectionType::TabBackward);
                } else {
                    content(&stack, &split);
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }
    window.add_controller(key);
    {
        let stack = stack.downgrade();
        let split = split.downgrade();
        nav.connect_row_activated(move |_, _| {
            if let (Some(stack), Some(split)) = (stack.upgrade(), split.upgrade()) {
                content(&stack, &split);
            }
        });
    }
}
