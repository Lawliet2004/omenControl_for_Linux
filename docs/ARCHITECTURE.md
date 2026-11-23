# Architecture

`omen-core` owns typed capabilities, samples, commands and versioned configuration.
`omen-hardware` owns read-only discovery and adapters. `omen-daemon` authorizes and
serializes typed operations. `omen-ipc` provides version-1 bounded NDJSON over a
Unix socket. `omen-cli` and the GTK4/libadwaita GUI consume these interfaces.

```text
GTK GUI / CLI (desktop user)
        | reads: daemon or direct read-only backend
        | writes: typed socket request, polkit
        v
omen-daemon (systemd, restricted filesystem)
        | capability + board/BIOS + write-policy checks
        +--> PPD, advertised profiles (explicit experimental opt-in)
        +--> sysfs/procfs monitoring
        +--> fan/RGB/charge adapters (fixtures only in this build)
```

Game manifests, custom launch commands and per-game bindings belong to the
signed-in user's configuration. The daemon refuses game execution. It cannot
turn a game/config string into a privileged subprocess.

## Monitoring and state

`omen-hardware::network::NetworkReader` batches `/proc/net/dev` counters and checks
sysfs interface identity/state. `get_network` is an additive read-only operation;
older daemons reject it without a live-data fallback. The shared typed snapshot
feeds Vitals, CLI and diagnostics, independently of hardware-control permission.
Rates use suspend-aware CLOCK_BOOTTIME deltas and rebase after reset/replacement
or a gap over 30 seconds. A 250 ms cache retains acquisition timestamps. Fixture
roots never read host network counters. See `NETWORK_MONITORING.md` for procfs
drop semantics, namespace/bounds and the non-atomic identity-check limitation.

hwmon units follow kernel ABI (temperature m°C, current mA, frequency Hz, power
µW). Sensor identifiers include the hwmon instance so multiple NVMe/DIMM sensors
do not overwrite one another. Samples carry acquisition timestamps. Aggregate
CPU utilization uses two /proc/stat samples and excludes duplicate guest ticks.
Reset/zero-delta counters yield unavailable, not invented utilization.

The backend caches live sweeps for 500 ms, shared by daemon clients. NVIDIA CSV
queries use a two-second deadline and refresh at most once every five seconds;
known runtime-suspended devices are not queried. AMD DRM usage/VRAM is read where
present. PPD properties are read through the bounded system busctl utility rather than repeated Python powerprofilesctl processes. The GUI updates only mapped pages and draws only visible charts. The GUI normally polls at one second and preserves individual metric
sample times. No background NVIDIA sweep occurs when nobody requests samples.
Kernel/sysfs stalls are not universally preemptible; this remains a limitation.

The device-isolated daemon cannot use NVIDIA device nodes. The unprivileged
`omen-client::monitoring` controller supplements successful real-daemon reads
with missing NVIDIA samples, cached for five seconds and skipped while the
selected GPU is suspended. Existing daemon samples remain authoritative; mock,
unknown, root and failed reads are not supplemented. Diagnostics identify the
local sensor IDs. All control requests retain the daemon boundary; local GPU
measurements do not enter its fan-policy loop. See `GPU_MONITORING.md`.

GTK draws bounded history with Cairo. The user’s `history_samples` preference
sets retention (default 300, bounded 10–10000); the axis is sample order rather
than wall-clock time. Non-finite values are excluded from graph buffers. CPU load average is dimensionless.
Missing values are displayed unavailable. GPU vendor alone does not prove
integrated/discrete topology; unclassified topology is labeled accordingly.

## IPC and authorization

The default socket is `/run/omen-linux/ctl.sock`; `OMEN_SOCK` selects an explicit
alternate socket for development. Live local clients may read monitoring.
Writes require SO_PEERCRED + root/polkit; subjects include PID/start-time/UID.
Protocol mismatches fail before dispatch. The server admits at most 16 clients,
limits request lines to 64 KiB, and applies read/write deadlines. The client
bounds replies to 1 MiB. Helper utility output and runtime are bounded.

Mock hardware uses unique temporary trees and a private socket; it never invokes
the host's PPD. Regular fixture files do not emulate firmware timing, permissions,
side effects, BIOS quirks, or safety behavior. Diagnostics include `mock: true`.

## Controls and configuration

`SysfsBackend::new()` and a root `/` backend deny live device-specific writes.
Explicit profile opt-in pins board and BIOS and never enables fan/RGB/charging
writes. Unknown boards cannot gain support from a marketing-name match alone.
The active PPD is preferred over direct platform_profile. Raw profile spellings
are preserved when writing (e.g. low-power). Choices come from the backend.

The daemon's single fan tick loop and commands serialize on one mode mutex.
Curve configuration is read on every tick; successful configuration persistence
precedes replacement of in-memory state. Shutdown freezes writes, then recovers
owned state. No hardware work occurs inside a signal handler or panic hook.
Live crash recovery is unverified and fan release is blocked. Revision 9 adds
recognized CPU/GPU input identities, acquisition/cohort checks, pending Auto
recovery retries and separate named fan/profile workers. A shutdown check after
profile reads prevents late automatic writes. Revision 10 puts mock fan writes
in a separate process over pinned regular files, with expiring leases and
explicit rearming after revocation. It rejects kernel/pseudo-filesystems; the
real backend remains write-blocked. See `THERMAL_GUARD.md` and `FAN_GUARDIAN.md`
for fixture limits and the missing production hardware supervisor.

User config: `$XDG_CONFIG_HOME/omen-linux/config.toml`. System config:
`/etc/omen-linux/config.toml`. Developer overrides: `OMEN_CONFIG_DIR` and
`OMEN_SYSTEM_CONFIG_DIR`. Schema 0 migrates to 1; future versions are rejected.
Unique temporary files, fsync and rename reduce partial-save risk; `.bak`
preserves the previous version. Battery automation defaults off. Startup curve
and lighting restoration settings are currently inert for live hardware.

## Unimplemented work

Live validated fan/RGB/charge release; complete power-policy ownership across
external desktop changes; network traffic
priority, additional launcher formats, per-key RGB, MUX switching,
comprehensive GTK widget automation, and multi-distribution packaging. The
master prompt remains an ongoing goal; these are not simulated as completed.
The application interface is English-only at the user's request. Native system
dialogs follow desktop language settings; additional translations are outside scope.

## Desktop tray boundary

`gui/src/shortcuts.rs` implements a separate, opt-in global Show action through
the desktop portal. A dedicated registered peer, token-matched asynchronous
requests, portal-owner pinning and generation checks isolate approval and late
callbacks. Only Show is exposed, with no hardware operation. A saved user opt-in
restores the session after startup, including minimized startup. Cancellation,
failed persistence, owner loss and shutdown close sessions; loss reveals a hidden
window. Wayland export handles are held and released with the session. The
desktop owns key assignment and conflict policy. `GLOBAL_SHORTCUTS.md` records
the protocol fixtures and the remaining real-key/desktop approval limits.

`gui/src/tray.rs` uses ksni on the session bus. Registration and state updates
run outside GTK, via a bounded snapshot queue. Callbacks enqueue UI commands;
profile requests pass through the same serialized action worker, daemon IPC and
backend capability checks as the Performance page. The tray never receives raw
hardware paths or introduces a root UI. Only changed profile, visibility or
capability state updates the menu. Monitoring snapshots refresh the profile
controls' availability and reason, including battery-policy ownership and daemon
loss. The latest capability also gates queued tray requests; the daemon remains
the authorization and control-policy boundary. Loss of an established watcher reveals the window and a return
re-registers it without applying the initial minimized preference again.

A missing initial host leaves the window open. Closing quits, hiding keeps the
window/application and monitoring alive, and Quit terminates the GUI. These
operations leave the daemon and its hardware ownership unchanged. App activation
reuses the existing window instead of creating duplicate polling threads.

## Scoped configuration ownership

Core `settings.rs` defines tagged User and Service backups over schema 1. User
backups include general/game data; Service backups include fan/lighting/power
policies. The shared unprivileged `omen-ipc::settings::SettingsController` serves
GUI and CLI. A user operation always loads/saves the explicit user path, never
falls back to a daemon config, and serializes in-process edits. Service operations
use typed GetServiceSettings / SetServiceSettings, with no user-file fallback.
The daemon always uses its system path, even when running unprivileged.

Service writes require the admin action. Merging, persistence and replacement
of the in-memory config share a mutex. Unsupported automation/profile targets
are refused before persistence. Legacy full-config IPC remains for compatibility;
new clients do not use it for backups. Typed service imports retain unrelated
legacy desktop fields but never add or overwrite them. User imports preserve
unrelated legacy policy fields in the user file.

Native file selection is asynchronous. Collection, file IO and authorization run
off GTK's main thread. Service import previews automation before Apply; the same parsed payload is sent
after consent. Saved fan and lighting replacement is stated explicitly. JSON exports are private
atomic files, and imports are bounded to 1 MiB. Malformed user files are not
replaced with defaults when editing bindings; failed selections revert.

An unset AC automation target serializes as `on_ac_profile = "unchanged"` in
TOML/JSON. JSON null is accepted for legacy imports. Explicit persistence avoids
reloading the legacy missing-field default (Balanced) after a restart. Existing
omitted fields retain their earlier defaults.

## Desktop launch controller

`omen-client` is linked by CLI/GUI, not the daemon. Its game workflow loads the
user configuration, resolves/validates the launcher and rejects UID 0 before
profile mutation. The hardware library's prepared launcher repeats the UID check
at spawn and observes initial process status for a bounded 300 ms window.
Receipts explicitly leave `game_execution_verified` false. Successful process
submission is not a claim about a game's independent process or lifetime.

Library discovery reads bounded Steam KeyValues and Heroic JSON files, plus
Lutris SQLite using a read-only connection that observes committed WAL data.
Native/Flatpak game IDs differ, keeping profile bindings and launch targets
distinct. Store commands use validated IDs and fixed argument vectors; only
explicit Custom entries invoke a user shell. See `GAME_LAUNCHERS.md` for paths,
reviewed source versions and exclusions.

A bound-profile transaction reads the prior state, skips redundant writes,
verifies application, then submits the launcher. Failure, including a partially
failed application request, triggers best-effort recovery. It checks current
state before restoration, preserves an observed different profile, verifies
restoration, and carries original/target metadata on uncertain outcomes.
No unbound launch reads or changes hardware profiles.

The daemon uses a profile mutex shared by manual writes, automatic policy ticks,
profile reads and shutdown. Policy updates use the same guard, so a tick’s cloned power policy stays current
through its write without holding the shared config mutex over driver IO. An
acknowledged policy change cannot leave an older tick pending outside that guard.
Shutdown restores owned fans before waiting for profile IO to drain. A profile read cannot overtake a write holding the mutex.
Manual requests are rejected while battery automation is enabled. This does not
make comparison/restoration atomic across processes or cancel pending/authorized
requests on transport timeout. External ABA/same-target intents remain unknown.

Fan monitoring uses a shared read-only reader independent of manual controller
selection. Mode-only hp-wmi devices can report kernel Auto/Maximum policy without
granting a write capability; missing/invalid manual duty is unknown. Known fan
process names are matched using Linux comm truncation, including OmenCore CLI,
but the inventory is best effort rather than an exclusive hardware lock.
See `FAN_MONITORING.md`.

## Compact monitoring window

`gui/src/overlay.rs` receives the same bounded snapshots used by pages/tray,
with collection-start timestamps for aging even when the poller stalls. The
existing GTK drain handles visible refreshes; there is no extra polling worker.
It uses a user-owned additive `general.overlay_enabled` field, native GTK
application windows, a tray action and a granted optional portal action. Closed
overlay windows release their toplevel/accessibility roots and are recreated
on demand. The main window remains the single-instance activation target and
closing it quits both. See `STATUS_OVERLAY.md` for desktop restrictions.
