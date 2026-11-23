# Fan monitoring and control boundaries

Revision 0.1.0-8 reads hp-wmi's reported fan policy independently of manual
target support. On the reference 8C30/F.10 Victus, `fan1_input`, `fan2_input` and
`pwm1_enable` exist while `pwm1` and `pwm2` do not. Previously the backend
discarded the readable policy when it could not construct a manual controller.
It now reports Auto or Maximum through the existing CLI, IPC and GUI state.
No protocol field, write allowlist or live-control opt-in was added.

## Meaning of the readings

The independently reviewed
[hp-wmi source](https://github.com/torvalds/linux/blob/ee02ed6308fbbd851c4e5c1f642d029617049a12/drivers/platform/x86/hp/hp-wmi.c)
defines `pwm1_enable` as 0 for Maximum, 1 for Manual and 2 for Auto. Manual target
attributes depend on board callbacks; the mode attribute can exist without them.
OmenCore's separate coarse-mode classifier is supporting prior art, not the
authority for these kernel values; see `OMENCORE_REVIEW.md`.

The shared read-only reader in `crates/hardware/src/fans.rs` observes policy and
RPM without constructing or granting a write controller. Missing, malformed or
unknown mode values produce an unknown mode while readable RPM remains present.
Because the existing Manual wire variant requires a duty reading, a missing or
invalid duty also leaves that mode unknown. It never fabricates a 0% duty.
Out-of-range raw PWM telemetry is unavailable instead of being clamped into an
apparently valid 100% reading. Write verification also rejects out-of-range
readbacks before normalization, including on the second channel. A valid
measured zero remains a valid observation.

The legacy `duty_pct` field normalizes the readable `pwm1` value. In hp-wmi that
value is derived from active RPM using firmware bounds. It is neither a command
echo nor a measurement of electrical PWM duty. The mode is the kernel's reported
policy, and attributes are read separately; the snapshot is not atomic. Neither
a policy readback nor an RPM sample proves physical response, thermal stability
or restoration of firmware cooling. RPM calibration remains unverified.

## Control gating and conflicting processes

The reader is separate from manual controller selection. Even in fixtures, a
mode-only board cannot apply Auto/Maximum through the manual-control API. The
real backend still rejects all device-specific writes before reaching any
adapter. Changing monitoring does not enable Auto, Maximum or custom curves on
the physical reference laptop.

The known-process inventory now includes `omencore-cli`, the executable used by
the canonical project's `omencore-cli daemon --run` service. Known full names
are matched against Linux's 15-byte `comm` representation, preserving detection
of the truncated `omen-space-daemon` name. A detected competitor blocks the
manual adapter before mutation and disables curve availability.

This is a conservative process-name check: an OmenCore CLI instance performing
only monitoring is also treated as a possible writer. The application never
stops that process. Renamed executables, interpreter hosts, hidden/unreadable
processes and other unlisted controllers may escape this inventory. Processes
can appear after a scan. It does not create a cross-project lock or establish
exclusive hardware ownership. Live fan release still requires the physical
response, suitable thermal telemetry, interruption and independent recovery
checks in `HARDWARE_SAFETY.md`.

## Validation

Eight new Rust cases cover mode-only reads, invalid/missing mode and duty data,
absence of a write grant, conflict rejection before mutation, Linux name
truncation, unrelated process entries and rejection of invalid write readbacks
on either channel. Fixture files are ordinary temporary
files; these checks do not emulate firmware.

`tests/fan_monitoring.py` exercises the actual daemon, protocol and CLI with
private Victus/manual-controller fixtures. It checks mode transitions, missing
interfaces and invalid readings, denied writes on mode-only fixtures, and a
synthetic OmenCore process that blocks a fixture adapter without changing its
policy/target files. The `--live-readonly` option additionally compares the real
daemon and direct CLI with independent mode-file reads on the reference HP
machine. It sends only read operations to the real daemon and generates no load.
Results and binary hashes are in `evidence/fan-monitoring-{debug,release}.json`.

`scripts/capture-fans.py` captures the application's own GTK widget tree using
explicit Wayland and a private session bus. The reference read-only page and a
mode-only Maximum fixture are recorded in `evidence/fan-captures.json`. The
fixture is visibly labelled; no host tray, portal or desktop framebuffer is
used. The first direct capture changed to Settings and was rejected, with its
record retained separately; the isolated capture checks for unexpected page
changes. A `dbus-run-session` attempt timed out; the final capture instead owns a
minimal bus without automatic service activation and confirms cleanup. These
captures do not validate physical fan control.

The live comparison validates the reported kernel policy and the read-only
control boundary. It does not test a physical fan transition, either fan's
cooling effectiveness, forced-kill recovery, suspend/resume or thermal safety.
