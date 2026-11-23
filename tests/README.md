# Tests

`cargo test --workspace --locked` exercises core, real adapters over temporary
fixture trees, daemon logic and server framing, IPC and cross-crate wire tests.
`cargo test --release --workspace --locked` checks the optimized build too.

`python3 tests/smoke.py` launches the actual debug daemon and CLI in mock mode.
`OMEN_TEST_PROFILE=release python3 tests/smoke.py` uses release binaries. No host
power manager is invoked by mocks. The older integration crate uses a wire shim;
production server coverage lives in `crates/daemon/src/server.rs`.

`python3 scripts/capture-pages.py` renders nine GTK pages into app-only screenshots
with the available graphical session. This is visual smoke coverage, not full
widget/accessibility automation. `scripts/benchmark.py` measures only read-only
operations and reports real process resource deltas.

`python3 tests/gui_settings.py` (or `OMEN_TEST_PROFILE=release`) uses actual
AT-SPI actions and selection against the application's native GTK dialogs on
an explicit `GDK_BACKEND=wayland` or `GDK_BACKEND=x11` session. It tests settings/diagnostics workflows against a private fixture
daemon, including preview cancellation/application, invalid input and service
loss. Session/accessibility buses, configuration and recent-file history are
isolated. It requires Python GI/Atspi, dbus-daemon and an active graphical
session. The GTK fallback picker is used; desktop portal dialogs, keyboard/pointer
coverage and graphical polkit prompts remain unverified.
Four additional Vitals workflows cover fixture network rates, missing data and
replacement. Sensor search and the service health display are also checked.
Set `OMEN_TEST_EVIDENCE_DIR` to an existing directory to save run logs and
results there instead of overwriting the recorded evidence in `docs/evidence`.

`python3 tests/tray.py` (or `OMEN_TEST_PROFILE=release`) tests six GTK and D-Bus
tray behaviors on KDE, including changing profile capability state without
changing the selected profile. Profile writes use fixture hardware only.

`python3 tests/game_launch.py` (or `OMEN_TEST_PROFILE=release`) exercises the
shared launch controller through real CLI/child processes and a private fixture
daemon. It covers native/Flatpak store routing, UID/argument boundaries and
early-failure profile recovery. No installed store client or game is run.

`python3 tests/package_lifecycle.py NEW.deb --baseline OLD.deb` uses an existing
local Ubuntu 26.04 Docker image and installs distribution dependencies inside a
disposable container. It tests package enablement/configuration/remove/purge and
production IPC with a private system bus and temporary polkit rules. It passes
no host bus, mounts or devices and removes the container even on failure. This
does not test a systemd PID 1, its sandbox, or a graphical authentication prompt.

`python3 tests/systemd_vm.py PACKAGE.deb` uses a signed Ubuntu 26.04 cloud image
and local QEMU/KVM runtime to test the installed service with systemd PID 1.
It checks actual mount permissions and capabilities, authorization, stop/start/
restart, crash recovery, two reboots and package lifecycle. The host package
remains uninstalled. See `docs/VM_VALIDATION.md` for input verification and setup.

No automated test in this suite is permission to perform live hardware writes.
See `docs/PROFILE_VALIDATION_PLAN.md` and the report's unverified list.

`python3 tests/notifications.py` starts a private D-Bus notification server and
fixture daemon, then verifies real GNotification delivery, opt-out, source
attribution and suppression of repeated high readings. It does not notify the
real desktop. Notification display and physical accessibility still require
desktop validation. `OMEN_TEST_PROFILE=release` selects packaged binaries.

`python3 scripts/capture-audit.py` captures six application-owned fixture views
in light/dark/high-contrast and narrow/requested-large-text configurations.
Metadata records requested and actual dimensions; requested font scaling alone
does not establish physical DPI or full large-text accessibility.

`python3 tests/fan_guardian.py` (or `OMEN_TEST_PROFILE=release`) tests the
independent fixture writer through the actual daemon. It stops/kills only its
own parent/helper processes, checks Automatic while the parent remains stopped,
requires explicit rearming, and rejects a replacement actuator path. A Linux
child subreaper reaps its own orphaned helpers. No live actuator is accepted.
The helper shares its daemon's cgroup; this is not whole-service crash recovery.

`python3 tests/live_gpu.py` performs read-only real NVIDIA queries and runs the
production daemon in a disposable user systemd namespace with physical devices
hidden. It requires an already-active GPU, verifies the CLI's local supplement,
and removes its transient service/files. It does not install the package or
test the root unit on physical hardware. `--capture` optionally captures the
connected GUI's own dashboard. See `docs/GPU_MONITORING.md` for scope and limits.

`GDK_BACKEND=wayland python3 tests/shortcuts.py` runs 24 native GTK/portal workflows
on private buses, with a private desktop entry and fixture hardware. Also run
with `GDK_BACKEND=x11`, and `OMEN_TEST_PROFILE=release` for both backends. Results
record the actual GDK display type. These tests exercise the normal single-instance
app, cancellation, persistence, activation filtering and desktop-service loss.
They do not assign host shortcuts or inject physical key events.

`python3 tests/shortcut_portal_probe.py` independently checks the real portal's
Register/CreateSession/ListShortcuts/Close path without binding keys. It requires
the desktop launcher; unsupported results are recorded without claiming success.
See `docs/GLOBAL_SHORTCUTS.md` for the full validation boundary.

`python3 tests/network.py` (or `OMEN_TEST_PROFILE=release`) runs nine production
IPC/CLI scenarios, including fixtures and compatibility with an extracted
revision 6 daemon. It also brackets real read-only host counters with independent
sysfs reads using an unprivileged daemon with controls disabled. It generates no
traffic and changes no network policy. The older package must exist at
`target/debian/omen-linux_0.1.0-6_amd64.deb`. See `docs/NETWORK_MONITORING.md`.

`python3 tests/fan_monitoring.py` (or `OMEN_TEST_PROFILE=release`) tests production
fan IPC/CLI against private mode-only and manual-target fixtures. It mutates
only those fixture files, verifies missing-data behavior and denies fixture
controls under an OmenCore process conflict. Add `--live-readonly` on the
reference HP machine to compare its real kernel-reported mode through a private
read-only daemon and direct CLI with independent sysfs reads. No real control
operation is submitted. See `docs/FAN_MONITORING.md` for the validation boundary.

`python3 tests/thermal_guard.py` (or `OMEN_TEST_PROFILE=release`) runs nine
fixture-only production curve/failure scenarios. It uses its own daemon,
SIGSTOP/SIGCONT and private files/FIFO; it never starts a real hardware backend.
The blocked power-worker case observes continued fan ticks and Auto restoration
before profile IO drains. This is not a physical thermal or suspend test.

Revision 11 extends `tests/shortcuts.py` with eight overlay workflows per build/
backend, including fixture measurements, hidden-main toggles, stale aging during
fixture SIGSTOP/CONT, preference failure and restart. It also drives both portal
actions through the same private approved-session fixture. Use explicit
`GDK_BACKEND=x11` / `wayland` and `OMEN_TEST_PROFILE=release` for the full matrix.
`scripts/capture-overlay.py` makes bounded app-only captures with fixture 1×/2×
and live read-only 1× sources. It captures GTK widget trees, not the desktop.

## Expanded launcher formats

`python3 tests/game_formats.py` checks 15 isolated discovery/routing and rollback
scenarios through the actual CLI and fixture daemon. `OMEN_TEST_PROFILE=release`
selects release binaries; `OMEN_TEST_EVIDENCE_DIR` selects an output directory.
`OMEN_TEST_BIN_DIR` and `OMEN_TEST_TEMP_DIR` support installed-container checks.
All records and executable routes are synthetic; no host launcher is installed.

For the four additional offline upstream Legendary checks, provide
`OMEN_LEGENDARY_SOURCE` for the pinned source recorded in
`docs/evidence/launcher-expansion/launcher-sources.json`,
`OMEN_LEGENDARY_DEPENDENCIES` for isolated Python dependencies/metadata and
`OMEN_NATIVE_GAME_TEST_BIN` for the native-session test helper. The helper is
built with `cargo test --locked -p omen-cli --features native-session-tests
--test managed-games-native --no-run` (add `--release` for release). Restore the
normal `cargo build --workspace --locked` afterward before invoking the suite,
so the real CLI does not inherit test-only authorization behavior. The upstream
source and dependency hashes are checked/recorded; an offline socket/DNS guard
rejects attempted network use. No real Epic account, game, DRM or Proton result
is implied by synthetic offline game execution.

`tests/gui_game_formats.py` uses private XTEST input and read-only AT-SPI
observation for six keyboard workflows, including GTK folder selection. It
verifies the selected installation and exact entered path before accepting them.
`tests/gui_game_formats_presentation.py` stages five expanded setup/location
captures through native accessibility actions, then measures actual font, text
bounds and sampled label contrast. It requires an explicit evidence directory.
Its staging actions are separate from the actual keyboard checks.

For an explicitly selected image with existing distribution dependencies,
`tests/package_lifecycle.py --offline-dependencies --game-format-fixtures ...`
purges only that new test container's old OMEN package, installs the local package
with dpkg, validates dependencies without downloads and runs 15 format checks as
UID 1000. Use a pre-helper baseline such as revision 1. Reused dependencies do not
establish a pristine distribution install, a systemd VM or graphical behavior.

## Offline systemd VM validation

`tests/systemd_vm.py --offline-repository REPOSITORY.tar PACKAGE.deb` installs
from a local signed Ubuntu repository in a fresh signed guest. QEMU restricts
networking to the explicit localhost SSH forward. APT retains signature/hash/date
checks and selects only package indexes, rather than optional desktop search
metadata. The optional `--resolve-only` mode records dependency URIs and exits
with result `resolved` without installing/running the app; it is not a test pass.

`scripts/prepare-offline-vm-repository.py` verifies copied InRelease signatures,
checks uncompressed package-index SHA-256 values and prepares metadata or a
resolution-specific archive set. It reuses verified caches or downloads over
HTTPS and verifies complete archives against the signed indexes' SHA-256 values.
No host package is installed. Exact reproduction commands, snapshot freshness
limits and revision 20's 33-check result are in
`docs/DEPLOYMENT_VALIDATION_2026-10-28.md`.

`--repository-component main --repository-component universe` selects both
signed components; pass the same components to the preparation script using
`--component`. `--extra-package` adds guest-only environment dependencies to
both resolution and installation. The preparer can resume incomplete transfers
and accepts `--download-timeout` for large archives. It still accepts an archive
only after its complete signed-index size and SHA-256 match.

The optional `--graphical-authorization --memory-mib 2048` stage requires a
repository containing Plasma Desktop, its Wayland session, SDDM/Breeze, the KDE
polkit agent and file portal, Xorg/Xwayland, AT-SPI/PyGObject and python3-libevdev.
It starts an actual local Plasma Wayland session in the guest's virtual GPU and
launches the installed GUI through desktop autostart. Guest-only uinput sends
keyboard events; the host keyboard/display and authentication agent are not
used. No authorization-grant rule is installed for this stage. Its temporary
guest password is held privately, then the prior account hash is restored.
The guest-root harness places its cache inspector in the verified local SDDM
scope before dropping it to UID 1000, because polkit requires the inspector and
temporary-authorization subject to share a session. The GUI itself is launched
normally through desktop autostart; its PAM authorization is not bypassed.
Battery automation remains disabled. The intended checks cover preview/dialog
cancellation, rejected and successful authentication, retained approval and its
natural expiry. A successful run establishes those virtual-desktop paths only;
it does not establish physical keyboard/display or hardware behavior.

Guest screen captures use the private QMP socket and are saved only inside the
selected evidence directory. Desktop/polkit journals are also retained. Set
`OMEN_TEST_EVIDENCE_DIR` to a new directory for every attempt. `--keep-failed` is
for debugging only: it stops QEMU but retains that attempt's disk and private
SSH inputs, which must be removed after inspection. Successful runs remove
their private VM inputs automatically.

`OMEN_TEST_PROFILE=release OMEN_TEST_EVIDENCE_DIR=target/icon-theme-run python3
tests/gui_icon_themes.py` checks the actual GUI's mapped icon resolution with a
missing icon theme, Adwaita and Breeze. It also measures wide light, narrow dark
large-font and installed-theme scenes using the existing geometry/contrast
collector. The output directory must be new. All desktop buses/input authority,
daemon data and configuration are private fixtures; screenshots capture only the
app's own widget tree. Valid theme icons must retain priority over bundled
hicolor fallbacks. Visual inspection remains a separate bounded batch.

`tests/flatpak_setup.py` tests the real CLI against a private home and an explicit
permission-report fixture. It checks private installation, repeated update,
permission denial, modified-file/link refusal and launcher-scoped removal.
`tests/gui_flatpak_setup.py` drives the same workflow using XTEST keyboard events
on a private native desktop. `tests/gui_flatpak_presentation.py` measures five
expanded setup/permission/recovery scenes in light, dark, high contrast and large
text. Set `OMEN_CONTROL_HIGH_CONTRAST=1` for the control-page scene matrix in
high contrast. These are bounded app-only captures, not desktop screenshots.

`tests/game_job_control.py` uses the real CLI, private PPD and actual user scopes
to check inherited descriptors and foreground PTY stop/resume behavior. Set
`OMEN_NATIVE_GAME_TEST_BIN` to the release `managed-games-native` test executable.

`tests/systemd_vm.py --real-flatpak-game ARCHIVE` installs signed Flathub Lutris
using a previously verified sideload export and a restricted HTTPS metadata
relay, then runs actual distro OpenTTD/OpenGFX inside that sandbox. The guest's
provider stays private. The game libraries are copied unchanged to app data to
match the distro ABI; no host package, user library or physical profile is
changed. This path requires the complete graphical/Flatpak/game dependency
repository and a disposable 24 GiB guest. It does not establish Steam/Proton,
Heroic, graphics acceleration or physical control compatibility.
