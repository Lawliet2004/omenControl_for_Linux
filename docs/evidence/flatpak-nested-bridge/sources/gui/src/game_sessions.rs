//! Native per-game setup and cached session status. No game or control work
//! runs on GTK's main thread; existing widgets survive status refreshes.
use crate::pages::Ui;
use gtk::prelude::*;
use omen_core::games::{Game, GameSession, GameSessionReport, GameSource, SessionPhase};
use omen_core::profile::ProfileKind;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .max_width_chars(85)
        .build()
}
fn set_text(label: &gtk::Label, text: &str) {
    if label.text() != text {
        label.set_text(text);
    }
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn mode_message(enabled: bool, sandboxed: bool) -> &'static str {
    match (enabled,sandboxed) {
        (true,true)=>"Automatic restore is saved, but wrapping for this sandboxed installation is unavailable. Choose manual restore to apply a launch profile.",
        (true,false)=>"Automatic restore is enabled. The actual game command must run through its wrapper.",
        (false,true)=>"Manual restore is selected. Automatic wrapping for this sandboxed installation is not available.",
        (false,false)=>"Manual restore is selected. Enable automatic restore after choosing Performance or Quiet and setting up the wrapper below.",
    }
}

pub fn setup(ui: &Rc<Ui>, game: &Game) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let config = match omen_core::config::OmenConfig::load() {
        Ok(c) => c,
        Err(error) => {
            panel.append(&label(&format!("Could not read profile setup: {error}")));
            return panel;
        }
    };
    let enabled = config.games.managed_game_ids.contains(&game.id);
    let supported = matches!(
        config.games.per_game_profiles.get(&game.id),
        Some(ProfileKind::Performance | ProfileKind::Quiet)
    );
    let sandboxed = game.id.contains("/flatpak/") || game.id.starts_with("steam/snap/");
    let mode = label(mode_message(enabled, sandboxed));
    mode.add_css_class("heading");
    panel.append(&mode);
    let help=label("Automatic restore uses Power Profiles Daemon and a dedicated systemd user scope. It needs an enabled, authorized daemon, a user systemd session and a supported kernel. A canceled hold is never automatically reapplied. Other profile holds can change the effective profile.");
    help.add_css_class("dim");
    panel.append(&help);
    let toggle = gtk::Button::with_label(if enabled {
        "Use manual restore"
    } else {
        "Enable automatic restore"
    });
    toggle.set_halign(gtk::Align::Start);
    toggle.set_sensitive(enabled || (supported && !sandboxed));
    if !supported {
        toggle.set_tooltip_text(Some("Choose Performance or Quiet in the profile selector first. Balanced and Cool cannot be held by Power Profiles Daemon."));
    }
    if sandboxed {
        toggle.set_tooltip_text(Some("Automatic wrapping inside this sandboxed installation is not validated; use manual restoration."));
        panel.append(&label("Automatic wrapping inside Flatpak or Snap is not validated. Use manual restoration for this installation; a host-spawned launcher is not proof of game lifetime."));
    }
    let feedback = label("");
    crate::widgets::announce_changes(&feedback);
    {
        let id = game.id.clone();
        let mode = mode.clone();
        let feedback = feedback.clone();
        let active = std::cell::Cell::new(enabled);
        toggle.connect_clicked(move|button|{
            let next=!active.get();
            let settings=omen_ipc::settings::SettingsController::new(None,omen_core::config::OmenConfig::path());
            match settings.set_game_managed(&id,next) {
                Ok(())=>{active.set(next);button.set_label(if next{"Use manual restore"}else{"Enable automatic restore"});mode.set_text(mode_message(next,sandboxed));feedback.set_text("Saved for future launches. Active sessions keep their current ownership; use Release profile hold to end it now.");}
                Err(error)=>feedback.set_text(&format!("Profile mode was not saved: {error}")),
            }
        });
    }
    panel.append(&toggle);
    if game.source == GameSource::Custom {
        panel.append(&label("Custom commands are wrapped automatically when launched here. Use the actual game command; a command that dispatches to an existing launcher outside the managed scope cannot establish game lifetime."));
    } else if !sandboxed {
        let executable = std::env::current_exe()
            .map(|p| p.with_file_name("omen-cli"))
            .unwrap_or_else(|_| std::path::PathBuf::from("/usr/bin/omen-cli"));
        let prefix = format!(
            "{} game-run {} --",
            quote(&executable.to_string_lossy()),
            quote(&game.id)
        );
        let text = if game.source == GameSource::Steam {
            format!("{prefix} %command%")
        } else {
            prefix
        };
        let instructions=match game.source {
            GameSource::Steam=>"Put this in the game's Steam Launch Options, keeping any required existing game arguments. Steam supplies the actual command at %command%.",
            GameSource::Lutris=>"Use this as the game's command prefix in Lutris. It must wrap the actual game command, not a lutris: URI launch request.",
            GameSource::Heroic=>"Use this prefix around the actual command in Heroic's wrapper configuration. Keep its executable and arguments separate if the launcher uses separate fields. Do not wrap a heroic: URI request.",
            GameSource::Legendary=>"Set this as the game's wrapper in Legendary's configuration. It must wrap the actual game command; wrapping a Legendary launch request alone is not lifetime evidence.",
            GameSource::Custom=>unreachable!(),
        };
        panel.append(&label(instructions));
        let command = gtk::TextView::builder()
            .editable(false)
            .accepts_tab(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        command.buffer().set_text(&text);
        command.update_property(&[gtk::accessible::Property::Label("Game wrapper command")]);
        panel.append(&command);
        let copy = gtk::Button::with_label("Copy wrapper command");
        copy.set_halign(gtk::Align::Start);
        let feedback_copy = feedback.clone();
        copy.connect_clicked(move|button|{button.clipboard().set_text(&text);feedback_copy.set_text("Wrapper command copied. Configure it in the launcher, then confirm automatic restore is enabled.");});
        panel.append(&copy);
        panel.append(&label("Launcher/runtime compatibility still needs verification for your game. If setup cannot establish a profile hold, the wrapped command reports an error; switch to manual restore to run without this integration."));
    }
    panel.append(&feedback);
    let _ = ui;
    panel
}

struct SessionRow {
    row: gtk::ListBoxRow,
    title: gtk::Label,
    summary: gtk::Label,
    detail: gtk::Label,
    release: gtk::Button,
}

fn render(
    ui: &Rc<Ui>,
    list: &gtk::ListBox,
    history_list: &gtk::ListBox,
    history: &gtk::Expander,
    rows: &Rc<RefCell<BTreeMap<String, SessionRow>>>,
    report: GameSessionReport,
    status: &gtk::Label,
) {
    if let Some(error) = report.recovery_error {
        status.set_text(&format!("Session recovery needs attention: {error}"));
    } else if report.sessions.is_empty() {
        status.set_text("No tracked sessions yet. Open a game's Profile setup to enable automatic restoration. Launcher submission alone does not create a session.");
    } else if let Some(problem) = report
        .sessions
        .first()
        .filter(|s| matches!(s.phase, SessionPhase::Failed | SessionPhase::Interrupted))
    {
        status.set_text(&format!(
            "Most recent session needs review: {} Open recent session history for details.",
            problem.detail
        ));
    } else {
        status.set_text(if report.mock {
            "Fixture sessions — no physical hardware validation."
        } else {
            "Sessions belong to your desktop user. Releasing a hold keeps the game running."
        });
    }
    let mut known = rows.borrow_mut();
    let removed: Vec<_> = known
        .keys()
        .filter(|token| !report.sessions.iter().any(|s| &s.token == *token))
        .cloned()
        .collect();
    for token in removed {
        if let Some(row) = known.remove(&token) {
            if let Some(parent) = row
                .row
                .parent()
                .and_then(|p| p.downcast::<gtk::ListBox>().ok())
            {
                parent.remove(&row.row);
            }
        }
    }
    for (index, session) in report.sessions.iter().enumerate() {
        let row = known.entry(session.token.clone()).or_insert_with(|| {
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
            let summary = label("");
            let detail = label("");
            detail.add_css_class("dim");
            let release = gtk::Button::with_label("Release profile hold");
            release.set_tooltip_text(Some(&format!(
                "Release only the profile hold for {}; keep its game running",
                session.game_id
            )));
            release.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
                title.upcast_ref(),
                summary.upcast_ref(),
            ])]);
            release.update_property(&[gtk::accessible::Property::Label(&format!(
                "Release profile hold for {}",
                session.game_id
            ))]);
            release.set_halign(gtk::Align::Start);
            let token = session.token.clone();
            let source = ui.source.borrow().clone();
            let ui = ui.clone();
            release.connect_clicked(move |button| {
                button.set_sensitive(false);
                let token = token.clone();
                let source = source.clone();
                ui.run_task("Release game profile".into(), move |_| {
                    source.release_game_session(&token)
                });
            });
            for child in [&title, &summary, &detail] {
                content.append(child);
            }
            content.append(&release);
            let row = gtk::ListBoxRow::builder()
                .activatable(false)
                .selectable(false)
                .child(&content)
                .build();
            SessionRow {
                row,
                title,
                summary,
                detail,
                release,
            }
        });
        let target = if matches!(
            session.phase,
            SessionPhase::Starting | SessionPhase::Tracking
        ) {
            list
        } else {
            history_list
        };
        if row.row.parent().as_ref() != Some(target.upcast_ref::<gtk::Widget>()) {
            if let Some(parent) = row
                .row
                .parent()
                .and_then(|p| p.downcast::<gtk::ListBox>().ok())
            {
                parent.remove(&row.row);
            }
            target.insert(&row.row, index as i32);
        }
        update_row(row, session);
    }
    let count = report
        .sessions
        .iter()
        .filter(|s| !matches!(s.phase, SessionPhase::Starting | SessionPhase::Tracking))
        .count();
    history.set_label(Some(&format!("Recent session history ({count})")));
    history.set_visible(count > 0);
    list.set_visible(
        report
            .sessions
            .iter()
            .any(|s| matches!(s.phase, SessionPhase::Starting | SessionPhase::Tracking)),
    );
}

fn update_row(row: &SessionRow, session: &GameSession) {
    let phase = match session.phase {
        SessionPhase::Starting => "Starting",
        SessionPhase::Tracking => "Tracking process tree",
        SessionPhase::Ended => "Ended",
        SessionPhase::Interrupted => "Interrupted",
        SessionPhase::Failed => "Failed",
    };
    let short: String = session.token.chars().take(8).collect();
    set_text(
        &row.title,
        &format!("{} · session {short}", session.game_id),
    );
    set_text(
        &row.summary,
        &format!(
            "{phase} · requested {} · profile hold {}",
            session.requested_profile,
            if session.profile_held {
                "active"
            } else {
                "released"
            }
        ),
    );
    set_text(&row.detail, &session.detail);
    row.release.set_sensitive(session.profile_held);
}

pub fn panel(ui: &Rc<Ui>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::builder()
        .label("Game sessions")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["section-title"])
        .build();
    let refresh = gtk::Button::with_label("Refresh sessions");
    heading.append(&title);
    heading.append(&refresh);
    panel.append(&heading);
    let effective = label("Current system profile: waiting for monitoring");
    effective.add_css_class("dim");
    panel.append(&effective);
    {
        let effective = effective.downgrade();
        ui.register(&panel, move |snapshot| {
            if let Some(label) = effective.upgrade() {
                let text = if snapshot.error.is_some() {
                    "Current system profile: unverified".into()
                } else {
                    format!(
                        "Current system profile: {}",
                        snapshot
                            .profile
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "unavailable".into())
                    )
                };
                set_text(&label, &text);
            }
        });
    }
    let status = label("Loading your session history…");
    panel.append(&status);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    panel.append(&list);
    let history_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let history = gtk::Expander::builder()
        .label("Recent session history")
        .child(&history_list)
        .visible(false)
        .build();
    crate::widgets::disclosure(&history);
    panel.append(&history);
    panel.append(&crate::launch_issues::panel(ui));
    let rows = Rc::new(RefCell::new(BTreeMap::new()));
    {
        let ui = ui.clone();
        let list = list.clone();
        let status = status.clone();
        let rows = rows.clone();
        let history_list = history_list.clone();
        let history = history.clone();
        refresh.connect_clicked(move|button|{
            if !button.is_sensitive(){return;}button.set_sensitive(false);
            let source=ui.source.borrow().clone();let identity=ui.source_identity();let(tx,rx)=std::sync::mpsc::channel();std::thread::spawn(move||{let _=tx.send(source.game_sessions());});
            let button=button.clone();let list=list.clone();let status=status.clone();let rows=rows.clone();let ui=ui.clone();let history_list=history_list.clone();let history=history.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(100),move||match rx.try_recv(){
                Ok(Ok(report)) if identity==ui.source_identity()=>{render(&ui,&list,&history_list,&history,&rows,report,&status);button.set_sensitive(true);gtk::glib::ControlFlow::Break},
                Ok(Ok(_))=>{status.set_text("Monitoring source changed; refresh sessions for the current service.");button.set_sensitive(true);gtk::glib::ControlFlow::Break},
                Ok(Err(error))=>{status.set_text(&format!("Session updates unavailable: {error}. Displayed history is not current; refresh after reconnecting."));for row in rows.borrow().values(){row.release.set_sensitive(false);}button.set_sensitive(true);gtk::glib::ControlFlow::Break},
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>{status.set_text("Session refresh stopped; retry Refresh sessions.");button.set_sensitive(true);gtk::glib::ControlFlow::Break},
                Err(_)=>gtk::glib::ControlFlow::Continue,
            });
        });
    }
    {
        let rows = rows.clone();
        let list = list.clone();
        let history_list = history_list.clone();
        let origin = RefCell::new(crate::data::SourceTracker::default());
        ui.register(&panel, move |snapshot| {
            if origin.borrow_mut().changed(&snapshot.origin) {
                rows.borrow_mut().clear();
                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }
                while let Some(child) = history_list.first_child() {
                    history_list.remove(&child);
                }
            }
        });
    }
    let weak = panel.downgrade();
    let refresh_weak = refresh.downgrade();
    gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
        let (Some(panel), Some(refresh)) = (weak.upgrade(), refresh_weak.upgrade()) else {
            return gtk::glib::ControlFlow::Break;
        };
        if panel.is_mapped() && refresh.is_sensitive() {
            refresh.emit_clicked();
        }
        gtk::glib::ControlFlow::Continue
    });
    refresh.emit_clicked();
    panel
}
