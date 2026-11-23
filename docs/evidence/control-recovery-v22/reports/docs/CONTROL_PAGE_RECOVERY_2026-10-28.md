# Control-page recovery — revision 22

Revision **0.1.0-22** makes the five control pages follow fresh capabilities and
readbacks from their selected source. An unavailable first report no longer
permanently omits profile choices, fan editors, lighting or charge thresholds.
Controls recover without rebuilding an already loaded editor. English-only
presentation and physical hardware restrictions remain unchanged.

## Behavior

| Page | Current behavior |
| --- | --- |
| Performance | Shows the current profile first. Advertised choices appear/disappear as capabilities change. Known choices remain disabled through an outage; no change is submitted while reconciling them. |
| Fans | Current RPM and observed mode precede firmware actions and requested manual duty. Manual controls can appear after startup and retain their slider value through an outage. Telemetry and control explanations follow fresh reports. |
| Fan curves | Advanced editing is disclosed separately. The stored curve loads only from a successful readback; later polls preserve edits. All schema-supported temperatures, **0–110 °C**, remain representable. Editing immediately disables activation until the stored curve matches. Activation also rechecks the saved curve in the worker before requesting Curve mode. The existing duty-history chart is retained. |
| Lighting | Current brightness/zone count precede the editor. A valid matching readback is required before Apply. Read failures disable the editor without discarding its values. A changed zone count loads the matching state and displays an explicit layout-change notice. Applying color/brightness preserves the observed animation field. |
| Graphics | PCI inventory, driver/runtime state and NVIDIA readings refresh from snapshots. Partial discovery is distinct from a proven empty inventory. Missing/expired data removes old inventory/readings. No MUX switching action is invented. |
| Battery | Current threshold and requested threshold are distinct. A valid fresh readback initializes the editor; later external changes update Current without replacing the user's requested value. Failed/missing/expired readbacks disable Apply. |

Lighting, threshold, curve and graphics reads run in the monitoring worker.
Constructing these pages and processing their capability updates no longer
perform those synchronous hardware/IPC reads on GTK's main thread. Expiry drops
the control readbacks as well as telemetry/capabilities. No polling, page switch
or capability recovery submits a control mutation. Backend safety checks and
authorization remain authoritative.

An edit is an explicit requested value. It is not continuously replaced by a
poll. Lighting layout changes are the exception because the old color vector
does not describe the new layout; the visible notice explains the reload.

## Verification

The final source passes **264 Rust tests in each debug/release profile**,
formatting and strict all-target/all-feature Clippy. The existing expiry test
also seeds successful/failed control readbacks and verifies that expiry removes
all four fields.

`tests/gui_control_recovery.py` uses the real private mock daemon, ordinary-file
hardware, a capability/read-failure proxy, private session/accessibility buses
and an authenticated Xwayland display. Twelve native workflows pass per
debug/release profile. They cover the initially empty capability report, changing
profile choices, real fixture writes, curve boundary values, unsaved activation
gating, lighting failure/layout changes, independent stale expiry, retained native
editors, threshold changes and live PCI runtime/incomplete-inventory updates.
Actual private XTEST input covers profile activation, slider/spin editing, the
native color dialog's open/cancel path and explicit threshold Apply. The private
X server has no window manager, so the harness explicitly returns X input focus
to its own main window after destroying the color dialog. This is not evidence
of physical focus restoration.

Two preserved negative cases demonstrate the defects: the revision 21 GUI cannot
recover profile choices after the initial empty report, and the first revision
22 candidate clamps a valid 0 °C curve point to 30 °C. The latter prompted the
schema-range correction before final packaging. Earlier native test setup
failures (sidebar roles/page order, the short Apply label and missing window
manager focus restoration) are retained separately from passing results.

The final release GUI also passes the existing **31 native settings, health and
network regressions on each X11/Wayland backend**. Twelve final presentation
scenes cover all five pages in wide/light and narrow/dark effective Sans 20,
including expanded/scrolled curve controls. They report no horizontal clipping,
collapsed text or failing measured contrast across **273 active text-label
samples**. The initial twelve scenes were inspected together. After the curve
range fix, the four fan confirmation captures were inspected in one further
batch; no subsequent UI edit was made. Non-label controls, hover/pressed states,
other themes and physical DPI remain outside those measurements.

The final candidate passes **39 checks** in a fresh Ubuntu/Plasma Wayland VM,
including six real KDE/PAM authorization workflows, natural authorization expiry
and two genuine guest reboots. QEMU exits 0 and removes its disk, seed and private
key. The exact candidate SHA-256 is
`f32dc1de793ed3d8db1b96fbe038618a67b16a996fa3d5dbbc0f50b22110ced2`.
Verification is recorded in `evidence/control-recovery-v22/validation-summary.json`. It uses the
same signed offline repository and graphical authorization methodology as
`DESKTOP_VALIDATION_2026-10-28.md`. An earlier candidate's successful VM run is
preserved separately and is not substituted for the final candidate's result.
The final documentation package must match the tested candidate's runtime,
maintainer scripts and control metadata; the evidence records both identities.

## Scope and remaining work

Recovery here follows capabilities/readbacks from the **already selected
source**. Automatic migration from direct monitoring to a daemon started later,
and refreshing source/fixture provenance if the service behind a socket changes,
remain separate software requirements. Starting without a daemon can still
require restarting the app to select it. These limits must not be described as
complete service-source failover.

Physical fan/RGB/charging/MUX controls, settling/recovery/suspend behavior, broader
launchers/accounts, full accessibility and physical desktop input remain open.
The CLI and daemon remain byte-identical to revision 21; its separate native
Lutris/OpenTTD game evidence is preserved rather than relabeled as a revision 22
GUI game test. No host package/service installation or physical write occurred.
