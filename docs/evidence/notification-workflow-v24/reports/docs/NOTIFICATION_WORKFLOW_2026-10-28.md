# Native notification workflow verification

This adds verification for the unchanged revision 24 application and package.
The earlier delivery fixture loaded notification preferences directly from a
file; it did not prove that a user could edit the checkbox and fields, save them,
follow a notification action or recover after a settings failure.

`tests/gui_notification_workflow.py` uses actual private XTEST keyboard input to
operate the existing GTK controls. AT-SPI observes names, state and feedback;
it does not toggle controls or replace editable text. A private D-Bus server
receives real `GNotification` delivery through the normal notification backend.
The receiver never displays a host-desktop notification.

The seven workflows are:

1. The native checkbox starts off and fresh fixture temperatures produce no
   message.
2. Space toggles it and keyboard input edits threshold, hysteresis and cooldown.
   Requested changes neither rewrite configuration nor enable delivery before
   Save.
3. Save writes only the desktop user file, updates the running monitor, and
   produces one notification with fixture origin, temperature and the
   informational-purpose statement. Repeated hot readings are suppressed.
4. After leaving Settings, the private server emits the actual notification
   default-action signal. The same running app opens Settings & Diagnostics.
5. A deliberately corrupted private settings file causes Save to fail visibly
   and remains byte-for-byte unchanged. The previously active configuration
   remains in effect: cooling and reheating rearm the monitor, and a second
   message arrives after the real minimum cooldown has elapsed.
6. Repairing the file and retrying saves the requested disabled state with
   private file permissions. Hot readings remain silent after waiting past the
   actual cooldown, without changing service configuration or physical controls.
7. Reopening the app retains the disabled checkbox and saved threshold values;
   hot fresh readings stay silent on the new monitor.

All seven workflows pass in both debug and release, with cleanup complete.
Exact hashes and raw results are published under
`evidence/notification-workflow-v24/`. Product sources,
compiled binaries and the existing Debian package are unchanged; no new package
revision is needed for this verification addendum.

Only ordinary files in the mock hardware tree are changed. Input targets a new
authenticated rootful Xwayland server, with private session/accessibility buses
and temporary settings. The fixture does not use host input devices or the host
notification service. Resources are removed when it exits.

This closes the direct checkbox/edit/save/action/error-recovery gap for the
stated private desktop conditions. Physical notification display, compositor
policy, broader desktops, audible synthesis, complete accessibility and hardware
validation remain open. Existing revision 24 package/VM and earlier real-game
results retain their separate attribution.
