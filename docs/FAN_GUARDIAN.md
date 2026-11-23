# Independent fixture fan writer

Revision 0.1.0-10 puts the mock daemon's fan writes in a separate process.
If the daemon stops renewing its lease, that process attempts Automatic using
the exact ordinary-file descriptors it inherited. This is development recovery
infrastructure. **Live fan writes remain disabled.** No physical fan, firmware
fallback, temperature limit or suspend behavior is validated by this work.

## Ownership and transport

`--mock-omen` wraps its hardware backend in `SupervisedBackend`. The raw mock
adapter is still used by in-process unit fixtures; the normal daemon routes
every mock fan write through its writer. The real sysfs backend has no writer
implementation and fails closed if asked to begin an independent lease.

An explicit control request starts the writer before acquiring the curve's
thermal frame. Starting it does not claim an override or change a file. A failed
first thermal preflight therefore preserves an external fixture setting.

The daemon starts its own executable with `--internal-fan-guard`, an empty
environment and fixed inherited descriptors: a private Unix socket plus mode,
two duty targets and controller name. There is no public guardian listener or
path in the wire protocol. Startup checks peer credentials. The executable has
no setuid installation or new capability. Invoking the internal entry point
without its descriptors fails before touching an actuator.

Requests have consecutive sequence numbers and bounded JSON frames. Supported
operations are Apply, Pulse and Status. Curve intent is calculated by the daemon;
the writer receives only a concrete Auto, Maximum or bounded Manual target.
Malformed, oversized or out-of-order requests revoke ownership. Status reads
never renew a lease. Heartbeats revalidate the pinned controller before renewal.
Fixed Manual/Maximum modes receive heartbeat pulses (revision 11 validates
their CPU/GPU thermal coverage/freshness/cohort before each renewal);
successful curve writes renew their lease.

## Expiry and restoration

The development lease is four seconds, measured with CLOCK_BOOTTIME, including
suspended time. The writer polls its private stream with a 250 ms read timeout.
These are fixture settings, not verified hardware response deadlines or a
real-time scheduling guarantee.

Expiry is checked before accepting another command. Revocation attempts
Automatic and refuses subsequent overrides or pulses, including commands from
a resumed daemon. Rearming requires a fresh writer and new preflight. Recovery
failure remains explicit and is retried while that writer is running. Channel
closure, SIGTERM or SIGINT also attempt recovery; closing recovery retries for
up to three seconds and exits with failure if Automatic cannot be confirmed.
No file I/O runs inside signal handlers.

The daemon tracks attempted ownership separately from the writer's lease. A
writer may restore a fixture before its daemon receives an acknowledgement.
Diagnostics expose `fan_supervisor` (PID, last acknowledged lease/revocation
state and recovery error), alongside `fan_policy` and observed `fan` telemetry.
Supervisor status is cached protocol state, not a physical fan measurement.

## Restricted actuator

The helper accepts owned regular files without group/other write permission
on explicitly allowed ordinary Linux filesystems. It rejects sysfs, procfs,
device nodes and other pseudo-filesystems before writing. Opens reject final
symlinks. Original descriptors remain pinned across helper replacement;
device/inode identity, link presence and the controller name are checked before
an operation. An unlinked descriptor cannot be redirected by recreating its path.
Writes verify ordinary-file echo and enforce the 20% request floor.

This fixture echo is not equivalent to hp-wmi's RPM-derived PWM readback. The
helper cannot establish physical fan settling or minimum airflow. In particular,
the observed 8C30/F.10 device lacks manual PWM targets and gains no support here.

## Process evidence and remaining release gates

`tests/fan_guardian.py` runs eight production process checks per build: healthy
lease renewal; Automatic while the parent remains SIGSTOPped; refusal of stale
renewal followed by explicit rearming; ordinary shutdown; parent SIGKILL;
writer SIGKILL with a surviving parent; pinned-actuator loss; and invalid internal
startup. It becomes a child subreaper and cleans up only its own fixture children.
Results and binary hashes are in `evidence/fan-guardian-{debug,release}.json`.
Rust tests also cover sequence errors, idle/unowned revocation, recovery retry,
deadline extension, request floors and rejection of pseudo-filesystems.

The processes have separate process groups but **share the service cgroup**.
Systemd's whole-cgroup termination, simultaneous writer/parent death, a hung
writer, kernel I/O stalls and loss of the controller can prevent recovery.
This phase installs no separate recovery service, durable root-owned hardware
lease or privileged writable sysfs mount. The production service retains its
read-only sysfs sandbox.

A physical release still requires a narrowly targeted, separately supervised
hardware implementation and tests for whole-service failure, competing writers,
partial writes, suspend/resume, firmware behavior and physical cooling. The
reference machine's GPU telemetry and CPU temperature-scale limitations in
`THERMAL_GUARD.md` also remain unresolved. A live fan test requires a concrete
reviewed procedure and separate approval.

## Mode-only fixtures (revision 12)

The fixture guardian now accepts absent PWM targets using pinned read-only name
descriptors as sentinels. It derives that restriction from descriptor identities,
refuses manual mode before writing and changes only the mode file for Auto/Max.
The filesystem rejection and shared-cgroup limitation remain. A mode-only process
scenario covers lease renewal, stalled parent expiry and parent SIGKILL. This is
not an independent production hardware recovery service.

## Revision 13 independent coarse writer

`coarse/` implements a separate Automatic/Maximum service, not a relaxation of
the ordinary-file guardian. The packaged template is static, has a separate
cgroup and exposes only the exact canonical mode file writable. Leases bind
process/executable/cgroup identity, pinned target, token and ordered commands.
Durable intent precedes Maximum; restart restores Auto before accepting work.
Late or invalid commands revoke ownership; missing/replaced hardware retains
intent without redirecting writes. A ten-second ceiling bounds the first trial.
The current evidence summary distinguishes process fixtures, systemd VM ordinary
files and absent physical validation. Blocked kernel I/O or loss of the systemd
recovery environment cannot be guaranteed recoverable.
