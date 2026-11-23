# OMEN Control for Linux

A Rust + GTK4/libadwaita desktop application for genuine Linux system monitoring,
with a CLI, restricted service and capability-aware controls. No account, ads or
telemetry. **Version 0.1.0 is a development snapshot, not a validated hardware-control
release.** The full engineering goal in `prompt.md` remains in progress.

![Read-only dashboard connected to a device-isolated user service](docs/screenshots/dashboard-sandboxed-service.png)

## Current behavior

| Area | Implemented status |
|---|---|
| Monitoring | CPU temperatures/utilization/clocks, GPU metrics where exposed, memory/storage, fan RPM, battery/AC and profile state. A read-only client NVIDIA fallback preserves GPU monitoring across the service's device sandbox. Missing readings stay unavailable |
| History | Bounded time-based charts with acquisition timestamps, explicit gaps, accessible readings, selectable ranges and private CSV export. NVIDIA polls at most every 5 seconds; runtime-suspended devices are not queried |
| Network | Per-interface receive/send rates, link state and cumulative errors/drops in Vitals, CLI and diagnostics. Read-only kernel counters; no packet capture, generated traffic or priority changes |
| Performance profiles | Current state and advertised choices. Writes off by default; experimental standard-profile testing requires an exact board/BIOS daemon opt-in and authorization |
| Fans | Real RPM monitoring. Automatic/Maximum buttons are separate from manual percentage/curve controls and fixture-tested; live writes are off by default. An exact-target experimental independent writer supports a ten-second Maximum trial after separate approval; sustained physical control remains unvalidated |
| Lighting / charging | Adapters and validation tests exist. **Live writes disabled**; interface absence does not prove missing physical hardware |
| Graphics | PCI/driver/runtime-PM status. No implemented MUX/Optimus switching; unknown topology is labeled |
| Games | Native/Flatpak/Snap Steam, account-scoped Steam shortcuts, standalone native Legendary, Heroic and Lutris installed-library discovery, rescan/search and selected record locations; custom commands as the desktop user. Opt-in actual-command wrappers hold Performance/Quiet until their process tree exits; manual mode retains submission transactions. Real store/runtime and Flatpak managed execution remain unvalidated. The privileged service refuses game execution |
| Settings | Versioned TOML, migrations and scoped User/Service JSON backups. Service import previews policy changes; exports use private atomic files |
| Desktop integration | KDE StatusNotifier tray; opt-in global Show shortcut through XDG Desktop Portal with native key assignment; cancellation, startup restoration and service-loss recovery. Physical-key/desktop approval validation remains pending |
| Packaging | `.deb` build script, systemd service, polkit actions and original desktop assets |

Nine pages cover Dashboard, System Vitals, Performance, Fans, Lighting, Graphics,
Games, Battery, and Settings/Diagnostics. Unsupported writes show a reason.
On Dashboard, **Choose metrics** controls which readings appear; the selection is
stored with user settings and included in user backups. Cards report a source or
report type and age when available, and identify stale or unavailable readings.
The library keeps a bounded set of native row controls as you scroll and preserves
expanded setup across filtering. Network readings update their existing native
rows; profile choices refresh as monitoring becomes stale or recovers.
Selecting network text holds a clearly labeled copy snapshot until the selection
is cleared or focus leaves; missing data still removes the reading.
Game Library also retains private launch-issue reports, including managed
failures before registration and late launch-process exits. Clearing those reports
does not change profiles or running sessions.
Standalone Legendary retains its selected configuration and checks runtime routing support before profile changes. See [expanded launcher support](docs/LAUNCHER_EXPANSION.md) for format boundaries and offline upstream verification.
The application interface is English-only. Native system dialogs follow desktop settings.
F6 moves between panes, Ctrl+1 through Ctrl+9 open pages, and Ctrl+F focuses
sensor/game search. Controls reflow with the configured text size, charts use
scaled Pango labels, and action feedback can announce through Orca. See
[keyboard access and presentation](docs/ACCESSIBILITY.md) for runtime support and
verification limits.
The sidebar folds in smaller windows. The application menu offers diagnostics,
the status overlay, tray actions and a skippable welcome panel. Settings includes
a live capability report and optional informational temperature notifications
with hysteresis and a rate limit. Notifications require the app to be running.

Revision 21 bundles native fallback symbols for incomplete desktop icon themes
while preserving installed theme priority. Its candidate passes 39 checks in a
fresh Ubuntu/Plasma Wayland systemd VM, including real KDE/PAM authorization,
natural approval expiry and two reboots. Four icon/theme scenes and 31 native
regressions per X11/Wayland backend also pass. The
[desktop validation report](docs/DESKTOP_VALIDATION_2026-10-28.md) records exact
identities and limits. The unchanged final package also passes a separate
[real native Lutris/OpenTTD validation](docs/REAL_GAME_VALIDATION_2026-10-28.md):
six game checks within a 39-check fresh VM suite. Game/data/free graphics total
about 33 MB. Launch/exit, explicit release, restart and guest fullscreen behavior
pass with a private profile provider and software video. Broader game/store,
desktop/distribution coverage,
complete accessibility and physical release gates remain open. The unchanged
revision 20 package retains its separate
[33-check deployment result](docs/DEPLOYMENT_VALIDATION_2026-10-28.md).

The [audit implementation tracker](docs/APPLICATION_AUDIT_IMPLEMENTATION.md)
records completed changes, verification and the remaining requirements.

## Build and run

```bash
# GTK4/libadwaita and GLib development tools plus Rust are required.
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p omen-cli -- detect
cargo run -p omen-gui
```

The GUI and CLI work read-only without installing or starting a daemon. For local
GTK development-prefix instructions and Debian packaging, see
[installation](docs/INSTALLATION.md).

```bash
./packaging/build-deb.sh
# Produces target/debian/omen-linux_0.1.0-21_amd64.deb on this host.
python3 tests/smoke.py
```

Mocks are development fixtures, explicitly labeled in the GUI and diagnostics.
They never establish real hardware support. Old `1.0.0` build artifacts predating
the safety audit may still exist under `target/`; use only the current `0.1.0`
artifact described by its matching validation report. Revision 19 adds keyboard,
screen-reader feedback and text-scaling improvements; its evidence is in
`docs/evidence/accessibility/validation-summary.json`.
Revision 18 adds persistent
launch-failure recovery in Game Library and the CLI; see
[setup and limits](docs/MANAGED_GAME_SESSIONS.md). Its verification is recorded in
`docs/evidence/launch-issues/validation-summary.json`.
Revision 17 adds the
virtualized library, retained row identities and current profile choices; its
[verification](docs/evidence/library-virtualized/validation-summary.json) includes
10,000-entry native workflows on X11 and Wayland. At that milestone, installation/VM
and the full physical release gates remained unverified. Revision 16 adds managed
command sessions; see [setup and limits](docs/MANAGED_GAME_SESSIONS.md) and its
[revision 16 verification](docs/evidence/audit-v16/validation-summary.json). Revision
16's container/VM checks remain unverified after dependency download failures. The earlier
revision 15 package retains its historical verification in
[`docs/evidence/audit-v15/validation-summary.json`](docs/evidence/audit-v15/validation-summary.json).

## Investigation and validation

* [Reverse-engineering report](docs/REVERSE_ENGINEERING_REPORT.md): pinned Linux
  source analysis, actual REA setup/use, prior-art implementation and issue review.
* [OmenCore review](docs/OMENCORE_REVIEW.md): canonical source, 42 verified file
  hashes, interface distinctions and remaining fan-recovery limits.
* [Fan monitoring](docs/FAN_MONITORING.md): kernel-reported mode without manual
  targets, missing-data handling and conservative writer-conflict detection.
* [Automatic/Maximum fan modes](docs/FAN_MODES.md): controls, run commands and live hardware release gates.
* [Development thermal guard](docs/THERMAL_GUARD.md): CPU/GPU coverage, fixture
  recovery and independent workers; hardware trials require exact-target approval.
* [Independent fixture writer](docs/FAN_GUARDIAN.md): recovery while the daemon
  is stopped or killed, pinned descriptors and remaining hardware release gates.
* [Windows artifact inventory](docs/WINDOWS_ARTIFACTS.md): official HP SDK hashes,
  native/managed classification and service dependencies; main Store app remains
  unverified. No HP binaries/assets are redistributed.
* [Compatibility](docs/HARDWARE_COMPATIBILITY.md): observed Victus 8C30/F.10 and
  source-backed candidates, kept separate from hardware-tested support.
* [Safety/recovery](docs/HARDWARE_SAFETY.md), [architecture](docs/ARCHITECTURE.md),
  [tests](docs/TEST_REPORT.md), and [REA setup](docs/REA_SETUP_PLAN.md).

The approved [profile transaction](docs/PROFILE_VALIDATION_PLAN.md) passed on
8C30/F.10: Performance → Balanced → Performance, with daemon and independent PPD
readback. This tests the standard Linux power profile, not HP Windows thermal-mode
parity. Fan safety, firmware safeguards, physical calibration, suspend/resume and
crash recovery remain unverified.

Unimplemented: validated live fan/RGB/charge controls, per-key lighting, MUX,
physical global-key/desktop approval validation, full-screen overlay behavior, traffic priority,
full GTK accessibility automation and cross-distribution package validation.

Launcher paths, supported formats and limitations are in
[GAME_LAUNCHERS.md](docs/GAME_LAUNCHERS.md). Installed-game records can be stale;
launcher submission does not prove a game started.

## License and attribution

GPL-3.0-or-later; see [LICENSE](LICENSE). Kernel board facts and reviewed projects
are attributed in the report. The vendored REA skill is independently MIT-licensed.
This independent project is not affiliated with HP and includes no proprietary HP
implementation or artwork.

The optional [status overlay](docs/STATUS_OVERLAY.md) reuses monitoring data,
marks stale/unavailable values, and can be toggled from Settings, the tray,
Ctrl+O or a separately enabled desktop portal shortcut. It is a compact native
window; placement and full-screen visibility depend on the desktop.
