# Complete the remaining OMEN Linux implementation

Work in `/home/papan/Desktop/Projects/omen`.

Continue the project described in `prompt.md`. Implement the remaining useful,
supportable functionality and close the outstanding validation gaps. This is a
request for implementation and verification, not another architecture proposal.
Preserve the existing native Rust, GTK4/libadwaita, CLI, hardware abstraction,
authenticated IPC, systemd and polkit architecture.

Do not start from scratch, repeat completed setup, or present fixture behavior
as working hardware integration. Make concrete progress on independent work
while an external artifact, compatible device or user action is unavailable.

## Read the actual project state first

Read applicable `AGENTS.md` files and relevant installed skills, followed by:

- `prompt.md`
- `docs/IMPLEMENTATION_STATUS.md`
- `docs/evidence/validation-summary.json`
- `docs/ARCHITECTURE.md`
- `docs/HARDWARE_SAFETY.md` and `docs/HARDWARE_COMPATIBILITY.md`
- `docs/THERMAL_GUARD.md` and `docs/FAN_GUARDIAN.md`
- `docs/GAME_LAUNCHERS.md` and `docs/GLOBAL_SHORTCUTS.md`
- `docs/TEST_REPORT.md`, `docs/PACKAGE_VALIDATION.md` and `docs/VM_VALIDATION.md`
- `docs/REVERSE_ENGINEERING_REPORT.md`, `docs/WINDOWS_ARTIFACTS.md` and
  `docs/OMENCORE_REVIEW.md`

Inspect the corresponding source and tests before changing behavior. Build a
short requirements checklist that distinguishes implemented code, fixture-tested
behavior, verified physical behavior, unsupported interfaces and missing external
evidence. Treat the files and fresh observations as authoritative if they differ
from this checkpoint.

## Checkpoint to preserve

- Current development package: `target/debian/omen-linux_0.1.0-10_amd64.deb`.
  Size: 2,678,566 bytes. SHA-256:
  `12ee20d102c7c129eb080564805bbaaf150d1e20c8cfb9623503e16bba2b7400`.
- Both debug and release passed 186 Rust tests, eight guardian process scenarios,
  nine thermal scenarios and eight fan-monitoring scenarios. The runtime passed
  19 container checks and 17 systemd VM checks. Preserve exact binary/package
  fingerprints when reporting these results.
- Revision 10 did not rerun every UI workflow. Thermal provider attribution is
  retained from revision 9, fan captures from revision 8, and broader native
  settings/tray/shortcut/network workflow evidence from revision 7 or earlier.
- The reference device is HP Victus board `8C30`, BIOS `F.10`, observed on kernel
  `7.0.0-38-generic`. Recheck current read-only identity before relying on it.
  Its observed HP hwmon interface has coarse mode reporting and lacks manual
  `pwm1`/`pwm2` targets. This does not qualify it for custom curves.
- The only physical control transaction completed was the separately approved
  PPD Performance → Balanced → Performance test. Both independent readbacks and
  restoration passed. PPD CPU-profile switching is not proof of HP firmware
  thermal-profile parity.
- REA 6.8.0 was connected and used. The approved setup uses existing Node
  24.22.1 and the matching skill. No analysis engines were installed. Do not
  repeat setup or modify agent configuration without a concrete need and approval.
- The host `.deb` and systemd service are not installed. The previously approved
  polkit action remains installed. An unrelated user mock daemon, previously
  PID `2124598`, must not be stopped or repurposed. Track and clean up only
  processes, services and paths created by your own tests.
- The earlier goal runner reached `usageLimited`; that is not project completion
  and does not authorize changing its status or creating duplicate goals.

## Authorization and hardware boundaries

Proceed autonomously with source changes, local builds, read-only discovery,
private fixtures and isolated container/VM tests. Do not repeatedly request
permission for that work or for already completed approvals.

This prompt does not authorize new physical fan, RGB, charging, graphics, power
limit, EC or firmware writes; stress workloads; host suspend/reboot; host package
or service installation; analysis-engine installation; or agent configuration
changes. Before a first test in a new physical-control category, complete the
code, fixtures, recovery path and a concrete reviewable test procedure, then
request the specific approval. The completed profile test is not blanket
authorization for additional profile experiments.

Never invent WMI commands, EC addresses, device protocols or compatibility.
Unknown devices stay read-only. Never disable firmware thermal protections or
enable experimental controls merely because a sysfs file is writable. Keep the
GUI and game processes unprivileged. Do not widen the daemon sandbox to obtain
generic hardware access.

## Workstream 1: production ownership and fan recovery

The current guardian writes only ordinary fixture files. It rejects sysfs and
other pseudo-filesystems and shares the daemon's service cgroup. It is not a
production hardware recovery service. Do not enable live fans by simply removing
that filesystem check.

Implement and test a narrowly scoped production ownership/recovery design:

1. Independently supervise recovery so killing the controller's entire cgroup
   does not also remove its only recovery mechanism. Preserve least privilege
   and the default read-only monitoring service.
2. Bind any hardware lease to the exact verified controller, interface, board,
   BIOS and relevant kernel support. Validate peer/process identity and pinned
   actuator identity. Do not accept caller-selected arbitrary paths or sweep
   unrelated hwmon devices.
3. Serialize mutations and recovery. Expired ownership must reject delayed writes
   and require explicit rearming. Cover partial writes, stale/replayed commands,
   competing controllers, missing/replaced devices and ownership transitions.
4. Cover fixed Manual/Maximum as well as curves. Confirm suitable CPU/GPU provider
   coverage, freshness, units and model-specific thermal policy. A plausible
   cached value or label match is insufficient. Preserve NVIDIA sandbox/cache
   limitations until actually resolved.
5. Exercise parent death, guardian death, controller-cgroup termination, stalled
   workers, protocol failure, service restart and simulated suspend gaps in
   isolated environments. State the behavior when both processes fail or kernel
   I/O stalls; do not promise recovery the implementation cannot guarantee.
6. Keep physical fan release blocked until approved tests verify actual response,
   settling, safe Automatic restoration, thermal behavior and suspend/resume on
   a compatible machine. Kernel policy/readback and fixture echo are distinct
   from measured airflow and physical safety.

Expose coarse Auto/Maximum and manual/curve capabilities separately when evidence
justifies them. Do not make missing manual PWM targets appear supported, and do
not conflate CPU power profiles with firmware fan modes.

## Workstream 2: game lifecycle and profile restoration

Current launch handling detects early submission failures; it does not reliably
track actual game exit or restore profiles after a client crash.

- Implement explicit profile ownership that handles concurrent launches,
  multiple clients, battery automation, cancellation, service loss and external
  profile changes without overwriting another owner's decisions.
- Track the actual supported game lifecycle. Steam/Heroic/Lutris launchers may
  detach; launcher exit or a fixed timeout does not prove game exit. Use verified
  launcher interfaces or reliable process identity, and represent uncertainty.
- Restore the original profile only under a documented ownership policy. Add
  recoverable handling for GUI/CLI crashes and abandoned leases. Keep games
  unprivileged and use fixed argument vectors.
- Support native and Flatpak routes where verified. Improve missing launcher
  formats and title metadata when primary sources justify the implementation.
- Add fixture process coverage for normal exit, failed launch, detached launch,
  client death, overlapping games and an externally changed profile. Use real
  games only in an explicitly agreed validation session.

## Workstream 3: overlay and desktop integration

- Implement a lightweight, opt-in status overlay using the existing monitoring
  data and capability model. Include useful metrics, clear unavailable/stale
  states, persistence and a reachable close/toggle action.
- Integrate with the existing tray and opt-in shortcut controller. Avoid duplicate
  high-frequency polling, unnecessary helper processes and focus stealing.
- Handle actual Wayland/X11 restrictions honestly. Verify any additional desktop
  protocol or dependency before using it; do not promise universal global
  placement or key behavior.
- Validate physical shortcut activation, compositor behavior, tray recovery,
  scaling, keyboard navigation, accessible names/focus and contrast on the target
  desktop where available. The user requested English only; additional translations
  are outside scope.
- Use the relevant UI skill for visual changes. Inspect bounded app-only captures
  and native interactions; do not repeatedly recapture unchanged pages or expose
  unrelated desktop content.

## Workstream 4: remaining hardware adapters

Investigate RGB/lighting, charging thresholds, graphics selection/MUX and any
remaining performance controls against actual upstream interfaces and exact
device evidence.

- Implement verified adapters with capability detection, typed validation,
  authorization, state readback, error reporting and appropriate recovery.
- Distinguish single-zone, four-zone and per-key keyboards. Unknown protocols
  remain unavailable. Do not copy HP assets or unverified register tables.
- Expose charging thresholds only when a verified supported interface exists.
- Distinguish per-application GPU selection from a firmware MUX switch. Show any
  verified logout/reboot requirement; never switch or reboot automatically.
- Do not translate PPD settings into unsupported claims about HP firmware,
  CPU/GPU power limits, overclocking or undervolting.
- Investigate optional traffic priority only if practical through an established,
  narrowly authorized Linux mechanism. Preserve existing network configuration
  and recovery; otherwise document why the feature is unavailable.

A polished, explicit unsupported state is required where the target lacks an
interface. Implementing a mock adapter does not grant live support. Prepare a
separate exact-target approval procedure for each first physical-control test.

## Workstream 5: remaining Windows reference analysis

Use authorized official HP/Microsoft distribution or user-supplied artifacts to
obtain the main Gaming Hub package and, if available, a compatible Windows
reference system. Existing official HP SDK analysis is not main-app parity.

Record versions, architecture, hashes, publisher/signature evidence, component
relationships, dependencies and service boundaries. Use the installed REA skill
and tools appropriate to the actual artifact; ordinary Rust source analysis does
not require REA. Do not assume an implementation language. Preserve evidence for
specific conclusions and separate source inference from observed behavior.

The canonical OmenCore repository is `theantipopau/omencore`; the existing pinned
review distinguishes development commit `840f03d58ddd4c3a709780b6d635828d9ef3d87c`
from release assets. Preserve license attribution and do not copy incompatible
code. Prefer primary documentation and source when refreshing research.

If artifacts, a Windows system or an optional native analysis provider are
unavailable, record the exact limitation and continue other implementation.
Request engine-installation approval only when a concrete artifact and analysis
question justify it. Do not repeatedly redo the completed REA setup.

## Workstream 6: deployment and validation

- Test graphical polkit success, denial, cancellation, authorization expiry and
  relevant active/inactive-session behavior. Distinguish mocked/container grants
  from real graphical authentication. Never collect or type user credentials.
- Test the packaged application in a clean graphical Ubuntu/Kubuntu environment,
  including native library dependencies, systemd sandbox, tray/portal behavior,
  configuration migration, install/upgrade/remove/purge and recoverable failures.
- Preserve the known `0.1.0-1` legacy upgrade limitation; do not claim the old
  package's running prerm preserves enabled state.
- Cover supported suspend/resume, shutdown/reboot and battery transitions in
  isolated environments first. Physical-host validation remains a separate
  approved step and cannot be inferred from a VM.
- Measure startup, idle CPU/RSS and monitoring overhead with current binaries and
  explicit methodology. Existing measurements are warm, short-window observations.
  Do not drop the host's global caches or call warm results cold-start data.
- Run meaningful tests appropriate to changed behavior, formatting, strict Clippy
  and debug/release workspace tests. Run affected native workflows when UI or
  desktop integration changes. Run final package/container/systemd-VM checks for
  the final runtime; retain hashes and cleanup evidence.
- Preserve historical results with their original scope. Documentation-only
  repackaging may use the existing verifier's exact runtime-equivalence checks.
  Do not repeatedly rerun unrelated full suites after checks pass without a new
  change, failure or unresolved concern.

The established local build environment is:

```bash
. /home/papan/gtk-prefix/env.sh
export LD_LIBRARY_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --release --workspace --locked
OMEN_SHLIB_PATH=/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu packaging/build-deb.sh
```

Recheck these paths before use. Do not reassign `HOME` or `CODEX_HOME`; use the
project's scoped config/environment overrides. Existing container/VM scripts
support `--apt-mirror https://mirrors.edge.kernel.org/ubuntu`; preserve archive
signature checks. Never kill unrelated processes or retain private VM keys.

## Execution and delivery

Prioritize usable missing software features and production ownership/recovery
over repeatedly rebuilding already verified milestones. Choose bounded, coherent
changes, implement them, test them and update the requirement checklist. Continue
independent work when one workstream needs external input. Ask concise questions
only for missing information or approval that actually blocks the next step.

Keep the application native, lightweight, local-first and free of mandatory
accounts, advertising and unnecessary telemetry. Preserve all nine existing
pages, real measurements, unsupported-state explanations and config compatibility.

Deliver the updated source, tests and a newly versioned `.deb` after runtime
changes; do not overwrite revision 10's evidence as though it tested new code.
Update implementation status, compatibility/safety documentation, test report,
installation examples and machine-readable validation summary. Report what
changed, how it was verified, exact artifact identity, supported target scope,
host changes and remaining external blockers.

Do not call the full goal complete merely because a UI, fixture suite or package
build passes. Clearly distinguish implementation completion, hardware support,
physical validation and Windows parity. If an unavailable artifact, unsupported
interface or missing physical authorization prevents full completion, state the
precise remaining requirement and deliver all independently completable work.
