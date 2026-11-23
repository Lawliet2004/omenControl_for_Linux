# Application audit implementation

This tracks the full scope of `APPLICATION_AUDIT_2026-10-27.md`. The original
audit remains a baseline; implemented code and passing fixtures do not close
physical validation requirements. Revision 22 is a development revision.

Revision 22 makes Performance, Fans, Lighting, Graphics and Battery follow fresh
capabilities/readbacks from their selected source. Readbacks run off GTK's main
thread. Editors survive outages, invalid/missing readings disable Apply, graphics
inventory refreshes, advanced curves are disclosed, and the full 0–110 °C curve
schema range is preserved. Twelve recovery/keyboard workflows pass per build
profile, alongside 264 Rust tests per profile and 31 native regressions on each
backend. Twelve final scenes pass with 273 measured active text-label contrast
samples. See `CONTROL_PAGE_RECOVERY_2026-10-28.md` and
`evidence/control-recovery-v22/validation-summary.json`. Direct-to-daemon source
migration/provenance refresh, complete accessibility and physical gates remain.
The final candidate also passes 39 fresh desktop VM checks, including real
KDE/PAM authorization and two reboots, and removes its private VM inputs.

Revision 21 embeds 15 GNOME symbolic fallback SVGs for incomplete desktop icon
themes. Installed themes retain priority. Asset sources, exact hashes and notices
are included in the package. The candidate passes 39 fresh systemd VM checks,
including real KDE/PAM authorization and two reboots, plus 31 native regressions
on each X11/Wayland backend. Four icon/theme scenes pass with 142 measured active
text-label contrast samples. See `DESKTOP_VALIDATION_2026-10-28.md` and
`evidence/desktop-v21/validation-summary.json`. Historical revision 20
package/evidence remain unchanged; physical and broader release gates remain.

The unchanged final revision 21 package also passes a separate 39-check VM run
with six actual native Lutris/OpenTTD checks. Three launches verify command-prefix
routing, real game scope/hold lifetime, normal exit, explicit release and service
restart; guest fullscreen transitions pass with software rendering. The game and
free graphics total about 33 MB. See `REAL_GAME_VALIDATION_2026-10-28.md` and
`evidence/real-game-v21/validation-summary.json`. Other launchers, store accounts,
GPU/physical behavior and managed Flatpak remain open.

Revision 16 connects managed command sessions to Launch, the CLI and per-game
setup. The daemon owns verified PPD holds while a pinned user cgroup remains
populated. Private restart history preserves no write authority. This covers
actual wrapped process trees; real store/runtime and managed Flatpak execution,
complete accessibility and physical release gates remain open. See
`MANAGED_GAME_SESSIONS.md`. Revision 15's package and evidence are preserved.

Revision 17 adds a virtualized native library, retained setup state and unchanged
rescan identities. Profile choices now follow fresh/stale capabilities without
rewriting bindings. Network value updates retain their native text objects;
new interface identities replace old rows. Full keyboard/screen-reader and
physical release requirements remain open.

Revision 18 adds persistent desktop launch issues, including worker failures
before daemon registration and late process exits. Game Library loads reports
after reopening, retains live feedback if saving fails and offers explicit reset;
the CLI can inspect and clear saved reports independently of hardware discovery.
Reports have no profile authority and contain no command arguments or captured
output. The outside-scope observer preserves exit status and ordinary termination
signals. Native row callback cleanup also runs during prolonged daemon outages.

Revision 19 adds window-local pane, page and search keys, fixes hidden-content Tab
traps, and keeps native control traversal when action rows reflow. Chart labels
use the configured Pango font; action feedback uses the stable accessible window
context when the runtime supports announcements. Network copy selections hold an
explicitly labeled snapshot until selection/focus ends, while missing data still
removes the row. English-only presentation remains the requested scope. See
`ACCESSIBILITY.md` for input behavior, runtime support and measurement limits.

Revision 20 adds native Legendary with configuration-bound routing, explicit Steam
Snap routes and account-scoped binary Steam shortcuts. It checks Legendary's
configuration support and Steam's recent-account selection before profile
mutation. The inventory has a desktop-only JSON route. Native disclosure keyboard
activation and concise accessible names are fixed. Pinned upstream Legendary
executes synthetic offline games and its actual-command wrapper in private
fixtures; this is not real Epic/Steam/Snap game compatibility. See
`LAUNCHER_EXPANSION.md` for supported formats and runtime limits.

## Confirmed findings

| Audit item | Current implementation | Evidence / outstanding requirement |
| --- | --- | --- |
| Timestamped history | Shared acquisition-time axis, unavailable/stale gaps, no invented cached acquisitions, source transitions, bounded retention | Chart tests cover alignment, cached data, outages and source changes. CPU/GPU selection rejects stale/future sensors. |
| Stale main-window values | Independent monotonic expiry, rejection of delayed snapshots, battery/fan clearing, capability invalidation and recovery; hidden pages replay current state | GUI freshness tests and native X11 stalled-daemon workflow. Expiry adds no history samples. |
| Game/profile lifecycle | Manual submission transactions plus opt-in actual-command wrappers; peer-bound daemon PPD ownership, final-descendant release, overlap/policy handling and private interrupted recovery; separate persistent desktop failure reports | Production IPC/private-PPD tests with real user scopes cover lifetime and crash behavior. Late/pre-registration recovery feedback is exercised in native GTK. One actual native Lutris/OpenTTD path passes with a private provider; other store/runtime and managed Flatpak execution remain unvalidated. A launcher receipt is insufficient lifetime evidence. Manual submission mode still cannot identify external same-value/ABA writes. |
| Local service availability | Four sessions per UID, sixteen globally, idle displacement, absolute input deadline and request bound; active dispatch protected | Real server/client saturation and trickled-input tests. Same-UID active saturation can temporarily reject further requests; dispatched operations retain existing timeouts. |
| Accessible chart data | Selectable native summary, keyboard-readable history, solid/dashed lines, private CSV, named groups and font-scaled Pango axes | Actual keyboard range/export, stable history selection and Orca-generated narration pass on private Xwayland. Other screen readers, audible synthesis, physical focus and expanded presentation states remain open. |
| Adaptive layout | Folding sidebar, wrapped labels, scrollable navigation, reflowing cards and measured action rows; wide forms scroll | Effective large-font and default-scene coverage is measured separately below. Expanded/transient states and effective physical DPI remain open. Additional languages and translated layouts are outside scope per the user's instruction. |
| Navigation hierarchy | Monitor/Controls/Library/Application groups, icons/text, current-page title, actionable health indicator, secondary actions popover | Native X11 menu/welcome/diagnostics workflow. Ordinary GTK buttons replace a tested MenuButton trigger that exposed no AT-SPI action. |
| Widget/sensor context | Dashboard profile/source/update summary, selected metrics, age/source per card, sensor search/sorting, raw IDs and stale labels | Freshness/filter tests and actual keyboard sensor search pass. Network rows retain native identity; selecting a reading holds a named copy snapshot. Keyboard selection/resume and missing-data removal pass on private Xwayland. Interface identity changes still replace rows. Physical focus behavior remains open. |
| Appearance | System appearance, semantic GTK surfaces/errors, theme-derived Pango chart labels and stronger primary-action text contrast | Defined light/dark/high-contrast/large-font scenes are measured below. Complete state/theme contrast and physical display validation remain open. |
| Library refresh/locations | Virtualized native rows, full-library name/ID search, retained expanded setup, incremental rescan, progress/errors and retained results on failure; extra Steam/Heroic/Lutris/Legendary locations and explicit Steam Snap routes | Parsing, routing, user-scope/backup and 10,000-entry native UI checks. Legendary retains its selected configuration; other launchers use their active configuration. Binary Steam shortcuts require a recent account and valid stored app ID. Real store games and further formats remain open. |
| Desktop/accessibility/localization | Expanded native fixture workflows; application messages remain English-only as requested | Real KDE/PAM cancellation, rejection, approval, retained authorization and natural expiry pass in a fresh Ubuntu/Plasma Wayland VM. English presentation and numeric-format tests pass. Additional translations are outside scope. Physical desktop input, broader assistive technology and full-screen validation remain open. |

## Design deliverables and additions

| Deliverable | State |
| --- | --- |
| Adaptive shell/top bar | Implemented. Menu offers diagnostics, welcome, overlay, tray and quit. Action feedback remains visible with folded navigation. |
| Grouped sidebar | Implemented with icons, labels and scrolling. |
| Contextual dashboard | User-scoped metric selection, source/age, profile and unavailable/stale states implemented. |
| Chart ranges/export | Implemented. CSV is private, atomic and refuses existing destinations, symlinks and active configurations. |
| Sensor search/grouping | Implemented with names, IDs, providers and devices. |
| Control-page clarity | Current state precedes actions and fresh unavailable reasons. Control readbacks run off GTK; late capabilities recover editors, stale/failed values block Apply, unsaved edits survive recovery, and advanced curves are disclosed. The complete curve schema range is preserved. Source migration and further presentation/accessibility states remain separate. |
| Health/capability center | Implemented in Settings with model, board, BIOS, source, age and status/reasons. Diagnostics shows the exact JSON before saving; native tests verify it matches the reviewed report. Availability does not imply physical validation. |
| Monitoring notifications | Off by default; configurable threshold/hysteresis/cooldown, fresh data only, no hardware control. Actual GNotification delivery, fixture attribution and suppression passed against a private notification server. Physical desktop display and direct checkbox interaction remain unverified. |
| Onboarding/help | Integrated skippable Dashboard welcome, diagnostics route, persistent dismissal, error/retry and reopening from the menu. Existing configurations do not unexpectedly show it. |
| Compatibility evidence | Exact model/board/BIOS/kernel historical evidence is shown separately from current capability. Fixtures and unknown sources receive no physical attribution; updated firmware/kernel combinations do not inherit results. The table currently contains the one documented reference target, so wider model coverage remains open. |
| Managed game sessions | Automatic Performance/Quiet holds for actual-command wrappers, final-descendant release, bounded daemon sessions and private restart history are implemented. Native setup, requested/effective profile, release and stable session rows are integrated. Native store wrappers require manual setup; real store execution and managed Flatpak remain open. |
| Broader integrations | Native Legendary, Steam Snap auto-detection and modern binary Steam shortcuts are implemented. Pinned upstream Legendary executes synthetic offline games; real store accounts/games, managed sandbox execution and further third-party formats remain open. |
| Language | English only, per the user’s instruction. French catalogs and the gettext build dependency have been removed. Native system dialogs follow desktop settings. |

## Remaining software and experience work

1. Native Lutris/OpenTTD now passes real-game launch/exit/release/restart checks in
   a private desktop VM. Verify additional Lutris games, native Steam/Proton and
   Heroic, then add managed Flatpak support only with established environment and
   lifetime behavior. Persistent pre-registration/late failure reports are now
   implemented and verified with private fixtures; actual store/runtime delivery
   and the observer's complete job-control compatibility remain unvalidated.
2. Extend keyboard/screen-reader and presentation coverage beyond the verified
   navigation, chart range/export, setup, recovery and network-copy workflows.
   Expanded/transient states, additional themes/accents, other screen readers,
   audible synthesis and physical focus/DPI remain open; the defined scene matrix
   does not establish complete contrast or accessibility conformance.
3. Complete physical focus/selection testing for the retained network rows.
   Library virtualization and 10,000-entry interaction checks are implemented;
   broader hardware/library-size performance coverage remains useful. Refine
   further control-page states beyond revision 22's verified disclosure,
   current/requested values and effective large-text scenes.
4. Verify real launcher/account/game behavior for the added Steam shortcuts,
   Snap and Legendary formats. Heroic sideload/Zoom/third-party formats,
   sandboxed Legendary and AppImages without a matching PATH runtime remain
   outside automatic routing. Preserve pinned formats and literal arguments.
5. Complete physical desktop authorization/shortcut/full-screen overlay trials
   and declared distribution compatibility. Retain the separate physical gates
   below for hardware controls and suspend/resume.
6. Add verified direct-to-daemon migration when the service starts after the app,
   and refresh source/fixture provenance when the service behind a socket changes.
   Revision 22 recovers capabilities from its selected source; it does not yet
   provide complete source failover.

## Hardware and release gates retained

Validated live fan/RGB/charging controls, per-key lighting, MUX switching,
physical settling/thermal/recovery behavior, suspend/resume, physical desktop
authorization/input, physical shortcut activation, compositor/full-screen
overlay behavior and cross-distribution graphical installation remain open.
No new physical writes or host package/service installation were performed for
this implementation. Existing exact-target trial plans remain applicable.

The earlier approved PPD switch/restore on board 8C30 / BIOS F.10 remains the
only documented physical profile transaction. Fixture/VM results must not be
relabeled as physical validation. Revision 14's package/VM evidence does not
validate revision 15 binaries. Revision 15 now has its own isolated package/VM
results below; neither is physical hardware validation.

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

Exact source/runtime/package identities, failed harness attempts and current
results are in `evidence/launcher-expansion/validation-summary.json`.
`LAUNCHER_EXPANSION.md` describes routes, version guards and runtime exclusions.
At that milestone, systemd VM verification remained open; the later addendum
above records its 33-check result. Real launcher games/accounts, full accessibility
and graphical/physical release requirements remain open. English-only presentation is retained.

## Revision 19 keyboard and presentation verification

The source passes 253 Rust tests per debug/release profile, formatting and strict
all-target/all-feature Clippy. Thirteen actual keyboard/Orca workflows pass in
both profiles on private Xwayland. They cover page/pane/search access, collapsed
disclosures, metrics, chart range/export/history, game setup, report reset,
narrow navigation, action feedback and stable network-copy selections. A held
reading does not survive missing data. No host keyboard events are injected.

The final release GUI passes a 36-scene default matrix over all nine pages in
light, dark, high contrast and effective Sans 20 text at a narrow width. There
are 1,041 measured text-label contrast samples and no detected horizontal
clipping, collapsed text allocation or failing measured contrast. Two additional
normal/large-font network-selection scenes verify the held-state notice (59
measured surrounding-label samples; native selection foreground is excluded).
Four wide/narrow own-app captures were inspected together once, with no further
visual change needed. This remains bounded scene and input coverage, not complete
accessibility or contrast certification.

Current package/results, exact identities and the initial AT-SPI timeout are
recorded in `evidence/accessibility/validation-summary.json`. The release CLI and
daemon are byte-identical to revision 18, whose managed/launch/smoke evidence
retains its original attribution. Twenty managed integration checks were rerun
against the current debug CLI and passed. Final release regressions pass on
both X11 and Wayland: 31 native checks, five 10,000-game library workflows, five
managed and six recovery workflows, plus 32 shortcut/overlay checks per backend.
Installed deployment, real store execution,
expanded/transient states, other themes/assistive technology and physical release
requirements remain open. See `ACCESSIBILITY.md` and `TEST_REPORT.md` for limits.

## Revision 18 launch recovery verification

The source passes 253 Rust tests per debug/release profile, formatting and strict
all-target/all-feature Clippy. Twenty managed integration checks per profile
exercise the actual CLI and production service with a private PPD server and
real user scopes. New checks cover nonzero status, direct SIGTERM forwarding,
inherited ignored SIGCHLD, scope failure before registration, closed reasons and
private report storage. No system power bus or physical writes are used.

The release GUI passes 31 regression checks, five 10,000-entry library workflows,
five managed-session workflows and six launch-recovery workflows on each of X11
and Wayland. Library checks now recycle rows repeatedly while the daemon is
stopped. Recovery checks cover delayed authorization denial after the receipt,
deduplicated reports, clearing without profile/configuration changes, reopening,
damaged saved data and live fallback. One additional X11 capture check produces
app-only wide/light and narrow/dark screenshots, inspected together once.
Sixteen manual-launch fixtures and seven daemon/CLI smoke checks also pass.

The revision 18 package is built and statically verified. Exact binaries, source
and harness manifests, results and limits are in
`evidence/launch-issues/validation-summary.json`. Its container/installed VM and
physical release requirements remain open; earlier packages retain their own
results. This feature milestone does not complete the full application audit.

## Revision 17 UI verification

The preserved revision 17 source passed 244 Rust tests in each debug/release profile, formatting
and strict all-target/all-feature Clippy. The final release GUI passes 31 existing
and extended native regression checks on each of X11 and Wayland. Five additional
large-library workflows per backend verify 10,000 entries, profile-capability
aging/recovery without configuration mutation, correct final-entry launch,
expanded setup and changed scan results. Managed-session UI workflows also pass
on both backends; two app-only light/dark captures were visually reviewed.

A controlled X11 fixture comparison at 2,000 entries measured 5.39 s / 679 MiB
resident memory for the preserved revision 16 GUI and 2.10 s / 311 MiB for the
new GUI. The measurements include scan plus the first accessible response and
are single observations on this machine. They are not universal performance
bounds or physical game/hardware validation. The new GUI also completed the
10,000-entry workflow on both tested backends.

Exact binaries, evidence and remaining requirements are recorded in
`evidence/library-virtualized/validation-summary.json`. The revision 17 package
is built and statically verified; no successful revision 17 container/VM
installation result is claimed. Revision 16's earlier package and evidence are
preserved. The full audit remains open.

## Revision 16 verification

| Check | Current revision 16 result |
| --- | --- |
| Rust workspace | 244 passed in debug and 244 in release; none failed or ignored |
| Formatting / Clippy | Formatting and strict all-target/all-feature Clippy passed |
| Managed command integration | 15 checks in debug and 15 in release: actual CLI, production IPC/service, private PPD and real user cgroups |
| Provider / kernel foundations | 12 checks in each debug/release profile |
| Native GTK regressions | 29 checks on each of X11 and Wayland, using the final release GUI |
| Native managed-session UI | Five interaction workflows on each backend; one additional wide/light and narrow/dark capture check on X11 |
| Manual launch / process smoke | 16 release launch fixtures and seven daemon/CLI smoke checks |
| Package | Built and statically verified against the release executables; no host installation |
| Container / installed sandbox VM | **Not verified for revision 16.** Dependency downloads stall on both attempted mirrors. Both attempts were stopped and cleaned up before application test execution. A private-provider installed-sandbox test is prepared, but has no passing result |
| Visual review | Final release wide/light and narrow/dark Games captures inspected in `docs/screenshots/managed-games/`; not a full contrast/large-text audit |

Exact results, hashes and source/test manifests are in
`docs/evidence/audit-v16/validation-summary.json`. Native checks use private
providers and no physical control writes. Revision 15's package/VM results below
remain attributed to that earlier snapshot. They do not validate revision 16's
changed daemon, CLI or GUI. Hardware gates and the full application goal remain
open. Dependency cache recovery is incomplete and supplies no deployment result.

## Verification

Current command outputs are retained in `target/audit-2026-10-27/` and audit
logs under `target/`. Durable results are collected under `docs/evidence/audit-v15/`.

| Check | Revision 15 result |
| --- | --- |
| Rust workspace | 240 passed in debug and 240 in release; none failed or ignored |
| Formatting / Clippy | Formatting check and all-target strict Clippy passed |
| Native settings / history / games / health / network | 29 checks on each of X11 and Wayland, against the final release GUI |
| Native shortcut / overlay workflows | 32 checks on each of X11 and Wayland, using private portal/tray fixtures |
| Notification delivery | Four release checks against a private notification server; no real desktop notification |
| Game routing / recovery | 16 release process-fixture scenarios; no real store client/game or physical profile write |
| Process smoke | Seven release fixture checks |
| Package | Static validation and 19 isolated container lifecycle checks passed; container removed |
| Systemd VM | 30 checks passed; guest exited and temporary resources were removed |
| Visual review | Six final release app-only fixture captures in `docs/screenshots/audit-v15/` cover wide/narrow, light/dark, requested high contrast and requested text scaling. Requested text scaling does not establish effective physical DPI or complete large-text support |

The package requires GTK 4.12+ and libadwaita 1.4+ for the adaptive split view.
All fixture GUI records identify the final GUI hash; the package verifier checks
the packaged binaries against the release executables. Package hashes and any
documentation-only equivalence are recorded in the final validation summary.
Earlier native evidence keeps its original binary attribution.

The goal remains open until incomplete requirements have authoritative
implementation and validation evidence. This tracker preserves the audit's
full scope rather than defining success around the implemented subset.
