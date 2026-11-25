# Continuation requirements and evidence

## Windows Gaming Hub Parity & Modernization (2026-11-24) — Done, verified release candidate 0.1.0-28

Execution of Windows Gaming Hub parity and modernization roadmap:
- **Top Process Resource Monitoring**: Added `/proc` process reader and stateful delta CPU% `ProcessMonitor` in `crates/hardware/src/processes.rs` (`omen_core::sensors::ProcessSample`) and integrated into native GUI System Vitals (`gui/src/pages.rs`), updating active desktop processes in place sorted by resident memory (excluded in mock/offline modes).
- **In-Game FPS & Frametime Overlay (MangoHud)**: Implemented opt-in MangoHud overlay wrapper in `crates/client/src/game_sessions.rs`, `crates/hardware/src/games.rs`, `crates/client/src/games.rs`, CLI (`omen-cli game-run --mangohud`), and native GUI toggle in Game Library (`gui/src/game_library.rs`) with live PATH detection.
- **Battery Health & Capacity Polish**: Surfaced battery cycle count and full vs design energy capacity (Wh) in native GUI Battery page (`gui/src/control_pages.rs`).
- **Packaging & Release**: Release `.deb` package built at `target/production-readiness-candidate/omen-linux_0.1.0-28_amd64.deb` (6,259,410 bytes, SHA-256 `dbfb8a5b3ec5a5267ba4c59237af8386f64186aa9e6c9730863949705c764a5e`), verified via `tests/package.py` (8/8 checks pass).
- **Workspace Verification**:
  - `cargo fmt --check`: Clean exit 0.
  - `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: Clean exit 0.
  - Rust tests: 308 unit/integration tests passed in both debug and release profiles.
  - Python tests: `tests/smoke.py`, `tests/settings.py`, `tests/network.py`, `tests/fan_monitoring.py --live-readonly`, `tests/game_launch.py`, `tests/live_gpu.py`, `tests/gui_settings.py`, `tests/gui_library.py`, `tests/gui_presentation.py` (vitals/battery/games), and `tests/tray.py` passed with zero errors in release profile.

## Production Readiness Release Candidate (2026-11-09) — Done, verified release candidate

Production-readiness roadmap from `PRODUCTION_READINESS_PROMPT.md` and `GOAL.md` fully executed across the Rust workspace and verification harnesses:
- **Baseline and Candidate Manifests**: Tracked via sorted relative SHA-256 manifests (205 files each). Baseline composite hash: `304e3e1501a68b54c1d296ce8d30c203ec580b0a1ae50f3fa47913d00f32d24a`; final candidate composite hash: `9284010c2149c2fab36c573fbbb2f7267cab62f0f306bb4cbbb6a9b04dc85755`. Zero Git metadata in checkout.
- **Packaging & Static PIE Helper**: Static PIE helper `target/release/omen-flatpak-game` (82,536 bytes, SHA-256 `f11b3d68396a8b30d331cf39499b66a74bb15260e41b7c220287e519f964c853`, non-executable stack, no dynamic dependencies) built via musl sysroot (`target/flatpak-nested-bridge/musl/root`). Release `.deb` package built at `target/production-readiness-candidate/omen-linux_0.1.0-26_amd64.deb` (3,863,434 bytes, SHA-256 `02098b100703b7c758abfd7f66ed986d56aaa84e98e8326388bb4ef94e7af567`), verified with `tests/package.py` (8/8 checks) and `tests/package_lifecycle.py` (20/20 checks in disposable Docker container).
- **Core Code Hardening & Fixes**:
  - `crates/ipc/src/client.rs`: Added `BufRead` trait in scope, hardened client framing with `DeadlineReader` and `BufRead::read_until`, rejecting corrupted packets and recovering from blank lines and foreign IDs.
  - `crates/cli/src/main.rs` & `gui/src/data.rs`: Direct-read diagnostics builders query `b.network()` and report `network` and `network_error`. Sensor failures return `sensors: null` with `sensors_error`.
  - `crates/ipc/src/settings.rs`: Added RAII `flock(LOCK_EX | LOCK_NB)` file locking on `.config.toml.lock` in `SettingsController::edit_user` to eliminate CLI/GUI lost updates.
  - `crates/hardware/src/gpu.rs`: Specified stable 12-char PCI identity, sysfs `0x03*` class discovery, distinct driver/power state handling, runtime suspension protection, and local read-only client NVIDIA supplement.
  - `tests/tray.py`: Replaced immediate label check with `wait_for` to resolve background poller snapshot timing.
- **Verification Summary**:
  - `cargo fmt --check` and `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: Clean exit 0.
  - Rust tests: 296 unit/integration tests passed in both debug and release profiles.
  - Python test suites: 22 test suites passed in both profiles, including smoke, settings, network, fan monitoring (live read-only on real Victus fb1002AX `hwmon7`), fan modes, thermal guard, fan guardian, coarse recovery, game launch/formats/job-control, flatpak setup, GUI settings/source-switch/control-recovery/notifications/sessions/launch-issues/accessibility, shortcuts (Wayland and X11), tray, live GPU (RTX 2050), benchmarks, and package tests.
  - Disposable environment testing: `tests/package_lifecycle.py` passed all 20 checks in disposable Docker container. `tests/systemd_vm.py` is accurately gated on host user `papan` not being in the `kvm` group for `/dev/kvm`.
  - Zero live physical fan, RGB, charge, MUX, or profile writes performed; default daemon remains strictly read-only.
- **Artifacts**: Summary published at `docs/evidence/final-candidate-2026-11-09/validation-summary.json`.

## M2 live-apply (2026-11-09) — done, fixture evidence

Live monitoring-settings apply and diagnostics failure path are implemented and verified
fixture-only in debug and release (287 Rust tests per profile; strict Clippy and
formatting pass; 7-check smoke per profile). Native Wayland GUI workflows pass:
`gui_settings.py` 25 checks per profile (including same-process history-cap shrink and
poll-interval cadence), `gui_source_switch.py` 6 per profile,
`gui_notification_workflow.py` 7 per profile. Run-specific evidence:
`evidence/m2-live-apply-2026-11-09/validation-summary.json` (+ copied GUI JSONs).
Binaries: debug gui `a0616e95…`, release gui `04813e1c…` (full hashes in the summary).
The historical `evidence/validation-summary.json` (0.1.0-14) is preserved unchanged.
No live hardware writes, no package installed. M3-M6 remain open.

Revision 26 follow-through adds private Flatpak helper setup/update/removal,
permission gating, static-helper packaging, installed Heroic sideload records
and further native keyboard, text-scaling, high-contrast and process-lifetime
coverage. Debug/release pass 283 Rust tests each; a fresh fixture VM passes 48
checks and two reboots. Real Flathub game verification is being completed
separately. See `AUDIT_COMPLETION_2026-10-28.md` and `FLATPAK_SETUP.md` for current
scope and the physical/account-dependent requirements that remain open.


The earlier revision 26 development milestone kept Flatpak games in a nested cgroup beneath the
original app scope, preserving its aggregate resource limits. A static sandbox
wrapper uses a private descriptor handoff and executes the original command in
place. Debug/release each pass 278 Rust tests; a 45-check VM suite covers 12
Flatpak workflows, two reboots and cleanup, including exit 7, paired SIGTERM,
ignored SIGCHLD, automatic-off and explicit memory/CPU/task limits. At that milestone no revision
26 package was delivered, and automatic GUI setup remained disabled. The current
follow-through above supersedes that software boundary. See
`FLATPAK_NESTED_BRIDGE_2026-10-28.md` and its separate evidence publication.

Revision 25 fixes misleading managed-session completion after the registered
live process leaves its game scope. It releases its profile hold and records
Interrupted while the command may still be running. Pinned process identity,
normal descendant lifetime and scope-name reuse protections are retained.
Debug/release each pass 276 Rust tests, 21 native session checks and 12 lower-level
scope/provider checks; the candidate passes 40 fresh installed VM checks and two
reboots with cleanup. See `SESSION_SCOPE_MIGRATION_2026-10-28.md` and
`evidence/session-migration-v25/validation-summary.json`. This does not enable
managed Flatpak integration or close physical and full accessibility gates.

A separate unchanged revision 24 VM run now passes 40 checks, including seven
Flatpak boundary observations and two reboots. The observations reproduce why
managed Flatpak remains unavailable: scope migration releases a profile while
the sandbox is alive, and the tested Flatpak 1.16.6 namespace-entry shortcut
loses exit-code/signal semantics. A detached entered child does retain the OMEN
scope/hold until final exit, but that prototype is not supported integration.
See `FLATPAK_SESSION_BOUNDARIES_2026-10-28.md` and
`evidence/flatpak-scope-probe-v24/validation-summary.json`. Product code and the
revision 24 package remain unchanged; physical controls were not exercised.

The unchanged revision 24 application now passes seven native notification
workflows per debug/release profile. Actual private keyboard input edits the
checkbox/fields and saves; real GNotification delivery and action routing pass.
Corrupt-file failure preserves the active monitor, real cooldown/rearm passes,
and disabled settings remain silent past cooldown and after reopening. See
`NOTIFICATION_WORKFLOW_2026-10-28.md` and
`evidence/notification-workflow-v24/validation-summary.json`. Physical desktop
notification display and full accessibility remain unvalidated.


Read-only recheck on 2026-10-26: Victus fb1002AX, board 8C30, BIOS F.10,
kernel 7.0.0-38-generic. Revision 10 archive matches its recorded 2,678,566
bytes and SHA-256 12ee20d102c7c129eb080564805bbaaf150d1e20c8cfb9623503e16bba2b7400.
No applicable AGENTS.md was found. The checkout has no Git metadata.

Revision 24 adds the audit's reported-device/type sensor groups, composable
filters, comparable sorting and stable copy snapshots. It passes nine native
browser workflows per profile, 276 Rust tests per profile, 31 regressions on each
native backend, 13 keyboard/Orca workflows, five final scenes and 38 fresh VM
checks including two reboots. Larger datasets, physical input/DPI, further
expanded states and complete accessibility remain open. See
`SENSOR_BROWSER_2026-10-28.md` and its separate evidence publication.

Revision 23 adds default direct/service source migration, snapshot provenance,
peer-bound actions and source-specific editor/history recovery. Native and Rust
checks pass; its fresh installed VM passes 38 checks including source changes and
two reboots. Six KDE/PAM checks pass separately on the identical candidate. See
`SOURCE_SWITCHING_2026-10-28.md`. This does not close physical, broad launcher/
distribution or complete accessibility requirements.

Revision 22 recovers controls from late capability/readback changes, preserves
edits, refreshes graphics inventory and discloses advanced curve editing. The
complete 0–110 °C curve schema range is preserved. Twelve native recovery/keyboard
workflows pass per build profile; twelve final scenes pass with 273 measured
active text-label contrast samples. Exact final package/VM attribution is in
`CONTROL_PAGE_RECOVERY_2026-10-28.md`. Revision 23 adds automatic direct-to-daemon migration and source/fixture
provenance refresh. Existing physical and full accessibility gates remain.

Revision 21 now has one actual native Lutris/OpenTTD result: six game checks in
a separate 39-check fresh VM suite. Real launch/scope/hold lifetime, normal exit,
explicit release and service restart pass with a private provider; guest game
fullscreen transitions pass with software rendering. The game and free graphics
total about 33 MB. See `REAL_GAME_VALIDATION_2026-10-28.md`. Additional games,
other launchers/accounts, GPU/physical behavior and managed Flatpak remain open.

| Requirement | Existing implementation / fixture evidence | Physical or external evidence / remaining gate |
|---|---|---|
| Fan ownership and recovery | Release Candidate 0.1.0-26: Automatic/Maximum UI actions, exact-target independent systemd writer, durable intent/restart recovery, 10-second Maximum ceiling, isolated NVIDIA temperature reader; 296 Rust tests per profile, 14 coarse recovery scenarios, 9 fan guardian scenarios, 11 thermal guard scenarios, 7 fan monitoring scenarios per profile | Default monitoring stays read-only and all host services are inactive. Host writes, RPM settling, thermal response and suspend/recovery remain unverified; the exact-target first trial is documented in `COARSE_FAN_VALIDATION_PLAN.md`. Target 8C30 exposes no manual PWM percentage/curve control. Live read-only observation verified on real Victus fb1002AX `hwmon7` (automatic policy 2, `pwm1_present: false`, `cpu_rpm: 3593`, `gpu_rpm: 3281`) |
| Games and profile restoration | Release Candidate 0.1.0-26: actual-command wrappers, peer-bound daemon holds through real user-scope lifetime, final-descendant release, overlap/policy handling, interrupted history and separate private desktop launch reports; production-service/private-PPD integration checks; managed session tests (`managed-games-native`, 2/2); 16 game launch checks; 15 game format checks; 9 Flatpak setup checks. Flathub cancellation gap attributed to test harness coordinate space; product code fail-closed authorization cancellation verified | Real store accounts/purchased games remain external dependencies. Pinned offline synthetic games and native Lutris/OpenTTD validated. Launcher submission alone does not establish game lifetime; reports have no profile authority |
| Keyboard and presentation | Release Candidate 0.1.0-26: window-local pane/page/search keys, native control traversal, chart range/export/history, setup/recovery and network-copy workflows; generated Orca speech on private Xwayland; AT-SPI accessibility verified (13/13 keyboard/AT-SPI/Orca checks) | Other screen readers, audible synthesis, expanded/transient presentation states, additional themes/accents and physical focus/DPI remain open. A defined scene matrix is not full accessibility certification |
| Overlay and desktop integration | Release Candidate 0.1.0-26: opt-in compact window, shared snapshots, stale/unavailable/source states, saved preference, native/tray/local/portal actions. 32 workflows in each debug/release × X11/Wayland variant; app-only captures inspected; tray poller timing race resolved with `wait_for` (6/6 checks per profile) | Physical shortcut activation, full-screen/compositor placement/focus, effective physical DPI and full accessibility remain unverified. English-only interface is requested; additional translations are outside scope |
| Other hardware adapters | Fixture RGB/threshold adapters; real monitoring; explicit unsupported states; multi-GPU identity specified (12-char PCI identity, sysfs `0x03*` class discovery, discrete vendor detection, runtime suspension protection) | No verified live RGB, charging threshold, MUX, power-limit or traffic-policy interface on this target; no new writes authorized. Local display telemetry supplement verified on real RTX 2050 (`0000:01:00.0`) |
| Windows reference | Official SDK static analysis and pinned community source provenance | Main Gaming Hub package and compatible Windows behavioral reference missing. No repeated REA setup or engine installation needed for ordinary Rust work |
| Deployment and validation | Release candidate `.deb` package `omen-linux_0.1.0-26_amd64.deb` (3,863,434 bytes, SHA-256 `02098b10...`) verified by `tests/package.py` (8/8) and `tests/package_lifecycle.py` (20/20 in disposable Docker container). Static PIE helper `omen-flatpak-game` (82,536 bytes, SHA-256 `f11b3d68...`) verified without dynamic dependencies or executable stack | `tests/systemd_vm.py` requires `/dev/kvm` access for QEMU (`q35,accel=kvm`); host user `papan` not in `kvm` group (`crw-rw----+ 1 root kvm /dev/kvm`). Unblocking requires `sudo usermod -aG kvm papan` |

This checklist tracks implementation separately from fixture tests, read-only
physical observations and authorized physical controls. The only completed
physical control transaction remains the earlier approved PPD switch/restore.

Revision 12 adds separate Automatic/Maximum capability and native actions with
mode-only fixture supervision. Live mode writes remain blocked. Independent
production recovery, exact-target thermal policy and approved physical validation
remain required; see `FAN_MODES.md`. Revision 11 evidence is preserved rather than
reattributed to the new binaries.

Revision 13 adds independently managed coarse-mode recovery and persistent intent
with an exact-target experimental trial path. Static packaging does not enable it.
The remaining physical gate is now a concrete five-second observation procedure
with an independent ten-second ceiling. Sustained control, physical recovery and
suspend/resume remain unvalidated. Manual/curve production work is separate.

Revision 14 adds a separate unprivileged NVIDIA telemetry reader so the isolated
controller can verify current GPU temperature without weakening its device
sandbox. The native Automatic/Maximum controls passed debug/release fixture
workflows; the package passed lifecycle and systemd VM validation. No physical
fan-control write was made.
