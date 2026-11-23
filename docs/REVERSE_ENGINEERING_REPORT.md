# HP OMEN investigation and Linux implementation report

Status: development investigation. Source-backed behavior, read-only observations,
fixture tests, and unverified Windows behavior are distinguished below. The
master prompt's full hardware-feature definition of done has not been met.

## Evidence and provenance

`evidence/sources.json` records exact upstream commits and SHA-256 hashes of the
files actually inspected. The following links pin the source, rather than a
moving branch:

* [Linux hp-wmi.c](https://github.com/torvalds/linux/blob/ee02ed6308fbbd851c4e5c1f642d029617049a12/drivers/platform/x86/hp/hp-wmi.c): board tables, fan callbacks, profile registration, hwmon read/write semantics.
* [OMEN Space](https://github.com/yunusemreyl/omen-space/tree/38d584b8f13b9c5fe3ba9599578485f2e990efdf): `boards.json`, `capabilities.rs`, `fan/mod.rs`, `ec.rs`, `rgb/mod.rs`, `tests.rs`.
* [OpenOMEN](https://github.com/openomen/openomen/tree/ecfd66bdc4fe2a588e06acb885b8c17802804ae4): executable **Python source** `openomen`, README, license. The previous report incorrectly described this as binary-only.
* [OmenMon](https://github.com/OmenMon/OmenMon/tree/d89340e6d4dc9802b609390729b0bfaec05563e2): .NET Framework 4.8 project, `Hardware/Bios.cs`, `BiosCtl.cs`, `Ec.cs`.
* [OmenCore](https://github.com/theantipopau/omencore/tree/840f03d58ddd4c3a709780b6d635828d9ef3d87c): canonical source established through mutual project/repository links; 42 selected files hashed. See `OMENCORE_REVIEW.md` and `evidence/omencore-sources.json` for scope, development/release distinction and recovery limitations.
* [HP OMEN RGB module](https://github.com/OmenLinux/omen-rgb-keyboard/tree/2aef1496aafd932904bf360dcafba63994d07327): README, `src/zones/omen_zones.c`, `src/fan/omen_fan.c`.

Live Linux evidence is stored in `evidence/live-readonly-diagnostics.json` and
`evidence/live-interface-inventory.json`. No serial numbers or MAC addresses are
collected. These are observations on one machine, not a universal device list.

## REA setup and actual use

Client: Codex desktop. The official [REA installation guide](https://github.com/morluto/rea/blob/main/docs/installation.md)
and [skill](https://github.com/morluto/rea/blob/main/.agents/skills/reverse-engineer-anything/SKILL.md)
were inspected first. The project skill was installed through the documented
`npx skills add morluto/rea --skill reverse-engineer-anything` workflow, scoped to
Codex. Installing instructions alone did not connect MCP.

The initial `rea-agents@6.8.0 doctor --client codex --json` found system Node
22.22.1 too old, absent registration, and absent optional analysis engines. A
compatible **existing** bundled Node 24.22.1 was found. A dry-run scoped plan was
reviewed and explicitly approved by the user. Setup backed up the Codex config,
installed its matching global skill, and added only the REA registration. The
runner was then pinned to the existing compatible Node. No engines were installed.

Evidence: `evidence/rea-applied.json`, `rea-doctor-after-setup.json`, and
`rea-standalone-mcp.json`. Scoped doctor reports `healthy: true`; environment
health remains false because optional native engines are missing. A standalone
MCP client actually initialized REA 6.8.0, enumerated 229 tools, and successfully
called `binary_session`. The active Codex chat now also exposes the tools and
successfully called `binary_session`; its direct response, server identity and
provider limitations are retained in `evidence/rea-chat-session.json`.
See `REA_SETUP_PLAN.md` for exact paths, backups, and recovery.

For complete source repositories, REA's own instructions prefer ordinary source
analysis. REA was not used to pretend that reading Linux source is Windows binary
reverse engineering. Native decompilation remains unavailable without a separately
approved engine; managed metadata and package inventory may not need one.

## Windows reference build

The main OMEN Gaming Hub Store application and a compatible Windows reference
system are unavailable here. Its version, package format, signatures, UI toolkit,
IPC, running services, and individual hardware command paths are **unverified**.
Do not assume Electron, .NET, UWP, or MSIX from the product's name or Store listing.
No Windows behavior was dynamically observed and no HP executable was executed.

An official supporting artifact was identified:
[HP SoftPaq sp166157](https://ftp.hp.com/pub/softpaq/sp166001-166500/sp166157.html),
OMEN Gaming Hub SDK Package 1.0.91 Rev A Pass 1. HP describes it as supporting
overclocking and Network Booster, and lists Windows 11 64-bit releases. This is
**not the main application and does not establish where fan/RGB logic lives**.
The official metadata is archived in `evidence/hp-sp166157-description.txt`.
The completed 221,403,808-byte download matches HP's published checksum and has
SHA-256 `25e38bb3d94bcf71ef7fec7679ef5100382cb5ce7c14d3522279103d27b4564d`.
Actual REA `open_binary` and `inspect_artifact` calls identified an x86_64 PE;
REA's root-only inventory did **not** inspect the embedded archive. Separately,
existing 7-Zip extracted the SDK and its capability-driver ZIPs. Actual REA
`inspect_managed_artifact` calls established .NET Framework/CIL identity for
OMENSDK_Ver, XtuService, XtuCoreServer and HardwareAccess; `inspect_managed_members`
retained XtuService method tokens and call edges. The native OmenCap service and
its driver relationships are established by packaged INF and PE imports.
Detailed findings and signature limitations are in `WINDOWS_ARTIFACTS.md`.
No HP binaries are included in the application.

Next Windows workflow: obtain the actual application legally, hash each package,
verify signatures with an appropriate trust store, inventory dependencies,
classify managed/native/JavaScript artifacts, select the corresponding available
REA operation, and trace one UI action at a time on an authorized Windows machine.
Record before/after state and IPC observations. Do not derive firmware writes
from UI names or unconfirmed string matches.

## Linux kernel findings

The pinned kernel implements model-specific WMI/ACPI logic internally. Our
application uses sysfs; it constructs no WMI commands and touches no raw EC ports.

* `hp_wmi_feature_boards`, `omen_thermal_profile_boards`, and related tables select
  firmware variants. A marketing name is insufficient evidence of a write API.
* `hp_wmi_fan_control_supported()` requires a board fan-speed callback. Manual
  `pwm1`/`pwm2` attributes are hidden when that callback is unavailable.
* `pwm1_enable` supports modes 0 (maximum), 1 (manual on supported boards), and 2
  (automatic). **Maximum and Automatic can exist without manual pwm attributes.**
  The earlier report conflated these capabilities. This build intentionally
  exposes neither as a live control until validation/recovery requirements pass.
* `hp_wmi_hwmon_write()` initializes manual targets from active fan speeds and
  rejects pwm-input writes outside manual mode. It clamps translated fan-speed
  requests using firmware min/max bounds.
* `hp_wmi_hwmon_read()` derives pwm input from **active RPM**, through
  `rpm_to_pwm()`. It is not a simple echo of the last requested electrical duty.
  Immediate ±1% readback over regular fixture files does **not** validate real
  fan settling behavior. This unresolved issue blocks live fan release.
* The kernel owns a delayed keep-alive (`KEEP_ALIVE_DELAY_SECS = 90`). A userspace
  crash cannot be assumed to reset fans after 120 seconds: the loaded kernel's
  worker may continue renewing the previous target. No SIGKILL recovery guarantee
  is made. A targeted, model-validated recovery mechanism is still needed.
* Standard `platform_profile` choices must be read at runtime. `quiet` and
  `low-power` names are not interchangeable when writing the raw ABI. When PPD
  is active, use PPD rather than a competing direct sysfs writer.
* The pinned hp-wmi source does not expose the RGB module's zone ABI or a MUX
  switch to this application. This is not a claim that no Linux project supports
  MUX switching through other drivers.

## Existing implementations: actual code and limitations

| Project | Implementation findings | Models and firmware | Tests/issues and licensing |
|---|---|---|---|
| OMEN Space | Rust/GTK plus daemon/D-Bus and out-of-tree `hp-omen-extra`; fan module detects hwmon, manual targets, keep-alives, thermal guards; separate EC and RGB modules | `boards.json` carries board-level capabilities and a separate verified-board list; WMI/EC fallbacks differ by model | `tests.rs` includes board, PPD mapping, ACPI-health, and conflict regression tests. Open issues #334/#333 request board verification; #332 reports a thermal-warning mismatch. GPL-3 license; no implementation copied |
| OpenOMEN | Python CLI reads hp-wmi fan attributes and uses `pwm1_enable` for boost modes with readback; profile handling is separate | README reports Victus 17-db100, 16-ap0xxx, 16-r0xxx, 15-fb0xxx and other OMEN models, on Linux 6.18+; these are upstream claims, not our validation | No test suite identified in reviewed checkout. Open #9 requests Ubuntu 22.04 compatibility; #12 asks for desktop LCD/pump support. GPL-3 |
| OmenMon | C#/.NET Framework 4.8; CIM/WMI BIOS calls in `Bios.cs`, typed fan and color-table operations in `BiosCtl.cs`, EC access in `Ec.cs` | Windows behavior and driver assumptions do not establish a Linux sysfs ABI | No automated suite identified in reviewed tree. Open #136 reports missing temperatures; several issues concern startup/.NET. GPL-3; proprietary HP internals cannot be inferred from this third-party implementation |
| OmenCore | Canonical source is `theantipopau/omencore`; .NET 8 Linux CLI and separate Windows WMI/EC backends. Linux classifier separates coarse hwmon mode access from manual targets; writer lock and normal-exit restoration do not prove forced-kill recovery | 8C30 keyboard entry is explicitly unverified/backlight-only; Linux runtime attributes and firmware determine access. No compatibility grants imported | xUnit source and CI inspected, not executed. Issue snapshots include missing targets (#126), temperature selection (#214), failed Auto recovery (#183, closed) and EC faults (#60). MIT; no implementation copied. Detailed pinned review: `OMENCORE_REVIEW.md` |
| omen-rgb-keyboard | Kernel WMI module, four-zone sysfs ABI, brightness/animation files; `zone_show()` reads firmware and returns `#rrggbb`, or an error string | README reports Omen 16-u0000sl, wf1xxx/wf0xxx, 15-dh0025nl/en1004ns, 16-wd0xxx, 14-fb0xxx, 15-ek0xxx; no per-key inference | Warns that loading it with hp_wmi causes conflicts. Open #10 concerns system integration. No automated hardware test suite identified. GPL-3; module is neither installed nor bundled |
| Linux hp-wmi | Preferred upstream driver; board-selected thermal and fan callbacks, hwmon, platform profiles | See pinned source and compatibility matrix; live kernel may differ from source | Our fixture tests cover adapter parsing/gates, not upstream hardware correctness. Source SPDX GPL-2.0-or-later |

Open-issue snapshots (titles and timestamps) are retained under `evidence/*-open-issues.json`.
Their existence is an investigation lead, not independent confirmation of a defect.

## Feature specification and confidence

| Feature | Windows behavior | Linux interface and evidence | Confidence / support | Risk and validation |
|---|---|---|---|---|
| Vitals | Per-sensor Windows behavior unverified | hwmon/procfs/power_supply; cpufreq; NVIDIA query CSV; AMD DRM. Live read-only snapshot on 8C30/F.10 | Verified readings as reported; physical calibration unverified | Unit fixtures check units, missing inputs and CPU deltas. Fan calibration and independent long-run comparison remain unverified |
| Profiles | Windows mode semantics unverified | PPD advertised D-Bus properties/set; platform_profile choices. Approved PPD Performance → Balanced → Performance observed on 8C30/F.10; daemon and independent D-Bus readback (`evidence/live-profile-test.json`) | One Linux profile transaction verified; Windows semantics and thermal response unverified. Writes remain opt-in | PPD amd_pstate backend, no firmware platform profile. Other modes, transitions and interrupted restoration require validation |
| Fans | Windows command path unverified | Pinned hp-wmi modes and board tables; live 8C30 has RPM and pwm1_enable, no pwm1/2 | RPM monitoring verified as kernel-reported. Manual/curve implementation **fixture-only** | Physical settling, thermal stability, GPU-aware curves, suspend/resume and crash recovery unvalidated: release-blocked |
| Lighting | Windows protocol unverified | Optional module zone/brightness ABI; plain LED class | Static adapter fixture-tested; live write policy disabled. Interface absence on reference host verified | No assumption of missing physical hardware; no per-key protocol. Animations not implemented for live use; error strings must not become black colors |
| Graphics | Windows MUX behavior unverified | PCI driver/runtime PM and GPU metrics | Monitoring only. No validated switch implemented | AMD/Intel topology marked unverified; no firmware writes; MUX/Advanced Optimus remain unavailable |
| Battery | Windows charging controls unverified | power_supply energy/charge/current/voltage/status; threshold file when present | Live monitoring verified; charge writes fixture-only | Missing voltage yields unavailable energy, not zero; no reference threshold file. Real limits and safe min thresholds unvalidated |
| Game library | Windows behavior unverified | User's native/Flatpak Steam, Heroic and Lutris records; validated installation-specific IDs; unprivileged launcher submission | Fixture detection/validation verified; local library may be empty | Never launch from daemon. Per-game profile failure stops launch. Actual game lifetime and crash-safe profile ownership remain incomplete |
| Network | Windows SDK contains Realtek driver declarations; original traffic-policy behavior unobserved | Linux procfs/sysfs per-interface counters and suspend-aware rates | Fixture IPC/CLI/UI and read-only host counter brackets pass | No traffic policy, packet capture, generated traffic or calibration claim. Interface counts are not summed across VPNs/bridges |
| Extras | Windows utilities unverified | Versioned user/system TOML, JSON import/export, local diagnostics, XDG GlobalShortcuts | Config migrations and native portal fixture workflows tested | KDE tray verified; Ctrl+H/Q are app-local. Opt-in global Show shortcut is implemented and protocol-tested on GTK X11/Wayland; physical key/desktop approval remain unverified. Revision 11 adds an opt-in native status overlay and granted portal toggle; compositor/full-screen and physical-key behavior remain unverified. Traffic prioritization and cloud-account replacements remain unavailable |

## Corrections and remaining release gates

The inherited report overstated live profile testing, universal service recovery,
stock fan-curve safety, and unsupported-model functionality. Those claims are
withdrawn. No hardware-control test has been performed during this run. The
existing generic systemd hwmon sweep was removed because it could write unrelated
controllers. Unknown machines remain read-only. Device-specific control release
requires exact board/BIOS evidence, approved live tests, physical response checks,
error rollback, suspend/resume validation, and a proven targeted crash recovery
path. No source-level or mock result substitutes for those gates.
