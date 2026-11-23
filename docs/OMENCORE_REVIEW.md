# OmenCore source review

Reviewed on 2026-10-26. This is third-party source evidence, not an execution of
OmenCore, a Windows Gaming Hub trace, or a hardware compatibility certification.

## Source identity and scope

The canonical project source is
[theantipopau/omencore](https://github.com/theantipopau/omencore).
[The project website](https://omencore.info/) links to that repository, and the
repository links back. GitHub metadata agrees. The supplied `github.com/omencore`
account did not establish this identity by itself; the earlier unresolved-source
finding is superseded. This establishes project identity, not HP affiliation.
The observed links, page hashes, HTTP results and timestamp are retained in
`evidence/omencore-canonical.json` and `omencore-repo.json`.

Analysis is pinned to commit
[`840f03d58ddd4c3a709780b6d635828d9ef3d87c`](https://github.com/theantipopau/omencore/tree/840f03d58ddd4c3a709780b6d635828d9ef3d87c),
dated 2026-10-26. The recursive tree contains 1,280 entries and is not truncated.
Forty-two selected source, test, documentation and packaging files were retrieved;
each file's size and Git blob identity were checked against that tree, and its
SHA-256 is recorded in `evidence/omencore-sources.json`. This is a targeted review,
not an audit of every repository file. No target program, installer, service,
driver or test suite was executed or installed.

The latest-release API snapshot reports
[v4.5.0](https://github.com/theantipopau/omencore/releases/tag/v4.5.0), published
2026-10-24, while the reviewed Linux project declares development version 4.5.1.
The source findings below therefore do not certify the behavior of the v4.5.0
binary assets. Release metadata and the complete tree snapshot are retained
alongside the source manifest.

## Interfaces, models and behavior

| Area | Source-backed finding | Consequence for omen-linux |
|---|---|---|
| Windows integration | `HpWmiBios.cs` uses the HP `hpqBIntM`/`hpqBIOSInt` WMI interface. `FanControllerFactory.cs` selects among distinct control backends | Third-party Windows code helps identify investigation targets; it does not establish the original Gaming Hub's calls or a Linux ABI |
| Linux implementation | `OmenCore.Linux.csproj` builds a .NET 8 CLI named `omencore-cli`; a separate Avalonia service adapter exists | This is inspectable source, not an unidentified native binary. No analysis engine is needed for this review |
| Capability detection | `LinuxCapabilityClassifier.cs` separates manual fan targets from coarse hwmon mode/profile access and treats reported board 8BCD ACPI failures as degraded | Detect actual attributes and validate physical response. An upstream classification or root access alone cannot grant our write support |
| Fan/profile selection | `LinuxEcController.cs` tries HP thermal interfaces, then ACPI profile/hwmon paths, and retains a legacy raw-EC fallback | Use the established kernel ABI. Its EC offsets and board assumptions are not imported into our backend |
| Coarse modes | Its ACPI/hwmon branch requests Auto/Maximum through `pwm*_enable`; other named presets require a profile interface | Mode-only access is distinct from manual duty control. The same distinction is independently established by the pinned hp-wmi source |
| Monitoring | `LinuxHwMonController.cs` ranks CPU die sensors ahead of generic ACPI zones and permits an explicit preference; telemetry resolution and NVML are separate helpers | A readable temperature is not necessarily a suitable thermal-control input. Source choice, unavailable data and freshness need their own validation |
| Lighting | `LinuxKeyboardController.cs` probes multiple driver-specific zone and LED paths; `LinuxFourZonePayload.cs` handles one four-zone text format. Linux per-key control remains explicitly unsupported there | Matching paths and formats must be tied to a known driver. A model substring or one four-zone protocol does not establish per-key support |
| Battery | `LinuxBatteryController.cs` reads power-supply state with a ten-second cache | This status reader is not evidence of a supported charging-threshold write on our machine |

File-level links and hashes are in the manifest. In particular, the Windows
[`KeyboardModelDatabase.cs`, lines 717–727](https://github.com/theantipopau/omencore/blob/840f03d58ddd4c3a709780b6d635828d9ef3d87c/src/OmenCore.Core/Services/KeyboardLighting/KeyboardModelDatabase.cs#L717-L727)
lists **8C30** as backlight-only with `UserVerified = false`. That is a
conservative upstream mapping, not independent confirmation of our 8C30/F.10
keyboard, a Linux lighting interface, or RGB functionality. Other named models
and issue reporters' boards must remain separate from our hardware-tested matrix.
`OmenBoard.cs` is a small HID lightbar helper, not a comprehensive support table.

## Fan ownership and recovery limits

[`WriterLock.cs`](https://github.com/theantipopau/omencore/blob/840f03d58ddd4c3a709780b6d635828d9ef3d87c/src/OmenCore.Linux/Hardware/WriterLock.cs)
uses an exclusive file handle with a separate information file. Its root and
unprivileged paths differ. This coordinates participating OmenCore writers;
releasing the handle after process death does not restore a fan setting or
exclude unrelated fan daemons. `FanWriterScanner.cs` additionally checks selected
process names without stopping them.

[`OmenCoreDaemon.ShutdownAsync`](https://github.com/theantipopau/omencore/blob/840f03d58ddd4c3a709780b6d635828d9ef3d87c/src/OmenCore.Linux/Daemon/OmenCoreDaemon.cs#L409-L435)
stops its curve engine and conditionally restores settings when `RestoreOnExit`
is enabled. The curve engine also calls its restore path when stopped. These are
normal execution paths, not evidence of SIGKILL, suspend or firmware recovery.
The packaged service uses `Restart=on-failure` with a five-second delay; a restart
policy alone does not prove safe cooling during or after an interruption.

The hwmon Auto path polls RPM and temperatures for a short window and can request
Maximum when cooling appears to have failed under high temperature. This is a
useful distinction between an accepted policy write and observed fan activity.
It does not prove sustained cooling, every fan's response, or successful fallback
on this Victus. The curve source also preserves the previous target when CPU and
GPU temperature data are both absent; that policy is not a model-validated
failsafe for our implementation.

For our pinned kernel, the delayed fan keep-alive can outlive a userspace process.
Consequently, neither a lock nor a successful `pwm1_enable` readback satisfies
our crash-recovery gate. This review authorizes no fan write and changes no
allowlist. Live fan controls remain disabled.

## Tests and issue evidence

The retrieved xUnit sources cover capability combinations, sensor ranking, sysfs
path maps, four-zone serialization, NVML absence, competing process names and
writer-lock behavior. `.github/workflows/linux-qa.yml` includes Linux build,
CLI smoke and test jobs. These files were inspected, **not run** here; CI
configuration is not a passing test result or physical hardware validation.

Issue snapshots were read as reports, without reproducing their hardware actions:

* [#126](https://github.com/theantipopau/omencore/issues/126), open in the snapshot:
  board 8BC2 was classified for control despite missing target files. The reviewed
  classifier now distinguishes coarse mode access and the reason for EC access;
  the older report is not proof that the current source has the same defect.
* [#214](https://github.com/theantipopau/omencore/issues/214), open: a reporter on
  8BCA describes a fixed ACPI temperature masking the CPU die temperature during
  curve operation. The reviewed ranking code addresses this source-selection
  class of error; field confirmation of the fix was not established here.
* [#183](https://github.com/theantipopau/omencore/issues/183), closed: a reporter on
  8D87 describes fans stopping after Max-to-Auto. Current source includes an Auto
  cooling check. Closure and source changes do not validate our board's recovery.
* [#60](https://github.com/theantipopau/omencore/issues/60), open: a reporter describes
  ineffective legacy EC control and EC fault indications on an OMEN Max. This is
  a reason to investigate firmware differences, not to reproduce raw writes.
* [#84](https://github.com/theantipopau/omencore/issues/84), open: a reported 8E35
  setup has fan telemetry and a mode file but failed writes, with a replacement
  driver in use. Attribute presence alone is insufficient evidence of control.

`omencore-open-issues.json` contains the 83 entries returned by the first
100-entry open-issues request, including any pull requests. It is a dated
snapshot, not an assertion about future issue state. Closed #183 and #135 have
separate snapshots. No quoted diagnostics or third-party model report is treated
as a measurement on the reference Victus.

## Implementation decisions and remaining gates

This review changes documentation and provenance only. It introduces no runtime
dependency, copied implementation, driver, control value or additional host
setup. The reviewed license is MIT; its hash is in the manifest. No OmenCore
source is redistributed in the application payload.

Before a first live fan test, our implementation still needs model-specific
settling checks, suitable CPU/GPU telemetry, a targeted independent recovery
mechanism, and fixture-tested interruption behavior. Revision 8's process-name
inventory now includes the verified `omencore-cli daemon --run` executable;
this remains a best-effort scan, and OmenCore's private lock does not coordinate
with omen-linux. See `FAN_MONITORING.md` and `HARDWARE_SAFETY.md`. The completed
approved PPD profile transaction retains its existing scope.
