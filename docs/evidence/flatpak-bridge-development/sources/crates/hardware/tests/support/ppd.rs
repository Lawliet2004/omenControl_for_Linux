//! Shared private PPD contract fixture. Never binds the system bus.
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use zbus::blocking::{connection::Builder, fdo::DBusProxy, Connection};
use zbus::zvariant::{OwnedValue, Str};
pub const SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
pub const OBJECT: &str = "/org/freedesktop/UPower/PowerProfiles";
#[derive(Clone)]
pub struct Held {
    owner: String,
    profile: String,
    reason: String,
    app: String,
}
pub struct State {
    manual: String,
    pub holds: BTreeMap<u32, Held>,
    next: u32,
    pub fail_reply: bool,
    pub hide_holds: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            manual: "balanced".into(),
            holds: BTreeMap::new(),
            next: 0,
            fail_reply: false,
            hide_holds: false,
        }
    }
}
impl State {
    fn active(&self) -> String {
        if self.holds.values().any(|h| h.profile == "power-saver") {
            "power-saver".into()
        } else if self.holds.values().any(|h| h.profile == "performance") {
            "performance".into()
        } else {
            self.manual.clone()
        }
    }
}

struct Ppd(Arc<Mutex<State>>);
#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Ppd {
    fn hold_profile(
        &self,
        profile: &str,
        reason: &str,
        application_id: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        if !matches!(profile, "performance" | "power-saver") {
            return Err(zbus::fdo::Error::InvalidArgs("profile".into()));
        }
        let mut state = self.0.lock().unwrap();
        state.next += 1;
        let cookie = state.next;
        state.holds.insert(
            cookie,
            Held {
                owner: header.sender().unwrap().to_string(),
                profile: profile.into(),
                reason: reason.into(),
                app: application_id.into(),
            },
        );
        if state.fail_reply {
            state.fail_reply = false;
            return Err(zbus::fdo::Error::Failed(
                "fixture reply lost after application".into(),
            ));
        }
        Ok(cookie)
    }
    fn release_profile(&self, cookie: u32) -> zbus::fdo::Result<()> {
        let mut state = self.0.lock().unwrap();
        // Upstream uses the cookie as authority; it does not bind ReleaseProfile
        // to the requester's unique name. Owner pinning must protect a recycled
        // cookie even with that provider behavior.
        if state.holds.remove(&cookie).is_some() {
            Ok(())
        } else {
            Err(zbus::fdo::Error::InvalidArgs("not owned".into()))
        }
    }
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.0.lock().unwrap().active()
    }
    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::fdo::Result<()> {
        let mut state = self.0.lock().unwrap();
        // Upstream PPD set_active_profile returns without canceling holds when
        // the requested value is already active. Mirror the provider contract.
        if state.active() == profile {
            return Ok(());
        }
        state.manual = profile.into();
        state.holds.clear();
        Ok(())
    }
    #[zbus(property)]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        if self.0.lock().unwrap().hide_holds {
            return Vec::new();
        }
        self.0
            .lock()
            .unwrap()
            .holds
            .values()
            .map(|h| {
                [
                    ("Profile", h.profile.as_str()),
                    ("Reason", h.reason.as_str()),
                    ("ApplicationId", h.app.as_str()),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), OwnedValue::from(Str::from(v.to_owned()))))
                .collect()
            })
            .collect()
    }
}

pub struct Fixture {
    connection: Option<Connection>,
    monitor: Option<Connection>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub state: Arc<Mutex<State>>,
}
impl Fixture {
    pub fn start(address: &str) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let monitor = Builder::address(address).unwrap().build().unwrap();
        let signals = DBusProxy::new(&monitor)
            .unwrap()
            .receive_name_owner_changed()
            .unwrap();
        let observed = state.clone();
        let thread = std::thread::spawn(move || {
            for signal in signals {
                let args = signal.args().unwrap();
                if args.new_owner().is_none() {
                    let name = args.name().as_str();
                    observed
                        .lock()
                        .unwrap()
                        .holds
                        .retain(|_, h| h.owner != name);
                }
            }
        });
        let connection = Builder::address(address)
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(OBJECT, Ppd(state.clone()))
            .unwrap()
            .build()
            .unwrap();
        Self {
            connection: Some(connection),
            monitor: Some(monitor),
            thread: Some(thread),
            state,
        }
    }
    pub fn profile(&self) -> String {
        self.state.lock().unwrap().active()
    }
    pub fn close(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.close();
        }
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.close();
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.close();
    }
}
