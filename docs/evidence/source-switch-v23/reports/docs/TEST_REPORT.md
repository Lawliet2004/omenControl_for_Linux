# Validation report

## Revision 23 source recovery

Revision 23 adds verified source reconnection and provenance, immutable action
routes and source-specific editor/history recovery. The source passes 271 Rust
tests per profile, formatting and strict all-target/all-feature Clippy. Six
source workflows and twelve control/keyboard workflows pass per profile; both
native backends pass 31 settings/health/network regressions. Six main-window
source scenes pass with 185 measured active text-label contrasts; six companion
overlay captures were reviewed. The fresh installed VM passes 38 checks,
including five default-source workflows and two reboots. Six real KDE/PAM checks
pass on the identical candidate in an earlier run with a later observer failure;
that entire run is retained as failed. All own VM inputs are removed. Exact
identities and limits are in `SOURCE_SWITCHING_2026-10-28.md` and
`evidence/source-switch-v23/`. Earlier evidence stays attributed
to its tested revision.


Development snapshot 0.1.0. This report replaces inherited claims that were not
reproduced. One approved standard-profile transaction is verified below;
device-specific controls remain unvalidated.

## Revision 22 control-page recovery

The final source passes 264 Rust tests per debug/release profile, formatting and
strict all-target/all-feature Clippy. Each profile passes twelve native control
recovery/keyboard workflows, including valid 0/110 °C curve boundaries. The
preserved revision 21 GUI reproduces the initial capability-recovery failure;
the first revision 22 candidate reproduces the curve-range clamping defect.
The final release GUI passes 31 existing regressions on each X11/Wayland backend
and twelve final layout scenes with 273 measured active text-label contrast
samples. Exact final candidate/package VM attribution, retained earlier attempts,
source identities and limits are recorded in `CONTROL_PAGE_RECOVERY_2026-10-28.md`
and `evidence/control-recovery-v22/validation-summary.json`. The CLI/daemon are
unchanged from revision 21; this does not reattribute its real-game evidence to
the new GUI. No host installation or physical write occurred.
The final candidate passes 39 fresh desktop VM checks, including six actual
KDE/PAM workflows, natural authorization expiry and two reboots, with cleanup.

## Revision 21 actual native game

The unchanged final package passes a separate 39-check fresh VM run, including
six actual native Lutris/OpenTTD checks and two guest reboots. Three real game
launches verify discovery, command-prefix routing, scope/profile lifetime,
fullscreen transition, normal quit, explicit release and service restart with
a private profile provider. OpenTTD/data/OpenGFX total about 33 MB. Software video
is used; GPU acceleration and other stores/games remain unvalidated. The VM exits
zero and removes its private inputs. See `REAL_GAME_VALIDATION_2026-10-28.md` and
`evidence/real-game-v21/validation-summary.json` for exact inputs, retained failed
runs, native screenshot observations, limits and cleanup. No Rust/runtime change
was made for this validation; revision 21's prior Rust/native checks remain
separately attributed below.

## Revision 21 desktop and icon fallback

The candidate passes 39 fresh Ubuntu/Plasma Wayland systemd VM checks, including
six real KDE/PAM authorization workflows and two reboots. It restores fixture
state, exits zero and removes its private inputs. Both native backends pass 31
regressions and nine observed file selections. Four icon/theme scenes pass with
71 mapped icon observations and 142 measured active text-label contrast samples.
The source passes 264 release Rust tests, formatting and strict all-target/
all-feature Clippy. Static package verification includes 15 exact icon sources
and notices. The final documentation package is compared with the tested
candidate for identical runtime/control payload. Exact identities, retained
failures, reproduction and limits are in `DESKTOP_VALIDATION_2026-10-28.md` and
`evidence/desktop-v21/validation-summary.json`. The CLI and daemon are unchanged
from revision 20. Historical statements below about then-open graphical gates
retain their original scope; physical input, full accessibility, real games and
broader desktop/distribution validation remain open. English-only is retained.

## Revision 20 deployment addendum

On 28 October, the unchanged revision 20 package passed 33 headless systemd VM
checks: 17 installed-service/lifecycle, nine independent coarse-recovery, four
isolated NVIDIA-reader and three managed-session sandbox checks. Two real guest
reboots passed. Signed local Ubuntu indexes/archives allowed a genuine offline
installation in a fresh signed cloud image; signature/date checks remained active.
The final VM exited zero and removed its disk, seed and private key. No host
installation or physical write occurred. Exact fingerprints, preparation, initial
bootstrap failures and limits are in `DEPLOYMENT_VALIDATION_2026-10-28.md` and
`evidence/deployment-v20/validation-summary.json`. Earlier evidence and package
bytes remain unchanged; bundled documentation predates this addendum.

## Revision 20 launcher formats and installed package

The final source passes 264 Rust tests per debug/release profile, formatting and
strict all-target/all-feature Clippy. Each profile also passes 17 launcher-format
checks and 20 managed-session integration checks against its actual normal CLI.
The additional launcher coverage includes selected Legendary configuration,
literal option-like IDs, Steam account changes, Snap preparation/failure and
malformed binary shortcuts. Four of the 17 checks use pinned, unmodified upstream
Legendary 0.21.1 to execute synthetic offline games and its configured
actual-command wrapper through production IPC/private PPD and real user scopes.
No outbound socket/DNS attempt occurred. All 55 archive source files match their
pinned hashes. This is not real Epic account/game or Steam/Snap compatibility.
The release CLI also passes 16 manual-launch and seven process-smoke checks.

Six launcher keyboard workflows and thirteen keyboard/Orca workflows pass in
each profile on private authenticated Xwayland with private accessibility buses.
The new workflows cover setup, literal routing/configuration, manual restore,
unsupported sandbox wrapping, GTK folder selection, persistence/rescan and
removal. The selector test observes actual selection before accepting it; native
GTK focus movement alone does not prove selection. The folder test observes the
exact entered path before acceptance rather than inferring it from typed keys.
No host keyboard input or audible speech synthesis is used.

On each X11 and Wayland backend, the final release GUI passes 31 native
settings/health/network regressions, five workflows over a 10,000-entry library,
five managed-session workflows, six launch-issue workflows and 32 shortcut/overlay
checks. All use controlled fixtures; they do not establish physical desktop
shortcut, graphical polkit or full-screen compositor behavior.

Four final release captures cover expanded Legendary/Snap setup and library
locations in wide light and narrow dark views with effective Sans 20. Their
geometry/label measurements report no horizontal clipping, collapsed text or
failing measured contrast across 91 visible text-label samples. Narrow captures
scroll to the relevant setup/location content. A first batched visual inspection
found over-tall Remove buttons beside wrapped paths; one correction centered
those buttons and replaced all-launcher instructions with contextual folder
hints. The four confirmation captures were inspected together once, with no
further UI edits. Non-label inputs, unsupported/partially visible contrast
samples, other states/themes and physical DPI remain outside this result.

The candidate package passes static verification, 20 isolated container lifecycle
checks and 13 installed launcher-format checks as UID 1000. The container reuses
existing Ubuntu 26.04 distribution dependencies, with no archive update/download.
Its separate temporary image was removed and the original container's identity,
PID, start time and running state remain unchanged. This is not a pristine distro
resolution run or systemd PID-1/sandbox test. The final package's runtime payload,
maintainer scripts and control metadata are checked against that exact tested
candidate; only packaged documentation/checksums may differ. No host package,
service or Python dependency was installed and no physical control write was made.

One initial keyboard run observed no text/traversal change after injected keys;
its cause is not established. The private input harness now verifies actual X
input focus and actual typed search text. Earlier selector probes distinguish
focus from selection, and both configuration/path choices are observed before
acceptance. Initial capture staging also needed to wait for the filtered row and
match the actual empty-state label. Those attempts remain separate from passing
final-binary evidence.

Exact source/runtime/package identities, failed harness attempts and current
results are in `evidence/launcher-expansion/validation-summary.json`.
`LAUNCHER_EXPANSION.md` describes routes, version guards and runtime exclusions.
At that milestone, systemd VM verification remained open; the later addendum
above records its 33-check result. Real launcher games/accounts, full accessibility
and graphical/physical release requirements remain open. English-only presentation is retained.

## Revision 19 keyboard and presentation

The final source passes 253 Rust tests in each debug/release profile, formatting
and strict all-target/all-feature Clippy. Thirteen actual keyboard workflows pass
in both debug and release on a new authenticated rootful Xwayland server with
private session/accessibility buses and the installed Orca screen reader. AT-SPI
observes the UI; no action/edit/selection API drives these workflows. They cover
pane/page/search navigation, collapsed disclosure focus, metrics, chart range,
CSV cancel/save, stable selectable history, reversible game setup, report reset,
narrow navigation, action announcements and network copy snapshots. Missing
counters remove selected network rows rather than preserving an outdated view.
Orca's generated utterances are checked; audible synthesis is not exercised.
On each X11/Wayland backend, the final release GUI also passes 31 existing native
regressions, five 10,000-game library workflows, five managed-session workflows,
six launch-recovery workflows and 32 shortcut/overlay checks. Two additional
capture checks per backend cover the managed/recovery states.

The final release presentation matrix passes 36 default scenes: all nine pages
in light, dark, high contrast and effective Sans 20 at a 600-pixel requested
window width. The app reports the actual font and Pango geometry. The matrix
measures 1,041 visible text-label contrast samples, with no detected horizontal
clipping, collapsed text allocation or failing measured contrast. Two additional
normal/large-font selected-network scenes measure the held-state notice and
surrounding text (59 more samples). Native highlighted-reading contrast is
explicitly excluded because the ordinary label foreground does not expose the
selection's foreground. Unsupported and partially visible samples are not
counted as measured passes. This does not establish every state/theme's contrast.

Four final own-app captures were inspected together once: wide dark Dashboard,
narrow large-text Settings and normal/large-text network selection. No further
visual correction was needed. The exact binaries, source/harness manifests and
results are in `evidence/accessibility/validation-summary.json`. The unchanged
release CLI and daemon retain revision 18's exact hashes; its twenty release
managed integration checks and launch/smoke results keep their original
attribution rather than being presented as new runs. The current debug CLI has
a different binary hash, so its twenty managed checks were rerun and passed
against the actual executable. All 65 backend source/manifest files compared
with revision 18 remain unchanged.

An initial concurrent library run reached a 500 ms AT-SPI reply timeout. An
isolated run of that same binary passed, as did the final sequential release
run. Load attribution is an inference, not an established root cause. The failed
record is retained. During tool discovery, invoking the installed accessibility
bus launcher with `--help` unexpectedly aborted. Its 103,860-byte system-managed
dump remains root-owned under `/var/lib/systemd/coredump`; unprivileged removal
was refused. No privilege change or unrelated dump/process modification was made.
The fixture runs clean up their own processes and temporary directories.

Capture harnesses now default to their own evidence directory. Earlier default
paths were shared with historical screenshots; the revision 16 and 18 images
were recovered from this chat's saved image outputs and restored with exact
matches to all four recorded SHA-256 values. Final regression captures use
separate per-backend folders. Additional managed/recovery captures have functional
checks but are not included in the four-image visual review above.

Current revision container/installed VM, real store/runtime, full accessibility,
graphical authorization, physical shortcuts/focus, compositor/full-screen and
hardware-control release gates remain open. No physical write or host package or
service installation was performed.

## Revision 18 desktop launch recovery

The preserved revision 18 source passes 253 Rust tests in each debug/release profile, formatting
and strict all-target/all-feature Clippy. Twenty native integration checks per
profile cover the actual CLI and production service on private PPD and real user
cgroup fixtures, including numeric status, SIGTERM forwarding, inherited SIGCHLD,
pre-registration scope failure and report privacy. No host power bus is used.

The final release GUI passes, on each X11 and Wayland backend, 31 existing native
regressions, five 10,000-entry library workflows, five managed-session workflows
and six new launch-recovery workflows. They exercise delayed authorization denial,
deduplication, clear/reset without changing profiles or preferences, reopening and
damaged-data/live-report recovery. Library rows are repeatedly recycled during a
stopped-daemon outage. One additional X11 capture batch covers wide/light and
narrow/dark own-app layouts, visually inspected together. Existing manual-launch
and daemon/CLI smoke suites pass sixteen and seven checks respectively.

The package is built and statically verified against the release executables.
Revision 18 results and exact hashes are in
`evidence/launch-issues/validation-summary.json`. No revision 18 container/installed
VM, real store-runtime, complete accessibility or physical-hardware release result
is claimed. Earlier evidence below remains attributed to its original binaries.

## Revision 17 library and network UI

The preserved revision 17 source passed 244 Rust tests per debug/release profile, formatting
and strict all-target/all-feature Clippy. The final release GUI passes 31 native
settings/history/game/health/network checks on each of X11 and Wayland. Network
checks now prove native text-object retention during value changes and replacement
when an interface name receives a different kernel index. These checks do not
establish full physical keyboard or screen-reader behavior.

Five large-library workflows per backend cover 10,000 entries, freshness-aware
profile choices without rewriting bindings, actual last-entry fixture execution,
expanded setup across filtering/rescan and removed entries. Managed-session UI
workflows pass on both backends, with wide/light and narrow/dark own-app captures
reviewed once. A controlled 2,000-entry comparison with the preserved revision 16
GUI measured 5.39 s / 679 MiB versus 2.10 s / 311 MiB for scan plus first accessible
response. These are fixture observations on this machine, not general bounds.

Current evidence is in `evidence/library-virtualized/validation-summary.json`.
The revision 17 package is built and statically verified. Container/installed
sandbox VM verification and the remaining real launcher, accessibility and
physical hardware gates remain open. Revision 16/15 records below are historical;
GUI results do not automatically transfer to the new binary.

## Revision 16 managed command sessions

The preserved revision 16 source passed 244 Rust tests in debug and release, formatting and
strict all-target/all-feature Clippy. Fifteen release integration checks run the
actual CLI and production socket/service paths against a private PPD server and
real desktop-user scopes. Twelve release foundation checks cover provider-owner
replacement, failed/unconfirmed replies, detached descendants and scope reuse.
No system power bus or physical controls are used. The final release GUI passes 29 native regression checks on each of X11 and
Wayland, plus five managed-session workflows on each backend. One additional
X11 capture check covers wide/light and narrow/dark views. The sixteen existing
manual-launch fixtures and seven process-smoke checks pass in release.

The revision 16 package is built and statically verified against the release
executables. Container and installed-sandbox VM checks have **not passed**:
dependency downloads stalled before application checks on both attempted mirrors.
Both attempts exited with container/VM and temporary resource cleanup confirmed.
A private-provider installed-sandbox test is prepared, but has no passing result.
The incomplete dependency-cache recovery is not deployment evidence. Current
records are in `evidence/audit-v16/validation-summary.json`; revision 15 results
below retain their historical attribution.

`MANAGED_GAME_SESSIONS.md` documents setup, ownership, journal recovery and
remaining real launcher/Flatpak, accessibility and hardware validation. The older
foundation-only evidence remains historical in `evidence/managed-game-foundations/`.
English is the only application language, as explicitly requested by the user.

## Revision 15 audit implementation

The English-only application audit implementation passes 240 Rust tests in each
debug/release profile, formatting and strict all-target Clippy. The final release
GUI passes 29 native settings/history/game-recovery/health/network checks and 32
shortcut/overlay workflows on each of X11 and Wayland. Four private notification
delivery checks, 16 launcher process fixtures and seven process-smoke checks also
pass. The isolated container lifecycle passes 19 checks; the signed-image
systemd VM passes 30. Both remove their temporary resources. The package identity
and final archive equivalence are recorded in
`evidence/audit-v15/validation-summary.json`.

The full requirement matrix, remaining work, visual review and evidence scope
are in `APPLICATION_AUDIT_IMPLEMENTATION.md`. The user explicitly requested
English only; additional translation catalogs are outside scope. At revision 15,
automatic game-exit restoration, complete desktop accessibility and physical
hardware release gates remained open. Revision 16 progress is described above. No new physical control write or host package/service
installation was performed. Later sections retain historical results and their
original revision attribution; they are not additional revision 15 reruns.

## Historical revision 11 summary

Revision 11 adds the compact status overlay, its desktop actions and fixed-mode
thermal coverage. Its binaries passed 193 Rust tests per build, 32 native
shortcut/overlay workflows for each debug/release × X11/Wayland combination,
16 settings plus four Vitals workflows and six tray checks per build, eleven
thermal scenarios and eight independent-writer scenarios per build. Fan-mode
regressions include seven fixture scenarios and a separate live read-only
comparison. Package and performance results are recorded with exact fingerprints
in `evidence/validation-summary.json`; revision 10's original summary is retained
in `evidence/revision10/validation-summary.json`.

Revision 11 does not repeat real global-key approval/activation, the earlier
approved physical PPD transaction, original Windows reference analysis, the
thermal calibration/provider probe or every nine-page UI workflow. Their older
results retain their original scopes and binary identities.

## Executed checks

* Debug and release Rust workspace suites: **193 tests passed in each profile**,
  zero failed/ignored. Logs are in `evidence/cargo-test-{debug,release}.log`;
  final checks and package hash are in `evidence/validation-summary.json`.
* Strict Clippy (`--workspace --all-targets -- -D warnings`) and formatting check:
  final logs retained with the evidence. No tests are skipped to make a build pass.
* `tests/smoke.py`: real daemon executable, real IPC framing/authorization entry
  point, and real CLI, all on temporary **fixture hardware**. Checks include
  private mock socket, explicit mock identity, protocol rejection, curve→Auto
  ordering, refusal to launch user commands, SIGTERM/socket cleanup, and the profile supervisor’s fixture restoration.
* Production server unit tests cover malformed requests, oversized frames,
  protected socket-path replacement, and fail-closed unidentified peer handling.
  Separate older cross-crate wire tests use a shim; they are not described as
  complete daemon authorization tests.
* Sensor fixtures verify milliamp/Hz/µW scaling, average power, missing files,
  CPU counter deltas/guest exclusion/reset behavior, and duplicate-chip identities.
* Control/config fixtures cover unsupported model rejection, invalid curves,
  migration/future-schema handling, atomic saves/backups, static RGB inputs,
  partial fan-write recovery, and loss-of-temperature recovery. They do not
  emulate physical firmware behavior.

## Revision 11 overlay and fixed-mode coverage

Four new pure overlay tests cover sample aging/invalid values, provider-based
selection, hottest GPU selection and whole-frame aging of fan/profile/battery
rows. Older configuration and scoped-backup tests now round-trip the additive
user-owned overlay preference. The native suite adds eight workflows per build/
backend for real fixture values, source attribution, tray/portal toggles with the
main window hidden, single-instance activation, a stalled poller, native close,
corrupt/unwritable preference handling and startup restoration.

Native testing reproduced a GTK AT-SPI root-enumeration crash when a closed
overlay remained as a hidden toplevel. Closed windows now release their surface
and are recreated on demand; all final native workflows pass on both backends.
A stack from the failed development run is retained in
`evidence/overlay-atspi-crash-stack.log`. The debugging copy of the core was
removed; four system-managed crash dumps from failed test GUIs remain root-owned
under `/var/lib/systemd/coredump`. No unrelated dump or process was changed.

App-only final release captures cover fixture requested 1×/2× and live read-only
requested 1×, with both overlay and Settings windows. The PNGs contain logical
widget renders and do not verify effective physical DPI. An independent review
scored both material error-state fixes resolved (source-neutral recovery copy and
full-value tooltips); its scope is `evidence/overlay-review.json`. Native theme,
metric labels and all nine pages remain intact. See `STATUS_OVERLAY.md` for
compositor, physical key and screen-reader limits. Additional translations are
outside the current English-only scope.

Fixed Manual and Maximum overrides now preflight and renew against the same
CPU/GPU provider, coverage, freshness, cohort and scheduling-gap guard as curves.
Three added Rust cases exercise both modes, and two added process scenarios
verify refusal before ownership, recovery after lost GPU data and explicit
rearming. Automatic needs no thermal input. These ordinary-file checks provide
no model-specific thermal envelope, physical cooling guarantee or separately
supervised production recovery service. Live fan writes remain blocked.

## Fan-mode monitoring regression

Revision 8 adds eight Rust cases and eight production IPC/CLI checks in each
build profile. Mode-only devices now report kernel Auto/Maximum policy without
gaining a write capability. Missing/invalid mode and duty data remain unknown;
an OmenCore process blocks a fixture adapter before mutation.
Write verification also rejects out-of-range raw readbacks on either channel
before normalization. Both real read-only
daemon and direct CLI agree with independent mode-file reads on the reference
8C30/F.10 machine. No real control request or physical fan transition occurred.
See `FAN_MONITORING.md` and `evidence/fan-monitoring-{debug,release}.json`.

## Thermal guard and worker isolation

The revision 9 thermal cases plus three revision 11 cases and eleven current
production IPC scenarios per build cover
recognized CPU/GPU sources, per-device coverage, unavailable/stale/future data,
cohort/power-state changes, scheduling gaps and retry of failed Automatic
recovery. A delayed-configuration test prevents applying work that aged while
waiting for config IO. A real fixture process is paused/continued, and a FIFO
blocks its named power worker while fan ticks continue to follow GPU heat.
Shutdown restores fixture Auto before profile IO drains and rechecks shutdown
before a late automatic profile write. Results: `evidence/thermal-{debug,release}.json`.

A revision 9 real read-only debug CLI probe verifies CPU/AMD PCI associations and
six NVIDIA sample associations against independent sysfs device identities.
`evidence/thermal-live-attribution.json` retains scope and binary identity.
This is association evidence, not calibration or a working physical curve.
See `THERMAL_GUARD.md` for cache/sandbox limits. No live fan control or S3 suspend
test occurred.

## Independent fixture writer

Revision 10 adds nine Rust tests and eight production process checks per build.
An expiring writer lease covers fixed Manual/Maximum and curve writes on pinned
ordinary files. The process suite verifies Automatic while its daemon remains
SIGSTOPped, parent SIGKILL, writer death, explicit rearming and refusal to redirect
an unlinked actuator to a replacement path. It reaps its own orphaned children.
Results and current binary hashes: `evidence/fan-guardian-{debug,release}.json`.

The helper rejects kernel/pseudo-filesystems and is not wired to real hardware.
It shares its parent's systemd cgroup: simultaneous death, whole-service kills,
blocked kernel IO, physical cooling and suspend remain unverified. These tests
do not release live fan controls. See `FAN_GUARDIAN.md`.

## Configuration regression coverage

New tests prove strict backup scopes, field preservation, legacy JSON migration
with nullable fields, unchanged AC policy across TOML/restart, private exports and active-config/symlink protection.
Connected-mode user editing works even with a deliberately unreachable daemon;
service failure never falls back to the user file. Corrupt user data is retained
on failed edits. Chart retention uses its configured limit and ignores non-finite
values. A saved unavailable game profile remains visible and is not replaced by
a fabricated default selection. The service admin-action mapping and unsupported automation rejection
are covered by unit tests.

`tests/settings.py` runs the actual CLI and daemon against isolated fixture
hardware with different user/system directories. It verifies missing service
config does not read the user file, imports affect only the chosen destination,
user backups retain custom games consumed by CLI discovery, service imports
require explicit application, mismatches/unknown fields are refused, and oversized
input/corrupt config do not replace data. Outcomes are in
`evidence/settings-integration.json` and profile-specific run logs. This exercises
the same settings controller used by GTK. No additional live hardware transaction
was performed for this change.

`tests/gui_settings.py` also drives the actual native GTK file chooser and
service-preview buttons through AT-SPI. Sixteen workflows passed in both debug
and release: export/import cancellation; private scoped exports; user import;
wrong-scope, malformed and oversized input; service preview cancellation and
explicit application; capability disable/re-enable after automation changes;
diagnostic export; service loss during preview; disabled profile controls after
disconnect; and user export while disconnected. Files, production IPC readbacks
and mutation logs independently check the results. All control data is fixture
hardware, and all test configuration, recent-file history and session/accessibility
buses are private. Evidence is in `evidence/gui-settings-{debug,release}.json`.

This exposed a stale-capability defect: the server refused manual profile
changes during battery automation, but GTK and the tray retained their initial
enabled state. Revision `0.1.0-4` refreshes capabilities with monitoring snapshots,
updates the explanation and disables/re-enables controls in place. The tray
also refreshes when only permissions/choices change. The original failing test
is retained as `gui-settings-before-capability-refresh.json`.

Current native settings tests explicitly select GTK Wayland and use its real
fallback picker, with no desktop portal. They do not cover KDE portal dialogs, graphical polkit prompts, complete
keyboard/pointer traversal or every widget's accessibility.
App-only screenshots `screenshots/performance-fixture.png` and
`screenshots/performance-automation-fixture.png` show enabled and read-only
states; both explicitly identify mock data. Their binary/image fingerprints are
in `evidence/profile-capability-captures.json`.

## Launch failure and recovery coverage

The shared `omen-client` controller has unit cases for successful submission,
unbound/no-service launch, already-active state, initial-read failure, partial
profile-write failure, readback mismatch, recovery failure and observed external
changes. Launcher plan tests verify numeric Steam IDs, a single URI argument and
no use of stored Steam shell text. Daemon tests verify battery-policy ownership
that profile readback waits behind an in-flight mutation guard, and that fan
recovery precedes waiting for unrelated profile IO. A queued fan mutation also
rechecks shutdown after acquiring its guard.

`tests/game_launch.py` runs real CLI/child processes against a private mock daemon
and fixture manifests/scripts. Sixteen scenarios cover missing executables without
mutation; kernel spawn failure after application; an already-active profile;
nonzero early command exit; a second IPC writer choosing Quiet; successful
fixture URI submission and UID 1000; battery-policy conflict; a killed mock
daemon with unverified recovery; unbound launch with clean JSON stdout; and a
longer-running process receipt. Outcomes are in `game-launch-integration.json`
and debug/release run logs. Profile values are independently checked through
production IPC, but the hardware is entirely fixture data.

The six additional process scenarios cover a missing Flatpak launcher before any
profile write, Flatpak Steam, native/Flatpak Heroic and native/Flatpak Lutris.
Actual child scripts record their exact argument vectors and effective UID.
Discovery fixtures cover multiple Steam roots, symlink deduplication, old/new
library lists, escaped/Unicode names and paths, malformed/incomplete manifests,
Heroic installed-only records, store/installation identity and corrupt files.
Lutris fixtures retain a live writer/WAL and verify installed-only rows are
visible while the database and WAL content remain unchanged by discovery.

No installed store client/game or live profile binding was exercised. Successful
launcher submission does not verify game execution, game lifetime, automatic
exit restoration or GUI-crash rollback. Revision 15 adds daemon-owned atomic
ownership for the short launch-submission transaction, not for the game lifetime.
Its native GUI fixture test also drives Launch, manual profile restore and
preservation of a different externally selected profile. See
`APPLICATION_AUDIT_IMPLEMENTATION.md` for current scope and verification.
No additional live hardware control test occurred in this change.

## Read-only real-machine observation

Network monitoring now has five added Rust cases and nine production IPC/CLI
scenarios per build. Fixtures cover deltas, reset/identity changes, unreadable
input, failure/recovery, CLI bounds, diagnostics and refusal by an older daemon.
An unprivileged real daemon's byte counters were bracketed by independent sysfs
reads for four stable host interfaces. Four additional native Vitals workflows
per build verify visible rates/errors/drops, missing-data reasons and row
replacement. Live/fixture app-only Wayland captures were inspected. No traffic
was generated and no network policy/control write occurred. Evidence and limits
are in `NETWORK_MONITORING.md`, `network-{debug,release}.json`, native settings
records and `network-captures.json`. Suspend-aware gap logic is unit-tested;
physical network churn and suspend/resume remain unverified.

Reference: HP Victus fb1002AX, board 8C30, BIOS F.10, kernel 7.0.0-38-generic.
The live JSON snapshot and interface inventory are retained under `evidence/`.

CPU/GPU temperatures/clocks, AMD usage, NVIDIA CSV values, memory/storage, two
hp-wmi RPM reports, battery/AC and current PPD profile were read. Numeric outputs
are kernel/driver reports; calibrated physical accuracy has not been independently
verified. No fan/RGB/charge value was written and no load/stress/suspend/reboot
experiment was run during this work.

## Read-only GPU sandbox regression

Revision `0.1.0-5` fixes NVIDIA measurements disappearing behind
`PrivateDevices=yes`. An actual transient user service on this Victus reproduces
the missing device nodes and raw daemon samples. With GPU devices visible, a
paired read-only query succeeds. The connected CLI now obtains the GPU name and
five numeric samples locally and records their origin; the GTK dashboard was
captured and inspected with the same service. Device-node capability reporting,
invalid-number handling and explicit PCI selection were also corrected.

`GPU_MONITORING.md` describes the method, fingerprints and limitations. The probe
uses UID 1000 and `PrivateTmp=no` with a private temporary directory; it does not
test a host-installed root unit. No hardware-control write occurred. Fixture
diagnostics continue to contain no live NVIDIA data. This does not establish
physical calibration, thermal stability or general multi-GPU support.

## Approved live profile transaction

The user separately approved Balanced once followed by restoration. On AC, with
no PPD holds or detected competing power manager and fresh battery-automation-off
configuration, a real daemon opted into exactly 8C30/F.10. The UID 1000 supervisor
passed the installed `com.omenlinux.control` polkit action and sent production IPC.
It verified Performance → Balanced → Performance through daemon reads and a
separate PPD D-Bus read. Performance remained active after clean daemon exit.

Evidence: `evidence/live-profile-test.json`, `live-profile-preflight.json`,
`live-profile-run.json` and `live-profile-daemon.log`. This host uses PPD's
`amd_pstate` CPU backend with a placeholder platform driver; this does not prove
HP firmware thermal-mode parity. The test daemon ran unprivileged in an isolated
directory, not as the installed systemd service. Administrator installation of
the reviewed action file succeeded via pkexec. That action file remains installed;
no service or .deb was installed on the host. The action's active-session authorization was
exercised; its inactive/admin prompt and denial paths were not tested live.

## GUI and package

Nine navigation pages were rendered by the actual GTK application. Screenshots
capture only its widget tree, not other windows or desktop contents. Dark-theme
contrast, disabled profile buttons, unavailable control reasons, missing data,
and source labels were inspected. This is scripted visual coverage, **not a
comprehensive GTK interaction/accessibility test suite**. Mock fan UI is labeled
fixture data and proves no hardware support.

`tests/tray.py` exercised actual GTK and D-Bus on this KDE session:
read-only menu gating, Hide, primary Activate, page navigation, clean Quit and
start-minimized restoration. A private bus verifies missing-host fallback and
host loss/re-registration while preserving a reachable window. Profile menu
requests used a separate mock daemon, not live hardware. Logs and outcomes are
in `evidence/tray-*.log` and `tray-integration.json`. This does not prove pointer
placement/keyboard traversal of every desktop shell's menu, GNOME extensions,
compositor focus-stealing policy. The retained runs selected GTK X11 in debug and
Wayland in release. Older reports described the Wayland host session while GTK
inherited `GDK_BACKEND=x11`; those runs did not prove native Wayland behavior.
The retained shortcut tests record the actual GDK display type in every build/backend.
Six checks include removing/restoring profile actions while the selected
profile and window visibility remain unchanged, as automation takes/releases
ownership. Both debug and release passed this regression check.

`tests/shortcuts.py` passed **32 native workflows** for each combination of
debug/release and GTK X11/Wayland. It uses private portal/tray/daemon fixtures,
AT-SPI buttons and the normal single-instance app. It verifies opt-in, activation
filtering, cancellation/error cleanup, version gating, settings persistence,
startup/minimized restoration and portal/bus loss with a reachable window.
App-only enabled/disabled captures were inspected. The real KDE portal separately
passed Register/CreateSession/ListShortcuts/Close with a temporary desktop entry,
which was removed. No host key binding was requested. Real desktop key approval,
physical key activation and compositor focus behavior remain unverified.
See `GLOBAL_SHORTCUTS.md`, `shortcut-portal-host.json`,
`shortcuts-{debug,release}-{x11,wayland}.json` and `shortcut-captures.json`.

Package build and structural checks are recorded under `evidence/`. The .deb is
built for Ubuntu 26.04 amd64. The revision 11 disposable stock Ubuntu container passed **19
lifecycle/authorization checks**: dependency resolution/linkage, private initial
config, offline enablement, enabled/disabled/masked reinstall and real legacy
upgrades, configuration retention, remove/reinstall/purge and production
IPC/polkit denial/grant/revocation/bus-loss behavior. Temporary grant rules apply
only inside that container; no mock authorization bypass was used. The container
was removed and the host package remains uninstalled.

`tests/package_lifecycle.py` and `PACKAGE_VALIDATION.md` describe the method;
`evidence/package-lifecycle.json` retains the image/package fingerprints and
individual results. Final documentation-only repackaging is checked against the
exact tested package: runtime payload, permissions, maintainer scripts and
control metadata must remain identical. This comparison is recorded in
`evidence/package-validation.json`.

The revision 11 signed Ubuntu VM suite passed **17 checks** under real systemd
259.5/kernel 7.0.0-38-generic, including two reboots. It verifies actual namespace
mount flags, capabilities/NoNewPrivileges/seccomp, UID 1000 reads and admin
authorization, stop/start/restart, guest-only SIGKILL recovery, boot enablement,
disabled/masked/enabled reinstalls, removal/reinstall and purge. The VM exited
cleanly and its private disk/key files were removed. No hardware-control request
was made; configuration saves kept automation disabled.

This found a real packaging defect: the previous `ReadOnlyPaths=/sys` did not
produce a read-only sysfs view on this image. Revision `0.1.0-3` uses an explicit
read-only bind and protects kernel tunables; the visible path's `statvfs` flags
and namespace mount data now verify the restriction. The same experiment also
exposed the old `0.1.0-1` upgrade hook disabling an enabled service; that legacy
first upgrade needs an explicit administrator state check and may need manual
re-enabling. Offline tests cannot prove that old running-manager transition.
See `VM_VALIDATION.md`, `evidence/vm-systemd.json` and its retained journals.

Graphical authentication prompts/cancellation/cache expiry, full Kubuntu desktop
installation, Ubuntu 24.04 binaries, Wayland/X11/scaling combinations and upgrade
failure/rollback remain unverified. VM success does not prove physical fan,
profile or firmware recovery.

## Performance

Revision 11 release measurements from `evidence/performance.json`: 12 logical
cores, warm page cache, five-second resource windows, live read-only Wayland
monitoring on a private minimal session bus without a tray host, portal or
accessibility bus. Package/container/VM and native suites finished before this
run. Other host applications remain uncontrolled. CPU percentages are relative
to one logical core; sequential short windows are noisy and do not establish
causal incremental overhead or cold-start performance.

| Measurement | Observed |
|---|---:|
| CLI detection median, five runs | 73.2 ms |
| Daemon socket ready | 10.8 ms |
| Daemon idle RSS / CPU, no clients | 3.54 MiB / 0.00% |
| GUI without overlay: window mapped | 680 ms |
| GUI without overlay: RSS / own CPU / helper CPU | 191.9 MiB / 2.40% / 1.40% |
| GUI with overlay and main window visible: mapped | 368 ms |
| GUI with overlay and main window visible: RSS / own CPU / helper CPU | 198.0 MiB / 2.20% / 1.20% |

Both GUI modes use the same one-second acquisition loop. The overlay creates no
additional hardware poller or helper. These observations measure both visible
windows, not an overlay-only hidden-main configuration. Startup order and warm
caches can explain differences; no faster-start or lower-CPU claim is derived.

Repeated Python PPD polling previously consumed 28.6% helper CPU in the recorded
baseline. Bounded busctl property reads and visible-page updates reduced that in
the measured run. This is not a cold-start benchmark or a universal performance
guarantee. Polling defaults to one second, sensor sweeps cache for 500 ms, and
NVIDIA refreshes at most every five seconds. Physical UI latency, cold starts and
long-duration resource behavior remain unmeasured.

The current direct measurement explicitly uses GTK Wayland and records
`GdkWaylandDisplay`. An earlier revision 7 connected, device-isolated user-daemon window is in
`evidence/gpu-monitoring.json`: GUI 152.2 MiB, 0.40% own CPU and
0.40% helper CPU; daemon 4.41 MiB, 1.60% own CPU and
0.60% helper CPU. This includes local NVIDIA sampling and network polling,
including while Vitals is hidden. These warm five-second windows ran concurrently
with disposable package validation and other checks; they are not isolated
benchmarks or measurements of a host-installed root unit. Global shortcuts
were disabled during measurement.

## Still unverified / release gates

The documentation-only OmenCore review verified the size, Git blob identity and
SHA-256 of 42 selected files against a pinned complete tree, and established
canonical source identity through mutual website/repository links. Results:
`evidence/omencore-review-validation.json`. No OmenCore executable, driver or
upstream test suite ran. This adds source provenance, not hardware validation or
additional application test passes. That source review initially changed only
documentation in revision 7. Revision 8 separately changes fan monitoring and
conflict detection, with new runtime tests described above. The package verifier
compares the final documentation refresh against the exact revision 11 archive
tested in the container and VM.

Other live hardware operations and physical response; inactive/admin polkit
prompts and denial paths; complete authorization lifecycle/PID-exit races; thermal
stability; manual target settling; RGB animation; MUX; charging limits;
physical fan watchdog/SIGKILL recovery; suspend/resume; Windows behavioral parity; original
Store application internals; calibration against independent references; clean
full desktop package lifecycle and upgrade rollback; full keyboard/accessibility coverage.

## Revision 12 verification scope

The current Automatic/Maximum change separates coarse policy from manual duty.
Current debug/release logs and native fan actions are recorded in
`docs/evidence/validation-summary.json`; revision 11 records are archived under
`docs/evidence/revision11/`. Historical overlay/settings/tray workflows retain
their original binary attribution and are not claimed as revision 12 reruns.
Coarse mode fixtures do not validate physical fan response or production recovery.

Revision 12 passed 195 Rust tests in each debug/release build, formatting and
strict Clippy. Each build passed nine guardian checks, eleven thermal checks,
eight fan-monitoring checks (including separate read-only host comparison), four
native Wayland fan actions/state scenarios, and process smoke. The runtime passed
19 container and 17 systemd VM checks; isolated resources were removed. Native
source/capture and documentation audits found no material issue. Full production
fan recovery and physical release remain unfinished; these fixture results are
not a hardware-control completion claim.
