# Development thermal guard

Revision 0.1.0-9 replaces the fixture curve loop's CPU-label heuristic with
recognized sensor identities, per-device GPU coverage, acquisition checks and a
pinned sensor cohort. It separates fan ticks from profile work and retries failed
Automatic recovery while the service records an attempted override. All live fan
writes remain disabled. This is fixture-tested infrastructure, not a validated
thermal controller or a new hardware support grant.

## Inputs and device identity

CPU candidates must be hwmon temperature inputs from `k10temp` or `coretemp`.
Display labels cannot qualify ACPI, storage or battery sensors as CPU sensors.
Every present candidate needs a finite, available value and a recent acquisition
timestamp. Invalid temperature files retain an unavailable sample, so one broken
channel cannot silently disappear behind cooler channels.

Linux documents [k10temp's Tctl](https://docs.kernel.org/hwmon/k10temp.html) as a
control value that is not universally a physical temperature, and
[coretemp's per-core/package readings](https://docs.kernel.org/hwmon/coretemp.html)
with model-dependent limits. The development maximum-of-inputs policy and 0–150
plausibility interval do **not** establish comparable physical scales, critical
temperatures or a safe curve for an arbitrary CPU. Calibration remains required.

GPU coverage uses discovered PCI functions individually. An empty readable
inventory differs from a failed/partial inventory. Class/vendor read errors mark
it incomplete; that cannot authorize CPU-only cooling. Each non-suspended GPU
needs a settled power state and its own recognized temperature provider:
AMD/amdgpu hwmon or NVIDIA/nvidia-smi. Missing device-specific readings refuse the
curve even when another GPU is cool. AMD/Intel vendor IDs do not establish
integrated/discrete classification.

Optional `SensorSample.device_id` records the provider-associated PCI function.
Hwmon association uses the canonical device link; the NVIDIA batch is tagged
with the same explicit PCI selector used by the query. Labels cannot supply this
identity. Older JSON without the field decodes with unknown identity. An older
missing `GraphicsStatus.inventory_complete` likewise defaults to false.

The kernel documents [amdgpu's temperature channels](https://docs.kernel.org/gpu/amdgpu/thermal.html).
Runtime-suspended AMD hwmon devices are skipped before acquisition. Suspended
PCI GPUs need no temperature while suspended, but remain in the pinned
inventory. Waking or changing topology/state stops the curve and requires rearming.

The NVIDIA provider still queries only its first bound device and retains its
five-second monitoring cache. Another active NVIDIA GPU, or a cached reading
outside the guard's stricter age limit, cannot be covered by a different sample.
PrivateDevices normally hides NVIDIA nodes from the privileged service. The
desktop's local monitoring supplement is **not** ingested into this control
loop. This change neither relaxes the sandbox nor supplies adequate privileged
NVIDIA control telemetry.

## Timing and cohort

Development limits in `crates/daemon/src/thermal.rs` are two seconds for sample
age/acquisition duration and three seconds between guarded ticks. These are
fixture-policy choices, not verified hardware deadlines. Zero, future, stale and
implausible samples fail. Wall-clock discontinuities can stop a curve rather
than make older data appear fresh.

Scheduling gaps use [CLOCK_BOOTTIME](https://man7.org/linux/man-pages/man3/clock_gettime.3.html),
which includes suspended time. CPU/GPU sensor identities and GPU states are
pinned when arming. Missing/additional channels, changed associations, changed
GPU state or a long gap stop the curve. Returning data does not resume it
automatically; explicit rearming and a valid preflight are required.

Acquisition timing is not proof of physical freshness. Kernel/firmware caching,
stuck plausible values, missing hardware outside the initial inventory, CPU
package coverage and provider accuracy still require validation. Reads are not
atomic; topology changes or suspend can occur after validation or during a
driver write. Gap detection does not prevent every in-flight stale write or
establish physical suspend safety.

An indefinitely blocked or panicked fan worker cannot perform its own recovery
ticks. Revision 10 adds a separate writer for ordinary-file fixtures, documented
in `FAN_GUARDIAN.md`; a production hardware supervisor remains unimplemented.

## Recovery and scheduling

Preflight precedes a new override attempt. A failed first preflight does not
claim or restore an external setting. Once a write is attempted, partial
failures remain owned for recovery. Guard/write failure stops the curve and
attempts Automatic. Failed recovery remains owned and is retried on later fan
ticks, without resuming the curve.

Writes, guard state and recovery share the mode mutex. The named `fan-policy`
worker ticks independently of `power-policy`; slow profile I/O cannot occupy
that same worker. Configuration waits precede thermal acquisition and its gap
check, so queued config IO cannot age an already-validated sample before writing.
Shutdown restores owned fan policy before waiting for profile
I/O. Automatic profile work rechecks shutdown after a potentially blocking read.

Daemon diagnostics add `fan_policy`: requested mode, guard/cohort, attempted
ownership, last guard/write error and recovery error. This differs from `fan`,
which reports kernel policy/RPM. Logs describe verified policy readback rather
than asserting restored physical cooling. Direct read-only mode has no service
ownership state.

Ownership is not a cross-project lease, and process detection remains heuristic
and racy. The kernel keep-alive can outlive userspace. Revision 10's fixture
writer covers a stopped/killed parent, but shares its cgroup and cannot recover
after simultaneous death. Revision 11 also guards fixed Manual/Maximum preflight and every heartbeat
renewal with the same CPU/GPU coverage, freshness, cohort and gap checks. It
does not establish a model-specific safe fixed duty or thermal trip policy. All live
device-specific writes remain blocked by the gates in `HARDWARE_SAFETY.md`.

## Validation boundary

Sixteen additional Rust cases cover source selection, GPU identity/coverage,
invalid/stale/future inputs, cohort/scheduling changes, recovery retry, partial
PCI discovery, unavailable channels, suspended AMD monitoring and older JSON.
CPU-only fixtures explicitly provide an empty PCI inventory; they do not
certify a physical OMEN model.

`tests/thermal_guard.py` runs eleven production IPC scenarios with private fixture
files. It checks GPU heat, lost data, preflight, cohort changes, a real
SIGSTOP/SIGCONT gap and recovery after pinned controller identity returns. It then
blocks the named power worker on a fixture FIFO, observes continued GPU-driven
fan ticks, and verifies Auto restoration before releasing profile I/O during
shutdown. File echoes and a FIFO do not emulate firmware, physical fan settling,
thermal stability or S3 suspend.

Results and daemon hashes: `evidence/thermal-{debug,release}.json`. No physical
fan test or workload is authorized by these results. A first live fan test still
requires a concrete reviewed procedure and separate approval.


Revision 11 adds three Rust cases (each covering Manual and Maximum) and two
production fixed-mode scenarios per build. Invalid GPU association refuses the
initial override without restoring an external setting; later invalid GPU data
restores owned Automatic and requires explicit rearming. Unit fixtures also
cover scheduling gaps and changed GPU inventory in both modes. These checks
still use ordinary files, not firmware, and no physical release gate changes.

## Revision 13 mode-only transport

The independent coarse writer keeps the controller's existing fresh CPU/GPU
preflight, cohort checks and suspend-aware renewal checks. It accepts no thermal
samples from clients and no manual/curve intent. Its own heartbeat verifies that
Maximum remains selected, and its hard ten-second ceiling cannot be renewed.
Active NVIDIA without daemon-visible telemetry remains a preflight failure; no
GUI cache or broad device access was added. This is a bounded physical-trial
preparation, not a validated sustained thermal policy.

Revision 14 adds an explicit isolated NVIDIA reader for the experimental coarse
controller. It stamps acquisition start, rejects queries longer than 1.5 seconds,
and retains timestamps through the protected transport and ordinary sensor cache.
The controller validates fresh PCI-identified NvidiaSmi temperature samples from
the root-private PID-1 socket; it rejects fixture frames on live paths. No user
cache is accepted and PrivateDevices is unchanged. Reader service loss is a
thermal coverage failure, leading to Automatic through independent recovery.
