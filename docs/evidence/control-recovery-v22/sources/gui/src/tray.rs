//! Session-bus tray integration. Callbacks only enqueue UI actions; hardware
//! requests retain the same capability and authenticated-daemon boundary.
use ksni::blocking::TrayMethods;
use ksni::menu::{StandardItem, SubMenu};
use ksni::{MenuItem, Tray};
use omen_core::profile::ProfileKind;
use std::sync::mpsc::{self, Receiver, SyncSender};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Show(Option<String>),
    Hide,
    ToggleOverlay,
    Page(&'static str),
    Profile(ProfileKind),
    Quit,
}

pub enum Event {
    Online,
    Offline(String),
    Action(Action),
}

pub enum Update {
    State {
        profile: Option<ProfileKind>,
        visible: bool,
        choices: Vec<ProfileKind>,
        writable: bool,
        overlay: bool,
    },
    Shutdown,
}

struct OmenTray {
    events: mpsc::Sender<Event>,
    source: String,
    profile: Option<ProfileKind>,
    choices: Vec<ProfileKind>,
    writable: bool,
    visible: bool,
    overlay: bool,
}

impl OmenTray {
    fn item(label: &str, action: Action) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.events.send(Event::Action(action.clone()));
            }),
            ..Default::default()
        }
        .into()
    }
}

impl Tray for OmenTray {
    fn id(&self) -> String {
        "com.omenlinux.Gui".into()
    }
    fn title(&self) -> String {
        "OMEN Control".into()
    }
    fn icon_name(&self) -> String {
        "utilities-system-monitor".into()
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.events.send(Event::Action(Action::Show(
            ksni::current_xdg_activation_token(),
        )));
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "OMEN Control".into(),
            description: gtk::glib::markup_escape_text(&format!(
                "{} · Profile: {}",
                self.source,
                self.profile
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "unavailable".into())
            ))
            .into(),
            ..Default::default()
        }
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let profile_label = format!(
            "Profile: {}{}",
            self.profile
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unavailable".into()),
            if self.writable { "" } else { " (read-only)" }
        );
        let mut items = vec![
            Self::item("Show OMEN Control", Action::Show(None)),
            Self::item("System vitals", Action::Page("vitals")),
            Self::item("Fan monitoring", Action::Page("fan")),
            Self::item("Battery and power", Action::Page("battery")),
            Self::item(
                if self.overlay {
                    "Close status overlay"
                } else {
                    "Show status overlay"
                },
                Action::ToggleOverlay,
            ),
            MenuItem::Separator,
            StandardItem {
                label: profile_label,
                enabled: false,
                ..Default::default()
            }
            .into(),
        ];
        if self.writable && !self.choices.is_empty() {
            items.push(
                SubMenu {
                    label: "Set power profile".into(),
                    submenu: self
                        .choices
                        .iter()
                        .map(|p| Self::item(&p.to_string(), Action::Profile(*p)))
                        .collect(),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        if self.visible {
            items.push(Self::item("Hide window", Action::Hide));
        }
        items.push(Self::item("Quit OMEN Control", Action::Quit));
        items
    }
    fn watcher_online(&self) {
        let _ = self.events.send(Event::Online);
    }
    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        let _ = self.events.send(Event::Offline(format!("{reason:?}")));
        true // keep the service available for the watcher's return
    }
}

/// All connection, update and shutdown work runs outside GTK's main thread.
/// A bounded state queue prevents slow tray hosts accumulating snapshots.
pub fn start(
    source: String,
    choices: Vec<ProfileKind>,
    writable: bool,
) -> (SyncSender<Update>, Receiver<Event>) {
    let (events, receiver) = mpsc::channel();
    let (updates, state) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let tray = OmenTray {
            events: events.clone(),
            source,
            profile: None,
            choices,
            writable,
            visible: true,
            overlay: false,
        };
        match tray.spawn() {
            Ok(handle) => {
                let _ = events.send(Event::Online);
                let mut previous = None;
                while let Ok(update) = state.recv() {
                    match update {
                        Update::State {
                            profile,
                            visible,
                            choices,
                            writable,
                            overlay,
                        } => {
                            let current = (profile, visible, choices.clone(), writable, overlay);
                            if previous.as_ref() == Some(&current) {
                                continue;
                            }
                            previous = Some(current);
                            if handle
                                .update(move |tray| {
                                    tray.profile = profile;
                                    tray.visible = visible;
                                    tray.choices = choices;
                                    tray.writable = writable;
                                    tray.overlay = overlay;
                                })
                                .is_none()
                            {
                                let _ = events.send(Event::Offline("tray service stopped".into()));
                                break;
                            }
                        }
                        Update::Shutdown => break,
                    }
                }
                handle.shutdown().wait();
            }
            Err(error) => {
                let _ = events.send(Event::Offline(error.to_string()));
            }
        }
    });
    (updates, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tray(writable: bool) -> (OmenTray, Receiver<Event>) {
        let (events, rx) = mpsc::channel();
        (
            OmenTray {
                events,
                source: "fixture".into(),
                profile: Some(ProfileKind::Balanced),
                choices: vec![ProfileKind::Balanced, ProfileKind::Performance],
                writable,
                visible: true,
                overlay: false,
            },
            rx,
        )
    }
    #[test]
    fn read_only_menu_has_no_profile_write_actions() {
        let (tray, _) = tray(false);
        assert!(tray
            .menu()
            .iter()
            .all(|m| !matches!(m, MenuItem::SubMenu(_))));
        assert!(tray.menu().iter().any(
            |m| matches!(m, MenuItem::Standard(i) if !i.enabled && i.label.contains("read-only"))
        ));
    }
    #[test]
    fn profile_menu_only_enqueues_advertised_choice() {
        let (mut tray, rx) = tray(true);
        let menu = tray.menu();
        let submenu = menu
            .iter()
            .find_map(|m| {
                if let MenuItem::SubMenu(m) = m {
                    Some(m)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(submenu.submenu.len(), 2);
        if let MenuItem::Standard(item) = &submenu.submenu[1] {
            (item.activate)(&mut tray);
        }
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Action(Action::Profile(ProfileKind::Performance)))
        ));
    }
    #[test]
    fn activation_and_hide_are_ui_commands() {
        let (mut tray, rx) = tray(false);
        tray.activate(0, 0);
        assert!(matches!(rx.try_recv(), Ok(Event::Action(Action::Show(_)))));
        let menu = tray.menu();
        let hide = menu
            .iter()
            .find_map(|m| {
                if let MenuItem::Standard(m) = m {
                    (m.label == "Hide window").then_some(m)
                } else {
                    None
                }
            })
            .unwrap();
        (hide.activate)(&mut tray);
        assert!(matches!(rx.try_recv(), Ok(Event::Action(Action::Hide))));
        tray.visible = false;
        assert!(!tray
            .menu()
            .iter()
            .any(|m| matches!(m, MenuItem::Standard(i) if i.label == "Hide window")));
    }
}
