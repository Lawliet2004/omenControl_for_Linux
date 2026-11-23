//! Virtualized library. Only bound GTK list items own controls. Search and
//! expanded setup belong to the model, so recycling never changes game identity.
use super::{game_profile_options, library_locations, section, GameRecovery, Ui, STALE_REASON};
use crate::i18n::tr;
use gtk::prelude::*;
use omen_core::{config::OmenConfig, games::Game, profile::ProfileKind};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

struct Entry {
    game: Option<Game>,
    searchable: String,
}
fn item(game: Option<Game>) -> gtk::glib::BoxedAnyObject {
    let searchable = game
        .as_ref()
        .map(|g| format!("{} {}", g.name, g.id).to_lowercase())
        .unwrap_or_default();
    gtk::glib::BoxedAnyObject::new(Entry { game, searchable })
}

fn game_row(
    ui: &Rc<Ui>,
    g: &Game,
    config: &Rc<RefCell<OmenConfig>>,
    recovery: &GameRecovery,
    expanded: &Rc<RefCell<BTreeSet<String>>>,
) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    let description = if let Some(root) = &g.library_root {
        format!("{}\n{}\nLibrary: {}", g.name, g.id, root.display())
    } else {
        format!("{}\n{}", g.name, g.id)
    };
    let name = gtk::Label::builder()
        .label(description)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .hexpand(true)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    let bound = config.borrow().games.per_game_profiles.get(&g.id).copied();
    let options = game_profile_options(&ui.latest_caps.borrow().profile_choices, bound);
    let labels: Vec<&str> = options.iter().map(|(label, _)| label.as_str()).collect();
    let picker = gtk::DropDown::from_strings(&labels);
    picker.update_property(&[gtk::accessible::Property::Label(&format!(
        "Profile for {}",
        g.name
    ))]);
    let initial = options
        .iter()
        .position(|(_, value)| *value == bound)
        .unwrap_or(0) as u32;
    picker.set_selected(initial);
    let options = Rc::new(RefCell::new(options));
    let selected_profile = Rc::new(std::cell::Cell::new(bound));
    let updating = Rc::new(std::cell::Cell::new(false));
    {
        let picker = picker.downgrade();
        let options = options.clone();
        let selected_profile = selected_profile.clone();
        let updating = updating.clone();
        ui.register(&picker.upgrade().unwrap(), move |snapshot| {
            let Some(picker) = picker.upgrade() else {
                return;
            };
            let next = game_profile_options(
                &snapshot.capabilities.profile_choices,
                selected_profile.get(),
            );
            if *options.borrow() == next {
                return;
            }
            let labels: Vec<_> = next.iter().map(|(label, _)| label.as_str()).collect();
            let model = gtk::StringList::new(&labels);
            let selected = next
                .iter()
                .position(|(_, p)| *p == selected_profile.get())
                .unwrap_or(0) as u32;
            *options.borrow_mut() = next;
            updating.set(true);
            picker.set_model(Some(&model));
            picker.set_selected(selected);
            updating.set(false);
        });
    }

    let reveal = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(160)
        .build();
    let open = expanded.borrow().contains(&g.id);
    reveal.set_reveal_child(open);
    if open {
        reveal.set_child(Some(&crate::game_sessions::setup(ui, g)));
    }
    let setup = gtk::Button::with_label("Profile setup");
    setup.set_tooltip_text(Some(&format!(
        "Configure automatic restoration for {}",
        g.name
    )));
    setup.update_relation(&[gtk::accessible::Relation::DescribedBy(&[name.upcast_ref()])]);
    setup.update_property(&[gtk::accessible::Property::Label(&format!(
        "Profile setup for {}",
        g.name
    ))]);
    {
        let ui = ui.clone();
        let game = g.clone();
        let expanded = expanded.clone();
        let reveal = reveal.clone();
        setup.connect_clicked(move |_| {
            let open = !reveal.reveals_child();
            if open {
                expanded.borrow_mut().insert(game.id.clone());
            } else {
                expanded.borrow_mut().remove(&game.id);
            }
            if open {
                reveal.set_child(Some(&crate::game_sessions::setup(&ui, &game)));
            }
            reveal.set_reveal_child(open);
        });
    }

    let launch = gtk::Button::with_label(&tr("Launch"));
    launch.add_css_class("suggested-action");
    launch.set_sensitive(false);
    {
        let button = launch.downgrade();
        ui.register(&launch, move |snapshot| {
            if let Some(button) = button.upgrade() {
                button.set_sensitive(snapshot.error.as_deref() != Some(STALE_REASON));
            }
        });
    }
    let ui2 = ui.clone();
    let id = g.id.clone();
    let recovery = recovery.clone();
    launch.connect_clicked(move |_| {
        let id = id.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        ui2.run_hardware_task("Launch game".into(), move |source| {
            let receipt = source.launch_game(&id)?;
            if receipt.profile_was_applied {
                if let (Some(original), Some(requested)) = (receipt.original_profile, receipt.requested_profile) {
                    let _ = tx.send((original, requested));
                }
            }
            Ok(receipt.message())
        });
        let recovery = recovery.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            match rx.try_recv() {
                Ok((original, expected)) => {
                    recovery.target.set(Some((original, expected)));
                    recovery.label.set_text(&format!("This launch selected {expected}. Restore the previous {original} profile when you finish. Another game may still be using the current profile."));
                    recovery.button.set_label(&format!("Restore {original}"));
                    recovery.widget.set_visible(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => gtk::glib::ControlFlow::Break,
                Err(_) => gtk::glib::ControlFlow::Continue,
            }
        });
    });

    let ui3 = ui.clone();
    let binding_config = config.clone();
    let id_for_bind = g.id.clone();
    let reverting = updating.clone();
    let reveal_for_picker = reveal.clone();
    let game_for_picker = g.clone();
    picker.connect_selected_notify(move |dd| {
        if reverting.get() {
            return;
        }
        let Some(profile) = options
            .borrow()
            .get(dd.selected() as usize)
            .map(|(_, p)| *p)
        else {
            return;
        };
        let settings = omen_ipc::settings::SettingsController::new(
            ui3.source.daemon().cloned(),
            omen_core::config::OmenConfig::path(),
        );
        if let Err(e) = settings.set_game_profile(&id_for_bind, profile) {
            reverting.set(true);
            dd.set_selected(
                options
                    .borrow()
                    .iter()
                    .position(|(_, p)| *p == selected_profile.get())
                    .unwrap_or(0) as u32,
            );
            reverting.set(false);
            ui3.status_label
                .set_text(&format!("Could not save binding: {e}"));
        } else {
            selected_profile.set(profile);
            if let Some(profile) = profile {
                binding_config
                    .borrow_mut()
                    .games
                    .per_game_profiles
                    .insert(id_for_bind.clone(), profile);
            } else {
                binding_config
                    .borrow_mut()
                    .games
                    .per_game_profiles
                    .remove(&id_for_bind);
            }
            if !matches!(profile, Some(ProfileKind::Performance | ProfileKind::Quiet)) {
                binding_config
                    .borrow_mut()
                    .games
                    .managed_game_ids
                    .remove(&id_for_bind);
            }
            ui3.status_label.set_text(&tr("Per-game profile saved"));
            if reveal_for_picker.reveals_child() {
                reveal_for_picker
                    .set_child(Some(&crate::game_sessions::setup(&ui3, &game_for_picker)));
            }
        }
    });
    content.append(&name);
    let actions = crate::widgets::action_row();
    crate::widgets::append_action(&actions, &picker);
    crate::widgets::append_action(&actions, &launch);
    crate::widgets::append_action(&actions, &setup);
    content.append(&actions.widget);
    content.append(&reveal);

    content.add_css_class("card");
    content.upcast()
}

struct Library {
    store: gtk::gio::ListStore,
    games: RefCell<Vec<Game>>,
    config: Rc<RefCell<OmenConfig>>,
    expanded: Rc<RefCell<BTreeSet<String>>>,
}

impl Library {
    fn replace(&self, games: Vec<Game>, config: OmenConfig) {
        let old = self.games.borrow();
        let old_config = self.config.borrow();
        let same = |a: &Game, b: &Game| {
            a == b
                && old_config.games.per_game_profiles.get(&a.id)
                    == config.games.per_game_profiles.get(&b.id)
                && old_config.games.managed_game_ids.contains(&a.id)
                    == config.games.managed_game_ids.contains(&b.id)
        };
        // Keep identical prefix/suffix objects. A no-change rescan sends no
        // items-changed signal and retains focused buttons, text and open setup.
        let prefix = old
            .iter()
            .zip(&games)
            .take_while(|(a, b)| same(a, b))
            .count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(games[prefix..].iter().rev())
            .take_while(|(a, b)| same(a, b))
            .count();
        let remove = old.len() - prefix - suffix;
        let additions: Vec<_> = games[prefix..games.len() - suffix]
            .iter()
            .cloned()
            .map(|g| item(Some(g)))
            .collect();
        drop(old_config);
        drop(old);
        let ids: BTreeSet<_> = games.iter().map(|g| g.id.as_str()).collect();
        self.expanded
            .borrow_mut()
            .retain(|id| ids.contains(id.as_str()));
        *self.config.borrow_mut() = config;
        *self.games.borrow_mut() = games;
        if remove != 0 || !additions.is_empty() {
            self.store
                .splice(prefix as u32 + 1, remove as u32, &additions);
        }
    }
}

fn summary(label: &gtk::Label, total: u32, matching: u32) {
    label.set_text(if total == 0 {
        "No games found. Check your launcher installations or add a custom command in user settings."
    } else if matching == 0 {
        "No games match this search."
    } else { "" });
}

pub fn build(ui: &Rc<Ui>) -> gtk::Widget {
    let header = section(&tr("Game library"));
    header.set_margin_bottom(14);
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let search = gtk::SearchEntry::builder()
        .placeholder_text(tr("Search games or sources"))
        .hexpand(true)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search games")]);
    let rescan = gtk::Button::with_label(&tr("Rescan library"));
    controls.append(&search);
    controls.append(&rescan);
    header.append(&controls);
    header.append(&library_locations(ui, &rescan));
    header.append(&crate::game_sessions::panel(ui));
    let recovery = GameRecovery::new(ui);
    header.append(&recovery.widget);
    let scan_status = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim"])
        .build();
    header.append(&scan_status);
    let filter_status = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim"])
        .build();
    header.append(&filter_status);
    let help = gtk::Expander::builder()
        .label("About launch and profile modes")
        .build();
    help.set_child(Some(&gtk::Label::builder()
        .label(tr("Games launch as your desktop user. A launch request does not confirm the game started. Manual mode applies a bound profile during submission and needs manual restoration. Automatic mode tracks the actual command through its configured wrapper and releases only its own profile hold after the process tree ends. Open Profile setup for requirements."))
        .wrap(true).xalign(0.0).css_classes(["dim"]).build()));
    crate::widgets::disclosure(&help);
    header.append(&help);

    let state = Rc::new(Library {
        store: gtk::gio::ListStore::new::<gtk::glib::BoxedAnyObject>(),
        games: RefCell::new(Vec::new()),
        config: Rc::new(RefCell::new(OmenConfig::default())),
        expanded: Rc::new(RefCell::new(BTreeSet::new())),
    });
    // The permanent controls item keeps search/recovery available even for an
    // empty result. The ListView remains the direct scrollable child, so GTK
    // can virtualize the entire library with one scrollbar at every width.
    state.store.append(&item(None));
    let query = Rc::new(RefCell::new(String::new()));
    let filter = gtk::CustomFilter::new({
        let query = query.clone();
        move |object| {
            let entry = object
                .downcast_ref::<gtk::glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<Entry>();
            entry.game.is_none() || entry.searchable.contains(query.borrow().as_str())
        }
    });
    let filtered = gtk::FilterListModel::new(Some(state.store.clone()), Some(filter.clone()));
    let factory = gtk::SignalListItemFactory::new();
    {
        let ui = ui.clone();
        let state = state.clone();
        factory.connect_bind(move |_, object| {
            let list_item = object.downcast_ref::<gtk::ListItem>().unwrap();
            let entry = list_item
                .item()
                .unwrap()
                .downcast::<gtk::glib::BoxedAnyObject>()
                .unwrap();
            let entry = entry.borrow::<Entry>();
            list_item.set_activatable(false);
            list_item.set_selectable(false);
            if let Some(game) = &entry.game {
                list_item.set_child(Some(&game_row(
                    &ui,
                    game,
                    &state.config,
                    &recovery,
                    &state.expanded,
                )));
            } else {
                list_item.set_child(Some(&header));
            }
        });
    }
    factory.connect_unbind(|_, object| {
        object
            .downcast_ref::<gtk::ListItem>()
            .unwrap()
            .set_child(gtk::Widget::NONE);
    });
    let view = gtk::ListView::builder()
        .model(&gtk::NoSelection::new(Some(filtered.clone())))
        .factory(&factory)
        .css_classes(["game-library"])
        .tab_behavior(gtk::ListTabBehavior::All)
        .build();
    view.update_property(&[gtk::accessible::Property::Label(
        "Game library and controls",
    )]);
    {
        let state = state.clone();
        let filter = filter.clone();
        let filtered = filtered.clone();
        let filter_status = filter_status.clone();
        search.connect_search_changed(move |entry| {
            *query.borrow_mut() = entry.text().trim().to_lowercase();
            filter.changed(gtk::FilterChange::Different);
            summary(
                &filter_status,
                state.store.n_items().saturating_sub(1),
                filtered.n_items().saturating_sub(1),
            );
        });
    }
    {
        let ui = ui.clone();
        let state = state.clone();
        rescan.connect_clicked(move |button| {
            button.set_sensitive(false);
            scan_status.set_text(&tr("Scanning game libraries…"));
            let source = ui.source.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = source.games().and_then(|games| OmenConfig::load().map(|config| (games, config)));
                let _ = tx.send(result);
            });
            let state = state.clone();
            let button = button.clone();
            let scan_status = scan_status.clone();
            let filter_status = filter_status.clone();
            let filtered = filtered.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                match rx.try_recv() {
                    Ok(Ok((games, config))) => {
                        let count = games.len();
                        state.replace(games, config);
                        summary(&filter_status, count as u32, filtered.n_items().saturating_sub(1));
                        scan_status.set_text(&format!("Found {count} {} from Steam, Heroic, Lutris, Legendary and custom entries. Missing or unreadable store records are skipped.", if count == 1 {"game"} else {"games"}));
                        button.set_sensitive(true);
                        gtk::glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        scan_status.set_text(&format!("Library scan failed: {error}. Existing results are retained."));
                        button.set_sensitive(true);
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        scan_status.set_text(&tr("Library scan stopped unexpectedly. Existing results are retained."));
                        button.set_sensitive(true);
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                }
            });
        });
    }
    rescan.emit_clicked();
    let page = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&view)
        .build()
        .upcast();
    crate::widgets::search_action(&page, &search);
    page
}
