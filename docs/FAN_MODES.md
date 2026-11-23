# Automatic and Maximum fan modes

## Victus service integration

On board 8C30, the desktop can also use an already installed and running
`victus-fan.service` through its root-owned `victus-fanctl` utility. The desktop
user needs permission to update that service's configuration (normally through
the `victusfan` group). The GUI will disable this path if another recognized fan
controller, including FanPilot, is active. An action is confirmed only after
both fresh service status and the hp-wmi mode agree. Automatic and Maximum are
service overrides: they remain selected when OMEN closes and the service resets
the override to its policy on service restart. This integration does not install
or start the Victus service and does not provide manual percentage controls.

The Fans page has separate **Automatic** and **Maximum** actions; CLI equivalents
are `omen-cli fan-set auto` and `omen-cli fan-set max`. Manual percentages and
curves require both PWM targets and remain unavailable on the reference 8C30.
The additive `fan_modes` capability is independent of manual control.

## Revisions 13–14 independent recovery

The normal daemon and direct GUI/CLI backend remain read-only. An explicit
experimental exact-target flag can connect the root controller to the separately
managed static `omen-fan-recovery@hwmonN.service`. It is never enabled or started
by installation. Only the verified 8C30/F.10/7.0.0-38-generic target is accepted;
only its canonical hp-wmi mode attribute is writable inside the helper sandbox.
The GUI remains unprivileged and IPC authorization still applies.

A root-private socket accepts only the installed daemon executable in the expected
controller cgroup. Each lease binds UID/PID/start time, executable and cgroup,
random token, sequence and pinned actuator identity. The writer persists intent
before attempting Maximum and removes it only after verified Automatic recovery.
A singleton lock precedes any startup recovery. Replaced devices/machine identity
are refused rather than redirected. Late/replayed commands cannot rearm ownership.
A controller stall/death revokes the four-second lease; systemd restarts a killed
writer and its journal restores Automatic before it accepts new work.

Maximum has a **ten-second absolute trial limit**, independent of heartbeat
renewal. This is preparation for the first approved physical trial, not a release
of persistent live fan control. The controller keeps CPU/GPU provider, freshness,
cohort and suspend-gap checks. Conflicting controllers, missing telemetry or lost
mode ownership stop renewal. Revision 14 supplies active NVIDIA data through a separate, capability-free
DynamicUser service running a fixed read-only nvidia-smi query. A root-private
PID-1 socket binds that provider; fixture/live tagging, PCI identity and original
acquisition time are checked. Stale/missing data stops control. The controller
keeps PrivateDevices; a user's GUI cache is never control data. These reader
units are static and are started only for an approved experimental session.

Mode-only fixture tests cover native buttons, thermal loss and dynamic capability
changes. Standalone writer process tests additionally cover expiry, replay,
malformed frames, competing peers, inode replacement and durable restart. The VM
suite exercises separate real systemd cgroups and the packaged writer sandbox
using ordinary files only. Exact current counts/hashes are in
`evidence/validation-summary.json`; revision 13 and earlier evidence is archived separately.

## Physical boundary and run instructions

No physical fan write, host package install, host service install or host reboot
was performed in this implementation. The first physical test still requires the
specific approval described in [COARSE_FAN_VALIDATION_PLAN.md](COARSE_FAN_VALIDATION_PLAN.md).
Unknown targets and manual/curve hardware controls remain read-only. Recovery
cannot be guaranteed during blocked kernel I/O, loss of hardware, power loss or
failure of systemd to run/restart the writer. Pending intent is retained on failure.

Do not replace or remove an active experimental writer during a trial. Return to
Automatic and stop it first. If upgrading with an old writer still active, the
new executable identity check fails closed; finish recovery and explicitly
restart the writer before rearming. Default package hooks do not start it.

From `/home/papan/Desktop/Projects/omen`, run the normal read-only application:

```bash
. /home/papan/gtk-prefix/env.sh
export LD_LIBRARY_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
./target/release/omen-gui --page fan
```

`--mock` and `--mock-omen` are clearly labeled simulations. Development-only
`--mock-recovery auto` connects a mock controller to a separately started
`--coarse-recovery-fixture ROOT --instance hwmon0` ordinary-file writer.
The tests configure required fixture GPU attribution; no mock controls host fans.
