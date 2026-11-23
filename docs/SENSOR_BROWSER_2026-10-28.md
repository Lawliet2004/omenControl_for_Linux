# Sensor browser and reading identity

Revision 24 completes the audit's device/type sensor browsing requirement. The
previous interface offered search and alphabetical order, but only divided
readings into temperatures and everything else. Its positional row reuse could
change the sensor represented by an existing accessible object when the list
changed. Earlier tracker text calling that complete device grouping was too broad.

Vitals now groups measurements by provider-reported device identity and type.
When no device identity is reported, the group names the provider and explicitly
says “device not reported.” A friendly label never establishes device ownership.
Raw identifiers, provider, reported device, units and acquisition age remain
available as selectable text.

Native Type, Device, Availability and Order controls combine with search.
“Highest value within type” compares fresh numeric values within each device/type
group; it does not rank watts against degrees. Missing and stale values sort
behind current numeric values. A missing selected device stays selected and is
marked “not reporting”; it is never silently changed to another device. Clear
sensor filters restores the defaults. Search and filters remain local to the
open window and do not write desktop or service configuration.

Rows are keyed by provider, raw identifier, measurement type and reported device.
Inserting or reordering another sensor preserves the row's native identity.
Changing its provider/device/type replaces it. Duplicate ambiguous identifiers
are omitted with a visible count rather than collapsed into one misleading row.
A source transition invalidates the previous rows.

Selecting a sensor value or its diagnostic detail holds both as an explicitly
labeled copy snapshot while current monitoring continues. Clear the selection or
leave the reading to resume updates. A stale/unavailable sample invalidates the
held value, and disappeared data removes the row. Stale numbers are replaced by
“Stale.” Vitals also stops calling unavailable CPU utilization “Collecting” after
an acquisition has completed.

## Validation

The source passes 276 Rust tests per debug/release profile, including five new
model tests for grouping/identity, availability, intersecting filters, comparable
sorting and duplicate rejection. Nine browser workflows pass per debug/release
profile, including actual keyboard filtering, copy selections, stable identity,
disappearance/recovery, generated Orca narration and 1,000 readings. Both native
X11 and Wayland backends pass the existing 31 settings/health/network regressions.
Thirteen existing keyboard/Orca workflows also pass with the final release GUI.

The candidate passes 38 checks in a fresh signed Ubuntu/Plasma Wayland VM,
including the installed current-helper sandbox, five source transitions and two
actual guest reboots. QEMU exits zero and all private inputs are removed. The
helper path is selected from the current Cargo compiler-artifact JSON. CLI and
daemon binaries remain byte-identical to revision 23; revision 24 changes the
GUI and packaging/documentation. No new real KDE/PAM or real-store-game test is
claimed for this revision. Final package/source/test identities are recorded in
`evidence/sensor-browser-v24/validation-summary.json`.

Five final scenes cover wide/light, narrow/dark Sans 20, actual high contrast,
a long selected device identifier and no search results. There are 190 measured
active text-label samples, with no measured horizontal clipping, collapsed text
or contrast failures. The first inspection found a closed-picker overflow for
long device strings. Selected text now ellipsizes; full text remains available
in the native popup, tooltip, raw details and accessibility tree. One confirmation
batch passes; no further visual polish cycle is planned.

The first empty-state capture encountered a transient missing render node in the
explicit developer screenshot path. That helper now retries briefly and reports
failure rather than panicking; it writes geometry only after a capture succeeds.
Normal app execution does not enable capture mode.

The actual private keyboard workflow also checks 1,000 reported readings and
searches the final identifier. Its elapsed time and resident memory are recorded
as single fixture observations, not universal performance bounds.

GTK's native searchable picker needs an explicit string expression when using
the custom wrapping item factory. Field-label relations also name the native
focusable toggle, not only the surrounding combo. Native input tests exposed
both issues. The implementation uses the public [GtkDropDown expression API](https://docs.gtk.org/gtk4/method.DropDown.set_expression.html).

## Boundaries

English remains the only app language. Reported device identities are provider
metadata, not physical validation. Private XTEST input and generated Orca speech
do not establish audible synthesis, physical focus/DPI or every screen-reader
combination. Larger datasets, additional expanded states and hardware
release gates remain part of the full audit.
