# Status overlay

Revision 0.1.0-11 adds an opt-in compact GTK monitoring window. Enable **Show
status overlay** in Settings → Desktop integration, use the tray's Show/Close
status overlay item, or press Ctrl+O while an OMEN window has focus. Close it
with its labeled Close overlay button, window close button or Escape. Open
OMEN Control returns to the main window. Closing the main window or choosing
Quit exits both; hiding the main window to the tray leaves the overlay visible.

The user's `general.overlay_enabled` preference defaults off, migrates from
older configs without changing the schema version, and round-trips through
user backups. Enabling must save successfully. Closing always works even if a
save fails; Settings reports “Overlay preference was not saved”; the previous startup preference
may remain.
Configuration failures preserve the existing file. No service setting or
hardware capability is enabled by this preference.

## Data and overhead

The overlay consumes the existing poller's bounded snapshot channel. It starts
no additional hardware poller, process, network request or timer. The existing
GTK drain refreshes visible values and their age. CPU/GPU temperatures use known
provider identities, with the highest available channel explicitly labeled;
GPU utilization likewise reports the highest channel, not a sum. This is
monitoring, not calibrated temperature or a thermal-control input. CPU usage,
memory, fan RPM, profile and battery/AC use the same typed data as other pages.
Profile control availability is shown separately from the observed profile.
Mock sources are labeled as fixture data.

Individual numeric samples retain their acquisition timestamps. A frame is
dated before collection begins, so a blocked or slow collection cannot make
old fan/profile/battery data appear freshly acquired. Values become Stale after
max(7 seconds, 3 × configured polling interval), capped at 30 seconds. Zero,
future or over-age timestamps are stale. An aged whole frame makes every row
Stale; within a fresh frame missing/non-finite values are Unavailable. A stale selected channel cannot be hidden behind a fresher
channel. Wall-clock changes may conservatively stale data. Provider-side caches
and sensor calibration remain limitations of the monitoring source.

## Desktop scope

The implementation uses ordinary GTK4 application windows and the existing
[GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html).
Its opt-in session requests Show OMEN Control and Toggle OMEN status overlay.
Only the pinned portal owner/current session and a granted action can activate
it. Missing overlay grants leave Show available and report the missing shortcut;
no synthetic global key handler is installed. Existing saved opt-in may cause
the desktop to ask about the additional action on the next fresh session.

The overlay maps through
[Widget.set_visible](https://docs.gtk.org/gtk4/method.Widget.set_visible.html),
without requesting activation through
[Window.present](https://docs.gtk.org/gtk4/method.Window.present.html).
Compositors still decide actual focus and stacking. This is a movable status
window, with native scale/theme/focus handling. It makes no promise of a fixed
screen corner, always-on-top, click-through, exclusive full-screen visibility,
game-frame injection or global key support on every desktop. No layer-shell,
X11 window-state hack or new native dependency is introduced.

Native labels/buttons and logical GTK sizing provide the accessibility and
scaling foundation. Full screen-reader traversal, physical keyboard shortcuts,
compositor focus policies and all desktop shells require further validation.
The application interface is English-only at the user's request. Native system
dialogs follow desktop language settings.

## Validation

`tests/shortcuts.py` extends the real GTK/AT-SPI/private portal and tray workflows
with overlay opt-in/close, measured fixture values, hidden-main toggles,
single-instance activation, independent stale aging during daemon SIGSTOP/CONT,
corrupt/unwritable preferences and startup restoration. These process signals
apply only to the harness's private fixture daemon. No host shortcut is bound.
The result files identify build hashes and actual GTK backends.

`scripts/capture-overlay.py` captures only the app's own widget trees on Wayland
at requested 1×/2× scale with fixture data, and 1× live read-only monitoring.
`evidence/overlay-captures.json` records image and executable identities. No
unrelated desktop framebuffer is read. These captures do not establish overlay
visibility over full-screen games or physical key activation.

The final release benchmark observes both modes with the main window visible:
191.9 MiB/2.40% own CPU without the overlay and 198.0 MiB/2.20% with it. These
warm sequential five-second samples are noisy (CPU is relative to one logical
core), not a controlled incremental-cost or cold-start comparison. The complete
method, helper CPU and hashes are in `evidence/performance.json` and TEST_REPORT.
