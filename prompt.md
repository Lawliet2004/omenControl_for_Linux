# MASTER ENGINEERING PROMPT — HP OMEN Gaming Hub for Linux

## 1. Role and Primary Objective

Act as a senior reverse-engineering engineer, Linux kernel integration specialist, Rust systems developer, and desktop UI architect.

Your mission is to **reverse engineer HP OMEN Gaming Hub for Windows and build a functional, lightweight, native Linux alternative**, prioritizing accurate hardware behavior, compatibility, reliability, and a polished user interface.

Use the **REA (Reverse Engineer Anything)** framework:

https://github.com/morluto/rea

The target operating system is **Kubuntu/Ubuntu Linux**, with an extensible architecture for other Linux distributions.

This is not merely a UI recreation. The software must genuinely control supported hardware, reflect real system states, and never present simulated functionality as working hardware integration.

The final application should provide as much of HP OMEN Gaming Hub's useful functionality as Linux and the underlying hardware actually permit.

Do not assume that a feature supported by one HP OMEN laptop works on every OMEN or Victus model.

---

## 2. Set Up and Use REA

First, inspect:

- https://github.com/morluto/rea
- https://github.com/morluto/rea/blob/main/.agents/skills/reverse-engineer-anything/SKILL.md
- https://github.com/morluto/rea/blob/main/docs/installation.md

Follow the official REA documentation rather than guessing tool names, analysis capabilities, or installation requirements.

### Environment preparation

1. Determine the AI coding client: Codex, OpenCode, Claude Code, or another supported client.
2. Check whether the `reverse-engineer-anything` skill and REA MCP tools are already installed.
3. If the skill is missing, install it from the official repository using its documented installation method.
4. Remember that installing the skill alone does not install or register the REA MCP server.
5. If the MCP integration is missing, inspect the installation guide and prepare a configuration repair plan.
6. Obtain approval before modifying existing agent configuration or installing analysis engines.
7. Verify the server is genuinely connected before claiming REA is operational.

The official guided setup command is:

```bash
npx rea-agents setup
```

Choose the appropriate coding agent, review the proposed changes, and restart or reconnect the agent when required.

Use REA tools according to the installed server's actual capabilities.

For example:

- `open_binary` for suitable native binaries or packaged artifacts.
- `inspect_managed_artifact` for compatible .NET assemblies.
- `analyze_javascript_application` for extracted Electron or JavaScript applications.
- Ghidra or another supported native analysis provider for native executable analysis.

Do not assume that HP OMEN Gaming Hub uses a particular programming language or packaging framework until the actual artifacts have been inspected.

If a component cannot be analyzed, document the limitation and continue with other reliable sources.

---

## 3. Identify and Reverse Engineer the Original Application

The reference application is **HP OMEN Gaming Hub for Windows**, including relevant official HP helper services, SDK components, and device-integration layers where legitimately available.

### Step A — Establish the reference build

Obtain an authorized copy of the software through official HP distribution channels or user-provided installation files.

Record:

- Application version and architecture.
- Package and executable formats.
- Digital-signature and publisher information.
- SHA-256 hashes.
- Installed components and dependencies.
- Relevant background services.
- Hardware and Windows-version dependencies.

Do not infer that an installer contains all hardware logic. Some capabilities may be implemented by external services, firmware, kernel drivers, or Windows APIs.

### Step B — Static reverse engineering

Use REA to examine the relevant artifacts.

Investigate:

1. Executable entry points and component relationships.
2. Managed and native libraries.
3. UI implementation technologies.
4. Configuration storage and serialization.
5. IPC mechanisms between the UI and services.
6. Hardware capability detection.
7. Thermal-profile selection behavior.
8. Fan-control command paths.
9. WMI and ACPI integrations.
10. Keyboard RGB configuration.
11. GPU and CPU monitoring.
12. Graphics switching.
13. Performance and power management.
14. Relevant failure and fallback behavior.

Use decompilation, symbol inspection, string references, and cross-reference analysis where appropriate.

For each significant conclusion, preserve evidence, such as an artifact hash, function reference, inspected data structure, or observed API call.

**Do not invent undocumented WMI commands, EC registers, firmware offsets, or hardware-control values.**

### Step C — Behavioral analysis

When a compatible Windows reference system is available, observe the original application performing individual operations.

For example:

- Switch from Balanced to Performance.
- Change the fan mode.
- Change keyboard lighting.
- Observe monitoring refresh intervals.
- Switch between supported graphics modes.
- Restore default settings.

Compare the system state before and after each operation.

Use appropriate tracing tools and REA-supported analysis workflows without bypassing security protections.

Collect only information necessary to understand the intended behavior.

### Step D — Evidence-based specification

Produce a document called:

`docs/REVERSE_ENGINEERING_REPORT.md`

For every feature, include:

| Field | Required information |
|---|---|
| Feature | Name and purpose |
| Windows behavior | Verified reference behavior |
| Interface | API, service, WMI, ACPI, or other mechanism |
| Evidence | Relevant files, calls, traces, or documentation |
| Linux equivalent | Available Linux integration |
| Confidence | Verified, partially verified, or unverified |
| Support | Supported hardware and required conditions |
| Risk | Potential failure modes |
| Validation | How functionality will be tested |

Separate directly observed behavior from hypotheses.

Use an independent implementation based on established interfaces and behavior rather than copying proprietary code or redistributing HP assets.

---

## 4. Research Existing Linux Implementations

Before writing hardware drivers, thoroughly examine these projects:

### Primary references

1. **OMEN Space**

   https://github.com/yunusemreyl/omen-space

2. **OpenOMEN**

   https://github.com/openomen/openomen

3. **OmenMon**

   https://github.com/OmenMon/OmenMon

4. **OmenCore**

   https://github.com/omencore

   Verify the canonical source repository before using code.

5. **HP OMEN RGB Keyboard**

   https://github.com/OmenLinux/omen-rgb-keyboard

6. **Linux kernel HP WMI driver**

   https://github.com/torvalds/linux/blob/master/drivers/platform/x86/hp/hp-wmi.c

### Research requirements

Inspect the actual implementations and document:

- Supported HP OMEN/Victus models.
- Hardware detection methods.
- Kernel interfaces and dependencies.
- Fan-control implementations.
- Performance profiles.
- RGB lighting protocols.
- Known firmware differences.
- Existing test coverage.
- Relevant open issues.
- Licensing restrictions.

Prefer upstream Linux interfaces and established drivers whenever they adequately support the hardware.

**Do not build a new kernel driver merely because the Windows version uses a proprietary driver.**

Reuse existing open-source work where justified and license-compatible, preserving attribution and complying with the corresponding licenses.

---

## 5. Required Application Features

Build these capabilities in a priority order based on verified hardware support.

### A. System Vitals and Monitoring

Create a real-time system monitoring dashboard displaying:

- CPU temperature, utilization, clock frequency, and power draw where available.
- GPU temperature, utilization, clock frequency, VRAM, and power consumption where available.
- System RAM and storage usage.
- Fan RPM and operating modes.
- Battery health, charging state, and discharge rate.
- System power-profile status.
- Historical sensor graphs.

Investigate Linux interfaces such as:

- `/sys/class/hwmon`
- `/sys/class/thermal`
- `/sys/class/power_supply`
- `lm-sensors`
- NVIDIA Management Library and `nvidia-smi`
- AMD GPU monitoring interfaces
- `upower`
- `/proc` and standard Linux performance interfaces.

All figures must represent genuine measurements. Handle unavailable sensors and differing update intervals correctly.

Avoid excessive hardware polling.

### B. Fan and Thermal Control

Implement supported modes:

- Automatic.
- Quiet/Silent.
- Balanced/Normal.
- Performance.
- Maximum Fan.
- Custom fan curves, where supported.

Investigate the upstream `hp-wmi` driver, its hardware-monitoring interfaces, and model-specific implementations.

Do not assume that `pwm1`, `pwm1_enable`, fan RPM, or manual fan control are present on every laptop.

Requirements:

- Detect actual available capabilities.
- Validate every operation before applying it.
- Never write to unknown EC registers.
- Avoid fan-stop commands.
- Avoid competing with another fan-control daemon.
- Detect write failures and state mismatches.
- Restore firmware-controlled Automatic mode upon normal shutdown or recoverable failure.
- Provide safe recovery if the application crashes.
- Preserve firmware thermal safeguards.

Do not claim that firmware protection is guaranteed unless verified for the target system.

### C. Performance Profiles

Implement supported combinations of:

- Quiet.
- Balanced.
- Performance.
- Custom configurations.

Use standard Linux facilities, including `platform_profile` and power-management APIs where applicable.

Respect the actual model-specific firmware interfaces.

Avoid allowing multiple power-management services to apply contradictory settings.

CPU/GPU power-limit changes must not be exposed as universal features.

### D. Keyboard RGB and Lighting

Support applicable hardware capabilities:

- Single-zone keyboards.
- Four-zone keyboards.
- Per-key lighting where genuinely supported.
- Brightness adjustment.
- Static colors.
- Supported animation modes.
- Saved lighting presets.
- Startup profile restoration.

Investigate HP-specific WMI interfaces, Linux LED subsystems, and supported peripheral protocols.

Do not assume a four-zone protocol can control per-key hardware.

Where a lighting interface is undocumented, make it unavailable rather than attempting arbitrary hardware writes.

### E. Graphics Switching

Investigate:

- Hybrid graphics.
- Dedicated GPU mode.
- Integrated GPU mode.
- Supported hardware MUX switching.
- Advanced Optimus behavior, if applicable.

Use Linux-compatible mechanisms only where verified.

Do not confuse application-level GPU selection with firmware MUX switching.

Warn when logging out or rebooting is necessary.

Never change graphics modes without confirming that the operation is supported and recoverable.

### F. Battery and Power

Implement:

- Battery and AC-power detection.
- Battery-aware performance profiles.
- Power-consumption monitoring.
- Supported charging thresholds.
- Automatic switching between safe profiles.

Battery charge limits must only be exposed when supported through verified hardware or kernel interfaces.

### G. Additional Functionality

Investigate and implement Linux-native equivalents, where practical, for:

- Game library detection and launching.
- Per-game performance profiles.
- Hotkey and shortcut integration.
- System tray controls.
- On-screen status overlays.
- Network monitoring and optional traffic-priority controls.
- Hardware diagnostic reports.
- Settings export/import.
- Startup and recovery behavior.

Treat HP-specific Windows utilities, account services, promotional content, and proprietary AI-enhancement features as separate compatibility questions.

Do not invent working replacements for Windows-only functionality.

---

## 6. Linux Hardware Abstraction Layer

Design a hardware abstraction layer that separates the UI from model-specific hardware controls.

Suggested architecture:

```text
             Linux Desktop UI
                    |
           Application Controller
                    |
           Hardware Abstraction API
                    |
        Privileged Hardware Service
                    |
       Capability Detection + Validation
                    |
       +------------+------------+
       |            |            |
     hp-wmi       hwmon       GPU APIs
       |            |            |
    ACPI/WMI     Sensors     Driver APIs
       |            |            |
       +------------+------------+
                    |
           Supported Hardware
```

Every feature must be capability-gated.

At startup, identify relevant hardware using read-only information from:

- DMI/SMBIOS.
- CPU and GPU information.
- Kernel version.
- Loaded drivers.
- Detected WMI interfaces.
- Available sysfs interfaces.
- Device-specific capability tables.

Use the exact board/model and firmware revision where required.

A laptop not recognized as compatible must remain in a safe, read-only monitoring mode.

Do not claim universal HP compatibility.

---

## 7. Preferred Technology Stack

Use a native, lightweight implementation.

### Backend

**Rust**

Responsibilities:

- Sensor monitoring.
- Capability detection.
- Hardware API adapters.
- Configuration management.
- Safe operation validation.
- Hardware-state reconciliation.
- Logging and diagnostics.

### Frontend

Prefer **GTK4 with libadwaita** for a native Linux desktop experience.

Use a different toolkit only if there is a clearly demonstrated technical advantage for the required interface and distribution targets.

### Privileged service

Use a narrowly scoped `systemd` service, authenticated IPC, and PolicyKit authorization where applicable.

Do not run the complete graphical application as root.

Restrict the privileged API to validated, device-specific operations.

### Configuration

Use a versioned TOML or JSON configuration schema.

Support migrations, backups, and safe defaults.

### Deployment

Primary target:

- Ubuntu/Kubuntu.
- Debian-family `.deb` package.

Structure the code so that support for Arch, Fedora, and other distributions can be added later.

Do not assume a Flatpak sandbox can directly access privileged hardware controls.

---

## 8. UI and User Experience

Analyze the original OMEN Gaming Hub interface using authorized screenshots and observable behavior.

Preserve the useful design concepts:

- Clean, dark visual appearance.
- Structured navigation.
- High-quality monitoring graphs.
- Easily discoverable hardware controls.
- Lighting configuration panels.
- Performance-mode selectors.
- Fan-control graphs.
- Consistent icons and transitions.

Build an original interface inspired by its usability rather than redistributing copyrighted visual assets.

### Main screens

1. Dashboard
2. System Vitals
3. Performance Control
4. Fan and Thermal Control
5. Lighting Studio
6. Graphics Switching
7. Game Library
8. Battery and Power
9. Settings and Diagnostics

Requirements:

- Responsive desktop layout.
- KDE-compatible window behavior.
- Wayland and X11 support where feasible.
- Native system tray integration where supported.
- Proper scaling.
- Low CPU usage when idle.
- Minimal monitoring overhead.
- Fast startup.
- No mandatory cloud account.
- No advertising.
- No unnecessary telemetry.

When hardware functionality is unsupported, disable the control and explain the reason.

**Do not create visually convincing but nonfunctional hardware settings.**

---

## 9. Hardware Safety and Reliability

Treat hardware safety as a release-blocking requirement.

1. Begin with read-only hardware discovery.
2. Use mocked hardware adapters for initial development.
3. Never issue unknown WMI or EC writes.
4. Never disable BIOS thermal safeguards.
5. Never automatically apply undervolting or overclocking.
6. Require explicit approval before first live hardware-control tests.
7. Validate commands against a documented model-specific allowlist.
8. Keep automatic fan-management recovery available.
9. Detect conflicting control services.
10. Implement timeouts, error handling, and logging.
11. Test suspend/resume, shutdown, and reboot behavior.
12. Do not modify firmware or BIOS settings unless a separate, independently validated procedure authorizes it.

Record the previous hardware state where readable.

Apply changes transactionally when possible, verify their results, and restore a safe state after failure.

Avoid raw EC access where supported kernel interfaces are available.

Any unresolved thermal-control risk must block that feature from release.

---

## 10. Testing and Validation

Implement a layered testing strategy.

### Automated tests

- Rust unit tests.
- Hardware-adapter mock tests.
- Capability detection tests.
- Configuration migration tests.
- IPC authorization tests.
- Error recovery tests.
- Unsupported-model tests.
- UI interaction tests.

### Real-hardware validation

Only test actual hardware when an authorized compatible machine is accessible.

Verify:

- Detected device identity.
- Supported firmware revision.
- Sensor accuracy against an independent reference.
- Fan RPM readings.
- Correct behavior when switching modes.
- Thermal stability during approved workloads.
- Keyboard lighting changes.
- Power-profile state changes.
- Restoration following application exit.
- Suspend/resume recovery.

Never claim hardware validation from mocks or virtual machines.

If actual hardware is unavailable, explicitly mark those tests as unverified.

### Performance validation

Measure:

- Cold startup time.
- Idle CPU usage.
- Resident memory usage.
- Monitoring refresh overhead.
- UI responsiveness.
- Background service resource consumption.

Optimize based on measured results, not assumptions.

---

## 11. Project Structure

Organize the implementation approximately as follows:

```text
omen-linux/
├── README.md
├── Cargo.toml
├── crates/
│   ├── core/
│   ├── hardware/
│   │   ├── detection/
│   │   ├── hp_wmi/
│   │   ├── fans/
│   │   ├── sensors/
│   │   ├── lighting/
│   │   └── power/
│   ├── daemon/
│   ├── ipc/
│   └── cli/
├── gui/
├── packaging/
│   ├── debian/
│   ├── systemd/
│   └── polkit/
├── tests/
│   ├── unit/
│   ├── integration/
│   └── mocks/
└── docs/
    ├── REVERSE_ENGINEERING_REPORT.md
    ├── HARDWARE_COMPATIBILITY.md
    ├── ARCHITECTURE.md
    ├── HARDWARE_SAFETY.md
    ├── TEST_REPORT.md
    └── INSTALLATION.md
```

Adapt this structure when a different organization has clear technical benefits.

---

## 12. Implementation Workflow

Follow these engineering phases.

### Phase 1 — Discovery

- Inspect REA installation and capabilities.
- Research original HP software.
- Examine existing open-source Linux implementations.
- Identify available Windows reference artifacts.
- Identify test hardware and its firmware/kernel versions.
- Produce an evidence-based feature compatibility matrix.

**Deliverable:** Verified investigation report and implementation plan.

### Phase 2 — Foundation

- Create the Rust workspace.
- Implement model detection.
- Implement mock hardware interfaces.
- Add read-only sensor support.
- Define privileged IPC security boundaries.
- Build basic CLI commands.

**Deliverable:** A working, testable hardware-monitoring foundation.

### Phase 3 — Hardware Controls

Implement verified features in this priority order:

1. Monitoring and device detection.
2. Supported performance profiles.
3. Safe fan modes.
4. Keyboard RGB.
5. Model-specific additional controls.

Use mock testing before any approved real-hardware writes.

**Deliverable:** Functional backend with validated safety behavior.

### Phase 4 — Desktop Application

- Implement the complete navigation shell.
- Connect genuine system monitoring.
- Add functional hardware-control screens.
- Add capability-aware UI states.
- Implement configuration persistence.
- Add tray controls and appropriate desktop integration.

**Deliverable:** Usable native Linux desktop application.

### Phase 5 — Validation and Release

- Run automated tests.
- Conduct permitted hardware integration tests.
- Verify recovery and error handling.
- Measure resource consumption.
- Build `.deb` packages.
- Document model-specific compatibility.
- Produce installation, removal, and recovery instructions.

**Deliverable:** Installable release candidate with clearly documented support boundaries.

---

## 13. Mandatory Engineering Rules

Follow these rules throughout the project:

- **Evidence before implementation:** Do not base hardware-control code on unsupported guesses.
- **No fabricated functionality:** Distinguish working features, mock features, experimental features, and unavailable features.
- **No unnecessary reverse engineering:** When reliable Linux source code or public documentation establishes behavior, prefer that evidence.
- **No unjustified driver replacement:** Reuse stable upstream kernel facilities.
- **No unsafe optimization:** Reliability is more important than a small performance gain.
- **No proprietary code copying:** Implement original code, observe applicable licenses, and credit reused open-source work.
- **No blanket root access:** Enforce least privilege.
- **No silent hardware writes:** Log control operations and their verified outcomes.
- **No unsupported compatibility claims:** Every supported device must have traceable evidence or test results.
- **No untested release claims:** Report exactly which tests were executed and which were not.

If the source Windows application or an HP test machine is unavailable, proceed with architecture, existing Linux-source analysis, mock testing, and read-only development. Explicitly identify which conclusions still require additional evidence.

---

## 14. Final Deliverables

Produce:

1. Complete Linux application source code.
2. REA reverse-engineering investigation report.
3. Model-specific hardware compatibility matrix.
4. Native Rust hardware integration layer.
5. Functional desktop GUI.
6. Privileged service with restricted hardware-control API.
7. Automated tests and test reports.
8. Debian package build configuration.
9. Installation and uninstallation instructions.
10. Hardware safety and recovery documentation.
11. Screenshots of the completed application.
12. Known limitations and unsupported features.

### Definition of Done

A feature is completed only if:

- Its underlying hardware interface has been verified.
- Its implementation builds successfully.
- Relevant automated tests pass.
- The UI reports actual state.
- The operation succeeds on tested, compatible hardware when testing is authorized.
- Failure and recovery paths are documented and validated.
- Model-specific support limitations are stated accurately.

A completed user interface alone does not constitute a completed hardware feature.

## 15. Start Executing

Begin by examining the REA repository, its `reverse-engineer-anything` skill, and the official installation documentation.

Then investigate HP OMEN Gaming Hub and the existing Linux-compatible projects.

Create the reverse-engineering report and hardware compatibility matrix before implementing device-specific write operations.

Proceed through the engineering phases, building and testing the code after each substantial change. Resolve implementation defects rather than silently skipping them.

Your ultimate objective is to deliver the **most complete, dependable, lightweight, and genuinely functional HP OMEN Gaming Hub alternative that Linux and verified hardware interfaces allow**.

Prioritize correctness, measurable performance, hardware safety, and maintainable engineering over superficial feature parity.