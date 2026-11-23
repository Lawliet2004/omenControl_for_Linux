//! Desktop-approved, session-scoped global Show action. All portal IO is async
//! on GTK's context; dedicated bus peers make cancellation release all sessions.
use futures_channel::oneshot;
use gtk::gio;
use gtk::glib::{self, variant::ObjectPath, variant::ToVariant};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const DEST: &str = "org.freedesktop.portal.Desktop";
const ROOT: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const ACTION: &str = "show-window";
const OVERLAY_ACTION: &str = "toggle-overlay";
type Dict = HashMap<String, glib::Variant>;
type Shortcuts = Vec<(String, Dict)>;
type PortalResult<T> = Result<T, String>;

fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "omen_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

struct Peer(gio::DBusConnection, String);
impl Drop for Peer {
    fn drop(&mut self) {
        if !self.0.is_closed() {
            self.0.close(None::<&gio::Cancellable>, |_| {});
        }
    }
}
impl Peer {
    async fn open() -> PortalResult<Rc<Self>> {
        if gio::DesktopAppInfo::new(&format!("{}.desktop", crate::APP_ID)).is_none() {
            return Err(
                "Install OMEN Control's desktop launcher to enable global shortcuts".into(),
            );
        }
        let address =
            gio::dbus_address_get_for_bus_sync(gio::BusType::Session, None::<&gio::Cancellable>)
                .map_err(|e| e.to_string())?;
        let connection = gio::DBusConnection::for_address_future(
            &address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
        connection.set_exit_on_close(false);
        let mut peer = Self(connection, String::new());
        let mut owner = peer
            .0
            .call_future(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetNameOwner",
                Some(&(DEST,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await;
        if owner
            .as_ref()
            .is_err_and(|e| e.matches(gio::DBusError::NameHasNoOwner))
        {
            peer.0
                .call_future(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "StartServiceByName",
                    Some(&(DEST, 0u32).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                )
                .await
                .map_err(|e| e.to_string())?;
            owner = peer
                .0
                .call_future(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "GetNameOwner",
                    Some(&(DEST,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                )
                .await;
        }
        let owner = owner.map_err(|e| e.to_string())?;
        peer.1 = owner
            .get::<(String,)>()
            .map(|v| v.0)
            .filter(|v| v.starts_with(':'))
            .ok_or("Desktop portal has no unique owner")?;
        let peer = Rc::new(peer);
        // Registry registration precedes every portal call on this new peer.
        if let Err(error) = peer
            .0
            .call_future(
                Some(&peer.1),
                ROOT,
                "org.freedesktop.host.portal.Registry",
                "Register",
                Some(&(crate::APP_ID, Dict::new()).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
        {
            let legacy = error.matches(gio::DBusError::UnknownMethod)
                || error.matches(gio::DBusError::UnknownInterface);
            if !legacy {
                return Err(format!("Desktop application registration failed: {error}"));
            }
        }
        Ok(peer)
    }

    async fn version(&self) -> PortalResult<u32> {
        let value = self
            .0
            .call_future(
                Some(&self.1),
                ROOT,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(IFACE, "version").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
            .map_err(|e| format!("Global shortcuts unavailable: {e}"))?;
        value
            .get::<(glib::Variant,)>()
            .and_then(|v| v.0.get::<u32>())
            .filter(|v| *v >= 1)
            .ok_or_else(|| "Desktop reported an unsupported global-shortcut interface".into())
    }

    fn path(&self, kind: &str, token: &str) -> PortalResult<String> {
        let sender = self
            .0
            .unique_name()
            .ok_or("Desktop bus has no unique connection name")?;
        Ok(format!(
            "{ROOT}/{kind}/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        ))
    }

    async fn request(
        &self,
        method: &str,
        token: &str,
        params: glib::Variant,
    ) -> PortalResult<Dict> {
        let path = self.path("request", token)?;
        let (tx, rx) = oneshot::channel();
        let tx = Rc::new(RefCell::new(Some(tx)));
        let completed = Rc::new(Cell::new(false));
        let timer_live = Rc::new(Cell::new(true));
        let sender = tx.clone();
        let done = completed.clone();
        // Subscribe before the method call: Response may precede its reply.
        let subscription = self.0.signal_subscribe(
            Some(&self.1),
            Some("org.freedesktop.portal.Request"),
            Some("Response"),
            Some(&path),
            None,
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, params| {
                done.set(true);
                if let Some(sender) = sender.borrow_mut().take() {
                    let response = params
                        .get::<(u32, Dict)>()
                        .ok_or_else(|| "Malformed desktop shortcut response".to_string())
                        .and_then(|(code, results)| match code {
                            0 => Ok(results),
                            1 => Err("Shortcut setup cancelled".into()),
                            _ => Err("Desktop declined the shortcut request".into()),
                        });
                    let _ = sender.send(response);
                }
            },
        );
        let sender = tx.clone();
        let owner_changed = self.0.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(DEST),
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, _| {
                if let Some(sender) = sender.borrow_mut().take() {
                    let _ = sender.send(Err(
                        "Desktop shortcut service changed during setup; try again".into(),
                    ));
                }
            },
        );
        let sender = tx.clone();
        let closed = self.0.connect_local("closed", false, move |_| {
            if let Some(sender) = sender.borrow_mut().take() {
                let _ = sender.send(Err("Desktop bus disconnected during shortcut setup".into()));
            }
            None
        });
        let live = timer_live.clone();
        let timer = glib::timeout_add_local_once(Duration::from_secs(120), move || {
            live.set(false);
            if let Some(sender) = tx.borrow_mut().take() {
                let _ = sender.send(Err("Desktop shortcut approval timed out; try again".into()));
            }
        });
        let _pending = PendingRequest {
            connection: self.0.clone(),
            path: path.clone(),
            owner: self.1.clone(),
            subscriptions: vec![subscription, owner_changed],
            closed: Some(closed),
            completed,
            timer: Some(timer),
            timer_live,
        };
        let reply = self
            .0
            .call_future(
                Some(&self.1),
                ROOT,
                IFACE,
                method,
                Some(&params),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
            .map_err(|e| e.to_string())?;
        if reply
            .get::<(ObjectPath,)>()
            .is_none_or(|reply| reply.0.as_str() != path)
        {
            return Err("Desktop returned an unexpected shortcut request handle".into());
        }
        rx.await
            .map_err(|_| "Desktop shortcut request ended without a response".to_string())?
    }
}

struct PendingRequest {
    connection: gio::DBusConnection,
    path: String,
    owner: String,
    subscriptions: Vec<gio::SignalSubscriptionId>,
    closed: Option<glib::SignalHandlerId>,
    completed: Rc<Cell<bool>>,
    timer: Option<glib::SourceId>,
    timer_live: Rc<Cell<bool>>,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        for id in self.subscriptions.drain(..) {
            self.connection.signal_unsubscribe(id);
        }
        if let Some(id) = self.closed.take() {
            self.connection.disconnect(id);
        }
        if self.timer_live.replace(false) {
            if let Some(timer) = self.timer.take() {
                timer.remove();
            }
        }
        if !self.completed.get() && !self.connection.is_closed() {
            self.connection.call(
                Some(&self.owner),
                &self.path,
                "org.freedesktop.portal.Request",
                "Close",
                None,
                None,
                gio::DBusCallFlags::NONE,
                1000,
                None::<&gio::Cancellable>,
                |_| {},
            );
        }
    }
}

struct Parent {
    id: String,
    wayland: Option<(gdk_wayland::WaylandToplevel, String)>,
}
impl Drop for Parent {
    fn drop(&mut self) {
        if let Some((surface, handle)) = &self.wayland {
            surface.drop_exported_handle(handle);
        }
    }
}
impl Parent {
    async fn export(window: &gtk::Window) -> PortalResult<Self> {
        if !window.is_mapped() {
            // Startup restoration may run after the tray has hidden the window.
            // Portals accept an empty parent when no window is displayed.
            return Ok(Self {
                id: String::new(),
                wayland: None,
            });
        }
        let surface = window
            .surface()
            .ok_or("Open the window before configuring its global shortcut")?;
        if let Ok(toplevel) = surface.clone().downcast::<gdk_wayland::WaylandToplevel>() {
            let (tx, rx) = oneshot::channel();
            let sender = Rc::new(RefCell::new(Some(tx)));
            let requested = toplevel.export_handle(move |surface, handle| {
                if let Some(tx) = sender.borrow_mut().take() {
                    let parent = handle
                        .map(|handle| Self {
                            id: format!("wayland:{handle}"),
                            wayland: Some((surface.clone(), handle.to_string())),
                        })
                        .map_err(|e| e.to_string());
                    let _ = tx.send(parent);
                }
            });
            if !requested {
                return Err("Could not export the window to the desktop shortcut dialog".into());
            }
            return rx.await.map_err(|_| "Window export ended".to_string())?;
        }
        if let Ok(surface) = surface.downcast::<gdk_x11::X11Surface>() {
            return Ok(Self {
                id: format!("x11:{:x}", surface.xid()),
                wayland: None,
            });
        }
        Ok(Self {
            id: String::new(),
            wayland: None,
        })
    }
}

struct Session {
    peer: Rc<Peer>,
    path: String,
    parent: Parent,
    subscriptions: Vec<gio::SignalSubscriptionId>,
}
impl Drop for Session {
    fn drop(&mut self) {
        for id in self.subscriptions.drain(..) {
            self.peer.0.signal_unsubscribe(id);
        }
        self.peer.0.call(
            Some(&self.peer.1),
            &self.path,
            "org.freedesktop.portal.Session",
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            1000,
            None::<&gio::Cancellable>,
            |_| {},
        );
        // Also handles portal/backend loss and pending RPC cancellation.
        if !self.peer.0.is_closed() {
            self.peer.0.close(None::<&gio::Cancellable>, |_| {});
        }
    }
}

fn description(shortcuts: &Shortcuts) -> PortalResult<String> {
    let mut matches = shortcuts.iter().filter(|(id, _)| id == ACTION);
    let (_, props) = matches
        .next()
        .ok_or("The desktop did not enable Show OMEN Control")?;
    if matches.next().is_some() {
        return Err("Desktop returned duplicate shortcut actions".into());
    }
    let show = props
        .get("trigger_description")
        .and_then(|v| v.get::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "registered; desktop did not report an assigned key".into());
    let mut overlays = shortcuts.iter().filter(|(id, _)| id == OVERLAY_ACTION);
    match (overlays.next(), overlays.next()) {
        (Some((_, properties)), None) => {
            let key = properties
                .get("trigger_description")
                .and_then(|v| v.get::<String>())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "key unavailable".into());
            Ok(format!("{show} · Overlay: {key}"))
        }
        (None, _) => Ok(format!("{show} · Overlay shortcut unavailable")),
        _ => Err("Desktop returned duplicate overlay shortcut actions".into()),
    }
}

pub struct GlobalShortcuts {
    pub widget: gtk::Box,
    status: gtk::Label,
    primary: gtk::Button,
    secondary: gtk::Button,
    window: glib::WeakRef<gtk::Window>,
    version: Cell<Option<u32>>,
    wanted: Cell<bool>,
    auto_start: Cell<bool>,
    generation: Cell<u64>,
    busy: Cell<bool>,
    session: RefCell<Option<Session>>,
    task: RefCell<Option<glib::JoinHandle<()>>>,
    overlay: std::rc::Weak<crate::overlay::Overlay>,
    overlay_granted: Cell<bool>,
}

impl GlobalShortcuts {
    pub fn new(window: &gtk::Window, overlay: &Rc<crate::overlay::Overlay>) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let status = gtk::Label::builder()
            .label("Checking global shortcut support…")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim"])
            .build();
        let primary = gtk::Button::with_label("Enable global shortcut…");
        let secondary = gtk::Button::with_label("Change shortcut…");
        let buttons = crate::widgets::action_row();
        crate::widgets::append_action(&buttons, &primary);
        crate::widgets::append_action(&buttons, &secondary);
        widget.append(&gtk::Label::builder().label("Show OMEN Control or toggle the status overlay from any application. Choose keys in your desktop's shortcut dialog; enabling restores them next launch.")
            .wrap(true).xalign(0.0).css_classes(["dim"]).build());
        widget.append(&status);
        widget.append(&buttons.widget);
        let saved = omen_core::config::OmenConfig::load();
        let wanted = saved
            .as_ref()
            .is_ok_and(|c| c.general.global_shortcut_enabled);
        let this = Rc::new(Self {
            widget,
            status,
            primary,
            secondary,
            window: window.downgrade(),
            version: Cell::new(None),
            wanted: Cell::new(wanted),
            auto_start: Cell::new(wanted),
            generation: Cell::new(0),
            busy: Cell::new(false),
            session: RefCell::new(None),
            task: RefCell::new(None),
            overlay: Rc::downgrade(overlay),
            overlay_granted: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.primary.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                if this.busy.get() || this.session.borrow().is_some() {
                    this.disable();
                } else if this.version.get().is_some() {
                    this.enable();
                } else {
                    this.probe();
                }
            }
        });
        let weak = Rc::downgrade(&this);
        this.secondary.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                if this.session.borrow().is_some() {
                    this.configure();
                } else {
                    this.disable();
                }
            }
        });
        let weak = Rc::downgrade(&this);
        window.connect_map(move |_| {
            if let Some(this) = weak.upgrade() {
                this.maybe_auto_start();
            }
        });
        this.probe();
        if let Err(error) = saved {
            this.status.set_text(&format!(
                "Shortcut startup preference could not be read: {error}"
            ));
        }
        this
    }

    fn render(&self) {
        let active = self.session.borrow().is_some();
        self.primary.set_label(if self.busy.get() {
            "Cancel shortcut setup"
        } else if active {
            "Disable global shortcut"
        } else if self.version.get().is_some() {
            "Enable global shortcut…"
        } else {
            "Retry shortcut support"
        });
        self.secondary.set_visible(
            !self.busy.get()
                && ((active && self.version.get().is_some_and(|v| v >= 2))
                    || (!active && self.wanted.get())),
        );
        self.secondary.set_label(if active {
            "Change shortcut…"
        } else {
            "Disable global shortcut"
        });
    }

    fn remember(&self, enabled: bool) -> PortalResult<()> {
        omen_ipc::settings::SettingsController::new(None, omen_core::config::OmenConfig::path())
            .set_global_shortcut_enabled(enabled)
            .map_err(|e| e.to_string())?;
        self.wanted.set(enabled);
        Ok(())
    }

    pub fn shutdown(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let task = self.task.borrow_mut().take();
        if let Some(task) = task {
            task.abort();
        }
        self.session.borrow_mut().take();
        self.busy.set(false);
        self.overlay_granted.set(false);
    }

    fn disable(&self) {
        self.auto_start.set(false);
        self.shutdown();
        let saved = if self.wanted.get() {
            self.remember(false)
        } else {
            Ok(())
        };
        match saved {
            Ok(()) => self.status.set_text("Global shortcut disabled"),
            Err(error) => self.status.set_text(&format!(
                "Shortcut disabled for this session; startup preference was not saved: {error}"
            )),
        }
        self.render();
    }

    fn probe(self: &Rc<Self>) {
        self.shutdown();
        self.busy.set(true);
        self.status.set_text("Checking global shortcut support…");
        self.render();
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let task = glib::MainContext::default().spawn_local(async move {
            let result = async {
                let peer = Peer::open().await?;
                peer.version().await
            }
            .await;
            if let Some(this) = weak
                .upgrade()
                .filter(|this| this.generation.get() == generation)
            {
                this.busy.set(false);
                this.task.borrow_mut().take();
                match result {
                    Ok(version) => {
                        this.version.set(Some(version));
                        this.status.set_text("Global shortcut available; disabled");
                    }
                    Err(error) => {
                        this.version.set(None);
                        this.status.set_text(&error);
                        if this.wanted.get() {
                            if let Some(window) = this.window.upgrade().filter(|w| !w.is_visible())
                            {
                                window.present();
                            }
                        }
                    }
                }
                this.render();
                this.maybe_auto_start();
            }
        });
        *self.task.borrow_mut() = Some(task);
    }

    fn maybe_auto_start(self: &Rc<Self>) {
        if self.auto_start.get()
            && !self.busy.get()
            && self.version.get().is_some()
            && self.window.upgrade().is_some()
        {
            self.auto_start.set(false);
            self.enable();
        }
    }

    fn enable(self: &Rc<Self>) {
        if let Err(error) = omen_core::config::OmenConfig::load() {
            self.status
                .set_text(&format!("Shortcut preference could not be read: {error}"));
            return;
        }
        self.shutdown();
        self.busy.set(true);
        self.status
            .set_text("Choose Show OMEN Control in your desktop's shortcut dialog…");
        self.render();
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let window = self.window.clone();
        let task = glib::MainContext::default().spawn_local(async move {
            let result: PortalResult<(Session, u32, String)> = async {
                let peer = Peer::open().await?;
                let version = peer.version().await?;
                let create = token();
                let session_token = token();
                let expected = peer.path("session", &session_token)?;
                let options = Dict::from([
                    ("handle_token".into(), create.to_variant()),
                    ("session_handle_token".into(), session_token.to_variant()),
                ]);
                let result = peer
                    .request("CreateSession", &create, (options,).to_variant())
                    .await?;
                let path = result
                    .get("session_handle")
                    .and_then(|v| v.get::<String>())
                    .filter(|p| p == &expected)
                    .ok_or("Desktop returned an unexpected shortcut session handle")?;
                let window = window.upgrade().ok_or("Application window closed")?;
                let parent = Parent::export(&window).await?;
                let mut session = Session {
                    peer,
                    path,
                    parent,
                    subscriptions: Vec::new(),
                };
                let bind = token();
                let shortcuts = vec![
                    (
                        ACTION.to_string(),
                        Dict::from([("description".into(), "Show OMEN Control".to_variant())]),
                    ),
                    (
                        OVERLAY_ACTION.to_string(),
                        Dict::from([(
                            "description".into(),
                            "Toggle OMEN status overlay".to_variant(),
                        )]),
                    ),
                ];
                let args = (
                    ObjectPath::try_from(session.path.as_str()).map_err(|e| e.to_string())?,
                    shortcuts,
                    session.parent.id.clone(),
                    Dict::from([("handle_token".into(), bind.to_variant())]),
                )
                    .to_variant();
                let results = session.peer.request("BindShortcuts", &bind, args).await?;
                let shortcuts = results
                    .get("shortcuts")
                    .and_then(|v| v.get::<Shortcuts>())
                    .ok_or("Desktop returned no shortcut list")?;
                let description = description(&shortcuts)?;
                let overlay_granted = shortcuts
                    .iter()
                    .filter(|(id, _)| id == OVERLAY_ACTION)
                    .count()
                    == 1;
                if let Some(this) = weak
                    .upgrade()
                    .filter(|this| this.generation.get() == generation)
                {
                    if !this.wanted.get() {
                        this.remember(true)?;
                    }
                    this.subscribe(&mut session, generation);
                    this.overlay_granted.set(overlay_granted);
                } else {
                    return Err("Shortcut setup cancelled".into());
                }
                Ok((session, version, description))
            }
            .await;
            if let Some(this) = weak
                .upgrade()
                .filter(|this| this.generation.get() == generation)
            {
                this.busy.set(false);
                match result {
                    Ok((session, version, description)) => {
                        *this.session.borrow_mut() = Some(session);
                        this.version.set(Some(version));
                        this.status
                            .set_text(&format!("Show OMEN Control: {description}"));
                    }
                    Err(error) => {
                        this.status.set_text(&error);
                        if error != "Shortcut setup cancelled" {
                            if let Some(window) = this.window.upgrade().filter(|w| !w.is_visible())
                            {
                                window.present();
                            }
                        }
                        if error == "Shortcut setup cancelled" && this.wanted.get() {
                            if let Err(save) = this.remember(false) {
                                this.status.set_text(&format!(
                                    "{error}; startup preference was not saved: {save}"
                                ));
                            }
                        }
                    }
                }
                this.render();
            }
        });
        *self.task.borrow_mut() = Some(task);
    }

    fn subscribe(self: &Rc<Self>, session: &mut Session, generation: u64) {
        let weak = Rc::downgrade(self);
        let path = session.path.clone();
        session.subscriptions.push(session.peer.0.signal_subscribe(
            Some(&session.peer.1),
            Some(IFACE),
            None,
            Some(ROOT),
            None,
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, signal, args| {
                let Some(this) = weak.upgrade().filter(|this| {
                    this.generation.get() == generation && this.session.borrow().is_some()
                }) else {
                    return;
                };
                if signal == "Activated" {
                    if let Some((session, id, _, options)) =
                        args.get::<(ObjectPath, String, u64, Dict)>()
                    {
                        if session.as_str() == path && id == ACTION {
                            if let Some(window) = this.window.upgrade() {
                                if let Some(token) = options
                                    .get("activation_token")
                                    .and_then(|v| v.get::<String>())
                                {
                                    window.set_startup_id(&token);
                                }
                                window.present();
                            }
                        } else if session.as_str() == path
                            && id == OVERLAY_ACTION
                            && this.overlay_granted.get()
                        {
                            if let Some(overlay) = this.overlay.upgrade() {
                                overlay.toggle();
                            }
                        }
                    }
                } else if signal == "ShortcutsChanged" {
                    if let Some((session, shortcuts)) = args.get::<(ObjectPath, Shortcuts)>() {
                        if session.as_str() == path {
                            this.overlay_granted.set(
                                shortcuts
                                    .iter()
                                    .filter(|(id, _)| id == OVERLAY_ACTION)
                                    .count()
                                    == 1,
                            );
                            match description(&shortcuts) {
                                Ok(description) => this
                                    .status
                                    .set_text(&format!("Show OMEN Control: {description}")),
                                Err(_) => this.lost(
                                    "The desktop removed the global shortcut; enable it again",
                                ),
                            }
                        }
                    }
                }
            },
        ));
        let weak = Rc::downgrade(self);
        session.subscriptions.push(session.peer.0.signal_subscribe(
            Some(&session.peer.1),
            Some("org.freedesktop.portal.Session"),
            Some("Closed"),
            Some(&session.path),
            None,
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, _| {
                if let Some(this) = weak
                    .upgrade()
                    .filter(|this| this.generation.get() == generation)
                {
                    this.lost("The desktop closed the shortcut session; enable it again");
                }
            },
        ));
        let weak = Rc::downgrade(self);
        session.subscriptions.push(session.peer.0.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(DEST),
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, _| {
                if let Some(this) = weak
                    .upgrade()
                    .filter(|this| this.generation.get() == generation)
                {
                    this.lost("Desktop shortcut service changed; enable it again");
                }
            },
        ));
        let weak = Rc::downgrade(self);
        // GDBus emits Closed in the connection creator's thread-default context.
        session.peer.0.connect_local("closed", false, move |_| {
            if let Some(this) = weak
                .upgrade()
                .filter(|this| this.generation.get() == generation)
            {
                this.lost("Desktop bus disconnected; enable the shortcut again");
            }
            None
        });
    }

    fn lost(&self, reason: &str) {
        eprintln!("Global shortcut session ended: {reason}");
        self.shutdown();
        self.status.set_text(reason);
        if let Some(window) = self.window.upgrade().filter(|w| !w.is_visible()) {
            window.present();
        }
        self.render();
    }

    fn configure(self: &Rc<Self>) {
        if self.version.get().is_none_or(|v| v < 2) {
            return;
        }
        let request = self.session.borrow().as_ref().map(|session| {
            (
                session.peer.clone(),
                session.path.clone(),
                session.parent.id.clone(),
            )
        });
        let Some((peer, path, parent)) = request else {
            return;
        };
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        self.status
            .set_text("Change the key in your desktop's shortcut dialog");
        glib::MainContext::default().spawn_local(async move {
            let result = peer
                .0
                .call_future(
                    Some(&peer.1),
                    ROOT,
                    IFACE,
                    "ConfigureShortcuts",
                    Some(
                        &(
                            ObjectPath::try_from(path).expect("validated session path"),
                            parent,
                            Dict::new(),
                        )
                            .to_variant(),
                    ),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                )
                .await;
            if let Some(this) = weak
                .upgrade()
                .filter(|this| this.generation.get() == generation)
            {
                match result {
                    Ok(_) => {}
                    Err(error) => this.status.set_text(&format!(
                        "Could not open desktop shortcut settings: {error}"
                    )),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_requested_action_supplies_a_key_description() {
        let props = Dict::from([("trigger_description".into(), "Meta+O".to_variant())]);
        assert!(description(&vec![("unrelated".into(), props.clone())]).is_err());
        assert_eq!(
            description(&vec![(ACTION.into(), props.clone())]).unwrap(),
            "Meta+O · Overlay shortcut unavailable"
        );
        assert!(description(&vec![
            (ACTION.into(), props.clone()),
            (ACTION.into(), props)
        ])
        .is_err());
        assert!(description(&vec![(ACTION.into(), Dict::new())])
            .unwrap()
            .contains("did not report"));
    }
}
