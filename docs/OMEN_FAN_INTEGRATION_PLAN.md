# Automatic and Maximum fan control in OMEN

Goal: offer the two HP firmware modes in OMEN without a separate desktop app.
The GUI remains unprivileged; the daemon and independent recovery service own
hardware access. The observed Victus 8C30/F.10 interface has no manual PWM
targets, so this plan does not add percentage or curve control.

1. Preserve the pinned hp-wmi actuator, authenticated IPC, thermal preflight,
   four-second lease and independent Automatic recovery. Refresh Maximum while
   a healthy owner still holds the mode, as FanPilot does. Keep the existing
   ten-second live-trial ceiling until sustained operation has been validated.
2. Add a crash-recoverable handoff for the known Victus fan services. Record
   which services were active before attempting a stop; only resume those
   services, and only after Automatic is confirmed. A failed stop, lost identity,
   unknown competing writer, failed restoration or failed restart must remain
   visible and fail closed. Persist intent before changing service state, and
   test interrupted stop, writer and controller crashes, restart, and partial
   failures. Do not allow the GUI to specify service names or executable paths.
3. Integrate handoff with capability discovery and explicit GUI actions. Show
   why control is unavailable, pending or failed; do not claim a selected mode
   until hardware readback confirms it. Ensure the ownership lifetime and exit
   behavior are clearly presented. Avoid a root helper loaded from a user's
   writable home directory.
4. Validate the service lifecycle and the thermal/recovery paths using isolated
   fixtures and a systemd VM, including service conflicts, suspend gaps, stale
   telemetry, hardware replacement and daemon/writer failure. Record the exact
   binaries and package tested.
5. On the target machine, prepare the packaged services and conduct the bounded
   Automatic → Maximum → Automatic trial with independent mode and fan RPM
   observations and confirmed recovery. Sustained Maximum must be separately
   validated before removing the trial ceiling or calling the feature complete.

Current status: OMEN already provides the GUI, experimental mode-only writer,
thermal checks and independent recovery. Heartbeat refresh has been added.
Conflict discovery now identifies the Victus systemd services even when their
process name is Python, and recognizes an active FanPilot helper. This is
best-effort discovery, not exclusive ownership or a service handoff. The
handoff, installation and physical validation remain outstanding. The normal
build remains read-only. FanPilot should remain available until OMEN's
replacement has been verified.
