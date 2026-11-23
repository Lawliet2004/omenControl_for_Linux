# Keyboard access, text size and presentation

The unchanged revision 24 application now passes seven native notification
workflows per debug/release profile. Actual private keyboard input edits the
checkbox/fields and saves; real GNotification delivery and action routing pass.
Corrupt-file failure preserves the active monitor, real cooldown/rearm passes,
and disabled settings remain silent past cooldown and after reopening. See
`NOTIFICATION_WORKFLOW_2026-10-28.md` and
`evidence/notification-workflow-v24/validation-summary.json`. Physical desktop
notification display and full accessibility remain unvalidated.


Revision 24 adds device/type sensor groups and named native Type, Device,
Availability and Order controls. Sensor values/details retain native identity
and hold a clearly labeled copy snapshot while selected. Missing/stale data and
source changes invalidate held rows. Search remains available through Ctrl+F.
Five final browser scenes pass with 190 measured active text-label contrasts;
long selected picker values ellipsize while full text remains accessible. See
`SENSOR_BROWSER_2026-10-28.md` for current native and scale verification.


Revision 23 retains the control keyboard workflows while updating source labels
and resetting editors only for a changed verified source. Six wide/light and
narrow/dark effective large-font source scenes, with six companion overlay
captures, pass the bounded presentation review. This adds 185 active text-label
contrast measurements and does not establish complete accessibility conformance.
See `SOURCE_SWITCHING_2026-10-28.md`.


Revision 22 adds native control-page recovery and keyboard coverage. Private
XTEST input activates a focused profile choice, edits fan sliders/spin fields,
opens/cancels the native color dialog, edits brightness and explicitly applies a
charge threshold. Twelve recovery/keyboard workflows pass per build profile.
The harness supplies input focus after a transient closes because its private
X server has no window manager; physical focus restoration remains unverified.
Advanced fan curve content uses the existing disclosure focus behavior and
retains the accessible duty-history chart. Twelve final control scenes cover
wide/light and narrow/dark effective Sans 20, including expanded/scrolled curves,
with 273 measured active text-label contrast samples. This extends defined
coverage, not complete accessibility conformance. See
`CONTROL_PAGE_RECOVERY_2026-10-28.md` for exact scope and source-switching limits.

Revision 19 adds window-local navigation and verifies native keyboard workflows
on an isolated Xwayland display with the installed Orca screen reader. The
application remains English-only. Native system dialogs follow desktop settings.

## Keyboard navigation

| Key | Action |
| --- | --- |
| F6 / Shift+F6 | Move forward/backward between header, navigation and page content |
| Up / Down in navigation | Choose a page; the folded sidebar stays open while choosing |
| Tab / Enter in navigation | Enter the chosen page; close the folded sidebar |
| Ctrl+1 … Ctrl+9 | Open Dashboard, Vitals, Performance, Fans, Lighting, Graphics, Battery, Games or Settings, in sidebar order |
| Ctrl+F | Focus sensor search in Vitals or game search in Games, scrolling library controls into view |
| Tab / Shift+Tab | Move between native controls and selectable readings |
| Space / Enter | Toggle or activate the focused native control |
| Escape | Close a menu or cancel a file chooser |
| Ctrl+O | Toggle the status overlay while the application has focus |
| Ctrl+H / Ctrl+Q | Hide to an available tray / quit while the application has focus |

These shortcuts do not install a global input grab. The separate optional portal
shortcut still needs desktop approval and physical-key validation.

Chart controls belong to a named group, including units. **Read history data**
opens a stable, selectable text snapshot with acquisition times, values, sources
and gaps. Polling does not replace that snapshot. Use **Refresh history readings**
to update it. Read-only history, diagnostics and wrapper-command text allow Tab
to leave the text view. CSV export uses the selected range and writes a new
private file; it does not replace existing files or active configurations.

Collapsed disclosures explicitly hide their contents as well as detaching them.
This prevents hidden selectable children from consuming Tab on the tested GTK
runtime. Controls retain native roles, state and selection behavior.

Network readings keep their native identity during ordinary value updates. While
a reading has keyboard focus and selected text, a changed acquisition leaves the
selected snapshot intact and shows **Reading held for selection**. Clear the
selection or leave the reading to resume current values on the next poll.
Monitoring continues. Missing data and replaced interfaces remove held rows as
well; a selection never conceals an outage. This also avoids the tested GTK/X11
behavior in which immediately restoring a selection after replacing label text
can be followed by another native selection clear.

## Action feedback

On GTK 4.14 or newer, current action feedback requests a queued screen-reader
announcement through the existing accessible window context. Newly shown labels
may not yet have a registered AT-SPI context, so sending the request through the
window matters. A delayed announcement is discarded if its message changed or
the label is no longer mapped. Continuously polled readings are not announced.

The announcement API is resolved at runtime. GTK 4.12 remains supported without
that API; the complete feedback text remains visible, selectable and exposed to
assistive technology. Automatic action announcements on that older runtime are
not claimed. No success is inferred merely from requesting an announcement:
native verification checks Orca's generated speech.

Disclosures inside the virtualized game list handle unmodified Space/Enter while
the disclosure itself is focused. Their accessible name stays the concise label
when expanded; their content remains independently accessible. Descendant text
controls keep their ordinary input behavior. This also covers Library locations.

## Large text and contrast

Adaptive action rows measure their children's real width and height requirements.
They switch orientation without introducing focusable grid wrappers. Font, DPI
and control-property changes refresh those measurements without a continuous
frame callback. Destroying a row disconnects its settings listener. Long checkbox
and disclosure labels wrap. Chart axes use Pango and the configured GTK font;
chart height and axis padding grow with the measured text.

Primary actions retain the semantic system accent, with a darker background to
improve small white text contrast in the tested theme. Native theme surfaces and
foreground colors remain in use. History series also retain solid/dashed styles
and an equivalent textual view.

`tests/gui_presentation.py` reads the app's own widget render and optional geometry
report. It records actual font, theme/high-contrast state, Pango text bounds,
ancestor clips, effective opacity and computed foreground. Rendered pixels supply
the background sample. It checks horizontal bounds, zero-size text allocations
and text contrast; inactive labels are recorded separately. Geometry checks also
include mapped content below the visible part of a scrolled page. Contrast
measurements need a fully visible text allocation and a sufficiently uniform
background. Unsupported samples are not presented as measured passes.
The selected-network scenes measure the held-state notice and surrounding
labels. They exclude the highlighted reading's contrast because a normal label
foreground does not expose the native selection foreground.

This is a defined scene matrix, not a universal contrast certification. Expanded
and transient states, hover/pressed styling, additional accents/themes, other
screen readers, audible synthesis, physical focus and effective display DPI need
further coverage. The original full application audit remains open.

## Verification

Native input tests require Xwayland, xauth, libX11/libXtst, AT-SPI, Orca, Python GI
and a Wayland session. The presentation test additionally uses Pillow. No packages
or services are installed by these tests. The private session/accessibility buses,
X authorization and configuration directories are temporary. XTEST sends events
only to the new authenticated X server. Selecting the private X window's input
focus stands in for the absent window manager; it does not establish physical
desktop focus/restoration behavior. Speech Dispatcher is unavailable to the
fixture, so Orca generates logged utterances without audible synthesis.

```bash
OMEN_TEST_PROFILE=release python3 tests/gui_accessibility.py
OMEN_TEST_PROFILE=release python3 tests/gui_presentation.py
OMEN_TEST_PROFILE=release python3 tests/gui_network_selection.py
```

Use `OMEN_TEST_EVIDENCE_DIR` to select an output directory. Presentation cases and
pages can be selected with `OMEN_PRESENTATION_CASES` and
`OMEN_PRESENTATION_PAGES`. `OMEN_UI_REPORT_PATH` is an explicit developer option
used with the existing own-window capture path; it writes a private new file and
refuses an existing destination. It does not inspect other windows.

Revision 21 adds native icon resolution metadata to that developer report and
bundles missing-theme symbol fallbacks without changing valid theme priority.
Four scenes cover a missing theme in wide light/narrow dark Sans 20, Adwaita
light and Breeze dark, with 142 measured active text-label contrast samples.
Actual KDE portal/PAM interaction also passes in a fresh private Wayland VM.
These are bounded virtual/fixture results, not physical focus/DPI or complete
accessibility conformance. See `DESKTOP_VALIDATION_2026-10-28.md`.

Revision 19's broad scene matrix and exact identities remain in
`evidence/accessibility/validation-summary.json`. Revision 20 reruns the thirteen
keyboard/Orca workflows per build profile and adds six launcher keyboard workflows
and expanded library presentation scenes. Its current identities and results are
in `evidence/launcher-expansion/validation-summary.json`. Earlier packages and
evidence retain their original attribution. Library folder guidance follows the
selected launcher; removal buttons retain their natural height beside wrapped paths.

The implementation uses the GTK [announcement API](https://docs.gtk.org/gtk4/method.Accessible.announce.html)
and libadwaita's [breakpoint container](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.4/class.BreakpointBin.html).
The reviewed GTK [expander implementation](https://github.com/GNOME/gtk/blob/4.22.0/gtk/gtkexpander.c)
detaches collapsed children while its focus path checks child visibility. The
application workaround is also verified by actual Tab input on the installed
GTK runtime; the source inspection alone is not its validation.
