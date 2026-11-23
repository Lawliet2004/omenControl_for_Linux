//! Desktop report history stays separate from daemon-owned session authority.
use crate::pages::Ui;
use gtk::prelude::*;
use omen_client::launch_issues::{History, Issue};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};

fn label(value: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(value)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .selectable(true)
        .max_width_chars(85)
        .build()
}
fn text(label: &gtk::Label, value: &str) {
    if label.text() != value {
        label.set_text(value);
    }
}
struct Row {
    row: gtk::ListBoxRow,
    title: gtk::Label,
    detail: gtk::Label,
    meta: gtk::Label,
}
struct Cache {
    generation: u64,
    saved: Vec<Issue>,
    error: Option<String>,
    alerted: BTreeSet<String>,
    rows: BTreeMap<String, Row>,
}
struct View {
    root: gtk::glib::WeakRef<gtk::Box>,
    summary: gtk::Label,
    list: gtk::ListBox,
    details: gtk::Expander,
    history: History,
    cache: RefCell<Cache>,
    status: gtk::glib::WeakRef<gtk::Label>,
    started: u64,
}
impl View {
    fn render(&self) {
        let live = self.history.live();
        let mut cache = self.cache.borrow_mut();
        let mut merged: BTreeMap<_, _> = cache
            .saved
            .iter()
            .cloned()
            .map(|i| (i.id.clone(), i))
            .collect();
        let live_ids: BTreeSet<_> = live
            .iter()
            .filter(|i| !merged.contains_key(&i.id))
            .map(|i| i.id.clone())
            .collect();
        for issue in live {
            merged.entry(issue.id.clone()).or_insert(issue);
        }
        let mut issues: Vec<_> = merged.into_values().collect();
        issues.sort_by(|a, b| (b.occurred_unix_ms, &b.id).cmp(&(a.occurred_unix_ms, &a.id)));
        issues.truncate(64);
        let Some(root) = self.root.upgrade() else {
            return;
        };
        root.set_visible(!issues.is_empty() || cache.error.is_some());
        if let Some(error) = &cache.error {
            text(&self.summary,&format!("Saved launch issues unavailable: {error}. Showing any live or last-loaded reports. Clear launch issues can reset damaged data; permission errors need repair first."));
        } else if let Some(issue) = issues.first() {
            text(
                &self.summary,
                &format!(
                    "{} · {}: {}",
                    issue.game_id,
                    issue.stage.label(),
                    issue.reason.description()
                ),
            );
        } else {
            text(&self.summary, "No desktop launch issues.");
        }
        let wanted: BTreeSet<_> = issues.iter().map(|i| i.id.clone()).collect();
        cache.rows.retain(|id, row| {
            if wanted.contains(id.as_str()) {
                true
            } else {
                self.list.remove(&row.row);
                false
            }
        });
        for (index, issue) in issues.iter().enumerate() {
            let row = cache.rows.entry(issue.id.clone()).or_insert_with(|| {
                let content = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(6)
                    .margin_top(10)
                    .margin_bottom(10)
                    .margin_start(10)
                    .margin_end(10)
                    .build();
                let title = label("");
                title.add_css_class("heading");
                let detail = label("");
                let meta = label("");
                meta.add_css_class("dim");
                content.append(&title);
                content.append(&detail);
                content.append(&meta);
                let row = gtk::ListBoxRow::builder()
                    .child(&content)
                    .activatable(false)
                    .selectable(false)
                    .build();
                self.list.insert(&row, index as i32);
                Row {
                    row,
                    title,
                    detail,
                    meta,
                }
            });
            if row.row.index() != index as i32 {
                self.list.remove(&row.row);
                self.list.insert(&row.row, index as i32);
            }
            text(
                &row.title,
                &format!("{} · issue {}", issue.game_id, &issue.id[..8]),
            );
            text(&row.detail, &issue.reason.description());
            let when = i64::try_from(issue.occurred_unix_ms / 1000)
                .ok()
                .and_then(|time| gtk::glib::DateTime::from_unix_local(time).ok())
                .and_then(|time| time.format("%Y-%m-%d %H:%M").ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Time unavailable".into());
            text(
                &row.meta,
                &format!(
                    "{} · {} · {}{}",
                    issue.stage.label(),
                    when,
                    if live_ids.contains(&issue.id) {
                        "Live desktop report; not yet saved"
                    } else {
                        "Saved desktop report"
                    },
                    issue
                        .scope_token
                        .as_ref()
                        .map(|s| format!(" · related scope {}", &s[..8]))
                        .unwrap_or_default()
                ),
            );
        }
        self.details
            .set_label(Some(&format!("Recent desktop reports ({})", issues.len())));
        self.details.set_visible(!issues.is_empty());
        if let Some(issue) = issues
            .iter()
            .find(|i| i.occurred_unix_ms >= self.started && !cache.alerted.contains(&i.id))
            .cloned()
        {
            if let Some(status) = self.status.upgrade() {
                status.set_text(&format!(
                    "Game launch issue for {}: {} Open Game Library to review launch issues.",
                    issue.game_id,
                    issue.reason.description()
                ));
            }
        }
        for issue in issues {
            cache.alerted.insert(issue.id);
        }
        // Retain only a bounded set of identifiers, as well as bounded reports.
        if cache.alerted.len() > 128 {
            cache.alerted = wanted;
        }
    }
}

pub fn panel(ui: &Rc<Ui>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_visible(false);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = label("Launch issues");
    title.set_hexpand(true);
    title.add_css_class("section-title");
    let clear = gtk::Button::with_label("Clear launch issues");
    clear.set_tooltip_text(Some(
        "Clear desktop reports without changing games, profiles or daemon session history",
    ));
    heading.append(&title);
    heading.append(&clear);
    root.append(&heading);
    let summary = label("");
    root.append(&summary);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let details = gtk::Expander::builder()
        .label("Recent desktop reports")
        .child(&list)
        .build();
    crate::widgets::disclosure(&details);
    root.append(&details);
    root.append(&label("Desktop reports describe launch processes. They do not establish game lifetime or profile ownership; review Game sessions and the current system profile separately. Command arguments and output are not stored."));
    let view = Rc::new(View {
        root: root.downgrade(),
        summary,
        list,
        details,
        history: History::for_config(&omen_core::config::OmenConfig::path()),
        cache: RefCell::new(Cache {
            generation: 0,
            saved: Vec::new(),
            error: None,
            alerted: BTreeSet::new(),
            rows: BTreeMap::new(),
        }),
        status: ui.status_label.downgrade(),
        started: omen_client::launch_issues::now_ms(),
    });
    let refresh = gtk::Button::new();
    {
        let view = view.clone();
        refresh.connect_clicked(move |button| {
            // Live in-process reports remain usable even behind a slow disk.
            view.render();
            if !button.is_sensitive() {
                return;
            }
            button.set_sensitive(false);
            let generation = view.cache.borrow().generation;
            let history = view.history.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(history.read());
            });
            let view = view.clone();
            let button = button.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
                Ok(result) => {
                    let mut cache = view.cache.borrow_mut();
                    if generation == cache.generation {
                        match result {
                            Ok(saved) => {
                                cache.saved = saved;
                                cache.error = None;
                            }
                            Err(error) => cache.error = Some(error.to_string()),
                        };
                    }
                    drop(cache);
                    view.render();
                    button.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    view.cache.borrow_mut().error =
                        Some("History refresh stopped; retry by reopening Game Library.".into());
                    view.render();
                    button.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(_) => gtk::glib::ControlFlow::Continue,
            });
        });
    }
    {
        let view = view.clone();
        let refresh = refresh.clone();
        clear.connect_clicked(move |button| {
            if !button.is_sensitive() {
                return;
            }
            button.set_sensitive(false);
            let history = view.history.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = history.clear();
                if result.is_ok() {
                    history.clear_live();
                }
                let _ = tx.send(result);
            });
            let view = view.clone();
            let button = button.clone();
            let refresh = refresh.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
                Ok(result) => {
                    if result.is_ok() {
                        let mut cache = view.cache.borrow_mut();
                        cache.generation = cache.generation.wrapping_add(1);
                        cache.saved.clear();
                        cache.error = None;
                    }
                    view.render();
                    if let Some(status) = view.status.upgrade() {
                        status.set_text(&match result {
                            Ok(()) => "Desktop launch issues cleared. Games, profiles and daemon sessions were not changed.".into(),
                            Err(error) => format!("Could not clear launch issues: {error}"),
                        });
                    }
                    button.set_sensitive(true);
                    refresh.emit_clicked();
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(_) => gtk::glib::ControlFlow::Continue,
            });
        });
    }
    refresh.emit_clicked();
    let anchor = ui.status_label.downgrade();
    let weak = view.root.clone();
    gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
        if weak.upgrade().is_none() {
            return gtk::glib::ControlFlow::Break;
        }
        if anchor
            .upgrade()
            .and_then(|label| label.root())
            .and_downcast::<gtk::Window>()
            .is_some_and(|window| window.is_mapped())
        {
            refresh.emit_clicked();
        }
        gtk::glib::ControlFlow::Continue
    });
    // The timer owns the refresh controller; this weak render does not create a
    // widget self-cycle. It stops when the application content is destroyed.
    root
}
