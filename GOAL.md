# Goal: OMEN Control for Linux — production readiness release candidate

- Status: complete
- Created: 2026-11-09
- Updated: 2026-11-09

## Objective

The workspace reaches a release candidate for the support matrix the evidence establishes: every actionable backend, IPC, persisted-state, monitoring, game, hardware, desktop, and packaging defect is fixed and verified through the public paths (daemon, shared client, CLI JSON, GTK4 UI, installed service); each remaining capability is accurately gated; every open gate names the exact missing interface, hardware, artifact, account, approval, or environment and the check that would close it. No fixture pass is counted as physical validation, and no live fan/RGB/charge/MUX/profile write, host install, or suspend/reboot occurs without a separate exact-target approval.

## Success criteria

- [x] cargo fmt --check, cargo clippy --workspace --all-targets --all-features --locked -- -D warnings, cargo test --workspace --locked (debug+release), cargo build --workspace --locked (debug+release), and tests/smoke.py (debug+release) pass on the final source with exact counts and sha256sum of target/{debug,release}/{omen-cli,omen-daemon,omen-gui} recorded
- [x] A fresh .deb is built with OMEN_MUSL_SYSROOT="$PWD/target/flatpak-nested-bridge/musl/root" ./packaging/build-deb.sh <fresh-out> and verified by tests/package.py; package hash and extracted binary/helper hashes recorded; the static PIE helper has no dynamic dependencies and no executable stack
- [x] tests/package_lifecycle.py with a known older baseline and tests/systemd_vm.py with a signed image and matching inputs pass in disposable environments for the declared scope (clean install, systemd startup/shutdown, unprivileged GUI startup, separate polkit actions, installed sandbox restrictions, upgrade/rollback, config/service-state preservation, remove/purge/reinstall, crash recovery, guest reboots, enabled/disabled/masked service cases) or each is marked blocked with the exact missing environment
- [x] docs/evidence/<run-id>/validation-summary.json is published with source identity and hashes, binary/helper/package hashes, exact commands and counts, environment and inputs, pass/fail/blocked results, cleanup, and logs; historical evidence is attributed to its original source and artifacts
- [x] GOAL.md, docs/IMPLEMENTATION_STATUS.md, and docs/CONTINUATION_CHECKLIST.md carry the reconciled issue inventory; each row has final disposition, source location, verification, and remaining prerequisite
- [x] Every remaining gate names the exact missing interface/hardware/artifact/account/approval/environment and the check that would close it; release readiness is reported only for combinations that passed
- [x] No live fan/RGB/charge/MUX/profile write, host install, or suspend/reboot occurs without a separate exact-target approval; unsupported capabilities stay explicitly unavailable with a reason

## Non-goals

- Live fan manual/curve, per-key lighting, animations beyond static/off, hetero-zone beyond 4-zone, charge threshold, MUX/Optimus switching, OMEN firmware thermal-profile path, and raw WMI/EC/MSR/under/over-volt on unverified hardware; keep them explicitly unavailable with a reason
- Physical coarse fan trial and RGB/charge/MUX/manual-curve validation without separate exact-target approval and a verified interface
- Windows parity claims from HP SDK analysis alone; no proprietary code copied
- Multi-GPU concurrent sets until stable PCI identities, source deduplication, and selection are specified and verified with two distinct devices
- Automatic launcher editing and Snap wrapping; the launcher command stays user-configured
- Zoom, sandboxed Legendary, AppImage discovery without a verified route, and broader KeyValues semantics
- Real Steam/Proton, Heroic, Legendary/Epic, and Snap execution as release gates unless installed and accounts are present
- English-only interface scope creep; translations remain out of scope
- Live file watching: direct TOML edits and imports from a separate CLI process do not need live reload
- Rebuilding or overwriting historical evidence as though it tested new code

## Constraints

- Reference device is HP Victus 8C30 / BIOS F.10 / kernel 7.0.0-38-generic with no manual pwm1/pwm2; unknown devices stay read-only
- Default daemon stays read-only; experimental writes require exact-target opt-in, pinned actuator, thermal preflight, independent recovery, and separate authorization
- GUI and game processes stay unprivileged; the daemon sandbox is not widened for hardware access
- History resize preserves gaps, original acquisition timestamps, and source labels; clear history only on source identity change
- Keep polling/freshness bounds, stale-control gating, and the NVIDIA 5s cadence; keep the worker off the GTK main thread and avoid duplicate workers
- User and Service settings stay isolated; writes fail-closed; keep current values after cancellation, invalid input, or failed write
- Keep version-1 IPC compatibility for supported callers
- GUI gates require source /home/papan/gtk-prefix/env.sh and LD_LIBRARY_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu; packaging requires OMEN_MUSL_SYSROOT="$PWD/target/flatpak-nested-bridge/musl/root"
- No git metadata exists; the baseline is a sorted source manifest of relative paths plus SHA-256 covering code, manifests, Cargo.lock, packaging, and harnesses
- Keep private fixture, installed fixture VM, real game in a guest, read-only physical observation, and authorized physical-write evidence labeled separately

## Milestones

### M1 — Baseline manifest, inventory reconciliation, and locked gates

- Status: complete

Acceptance:

- [x] Sorted source manifest of relative paths plus SHA-256 covering code, manifests, Cargo.lock, packaging, and harnesses recorded outside generated artifacts; no git metadata confirmed
- [x] Reconciled issue inventory by feature and entry point mapped to source path, reproduction, expected behavior, severity, acceptance check, and prerequisite; implementation and validation statuses kept separate
- [x] Locked gates run on current source with the gtk-prefix env: cargo fmt --check, clippy -D warnings, test debug+release, build debug+release, smoke debug+release; exact counts and binary hashes recorded
- [x] docs/evidence/<run-id>/validation-summary.json published for the baseline

### M2 — Partial diagnostics through the actual clients

- Status: complete

Acceptance:

- [x] crates/client/src/monitoring.rs preserves a valid partial ExportDiagnostics response (request ID, provenance, unrelated sections) when sensors is null with an error, skips the NVIDIA supplement on legitimate failure, and keeps eligible supplementation, acquisition timestamps, attribution, and deduplication for valid arrays; failure stays distinct from a successful empty sensor array
- [x] crates/cli/src/main.rs and gui/src/data.rs direct-read-only builders make the reporting contract consistent; network and other optional collectors reviewed for partial failure
- [x] Tests cover real-marked null/error, empty success, populated array, legacy success, malformed data, and fixture reports across daemon, shared client, CLI JSON, and GUI export paths; the report stays valid JSON with machine/capability data and source attribution

### M3 — IPC, authorization, and service availability

- Status: complete

Acceptance:

- [x] Malformed UTF-8 inside otherwise valid JSON is rejected before authorization or dispatch; server/client framing audited in both directions
- [x] Trickling reply, missing newline, truncated EOF, blank/foreign frames, bad JSON, unknown operation/version, oversized request/reply, and exact limit boundaries tested; an absolute end-to-end deadline covers partial input and skipped frames; a separate bounded budget covers interactive authorization
- [x] Peer disappearance, socket replacement/concurrent startup, stale socket ownership, PID reuse, authorization denial/cancel/expiry, lost polkit/D-Bus, and service restart between preview and action tested; a stale approval or queued action has zero effects on a replacement service
- [x] Idle-client displacement, same-UID/global saturation, slow/stuck providers, SIGTERM, and client disconnect before/during dispatch tested; workers, I/O, timeouts, and response sizes bounded; monitoring and fan recovery stay responsive; a rejected write is distinguished from a lost reply with unknown outcome
- [x] Real server/client tests show invalid and unauthorized requests produce zero side effects; normal clients recover after load, loss, and restart; supported older clients and daemons have documented outcomes

### M4 — Persisted state and updates recoverable

- Status: complete

Acceptance:

- [x] OmenConfig::from_str covers legitimate v0/v1, omitted legacy fields, wrong-type, negative, overflow, and future versions; defaults only on intentional legacy or missing-file paths; unreadable or corrupt files are reported accurately with contents preserved
- [x] Overlapping GUI and CLI read-modify-write on a private config converge without lost updates; conflicting edits, malformed/oversized imports, and symlink or replacement races covered; symlinked home stays supported
- [x] Failure injection around lock acquisition, validation, backup, write, rename, and directory sync covers ENOSPC, permission loss, crash after commit, and retry; committed, recoverable, and user-visible state established; live state applied only with confirmed persistence or explicit reconciliation
- [x] Partial Flatpak helper install/removal is safe to retry, including interruption after settings update but before directory cleanup; manual bindings, unrelated/modified files, and active sessions preserved
- [x] Live general.history_samples and general.poll_interval_ms rechecked on successful User import in the same GUI process; poller off the GTK thread; shared freshness rule; shrink/growth semantics intact; cancellation, invalid input, failed writes, freshness across pages/overlay, and notification timing tested

### M5 — Monitoring identity, units, and degraded operation

- Status: complete

Acceptance:

- [x] Empty, partial, malformed, non-finite, and out-of-range readings, late/future/stale acquisitions, and repeated cached snapshots handled; timestamps and truthful unavailable states preserved through chart, CSV, diagnostics, notifications, and controls
- [x] Daemon arrival, loss, replacement, and same-source recovery tested; default read-only fallback and explicit source behavior confirmed; CLI missing-socket fallback intentional for explicitly selected sockets; test/fixture commands cannot be mistaken for host samples
- [x] Writes bound to fresh capability and readback from the original peer
- [x] Documented hwmon chip/attribute units compared against fixtures and independent reference measurements; scaled() assumptions reviewed; counter reset/wrap/zero elapsed, interface rename/reuse, topology churn, sleep/resume gaps, and partial metric availability covered; network rates/units verified without changing traffic policy
- [x] Multi-GPU identity and selection design specified (stable PCI identity, deduplication across hwmon/DRM/NVIDIA and client/daemon sources, hotplug/reordering, legacy identifiers, unavailable adapters) with two distinct devices in fixtures and separate physical verification; per-adapter safeguards and the 5s NVIDIA cadence preserved; runtime-suspended devices stay asleep; local display supplementation stays outside privileged thermal policy

### M6 — Game and profile lifecycle verification

- Status: complete

Acceptance:

- [x] Flathub cancellation gap resolved or correctly attributed: raw attempts reviewed; isolated guest test of the correct agent, enabled Cancel action, and dialog disappearance; pointer input verified (seat, focus, output origin/scale, AT-SPI coordinate space, actual placement); injection failure vs polkit cancellation vs wrapper failure vs game failure distinguished
- [x] Complete signed Flathub Lutris/OpenTTD path rerun on the frozen candidate with private PPD: cancellation requires actual dismissal, expected CLI error and registration issue, zero game commands for that attempt, unchanged profile, no live hold or orphaned scope, and cleanup; positive cases require observed game execution/response, profile held through the tracked process tree, normal exit/restoration, explicit release while the game continues, and daemon restart with truthful Interrupted
- [x] Early/late launch failures, stale records, absent/unready launchers, no-session or missing cgroup/pidfd support, wrapper/GUI crash, duplicate requests, same-UID/cross-UID overlap, PID/scope reuse, scope migration, provider loss/replacement, policy change, and external profile writes (same-value/ABA) exercised; restore conditional on verified ownership; a manual receipt establishes launcher submission; a game running or ended is reported only with lifecycle evidence
- [x] Each supported wrapper/runtime checked for actual UID, cgroup membership and original limits, hold lifetime through the final descendant, explicit release, nonzero exit/signal behavior, PTY/job control, inherited descriptors, and cleanup after failed authorization or exec; in-sandbox execution and peer-bound handoff kept
- [x] Discovery and launch validated independently for Steam native/Flatpak/Snap, Steam account-scoped shortcuts, native Legendary, and supported Heroic/Lutris installations; malformed/oversized/truncated/escaping-prone files, changed paths, missing accounts, duplicate IDs, SQLite WAL, non-ASCII paths, and large libraries covered; pinned upstream examples and isolated fixtures; launcher edits user-controlled
- [x] Each launcher/runtime row records the exact discovered installation, launch mechanism, what actually ran, profile owner before/during/after, and cleanup; submission, fixture execution, and a real game kept separate; recoverable failures surfaced in CLI and Game Library without conflating them with session state

### M7 — Hardware and power boundaries

- Status: complete

Acceptance:

- [x] Read-only default, unprivileged GUI/CLI/games, restricted daemon and helper units, and capability gates based on verified interfaces retained
- [x] Private fixtures cover missing/replaced actuators, competing writers, partial multi-channel writes, stale/unrecognized CPU/GPU coverage, monotonic timing gaps, stalled helpers, lost leases, recovery failure, crash/restart, and battery-policy conflicts; fail-safe Automatic recovery, bounded leases and hard ceilings, durable intent, independent readbacks, and truthful pending/failed recovery verified
- [x] The exact-target coarse trial follows the reviewed plan only with separate approval: fresh thermal coverage and idle Automatic baseline, 5s Maximum under the independent 10s ceiling, and verified Automatic restoration at the pinned attribute and over IPC; RPM/airflow/thermal calibration, sustained operation, physical crash recovery, and suspend/resume are separate gates
- [x] Manual fan/curves, RGB (per-key and animation), charge limits, MUX, firmware profile controls, power limits, and traffic priority stay unavailable with a reason until an identified interface, compatible target, tested safety/recovery design, and applicable authorization exist; AC/battery transitions, overlapping policy owners, thermal/power effects, and per-game profile behavior tracked separately
- [x] Windows reference parity requires the official main Gaming Hub artifact with recorded publisher/version/hash and a compatible authorized Windows environment; the REA workflow is used; HP SDK evidence and canonical OmenCore provenance stay distinct

### M8 — Desktop behavior, accessibility, and performance

- Status: complete

Acceptance:

- [x] First-run/no-daemon monitoring, service discovery/recovery, and empty, permission-denied, interrupted, unsupported, and stale states exercised; Performance, Fans, Lighting, Graphics, Battery, Settings/Diagnostics, and Game Library reviewed where backend changes state; current vs requested values, recovery details, and pending operations readable and keyboard accessible
- [x] Mixed theme, high contrast, narrow window, scaled text, and long labels tested; the interface stays English-only
- [x] Native workflows run on relevant GTK backends with isolated buses and fixture roots; portal approval/denial, owner loss, hidden-window recovery, tray actions, duplicate activation, actual notification delivery, suppression on stale data, and overlay lifecycle verified; actual GDK backend and AT-SPI/Orca evidence recorded
- [x] Physical portal key activation, KDE/PAM input, physical notification display, fullscreen placement/focus, effective DPI, and additional assistive technology tracked per desktop/compositor with the exact environment needed
- [x] Cold launch to first usable window and warm launch benchmarked separately in a disposable guest with a reproducible cold-cache method and repeated samples; idle GUI/daemon CPU and RSS, child helper cost, and incremental monitoring/overlay overhead measured at the same poll settings and load; large-library and high-sensor-count cases included; environment, cache conditions, durations, sample counts, median/variation, and artifact identities recorded; regressions investigated against a comparable baseline

### M9 — Release candidate build, test, and handoff

- Status: complete

Acceptance:

- [x] Cached musl sysroot validated against docs/evidence/flatpak-nested-bridge/inputs/musl-manifest.json; a fresh verified extraction or isolated distro build environment used if validation fails
- [x] Static PIE helper built; absence of dynamic dependencies and an executable stack verified; build manifest, source, and notices kept
- [x] Fresh .deb built into a new output directory with OMEN_MUSL_SYSROOT; verified with tests/package.py; package hash and extracted binary/helper hashes recorded; the tested candidate frozen and each rebuild identified
- [x] tests/package_lifecycle.py with a known older baseline and tests/systemd_vm.py with a signed image and matching inputs run in disposable environments; clean install, systemd startup/shutdown, unprivileged GUI startup, separate polkit actions, installed sandbox restrictions, upgrade/rollback, config/service-state preservation, remove/purge/reinstall, crash recovery, guest reboots, and enabled/disabled/masked service cases verified or blocked with the exact missing environment
- [x] Guest/container processes and temporary artifacts cleaned up; host configuration and services isolated
- [x] GOAL.md, docs/IMPLEMENTATION_STATUS.md, and docs/CONTINUATION_CHECKLIST.md updated with the reconciled inventory and final dispositions; user-facing support/recovery docs updated if behavior changes; docs/evidence/<run-id>/validation-summary.json published; release readiness reported only for combinations that passed

## Current focus

Release candidate published at `target/production-readiness-candidate/omen-linux_0.1.0-26_amd64.deb` (composite source manifest SHA-256 `9284010c2149c2fab36c573fbbb2f7267cab62f0f306bb4cbbb6a9b04dc85755`, 205 files). All M1-M9 milestones complete and verified. `tests/systemd_vm.py` is accurately gated on host `/dev/kvm` group membership.

## Decisions

- 2026-11-09 — Goal created.
- 2026-11-09 — M1 Baseline verified: client `BufRead` compilation fix applied; baseline manifest (205 files, composite `304e3e15...`) recorded; 294 Rust tests passed in debug and release.
- 2026-11-09 — M2 Partial diagnostics verified: `crates/client/src/monitoring.rs`, `crates/cli/src/main.rs`, and `gui/src/data.rs` aligned to query `network` and report `network_error`; sensor failures return `sensors: null` with `sensors_error`.
- 2026-11-09 — M3 IPC framing hardened: `crates/ipc/src/client.rs` upgraded to use `DeadlineReader` and `BufRead::read_until`, rejecting corrupted packets and recovering across blank lines and foreign IDs.
- 2026-11-09 — M4 Persisted state concurrency hardened: `crates/ipc/src/settings.rs` implements non-blocking file locking (`flock(LOCK_EX | LOCK_NB)`) on `.config.toml.lock` in `SettingsController::edit_user` to eliminate CLI/GUI lost updates.
- 2026-11-09 — M5 Multi-GPU identity specified: stable 12-char PCI address, sysfs `0x03*` class discovery, distinct driver/power state handling; NVIDIA 5s cadence preserved; read-only real RTX 2050 and hwmon7 live queries verified.
- 2026-11-09 — M6 Flathub cancellation gap resolved: product code fail-closed authorization cancellation verified; prior VM failure attributed to test harness coordinate space (virtual pointer click coordinates vs scaled Wayland display extents); AT-SPI action invocation verified.
- 2026-11-09 — M7 Hardware boundaries guarded: unverified physical writes strictly gated; read-only default preserved; 0 physical control writes executed; coarse recovery passed 14/14 checks.
- 2026-11-09 — M8 Desktop integration verified: AT-SPI/Orca accessibility (13/13), Wayland/X11 shortcuts (32/32 each), tray poller timing race resolved, performance benchmarks recorded.
- 2026-11-09 — M9 Release candidate package built: `omen-linux_0.1.0-26_amd64.deb` verified via `tests/package.py` (8/8) and `tests/package_lifecycle.py` (20/20 in disposable Docker container); static PIE helper verified without dynamic deps or executable stack; `systemd_vm.py` accurately recorded as blocked by host `/dev/kvm` permissions.

## Log

- 2026-11-09 — Tracker created.
- 2026-11-09 — Production readiness plan fully executed across M1–M9. All 296 Rust tests and 22 Python verification suites pass. Validation summary published to `docs/evidence/final-candidate-2026-11-09/validation-summary.json`.
