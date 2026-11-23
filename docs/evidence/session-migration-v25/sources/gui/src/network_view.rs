//! Keep native focus and accessibility identities while network values change.
use super::Ui;
use gtk::prelude::*;
use omen_core::network::{NetworkSnapshot, NetworkSource};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

type Key = (bool, u32, String);
struct Row {
    root: gtk::Box,
    heading: gtk::Label,
    detail: gtk::Label,
    held: gtk::Label,
}
#[derive(Default)]
struct Rows {
    known: BTreeMap<Key, Row>,
    order: Vec<Key>,
}
fn text(label: &gtk::Label, value: &str) {
    if label.text() != value {
        label.set_text(value);
    }
}
impl Rows {
    fn clear(&mut self, list: &gtk::Box) {
        for (_, row) in std::mem::take(&mut self.known) {
            list.remove(&row.root);
        }
        self.order.clear();
    }
    fn update(&mut self, list: &gtk::Box, report: &NetworkSnapshot) {
        let key = |i: &omen_core::network::NetworkInterface| {
            (
                report.source == NetworkSource::Fixture,
                i.ifindex,
                i.name.clone(),
            )
        };
        let wanted: BTreeSet<_> = report.interfaces.iter().map(key).collect();
        self.known.retain(|key, row| {
            if wanted.contains(key) {
                true
            } else {
                list.remove(&row.root);
                false
            }
        });
        let count = |value: Option<u64>| value.map(|v| v.to_string()).unwrap_or_else(|| "—".into());
        for interface in &report.interfaces {
            let row = self.known.entry(key(interface)).or_insert_with(|| {
                let root = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(4)
                    .css_classes(["sensor-row"])
                    .build();
                let heading = gtk::Label::builder()
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .css_classes(["heading"])
                    .build();
                let detail = gtk::Label::builder()
                    .xalign(0.0)
                    .wrap(true)
                    .selectable(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .build();
                let held = gtk::Label::builder()
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .css_classes(["dim"])
                    .visible(false)
                    .build();
                crate::widgets::announce_changes(&held);
                root.append(&heading);
                root.append(&detail);
                root.append(&held);
                list.append(&root);
                Row {
                    root,
                    heading,
                    detail,
                    held,
                }
            });
            text(
                &row.heading,
                &format!(
                    "{}{} · {}",
                    interface.name,
                    if interface.loopback == Some(true) {
                        " (loopback)"
                    } else {
                        ""
                    },
                    interface.link_state.as_deref().unwrap_or("unknown")
                ),
            );
            let value = format!(
                "Receive: {}  ·  Send: {}\nErrors RX/TX: {} / {}  ·  Dropped RX/TX: {} / {}",
                omen_core::network::format_rate(interface.rx_bytes, interface.rx_bytes_per_second),
                omen_core::network::format_rate(interface.tx_bytes, interface.tx_bytes_per_second),
                count(interface.rx_errors),
                count(interface.tx_errors),
                count(interface.rx_dropped_or_missed),
                count(interface.tx_dropped)
            );
            // A selectable live label otherwise cancels the copy selection on
            // every changed acquisition. Keep a named snapshot only while the
            // user is selecting this reading; all monitoring continues. Missing
            // data or interface replacement still removes the old row immediately.
            if row.detail.has_focus() && row.detail.selection_bounds().is_some() {
                if row.detail.text() != value {
                    row.held.set_visible(true);
                    text(&row.held, "Reading held for selection. Clear the selection or leave this reading to resume live updates.");
                }
            } else {
                text(&row.detail, &value);
                row.held.set_visible(false);
                text(&row.held, "");
            }
        }
        let order: Vec<_> = report.interfaces.iter().map(key).collect();
        if self.order != order {
            let mut previous: Option<&gtk::Box> = None;
            for key in &order {
                let row = &self.known[key];
                list.reorder_child_after(&row.root, previous);
                previous = Some(&row.root);
            }
            self.order = order;
        }
    }
}

pub fn bind(ui: &Rc<Ui>, page: &impl IsA<gtk::Widget>, list: gtk::Box, status: gtk::Label) {
    let rows = RefCell::new(Rows::default());
    let origin = RefCell::new(crate::data::SourceTracker::default());
    ui.register(page, move |snapshot| {
        if origin.borrow_mut().changed(&snapshot.origin) {
            rows.borrow_mut().clear(&list);
        }
        let Some(report) = &snapshot.network else {
            rows.borrow_mut().clear(&list);
            text(
                &status,
                &format!(
                    "Network monitoring unavailable: {}",
                    snapshot
                        .network_error
                        .as_deref()
                        .unwrap_or("no counter report")
                ),
            );
            return;
        };
        let mut message = if report.source == NetworkSource::Fixture {
            "Fixture network counters"
        } else {
            "Kernel network counters"
        }
        .to_string();
        if report.interfaces.is_empty() {
            message.push_str(": no readable interfaces");
        }
        if report.omitted_interfaces > 0 || report.unreadable_interfaces > 0 {
            message.push_str(&format!(
                "; {} omitted, {} unreadable",
                report.omitted_interfaces, report.unreadable_interfaces
            ));
        }
        text(&status, &message);
        rows.borrow_mut().update(&list, report);
    });
}
