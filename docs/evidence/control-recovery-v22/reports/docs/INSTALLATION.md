# Build, install and remove

## Build from source

Target: Ubuntu/Kubuntu with GTK 4.12+ and libadwaita 1.4+, a current Rust toolchain,
C development tools, SQLite development headers, pkg-config, Python 3, and Debian packaging tools. The current
binary package was built on Ubuntu 26.04 amd64. For an older distribution, build
from source there and verify its actual library requirements; do not assume a
26.04 binary runs on 24.04 or Debian stable.

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libglib2.0-dev-bin libsqlite3-dev python3 dpkg-dev
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p omen-cli -- detect
cargo run -p omen-gui
```

On this development machine, headers and libadwaita are already in a local prefix:

```bash
. /home/papan/gtk-prefix/env.sh
export LD_LIBRARY_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
cargo build --workspace --locked
./target/debug/omen-gui
```

Those paths are specific to this machine. For dependency inspection when building this host’s package, also pass
`OMEN_SHLIB_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu` to the build script.
The package does not bundle that prefix
or change the user's library loader. Normal installations use distribution libs.

## Build and inspect a Debian package

```bash
./packaging/build-deb.sh
# Artifact: target/debian/omen-linux_0.1.0-22_amd64.deb on this host
# Inspect without installing:
dpkg-deb --info target/debian/omen-linux_0.1.0-22_amd64.deb
dpkg-deb --contents target/debian/omen-linux_0.1.0-22_amd64.deb
```

The script uses a fresh staging directory, a locked release build, generated shared-library dependencies, and root-owned
package metadata. It does not install or start anything on the build host. The
binary package has distribution/runtime dependencies; see the generated control
metadata and test report. Revision 15 passed the stock Ubuntu container lifecycle
and signed Ubuntu systemd VM suites. Later packages do not inherit those results;
revision 20 has its own 20-check container lifecycle result with reused distro
dependencies and 13 installed launcher-format checks. The unchanged package now
also passes 33 headless systemd VM checks with two reboots and private managed
sessions; see [the deployment addendum](DEPLOYMENT_VALIDATION_2026-10-28.md).
Revision 21 has its own 39-check fresh Ubuntu/Plasma Wayland VM result, including
real KDE/PAM authorization and two reboots; its final documentation package is
checked for runtime/control equivalence to that exact candidate. See
[desktop validation](DESKTOP_VALIDATION_2026-10-28.md). Other distributions,
physical desktop input and hardware controls remain unvalidated. See [PACKAGE_VALIDATION.md](PACKAGE_VALIDATION.md)
and [VM_VALIDATION.md](VM_VALIDATION.md) for commands and scope.

Revision 22 has its own final-candidate 39-check fresh desktop VM verification,
including actual KDE/PAM authorization, natural expiry and two reboots, with
cleanup. The final package is checked for identical runtime/control payload to
that candidate. It adds control-page capability/readback recovery, retained
edits and complete curve-schema rendering. See
[control-page recovery](CONTROL_PAGE_RECOVERY_2026-10-28.md) for exact evidence
and limits, including pending automatic direct-to-daemon source migration.

## Install and run

```bash
sudo apt install ./target/debian/omen-linux_0.1.0-22_amd64.deb
omen-cli status
omen-cli detect
omen-gui
```

The package's post-install uses Debian's service helpers. A fresh installation
follows systemd presets and requests service startup when systemd is running
and `policy-rc.d` permits it. Upgrades preserve disabled/masked state and request
a restart of an enabled/running service. Upgrade does not disable the unit.
Removal requests a stop; purge removes helper-managed enablement links.
The old development revision `0.1.0-1` disabled its service in its own upgrade
hook, before the replacement package could observe the previous choice. On a
running systemd machine, check its first upgrade and explicitly run
`sudo systemctl enable --now omen-linux.service` if you want it enabled.
The installer reports this legacy exception; it cannot safely infer the lost
choice. This exception does not affect upgrades from helper-managed revisions.
Startup is read-only;
installing the service does not change profiles, fans, lighting or charging.
The GUI/CLI also support direct read-only operation with no daemon. Each CLI read
command returns its requested data; diagnostics is valid JSON on stdout.

`/usr/share/polkit-1/actions/com.omenlinux.policy` defines narrow mutation actions.
Authenticated IPC never overrides the backend's hardware policy. The service does
not execute user game commands. Do not run the GUI as root.

## Network monitoring

System Vitals also shows read-only network rates and cumulative errors/drops per
interface. `omen-cli network --json --interval-ms 1000` samples the same counters.
The CLI waits between two acquisitions; first/reset samples show Collecting and
missing counters show Unavailable. VPNs/bridges may count the same traffic, so no
system total is displayed. This is not a speed test or Internet connectivity
check. No network policy is changed. See `NETWORK_MONITORING.md` for details.

## GPU monitoring

The service keeps physical GPU devices isolated. When NVIDIA readings are
missing from a successful real-daemon response, the desktop/CLI can query them
locally as the unprivileged user. This requires the installed NVIDIA driver's
`/usr/bin/nvidia-smi` and device access. Suspended GPUs are not queried; unavailable
metrics remain absent. Diagnostics identify locally collected sensor IDs.
No extra daemon device permission is needed. See `GPU_MONITORING.md` for the
real-device probe, single-NVIDIA-GPU scope and remaining deployment limits.

## Game launch and profile recovery

Game discovery and commands run as the desktop user; launch is refused for
effective UID 0. The system service never executes game commands. List entries
with `omen-cli games`, then submit one using the same controller as the GUI:

```bash
omen-cli game-launch steam/620
omen-cli game-launch custom/my-game --json
```

Launchers are resolved/validated before a profile change. Steam, Heroic and
Lutris receive validated URI arguments, with explicit application IDs for Flatpak
routes; stored store shell text is ignored. Paths, discovery formats and excluded
integrations are in [GAME_LAUNCHERS.md](GAME_LAUNCHERS.md). User-authored
custom commands run via `/bin/sh`, with null stdin/stdout and inherited stderr.
Custom commands requiring a terminal should explicitly open one. JSON CLI output
remains separate from child output.

Bindings are read from the user file. A bound profile needs a daemon to check its
state; changing it also requires advertised support, explicit profile opt-in and
authorized IPC. Enabled battery automation blocks bound launches and manual
profile changes; remove the binding or disable that policy first.

Automatic mode is opt-in through Games → Profile setup. Performance/Quiet
holds require the actual command wrapper, user systemd/cgroup v2, SO_PEERPIDFD,
authorized IPC, available PPD support and the same explicit hardware opt-in.
Custom launches are wrapped automatically; native store entries display manual
launcher setup instructions. A store URI does not pre-apply an automatic profile.
See [managed sessions](MANAGED_GAME_SESSIONS.md) for commands, session history,
release/restart behavior and unvalidated launcher/Flatpak routes.

In the default manual mode, the app checks the launcher for up to 300 ms. A spawn error or an observed early
nonzero exit stops the operation and attempts to restore the prior profile if
this request changed it. Recovery reads back the result and reports success, a
third-party profile it preserved, or an unverified outcome with the original
profile for manual recovery. A profile that was already active is not owned or
restored by this request. `--json` includes original/requested profiles, failure
stage, recovery details and a process receipt on success.

Success means launcher submission, not verified game execution. Steam may hand
the URI to another process. Later launcher failures and game exit do not restore
the profile. Live children are reaped in the background while the app lives.
There is no global profile lease, crash recovery or atomic protection against
external changes between comparison and restoration, including same-value/ABA
changes. A timeout does not prove a remote mutation was cancelled. Keep those
limits in mind when enabling experimental profile control.

For unprivileged fixture-only process coverage:

```bash
python3 tests/game_launch.py
OMEN_TEST_PROFILE=release python3 tests/game_launch.py
```

## System tray and window behavior

On a desktop with a StatusNotifier host (including the tested KDE Wayland session),
the app registers a tray menu. It offers Show, Hide, page navigation, current
profile, and Quit. Profile commands appear only when the connected daemon
advertises write support; all requests use the existing authorization boundary.
The app itself always runs as the desktop user. Closing the window quits; use
**Hide to tray** or Ctrl+H to keep monitoring. Ctrl+Q quits. These shortcuts apply
inside the application, not globally. Reopening the app presents its existing window.

Settings → Desktop integration → **Start in system tray** saves a user preference.
The window hides only after a host accepts registration. Missing host support
keeps it visible and disables Hide. Losing an established host reveals the
window; the tray re-registers when that host returns. If no host existed at
initial startup, enable desktop tray support and relaunch the app. No desktop
extension is installed automatically. GNOME requires separately configured host
support; that environment has not been tested.

With a graphical KDE session and Python GObject bindings installed:

```bash
python3 tests/tray.py
# Add OMEN_TEST_PROFILE=release to exercise the release binaries.
```

The test uses real GTK/D-Bus, isolated user settings and a private-bus watcher
fixture for failure cases. Its profile writes target a mock daemon only.

Settings → Desktop integration → **Enable global shortcut…** offers a separate
desktop-wide Show action. The installed desktop launcher and a GlobalShortcuts
portal backend are required. Choose the key in the desktop dialog; the app sends
no preferred combination. Successful binding saves startup restoration. Disable
releases the session and saves opt-out; Change opens desktop configuration on
portal version 2. Cancellation, missing support and service loss are reported;
loss reveals a hidden window. No shortcut requests hardware control.
See `GLOBAL_SHORTCUTS.md` for evidence and the remaining physical-key/desktop
approval validation. Ctrl+H/Q remain app-local. `--new-instance` is an explicit
diagnostic option; ordinary launches reuse the running window.

## Experimental standard profiles

Live fan/RGB/backlight/charge writes are blocked in this build. Only standard
profile testing has an explicit opt-in. Read `PROFILE_VALIDATION_PLAN.md` and
obtain approval for first live tests before enabling it. The exact board and BIOS
must match discovery; unknown boards are refused. Do not change the installed
service to a test configuration without keeping its default read-only behavior
and rollback procedure available.

The default service mounts `/sys` read-only, so the experimental PPD path can work
without broad sysfs access. A sysfs-only profile test needs a separately reviewed
service sandbox exception; permission errors are surfaced, not worked around.

## Configuration and diagnostics

User settings: `~/.config/omen-linux/config.toml` (or XDG_CONFIG_HOME). General
preferences and game entries/bindings belong here, with or without the daemon.
System settings: `/etc/omen-linux/config.toml`, schema version 1. The daemon
**always** uses this path (or `OMEN_SYSTEM_CONFIG_DIR` in isolated development);
missing system config yields safe defaults and never loads the user's file.
An unprivileged development daemon needs an explicit writable system directory
override to save service policies. Permission errors do not change the destination.

Settings → Settings backups offers separate User and Service actions and a native
file picker. User backups contain `general` and `games`; service backups contain
`fan`, `lighting` and `power`. Each has an explicit `scope` and embedded schema
version. Wrong-scope/unknown-field/future-schema backups are rejected. Untagged
legacy JSON exports migrate, then only the chosen scope is applied. Optional
JSON null fields survive migration. An unset AC profile is saved as `"unchanged"`
so a restart preserves it; legacy omitted fields retain their previous defaults. Imports are bounded to 1 MiB; saves retain
`.bak`, and JSON exports use private 0600 files with atomic replacement.

User imports save immediately and preserve unrelated legacy fields in the user
file. Restart the app to refresh desktop/game views and polling; the next game
launch reads the newly saved binding. History retention honors
`general.history_samples` (300 by default). Service imports require a connected
current daemon, show an automation summary before Apply, use the admin polkit
action, and never copy game commands or desktop preferences to the service.
Enabling battery automation requires enabled profile control and advertised
target profiles. Startup fan/RGB restoration remains disabled on live hardware.

```bash
omen-cli settings export --scope user my-desktop-settings.json
omen-cli settings inspect --scope user my-desktop-settings.json
omen-cli settings import --scope user my-desktop-settings.json
omen-cli settings export --scope service service-policies.json
omen-cli settings inspect --scope service service-policies.json
# After reviewing: service policies may activate automation.
omen-cli settings import --scope service --apply-service-policy service-policies.json
```

Diagnostic export also offers a native local-file picker and private atomic
writes, with collection errors surfaced. The CLI prints read-only JSON:

```bash
omen-cli diagnostics > omen-diagnostics.json
```

Review diagnostic output before sharing; it includes machine/BIOS/kernel identity
and hardware state. No serial numbers or MAC addresses are intentionally collected.

## Mock development and validation

```bash
./target/debug/omen-daemon --mock-omen
# Prints its private /tmp/omen-mock-PID.sock. In another terminal:
OMEN_SOCK=/tmp/omen-mock-PID.sock ./target/debug/omen-gui
python3 tests/smoke.py
```

Mocks use fixture files and bypass polkit only on a private 0600 socket. The GUI
labels the source MOCK. Never interpret fixture success as hardware validation.
`python3 scripts/capture-pages.py` saves app-only screenshots when a graphical
session is available; it performs no screen/desktop capture.

## Remove / purge

```bash
sudo apt remove omen-linux
# To also remove system configuration:
sudo apt purge omen-linux
```

Removal stops/disables the service; user settings remain for the user to manage.
Purge removes only the named system config and backup. No DKMS module, hp_wmi
blacklist, firmware/BIOS setting, raw EC helper or kernel driver is installed by
this package. Recovery limitations and remaining validation gates are documented
in `HARDWARE_SAFETY.md`.

## Optional status overlay

Settings → Desktop integration → Show status overlay enables the compact status
window and saves the startup preference. Tray Show/Close status overlay and
Ctrl+O while the app is focused toggle it; Escape/Close overlay closes it.
The opt-in global-shortcut session also requests a desktop-assigned Toggle
OMEN status overlay action. No keys are bound by default. Window placement,
stacking and visibility over full-screen games depend on the compositor.
See [STATUS_OVERLAY.md](STATUS_OVERLAY.md).
