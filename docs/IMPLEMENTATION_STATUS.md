# Goal status and continuation

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


Revision 23 verification note: the initial session commands selected a cached
helper. A supplemental run now passes 20 native checks and 38 fresh installed-VM
checks with the current helper and unchanged final package, including two reboots
and cleanup. See `evidence/source-switch-v23-current-helper/` and the correction in
`SOURCE_SWITCHING_2026-10-28.md`. Published evidence and the final package remain
unchanged.


The active goal is implementation of `APPLICATION_AUDIT_2026-10-27.md`;
the broader product scope remains in `prompt.md`. This is a substantial development milestone,
not completion of its full hardware validation definition of done.

Current source revision: **0.1.0-24** adds a native sensor browser with reported
hardware-device/type groups, filters and ordering, stable raw-identifier row
identity and explicit copy snapshots. The source passes 276 Rust tests per
profile; five final scenes pass with 190 measured active text-label contrasts.
Nine browser workflows pass per profile, including 1,000 readings. Both native
backends pass 31 regressions; 13 keyboard/Orca workflows and 38 fresh installed
VM checks with two reboots pass, with private-resource cleanup. See
`SENSOR_BROWSER_2026-10-28.md`. The full audit remains active.

The preserved source revision: **0.1.0-23** adds automatic monitoring-source recovery,
snapshot provenance and immutable peer-bound actions. Default direct/service
transitions refresh all source displays; explicit fixtures never fall back to
host readings. New identities reset editors/history while same-service outages
retain edits. Debug/release pass 271 Rust tests, six source workflows and twelve
control/keyboard workflows each. Both native backends pass 31 regressions. The
source matrix passes six main-window scenes, with six overlay captures and 185
active text-label contrast measurements. A fresh installed VM passes 38 checks
including five source workflows and two reboots. The identical candidate passes
six KDE/PAM workflows in an earlier run whose later observer failure is retained.
All own VM inputs are removed. See `SOURCE_SWITCHING_2026-10-28.md`. Full accessibility, broader
launcher/distribution coverage and physical release gates remain open.

The preserved revision **0.1.0-22** adds recovery for all five control pages,
background readbacks, retained edits, current/requested values, live graphics
inventory and disclosed curve editing. It preserves valid 0–110 °C curve points
and blocks unsaved curve activation. Debug/release pass 264 Rust tests and twelve
native recovery/keyboard workflows each; both native backends pass 31 regressions.
Twelve final presentation scenes pass with 273 measured active text-label contrast
samples. Its final candidate passes 39 fresh desktop VM checks, six actual KDE/PAM
authorization workflows, natural cache expiry and two reboots, with cleanup.
See `CONTROL_PAGE_RECOVERY_2026-10-28.md` and
`evidence/control-recovery-v22/validation-summary.json` for final package/VM
identity, retained failures and limits. Revision 23 adds direct-to-daemon/source-
provenance recovery; full accessibility and physical release gates remain open.

The preserved **0.1.0-21** embeds native symbolic icon fallbacks while
preserving installed theme priority and fixes relative package output paths.
Its candidate passes 39 fresh Ubuntu/Plasma Wayland systemd VM checks, including
real KDE/PAM authorization, natural cache expiry and two reboots. Both native
backends pass 31 regressions with nine observed file selections. Four icon/theme
scenes pass with 142 measured active text-label contrast samples. The source
passes 264 release Rust tests, formatting and strict Clippy. The final package's
runtime/control payload is compared with the tested candidate. See
`DESKTOP_VALIDATION_2026-10-28.md` and
`evidence/desktop-v21/validation-summary.json`. No host installation or physical
control write occurred. Broader real-game compatibility, full accessibility, broader distributions
and physical release gates remain open. English-only presentation is retained.

The unchanged final revision 21 package additionally passes a separate 39-check
fresh VM run with six actual native Lutris/OpenTTD checks. The game and free
graphics total about 33 MB. Production launch routing reaches the real game;
scope/hold lifetime, fullscreen transition, normal exit, explicit release and
daemon restart pass with a private profile provider. The VM uses software video;
GPU acceleration, other launchers/accounts, managed Flatpak and physical recovery
remain open. See `REAL_GAME_VALIDATION_2026-10-28.md` and
`evidence/real-game-v21/validation-summary.json`. All private VM inputs are removed.

The preserved **0.1.0-20** adds native Legendary with selected
configuration/version checks, Steam Snap and account-scoped binary Steam
shortcuts, desktop-only JSON inventory and disclosure keyboard/name fixes. It
passes 264 Rust tests, 17 format checks and 20 managed integration checks per
profile. Pinned upstream Legendary executes synthetic offline games and its
actual-command wrapper; real Epic/Steam/Snap compatibility is not established.
Six new native launcher and thirteen keyboard/Orca workflows pass per profile.
The package passes 20 isolated lifecycle and 13 installed format checks using
reused distribution dependencies, with no host installation. Temporary container
and image resources were removed. The unchanged package also passes 33 headless
systemd VM checks, including managed-session sandbox behavior and two reboots;
see `DEPLOYMENT_VALIDATION_2026-10-28.md`. Graphical/physical and full accessibility
gates remain open. Current identities and results are in
`evidence/launcher-expansion/validation-summary.json`; supported formats and
limitations are in `LAUNCHER_EXPANSION.md`. The application stays English only.

The preserved revision **0.1.0-19** adds pane/page/search keyboard navigation,
native focus fixes, measured adaptive action rows, Pango chart labels, accessible
action feedback and explicit stable network-copy snapshots. It passes 253 Rust
tests per profile and thirteen release keyboard/Orca workflows. Thirty-six
default presentation scenes cover nine pages in light, dark, high contrast and
effective Sans 20 text at a narrow width; two more cover selected network readings.
Current results, exact identities and limits are in
`evidence/accessibility/validation-summary.json`. Full state/theme contrast,
other assistive technology, real launcher execution, deployment and physical
release requirements remain open. See `ACCESSIBILITY.md` for behavior and scope.

The preserved **0.1.0-18** adds persistent desktop launch-failure
reports in Game Library and the CLI, native recovery/reset, live fallback and an
outside-scope observer that preserves command status and ordinary termination
signals. Reports do not establish game lifetime or profile authority. It passes
253 Rust tests per profile, twenty managed integration checks per profile and
native workflows on X11 and Wayland. The package is statically verified; installed
container/VM, real launcher/runtime, complete accessibility and physical release
requirements remain open. Its evidence is
`evidence/launch-issues/validation-summary.json`.

The preserved **0.1.0-17** adds native library virtualization, retained setup/rescan
state, fresh profile choices and stable network row identities. Its evidence stays
in `evidence/library-virtualized/validation-summary.json`.

The preserved revision **0.1.0-16** adds managed command sessions to the audit
implementation. Requirements and remaining work are tracked in
`APPLICATION_AUDIT_IMPLEMENTATION.md`; `MANAGED_GAME_SESSIONS.md` describes
actual-command wrappers, daemon ownership, per-game setup and restart recovery.
The broader audit and original hardware validation definition remain incomplete.
Revision 16 passes 244 Rust tests per profile, fifteen managed integration checks
and twelve foundation checks per profile, and release native GTK workflows on
X11 and Wayland. The package is built and statically verified. Its container/VM
checks remain unverified because dependency downloads stalled; stopped attempts
were cleaned up. See `evidence/audit-v16/validation-summary.json`. The preserved revision 15 package and
`evidence/audit-v15/validation-summary.json` retain their original attribution;
they are not evidence for later source changes.
New code and fixture checks do not satisfy the outstanding physical release
gates. The earlier packaged hardware-safety revision **0.1.0-14** adds independently managed,
exact-target Automatic/Maximum recovery, persistent intent and a bounded trial
path. Default monitoring remains read-only. Revision 14 results and exact package
identity are in `evidence/validation-summary.json`; earlier revisions keep their
own evidence under `evidence/revision13/`, `evidence/revision12/` and
`evidence/revision11/`.
The requirements/evidence checklist is `CONTINUATION_CHECKLIST.md`.
No physical fan-control write or host package/service installation was performed.
The exact-target trial remains pending under `COARSE_FAN_VALIDATION_PLAN.md`.

Completed this run:

* Audited existing source and removed privileged game execution, unsafe generic
  hwmon recovery writes, signal-handler locking and curve-worker races.
* Installed project REA skill; user explicitly approved Codex registration repair
  and matching global skill. Existing Node runtime used; no engines installed.
  Verified a real standalone MCP connection and performed SDK static analysis.
* Downloaded official HP SDK, verified published checksum, recorded SHA-256,
  inventoried native/managed components and service/driver relationships.
* Pinned/reviewed Linux hp-wmi, OMEN Space, OpenOMEN, OmenMon, OmenCore and RGB
  module source, licenses and dated issue snapshots. OmenCore's canonical source
  is established as theantipopau/omencore; see `OMENCORE_REVIEW.md`.
* Implemented live read-only policy, explicit exact-target profile opt-in, safe
  fixture control loops, bounded/authorized IPC, config validation/atomic saves,
  real CPU/memory/storage/GPU/battery monitoring and source-aware UI states.
* Measured and removed repeated Python PPD polling overhead; hidden GTK pages
  stop rebuilding. Benchmarks retain before/after methodology and results.
* With separate user approval, installed the reviewed polkit actions and verified
  a real 8C30/F.10 PPD Performance → Balanced → Performance transaction through
  production IPC as UID 1000. Both independent readbacks and restoration passed.
* Added native StatusNotifier tray integration, Show/Hide/Quit and page actions,
  capability-gated profile actions, startup preference and no-host/host-loss
  fallbacks. Real KDE and private-bus GTK integration tests passed; profile
  mutations were tested only against fixture hardware. App activation reuses
  the existing window. Package builds now preserve resolved dependency notices.
* Built/debugged native GUI and .deb; refreshed reports/screenshots. Validation
  summary and exact final package hash are in `evidence/validation-summary.json`.

Next actions:

1. The first profile transaction is complete; retain the default read-only daemon.
   Physical-host systemd/profile integration, graphical authentication, battery
   transitions and thermal effects still need validation. Any broader first hardware tests need
   their own concrete plan and approval.
2. Direct REA MCP is now connected and verified in this chat. Retain the approved
   configuration; optional native engines are still unavailable. No setup repeat
   is needed for connection.
3. Main Gaming Hub Store package and Windows reference behavior remain unavailable;
   use a legally supplied copy/system before claiming original-app parity. Native
   decompilation engines require separate approval if actually needed.
4. Fan release remains blocked by physical target settling, validated CPU/GPU
   providers/thermal policy, suspend/resume and targeted watchdog/SIGKILL recovery. Keep
   live writes disabled; mocks and kernel source do not satisfy those checks.
5. Remaining optional/product work includes physical global-key validation, compositor/full-screen overlay validation, additional
   launcher formats, per-key lighting, MUX support, traffic controls, comprehensive GTK
   interaction/accessibility and full desktop .deb lifecycle tests. Additional
   translations are outside scope: the user requested English only.

Do not mark the goal complete solely because a UI, fixture suite or .deb builds.

OmenCore research now distinguishes development commit 840f03d58ddd from the
v4.5.0 release assets, records 42 verified source-file hashes, and documents the
unverified 8C30 keyboard mapping. Its coarse fan-mode path, private writer lock
and normal-exit restore do not satisfy our physical settling or forced-kill
recovery gates. Live fan writes remain disabled. Revision 8 now includes its CLI in the
conservative process-name inventory; this does not establish exclusive ownership.
The only live control writes in this run were the approved Balanced request and
restoration to Performance. No fan/RGB/charging/graphics/firmware writes or engine
installs were performed. The test daemon exited and removed its socket; the
reviewed polkit action file remains installed. The systemd service is not installed on the host.

Configuration ownership audit completed: game bindings were already user-local.
Backup import/export had selected the daemon’s full config whenever connected;
this is replaced with explicit User and Service scopes. The daemon no longer
falls back to the user path. Typed service policy IPC uses admin authorization,
validates automation targets and preserves desktop fields. Native file pickers,
a service-policy preview, private atomic JSON exports and bounded input are
implemented. The CLI shares the same controller and process tests exercise both
files. Legacy JSON import/nullable fields and configured chart retention were
also repaired. Sixteen native GTK/AT-SPI settings interactions now pass in both
build profiles, including real file selection/cancellation, service preview and
disconnect behavior. The tests use fixture hardware and the GTK fallback picker;
KDE portal dialogs and graphical authorization remain unverified.

Game-launch failure recovery is implemented in the shared unprivileged
`omen-client` controller. Launcher preparation precedes mutation; a 300 ms
startup check catches immediate errors. Original/target profiles and recovery
outcomes are explicit, including preservation of observed external changes and
unverified recovery when the endpoint disappears. Both GUI and new CLI commands
use this path. The daemon serializes manual/automatic writes and profile reads,
blocks manual writes while battery automation owns selection, and advertises
that policy as read-only. Fixture process tests validate submission/rollback and
UID/argument boundaries; real Steam/game execution
remain unverified/unimplemented respectively.

Installed-library discovery now covers native/Flatpak Steam, Heroic Epic/GOG/
Amazon and Lutris, using pinned primary source formats. Steam unions roots and
old/new library lists, correctly parses hierarchy/quoted escapes and checks
manifest IDs. Lutris opens SQLite read-only, preserving committed WAL visibility.
Installation-specific IDs route fixed argument vectors to native or Flatpak
launchers; store-provided shell text is ignored. Rust and process fixtures cover
these routes, not actual games. GOG titles currently fall back to product IDs.
See `GAME_LAUNCHERS.md` for remaining exclusions and source fingerprints.

Package lifecycle validation now passes 19 checks in an isolated stock Ubuntu
26.04 container. Offline dpkg upgrades from 0.1.0-1 preserve enabled, disabled
and masked links. Reinstall, config retention and purge pass. Debian
helpers replace direct enable/disable calls and respect policy-rc.d. Production
IPC uses a private real D-Bus/polkit instance to verify denial, separate admin/
control grants, revocation and bus loss, with no mock bypass or hardware-control
requests. The container was removed; the host package remains uninstalled.
See `PACKAGE_VALIDATION.md` and `evidence/package-lifecycle.json`.

The packaged service also passes 17 checks in a signed Ubuntu VM with systemd
PID 1, actual mount restrictions/capabilities, production authorization, start/
stop/restart and crash recovery, two reboots, and remove/reinstall/purge. The
test exposed writable sysfs under the old unit. Revision 0.1.0-3 binds sysfs
read-only and protects kernel tunables; the actual visible path now verifies
read-only. The VM and private key/disk were removed, and no hardware-control
request occurred. A separate real-manager legacy experiment found that the old
0.1.0-1 prerm disables an enabled unit before replacement; its first upgrade may
require manual re-enabling. The new installer reports that exception.
See `VM_VALIDATION.md` and `evidence/vm-systemd.json`.

Native tests found and fixed stale profile capability state in GTK and the tray.
Revision 0.1.0-4 refreshes availability while monitoring, reflects automation
ownership/disconnects, and restores controls when ownership is released. Six
tray checks pass in debug and release, with no additional live hardware writes.

Revision 0.1.0-5 fixes a real GPU sandbox regression. A transient user service on
this machine reproduces missing NVIDIA nodes/samples under PrivateDevices; the
connected CLI and GUI now use a cached read-only local NVIDIA supplement. Raw
daemon and fixture reports retain their own origins. Seven new Rust cases and a
paired real-device probe cover the change. See GPU_MONITORING.md for the UID 1000
probe's differences from an installed root unit and its remaining limits.

Next practical work: graphical polkit prompts/session classification. Full
physical deployment of the root systemd unit remains unverified.
Actual game lifecycle tracking, production hardware recovery and broader physical
hardware validation are still outstanding. Do not install the package on this
host or perform new hardware writes merely to satisfy those checks.

Revision 0.1.0-6 adds an opt-in global Show action through XDG Desktop Portal,
user preference restoration, native parent handles and cleanup/recovery after
cancel or service loss. Twenty-four workflows pass for debug/release × GTK
X11/Wayland, with the actual backend recorded. The real KDE portal passed a
session/list/close probe with a temporary launcher, removed afterward; host key
assignment/physical activation remain unverified. Earlier Wayland wording had
used the host session type despite inherited GTK X11; current native settings,
tray and shortcut runs state their explicit backends. See GLOBAL_SHORTCUTS.md.
REA's direct chat `binary_session` now succeeds, with complete identity and
provider limitations retained in `evidence/rea-chat-session.json`.

Revision 0.1.0-7 adds real read-only per-interface network monitoring to Vitals,
CLI and diagnostics. Five added Rust cases, nine IPC/CLI scenarios per build and
four native Vitals workflows cover rates, reset/identity changes, failure/recovery
and older-daemon rejection. Live counters were bracketed by independent sysfs
reads; no traffic or network policy change was generated. Virtual interfaces are
not summed into misleading totals. See NETWORK_MONITORING.md for namespace,
suspend-aware timing, bounds and remaining physical validation limits.

Revision 0.1.0-8 separates kernel-reported fan mode from manual-control support,
preserves unknown/invalid duty readings and includes OmenCore plus Linux name
truncation in competing-process detection. Eight Rust cases and production
IPC/CLI regressions cover those paths; independent read-only host mode checks
do not authorize or establish physical fan response. See `FAN_MONITORING.md`.

Revision 0.1.0-9 adds the fixture thermal guard, optional provider-associated PCI
identity, explicit incomplete GPU inventories and unavailable thermal channels.
Sixteen new Rust cases and nine production process scenarios per build cover
CPU/GPU coverage, timing/cohort changes, recovery retry and independent workers.
The blocked-profile FIFO test verifies that fan ticks continue and shutdown
restores fixture Auto before profile IO drains. This does not establish physical
fan safety, S3 recovery, a functioning heartbeat supervisor or adequate NVIDIA
control telemetry under PrivateDevices. See `THERMAL_GUARD.md`.

Revision 0.1.0-10 routes every mock-daemon fan write through a separate process
with a four-second expiring lease and pinned ordinary-file descriptors. It
restores fixture Automatic while its parent remains stopped or after parent
SIGKILL. Expired ownership rejects late overrides and requires explicit rearming;
heartbeats revalidate the controller. Nine added Rust cases and eight production
process checks per build cover protocol, identity and recovery failures.
The helper rejects sysfs/pseudo-filesystems and shares the daemon's service
cgroup. It is not the separately supervised physical recovery implementation
required for live release. The real backend and read-only sysfs sandbox remain
blocked for fan writes. See `FAN_GUARDIAN.md`.

Revision 0.1.0-11 implements the optional compact GTK status overlay, fed by the
existing bounded snapshot channel. It shows real available metrics with
per-sample and whole-frame stale states, fixture/source attribution and profile
availability. User-owned startup preference, native close/open actions, tray,
Ctrl+O/Escape and a second opt-in portal action are integrated. Closed windows
are destroyed/recreated to avoid a reproduced GTK AT-SPI root enumeration crash.
The window follows native scaling and compositor placement; always-on-top,
full-screen game visibility and physical shortcut activation are not claimed.
See `STATUS_OVERLAY.md` for behavior, tests and limits.

Fixed Manual and Maximum overrides now require the same provider identity,
CPU/GPU coverage, cohort/freshness and scheduling-gap checks as fixture curves,
both at preflight and before renewing their guardian lease. Missing data stops
an owned override and retries Automatic; returning data cannot rearm it.
Automatic remains available without thermal data. This closes the fixed-mode
coverage bypass, not the missing model-specific thermal policy, independent
production cgroup recovery or physical fan safety gates. Game lifetime/profile
ownership software remains outstanding and is not labeled externally blocked.

## Revision 12: coarse firmware modes

Automatic/Maximum actions and capability reporting are now independent of manual
PWM targets. Mode-only fixtures use pinned ordinary files with the existing
expiring guardian, thermal preflight and recovery. Native controls refresh their
availability after capability changes. See [FAN_MODES.md](FAN_MODES.md) for the
implementation boundary and reproducible run commands. Live writes remain
blocked; independent production recovery and hardware validation remain open.
Revision 11 evidence is preserved under `docs/evidence/revision11/`.

## Revision 13: independently managed coarse recovery

Implemented a separate static writer unit, root-private authenticated transport,
pinned exact-target mode actuator, durable ownership intent and restart recovery.
Manual percentages/curves stay disabled in this transport. Default monitoring
stays read-only; an explicit experimental target flag exposes a ten-second
Maximum trial. The trial script is read-only by default. Current fixture/VM
evidence is in the validation summary; revision 12 is archived. First physical
execution still requires the specific approval in COARSE_FAN_VALIDATION_PLAN.md.

## Revision 14: isolated NVIDIA control telemetry

Added a socket-activated, capability-free DynamicUser reader with two explicit
NVIDIA device-node permissions and a fixed read-only command. The controller's
PrivateDevices sandbox remains intact. Only its explicit experimental path reads
the root-private PID-1 endpoint; fixture tags, PCI association, source and original
timestamps are validated. User GUI caches are not trusted. Missing/stale/future
reader data remains fail-closed. Debug and release suites each pass 205 Rust tests;
the final v14 package passes 19 isolated container checks and 30 real-systemd VM
checks. Exact hashes and detailed evidence are in `evidence/validation-summary.json`.
Physical provider and actuator checks remain unverified.
