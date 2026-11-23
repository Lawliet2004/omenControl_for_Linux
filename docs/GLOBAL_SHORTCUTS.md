# Global Show shortcut

Revision `0.1.0-6` adds a session-scoped **Show OMEN Control** action through the
[XDG Global Shortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html).
It can reveal the existing window while another application has focus. It
performs no hardware operation. Ctrl+H and Ctrl+Q remain window-local shortcuts.

## Use

Install the package's `com.omenlinux.Gui.desktop` launcher, then open Settings →
Desktop integration → **Enable global shortcut…**. Choose the key in the
desktop's dialog. The app supplies no preferred key and does not claim an
unassigned key is active. The returned trigger description appears in Settings.
The desktop decides availability, conflicts, permission and persistence of key
assignments. This requires a portal backend implementing GlobalShortcuts.

After successful binding, the user preference `general.global_shortcut_enabled`
restores the session on subsequent app launches. It defaults to false and is
omitted from serialized configuration while false, preserving old default files.
User backups include the opt-in; they do not contain or set the desktop's key
assignment. Importing preferences still requires restarting to refresh the UI.

**Disable global shortcut** releases this app's current session and saves startup
opt-out. It does not erase any desktop-managed remembered assignment. Quitting
releases the session but retains the opt-in. **Change shortcut…** opens desktop
configuration on portal version 2; version 1 uses disable/enable to rebind.
No shortcut is bound before opt-in. Startup support probing
does not create a shortcut session. Source checkouts without a desktop launcher
show an installation explanation instead of requesting an invalid app identity.

## Boundary and recovery

Portal work uses a separate async GIO session-bus connection. The app registers
its desktop identity through the [host Registry](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html)
before using the portal. Legacy desktops without that interface may use their
own identity mechanism; explicit registration refusals fail. Calls and signal
subscriptions are pinned to the portal's unique bus owner. Activation must
match the current session and the exact Show action. A desktop activation token
is passed to GTK when supplied. No global key capture or raw input device access
is used.

Response subscription precedes the request, since a response may arrive before
its method reply. Request/session handles are checked against their tokens.
Method calls have a five-second limit; asynchronous approval requests have a
two-minute limit and an explicit Cancel button. Cancelling, refusal, malformed
handles, preference-write failure and shutdown release the session/peer. Late
callbacks cannot reactivate a cancelled generation. The GUI stays responsive.

The preference is saved only after binding succeeds. Failed persistence closes
the approved session. Corrupt preferences are preserved and prevent starting
setup. A failed disable save is reported explicitly; the current session is still
released. Portal/session/bus loss is reported and reveals a hidden window.
Owner recovery requires re-enabling through a fresh registered peer; it does not
silently repeat an approval dialog. Saved startup opt-in can be disabled even
when the desktop service is unavailable. Slow portal startup can restore the
shortcut after the tray has hidden the window, using an empty parent identifier.

Wayland dialogs use an exported GDK toplevel handle, released with the session;
X11 uses the window's XID. Unmapped startup windows use an empty parent. The GUI's
normal GApplication single-instance path also survives session-bus loss. Diagnostic
`--new-instance` explicitly creates an independent process; readiness logging no
longer disables normal single-instance behavior.

## Evidence and limitations

`tests/shortcuts.py` runs real GTK, native AT-SPI button actions and DBusMenu
visibility actions against private portal/tray/daemon protocol fixtures. No host
key assignment is made. **24 workflows passed in each of debug/release ×
X11/Wayland**. Evidence files `shortcuts-{profile}-{backend}.json` record the actual
GDK display type, binary digest, checks and cleanup. They cover opt-in, parent
export, grant/cancel/refusal, early change signals, malformed handles, registration
failure, pending cancellation, portal loss, preference read/write failure,
version gating, session closure, owner recovery, restart restoration, slow
minimized startup, activation filtering, quit cleanup and whole-bus loss.

`tests/shortcut_portal_probe.py` independently exercised the real KDE portal's
Register/CreateSession/ListShortcuts/Close path. Without an installed launcher it
was correctly rejected. With a temporary user launcher it passed, listed no
previous shortcuts, and closed the session. The launcher was created exclusively
and removed afterward; no package was installed. See `shortcut-portal-host.json`
and `shortcut-portal-host-no-launcher.json`. It never called BindShortcuts or
ConfigureShortcuts and does not prove real key assignment or activation.

`scripts/capture-shortcuts.py` captures only the app's GTK tree on Wayland with
private fixtures. Disabled/enabled screenshots are in `docs/screenshots/`, with
fingerprints and cleanup in `shortcut-captures.json`. The displayed Meta+O key is
fixture data, not a registered host shortcut.

Actual desktop approval/cancellation UI, physical key activation, compositor
focus transfer, lock-screen behavior, desktop-managed persistence and non-KDE
backends remain unverified. This feature is implemented and protocol-tested; it
is not a claim of universal desktop support. Revision 11 adds the optional status overlay and its granted Toggle action;
see `STATUS_OVERLAY.md`. Global hardware-control shortcuts remain unavailable.

```bash
GDK_BACKEND=wayland python3 tests/shortcuts.py
GDK_BACKEND=x11 python3 tests/shortcuts.py
# Add OMEN_TEST_PROFILE=release for optimized binaries.
python3 tests/shortcut_portal_probe.py  # requires an installed desktop launcher
```

Revision 11 extends the same opt-in session with `toggle-overlay`. Descriptions
show its returned key or explicitly say the overlay shortcut is unavailable.
A Show-only grant still works. Duplicate overlay actions are refused; activation
requires that the overlay action was granted and belongs to the current session.
Existing opt-in users may see desktop approval for the additional action on
session restoration. No additional desktop protocol or global listener is used.
