# Exact-target Automatic/Maximum trial

This is a reviewable procedure, not an authorization or a completed physical test.
No host package/service install or fan write was performed while preparing it.

## Scope

Only Victus fb1002AX, board **8C30**, BIOS **F.10**, kernel
**7.0.0-38-generic**, and the canonical controller
`/sys/devices/platform/hp-wmi/hwmon/hwmon7/pwm1_enable` observed on this host.
The instance must be rechecked immediately before the trial; a changed kernel,
BIOS, board or controller requires a new review. `pwm1`/`pwm2` are absent, so no
manual percentage or curve is included. No load generation, stress, suspend,
reboot, raw WMI/EC operation or firmware protection change is included.

The helper only writes documented hp-wmi modes 0 (Maximum) and 2 (Automatic).
The daemon still requires fresh recognized CPU/GPU coverage before Maximum and
each renewal. Active NVIDIA requires the separately managed unprivileged NVIDIA reader. Its
root-private PID-1 socket, PCI identity and original acquisition timestamp are
validated; the GUI fallback does not authorize control. Missing/stale worker data
fails preflight. Do not weaken the controller device sandbox or invent a sensor
association to force the trial to run.

## Software prerequisites

Approval also covers the installed static read-only reader units and their
explicit activation for this trial. Default package hooks start neither the
reader nor the fan writer. The tested revision 14 package must match the final SHA-256 in
`evidence/validation-summary.json`. Its installed daemon must match that package's
binary digest. The static `omen-fan-recovery@.service` is packaged but is never
enabled or started by package installation. Default monitoring remains read-only.
The independent writer binds only the named mode attribute writable and uses a
root-private persistent runtime intent, singleton lock and private socket.

The trial helper has a **ten-second absolute Maximum ceiling**, even with healthy
controller heartbeats. Separately, its four-second lease expires when the
controller stops responding. A writer crash leaves durable intent for systemd to
restore Automatic before accepting new commands. A replaced actuator is refused,
with intent retained. No guarantee is made if kernel I/O blocks, systemd itself
fails, both services cannot restart, hardware disappears or power is lost.

## First separately approved execution

Approval must cover installation of the exact reviewed `.deb`, starting its
read-only service, the temporary experimental service configuration below,
a five-second Maximum observation and Automatic restoration. It must explicitly
exclude a persistent hardware-control release.

1. Record identity, kernel, canonical HP hwmon path, mode, both RPM readings,
   CPU/GPU provider attribution and current controller processes. Require an
   Automatic baseline. Check that the system is idle and not heating under a
   workload. Stop if identity/interface or provider coverage differs.
2. Install the exact reviewed package. Verify installed binary digests and
   that the monitoring service starts without hardware writes. Capture status
   and journal output. Never run the GUI as root.
3. Start the static `omen-nvidia-reader.socket`. Confirm that socket activation
   runs `omen-nvidia-reader.service` with a DynamicUser and zero capabilities.
   Run the installed daemon's `--nvidia-reader-probe` as root to verify a fresh
   `nvidia/temp` sample for `0000:01:00.0` when that GPU is active. The worker
   runs only the fixed read-only nvidia-smi query; its cgroup permits only
   `/dev/nvidiactl` and `/dev/nvidia0`. Stop before fan testing if provider
   access/identity/freshness fails. Then start `omen-fan-recovery@hwmon7.service`. Require its expected independent
   cgroup, restricted mount, private endpoint and no unowned startup write.
4. Add a temporary **runtime** drop-in for `omen-linux.service` which resets
   ExecStart and runs:
   `/usr/bin/omen-daemon --experimental-coarse-fans-for 8C30:F.10:7.0.0-38-generic:hwmon7`.
   Reload/restart only the controller and verify that its sysfs mount remains
   read-only. No sandbox/device expansion is permitted.
5. First run the script without an approval argument:
   `python3 scripts/coarse-fan-test.py --output docs/evidence/coarse-physical-preflight.json`.
   This is read-only. Require exact identity, non-mock daemon and ready capability.
6. Only within the specific approval, run:
   `python3 scripts/coarse-fan-test.py --approve-exact-target 8C30:F.10:7.0.0-38-generic:hwmon7 --output docs/evidence/coarse-physical-trial.json`.
   It requests Maximum, samples kernel-reported mode/RPM and the guarded thermal
   cohort for five seconds, then requests Automatic in its cleanup path. Its
   live readbacks are independent of the control reply. Any thermal/ownership
   error aborts to Automatic. The helper's ceiling remains active if the script
   crashes or its cleanup cannot reach the controller.
7. Verify final Automatic directly from the same pinned kernel attribute and
   from IPC, inspect RPM settling and both service journals, and confirm that
   the helper removed its owned intent. RPM changes and kernel echo are recorded
   separately; neither establishes calibrated airflow or full thermal safety.
8. Remove only the runtime drop-in created for this trial, reload/restart the
   default read-only controller, stop the helper after confirmed recovery, stop the reader socket/service and
   verify mode and read-only capabilities again. If recovery cannot be confirmed,
   retain the installed helper and ownership evidence for diagnosis. Do not
   remove its binary or discard pending intent.

A successful short trial is a first physical observation. Sustained Maximum,
physical crash/restart, suspend/resume, thermal behavior and removal of the trial
ceiling require additional explicit validation; fixture tests are not that proof.
