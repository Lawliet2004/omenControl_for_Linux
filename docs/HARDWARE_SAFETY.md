# Hardware safety and recovery

## Default policy

OmenCore's canonical source and fan-recovery limits are documented in
`OMENCORE_REVIEW.md`. Revision 8 includes its CLI in conservative process-name
conflict detection; its private writer lock does not coordinate with this
application. A process scan does not establish exclusive ownership. Live fan
writes remain disabled. See `FAN_MONITORING.md` for read-only mode telemetry and
the inventory's limits.

Live startup, GUI direct mode, and daemon installation perform **no hardware
writes**. Read-only discovery inventories DMI and present kernel interfaces. All
live fan/RGB/backlight/charge-control writes are blocked in `SysfsBackend` even
if an interface exists. Manual fan curves have unresolved hardware safety and
recovery requirements and are not released. No firmware protections are changed.

Standard Linux profiles can be enabled for controlled testing with the daemon's
explicit `--enable-profiles-for BOARD:BIOS` argument. It must match the discovered
identity and a source-known board or the observed 8C30/F.10 reference. This is an
experimental operator opt-in, **not proof of general hardware validation**. The
separately approved 8C30/F.10 PPD switch/restore transaction passed; see
`PROFILE_VALIDATION_PLAN.md`. That approval excludes other hardware controls.

No arbitrary path, raw WMI command, EC register, MSR, undervolt, overclock,
firmware-update or shell-execution operation is present in the daemon API. Games
are discovered and launched as the desktop user. The GUI never runs as root.

## Privileged boundary

The systemd service restricts address families, devices and filesystem writes.
Sysfs is mounted read-only; profile control through PPD can be authorized without
opening arbitrary sysfs writes. No generic hwmon sweep is installed. Read IPC is
available locally; mutation requires peer credentials and root or polkit. `pkcheck`
uses PID, process start time and UID; inability to identify/authenticate fails
closed. Mock mode has a private 0600 socket and explicitly identifies fixture data.
Clients, lines and subprocesses have bounds and timeouts. Configuration is typed,
validated, backed up and saved with a unique temporary file plus atomic rename.

Known competing fan/power-process detection is heuristic, not a complete guarantee
against all writers. PPD is preferred while active, its advertised choices are
read, and holds block profile mutation. Battery-aware policy is off by default.

## Fixture fan implementation and residual risks

Fan writes and curve ticks share a mutex, so an obsolete curve worker cannot
write after Automatic. All non-Automatic override preflights check CPU/GPU inputs before claiming a
new override. Fixed Manual/Maximum heartbeat renewals use the same thermal
freshness/cohort/gap checks as curves (revision 11). Guard or write failure stops the curve and attempts Automatic;
failed recovery remains owned and later ticks retry. Partial writes also attempt
rollback. `THERMAL_GUARD.md` documents identity/coverage checks, pinned cohorts
and fixture timing limits. Fan/profile workers are independent, so blocked
profile IO does not occupy the fan worker. Recovery errors remain explicit.
Revision 10 routes mock-daemon fan writes through a separate, leased writer over
pinned ordinary-file descriptors. It restores fixture Automatic after parent
SIGSTOP/SIGKILL and refuses a resumed parent's expired lease. See
`FAN_GUARDIAN.md` for protocol limits and process evidence. This helper rejects
sysfs and supplies no live hardware recovery path.

Signal handlers only set an atomic flag. Recovery runs in normal thread context.
Shutdown rejects subsequent mutations, restores only state the service attempted
to own, and removes its socket. These paths have fixture and process smoke tests.

These are **not real-hardware guarantees**:

* The 20% editor/request floor bounds input; it does not prove minimum physical
  airflow, prevent every possible stall, or validate any model's thermal envelope.
* hp-wmi's pwm readback is derived from active RPM. Immediate fixture-file echo
  checks do not establish physical settling or fan accuracy.
* SIGKILL cannot run a signal handler or destructor. The kernel's keep-alive may
  renew a direct manual target after its userspace daemon dies. No 120-second
  fallback is promised; direct manual/curve controls remain blocked. The separate
  coarse-mode writer has its own lease and recovery service, but physical recovery
  has not been verified.
* Firmware safeguards, shutdown, reboot and suspend/resume behavior have not been
  independently tested on a fan-control-capable machine.
* The CPU/GPU guard is fixture-tested; provider coverage, CPU control-value
  scales, model-specific curves and thermal stability remain unvalidated.
* A blocked or panicked controller cannot execute its own recovery. The
  experimental coarse-mode writer runs in an independent systemd cgroup and
  passed whole-controller-cgroup failure tests in a VM. Physical recovery,
  suspend/resume and thermal response still block a hardware release.

## Recovery procedures

In this default monitoring build, stopping the app requires no hardware rollback:
`systemctl stop omen-linux.service` (administrator) or close the desktop window.

If experimental **profile** testing was authorized, record the exact prior PPD
profile before the test and restore it with `powerprofilesctl set PREVIOUS`.
Verify with `powerprofilesctl get`; if denied, use the desktop's authorized power
settings. Do not assume an interrupted multi-step transaction restored itself.

The experimental Automatic/Maximum path uses a recovery helper pinned to the
exact driver/board/controller and a root-owned lease. It avoids unrelated hwmon
devices and retains intent when it cannot verify Automatic. Its five-second
physical trial procedure is documented in `COARSE_FAN_VALIDATION_PLAN.md`.
Physical crash/restart, suspend/resume, partial failures, device loss and
external-controller takeover still need dedicated hardware tests before release.
Do not use a wildcard shell loop.

## Policy import

User backups cannot enable service automation. Service backups use a separate
admin-authorized operation and destination, validate exact advertised profile
targets when automation is enabled, and fail if profile control is disabled. The
GUI previews automation before applying; CLI service import requires an explicit
`--apply-service-policy` argument after inspection. Saving a live fan/lighting
policy does not enable the blocked hardware adapters or apply startup restoration.

## Launch-bound profile changes

Launcher preparation happens before profile mutation. The shared desktop
controller records the prior state, verifies the requested profile and attempts
recovery after spawn/early-status failure, including a failed write that may have
partially applied. It preserves a different profile observed before rollback and
reports unknown/rejected recovery explicitly. Launch preparation/spawn refuse
root; privileged IPC still never accepts game commands.

Manual and automatic profile operations are serialized against current service
policy. Battery automation blocks manual profile requests and bound launches.
Profile readback waits behind a write already holding that guard.

These are fixture-tested safeguards, not new live-hardware validation. There is
no atomic global lease or guaranteed cancellation/rollback after a client crash
or timeout. External changes between read and restore, including same-target/ABA
changes, cannot be fully detected. Game exit and late launcher failures do not
restore a successfully submitted profile.

## Desktop status window

The optional overlay consumes existing monitoring snapshots and user-local
preferences only. It cannot mutate fans, profiles or other hardware. It retains
all missing/stale/source states and does not supply a thermal-policy input.
Its native/tray/portal toggles require no privileged daemon operation.

## Revision 13 exact-target experimental writer

The default live backend is unchanged and read-only. A separately managed static
service now pins the sole hp-wmi mode attribute for 8C30/F.10/7.0.0-38-generic.
It has its own cgroup, root-private endpoint, trusted executable/process lease,
durable intent and restart recovery. Explicit target opt-in is required, Maximum
is limited to ten seconds, and manual/curve access is refused. No physical write
or host installation has been performed. See `COARSE_FAN_VALIDATION_PLAN.md` for
the separate approval scope. Earlier statements that all adapters reject live
writes describe default deployment; this explicitly opted-in writer is separate.

Revision 14 supplies NVIDIA control telemetry through a separate DynamicUser
service with no capabilities, read-only system/sysfs and only nvidiactl/nvidia0
device permissions. It accepts no command/path from clients, only sends the
fixed query result. Root-only socket activation is the provider trust boundary;
desktop caches cannot be substituted. All units remain inactive unless explicitly
started. Physical provider and actuator checks still require host-trial approval.
