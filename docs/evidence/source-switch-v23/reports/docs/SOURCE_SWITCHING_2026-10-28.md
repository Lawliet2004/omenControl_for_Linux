# Monitoring source switching

Revision 23 reconnects the GUI when the daemon starts after the application,
falls back to direct read-only monitoring when the default daemon is unavailable,
and refreshes source provenance when the process or its reported identity changes.
English remains the only application language.

Source selection and all identity/readback requests run on the monitoring worker.
The GTK thread starts in a waiting state. Each snapshot carries its own source
label, fixture classification and immutable action route. The Dashboard,
sidebar, health center, overlay, tray and temperature notifications use that
snapshot's provenance. An explicit `OMEN_SOCK` endpoint never falls back to host
hardware when it fails; this keeps private fixtures isolated.

A service route is pinned to its socket device/inode/owner, authenticated Unix
peer UID/PID and process lifetime. Linux peer pidfds are used when available;
process start time is the fallback. GetInfo must report a supported protocol.
The complete observed identity is checked after acquisition, and mixed frames
are discarded. Nonmock service data receives physical-origin attribution only
with a root-owned peer and socket. Other nonmock endpoints are labeled
“Source unverified,” receive unknown compatibility attribution and do not gain
local GPU supplementation. Availability still does not establish physical
validation of any control.

Queued writes recheck the observed GetInfo on the same connection used for the
mutation. A replacement endpoint cannot receive an old action. File-picker
operations, service-policy previews, game-session releases and manual launch
profile restoration retain their original source. Tray profile actions carry
the source identity advertised by their menu. Session refreshes discard results
from a source that is no longer current.

A temporary outage retains unsaved edits if the same process/identity returns.
A new verified identity clears chart history, notification latches and stale
network/session rows, and reloads manual, curve, lighting and threshold editors.
A visible message asks the user to review the new values. No transition submits
a hardware or policy write. Notification cooldown remains in effect.

## Validation

The source passes 271 Rust tests per debug/release profile, formatting and
strict all-target/all-feature Clippy. The seven new Rust tests cover pinned
socket replacement, metadata changes on the write connection, protocol mismatch,
late connection, mixed-frame rejection, source tracking and unknown provenance.
Six native source workflows pass per build profile: late connection, complete
endpoint failure, same-source edit retention, import-preview replacement,
control reload and dynamic provenance. Twelve control/keyboard workflows per
profile still pass, including independently blocked acquisition/expiry. Both
native X11 and Wayland backends pass 31 settings/health/network regressions.
Twenty private managed-game integration checks pass with actual CLI and service
processes, real user scopes and a private PPD provider.

Six app-only main-window scenes cover fixture, unverified and unavailable sources
in wide/light and narrow/dark effective large-font layouts, with six companion
overlay captures. There are no measured horizontal clipping, collapsed-label or
contrast failures among the 185 active text-label samples. A single batched
review and one installed-desktop confirmation were used; no subsequent UI edit.
Intentional compact overlay ellipsis retains accessible text/tooltips and does
not establish complete large-text accessibility.

The installed candidate passes five source workflows on the default endpoint in
Plasma Wayland: Direct → Fixture → Direct → Service, then daemon restart and a
fresh diagnostic report through the new route, all in the same GUI process.
The full fresh VM run passes 38 checks, including two actual guest reboots,
QEMU exit 0 and removal of its private disk/seed/keys.
A preceding run of the **identical candidate** passes six real KDE/PAM checks,
including denial, approval, retained authorization and natural 300-second expiry;
its later source-observer setup failure is retained, rather than labeled as a
passing full VM run. All failed VM inputs are removed.

The retained harness failures include a defunct accessibility object during
session startup, an overbroad model check matching About copy, and an assertion
that ran during expected restart invalidation. Each was corrected in the
observer; no app runtime change was required. Two old regressions were updated
for the explicit unavailable-source state: total endpoint failure now has its
own test, while blocked acquisition still exercises the independent stale timer;
an offline service preview may be rejected before a connection is attempted.
The actual replacement-preview test verifies no mutation reaches the new server.

Exact package/source/test identities and raw results are published in
`evidence/source-switch-v23/validation-summary.json`. Historical revision 22 and
revision 21 packages/evidence remain unchanged and separately attributed.

## Limits

This is software recovery and provenance validation. It does not establish
physical fan, RGB, battery, GPU, suspend/resume or desktop-input behavior.
Complete accessibility, additional distributions and store/runtime compatibility
remain part of the application audit. The real native Lutris/OpenTTD result stays
attributed to revision 21; it is not a new revision 23 game test.

A root-owned service is a trusted local component, not a cryptographic statement
about hardware validation. Deliberately inconsistent data from a compromised
root service is outside this identity check. The interface can detect endpoint
or GetInfo changes, rather than prove the semantics of an arbitrary server.
