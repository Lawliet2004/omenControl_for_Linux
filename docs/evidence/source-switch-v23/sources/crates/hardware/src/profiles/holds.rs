//! Power Profiles Daemon owns restoration and arbitration. We never emulate a
//! hold by remembering a value and writing it back after a process exits.
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;
use zbus::blocking::{connection::Builder, Connection, Proxy};
use zbus::zvariant::OwnedValue;

const APP_ID: &str = "com.omenlinux.Gui";
const TIMEOUT: Duration = Duration::from_secs(3);

fn error(error: impl std::fmt::Display) -> OmenError {
    OmenError::Io(format!("PPD profile hold: {error}"))
}

// Closing explicitly also covers an error/timeout whose request reached PPD
// but whose reply was lost. PPD removes holds when this unique peer disappears.
struct Peer(Option<Connection>);
impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(connection) = self.0.take() {
            let _ = connection.close();
        }
    }
}

/// A single hold on one pinned PPD service owner. It cannot migrate to a
/// replacement daemon or accidentally release a recycled cookie on that daemon.
pub struct ProfileHold {
    peer: Peer,
    owner: String,
    service: String,
    reason: String,
    profile: String,
    cookie: u32,
    released: bool,
}

/// Closes this lease's dedicated peer, including an in-flight status read.
/// It cannot write a profile or close another lease's independent peer.
pub struct HoldCancellation(Connection);
impl HoldCancellation {
    pub fn cancel(&self) -> Result<()> {
        if self.0.is_closed() {
            return Ok(());
        }
        self.0.clone().close().map_err(error)
    }
}

impl ProfileHold {
    pub fn cancellation(&self) -> Result<HoldCancellation> {
        self.peer
            .0
            .as_ref()
            .cloned()
            .map(HoldCancellation)
            .ok_or_else(|| error("hold peer is closed"))
    }
    pub fn system(service: &str, profile: ProfileKind, reason: &str) -> Result<Self> {
        Self::connect(Builder::system().map_err(error)?, service, profile, reason)
    }

    /// Explicit bus address for a private protocol fixture. Production callers
    /// use `system`; this does not alter backend capability or authorization.
    pub fn at_address(
        address: &str,
        service: &str,
        profile: ProfileKind,
        reason: &str,
    ) -> Result<Self> {
        Self::connect(
            Builder::address(address).map_err(error)?,
            service,
            profile,
            reason,
        )
    }

    fn connect(
        builder: Builder<'_>,
        service: &str,
        profile: ProfileKind,
        reason: &str,
    ) -> Result<Self> {
        if !matches!(
            service,
            "org.freedesktop.UPower.PowerProfiles" | "net.hadess.PowerProfiles"
        ) {
            return Err(OmenError::InvalidArgument("unknown PPD service".into()));
        }
        let profile = match profile {
            ProfileKind::Performance => "performance",
            ProfileKind::Quiet => "power-saver",
            _ => {
                return Err(OmenError::Unsupported(
                    "Power Profiles Daemon can hold Performance or Quiet, not Balanced/Cool".into(),
                ))
            }
        };
        if reason.is_empty() || reason.len() > 512 || reason.chars().any(char::is_control) {
            return Err(OmenError::InvalidArgument(
                "invalid profile hold reason".into(),
            ));
        }
        // PPD does not include cookies in ActiveProfileHolds. Give this lease a
        // unique reason so another hold with identical display text cannot be
        // mistaken for ours after cancellation.
        let mut nonce = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let reason = format!("{reason} [{nonce}]");
        let peer = Peer(Some(
            builder.method_timeout(TIMEOUT).build().map_err(error)?,
        ));
        let connection = peer.0.as_ref().expect("connected peer");
        let bus = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(error)?;
        let owner: String = bus.call("GetNameOwner", &(service,)).map_err(error)?;
        let proxy = Proxy::new(
            connection,
            owner.as_str(),
            super::ppd_path(service),
            service,
        )
        .map_err(error)?;
        let cookie: u32 = proxy
            .call("HoldProfile", &(profile, reason.as_str(), APP_ID))
            .map_err(error)?;
        drop(proxy);
        drop(bus);
        let hold = Self {
            peer,
            owner,
            service: service.into(),
            reason,
            profile: profile.into(),
            cookie,
            released: false,
        };
        // The property must report our precise reason/application/profile. A
        // successful method reply alone does not prove an active lease.
        if !hold.active()? {
            return Err(OmenError::StateMismatch(
                "PPD did not retain the requested hold".into(),
            ));
        }
        Ok(hold)
    }

    pub fn active(&self) -> Result<bool> {
        if self.released {
            return Ok(false);
        }
        let connection = self.peer.0.as_ref().ok_or_else(|| error("peer closed"))?;
        // An explicit Properties.Get avoids a potentially stale proxy cache.
        let proxy = Proxy::new(
            connection,
            self.owner.as_str(),
            super::ppd_path(&self.service),
            "org.freedesktop.DBus.Properties",
        )
        .map_err(error)?;
        let value: OwnedValue = proxy
            .call("Get", &(&self.service, "ActiveProfileHolds"))
            .map_err(error)?;
        let entries: Vec<HashMap<String, OwnedValue>> = value.try_into().map_err(error)?;
        Ok(entries.iter().any(|entry| {
            [
                ("Profile", self.profile.as_str()),
                ("Reason", self.reason.as_str()),
                ("ApplicationId", APP_ID),
            ]
            .into_iter()
            .all(|(key, expected)| {
                entry.get(key).and_then(|v| <&str>::try_from(v).ok()) == Some(expected)
            })
        }))
    }

    /// Remove only this cookie on the original owner. A canceled hold stays
    /// canceled; release never reapplies a profile or restores a saved value.
    pub fn release(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }
        if !self.active()? {
            self.released = true;
            return self
                .peer
                .0
                .take()
                .expect("connected peer")
                .close()
                .map_err(error);
        }
        let connection = self.peer.0.as_ref().ok_or_else(|| error("peer closed"))?;
        let proxy = Proxy::new(
            connection,
            self.owner.as_str(),
            super::ppd_path(&self.service),
            self.service.as_str(),
        )
        .map_err(error)?;
        let outcome: std::result::Result<(), _> = proxy.call("ReleaseProfile", &(self.cookie,));
        // Even if the cookie was canceled or the reply was lost, disconnecting
        // the peer releases any remaining lease without touching another peer.
        self.released = true;
        self.peer
            .0
            .take()
            .expect("connected peer")
            .close()
            .map_err(error)?;
        outcome.map_err(error)
    }
}
