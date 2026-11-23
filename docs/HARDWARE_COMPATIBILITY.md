# Hardware compatibility

This is the implemented application's status, distinct from capabilities claimed
by upstream projects. No device-specific control is released as hardware-tested.
Exact source revisions/hashes: `evidence/sources.json`.

| Machine / condition | Monitoring in this build | Live writes in this build | Evidence / remaining gate |
|---|---|---|---|
| HP Victus fb1002AX, board **8C30**, BIOS **F.10**, kernel **7.0.0-38-generic** | CPU/GPU, memory/storage, fan RPM and kernel-reported mode, battery/AC and PPD profile state observed read-only | Off by default; only standard profiles have an explicit experimental board/BIOS opt-in | Approved PPD Performance → Balanced → Performance transaction passed; `evidence/live-profile-test.json`. Other modes/transitions and thermal behavior unverified |
| 8A3D, 8B2F, 8BBE, 8BD4, 8BD5, 8C99, 8C9C | Generic sensors if exposed; kernel source lists manual fan callbacks | Fan, curve, RGB, charge writes disabled | Source-backed interfaces; fixture tests only; no approved live fan safety or recovery validation |
| OMEN thermal-profile boards in `board_tables.rs` | Runtime sensor/profile discovery | Standard profiles only after explicit target opt-in; device-specific writes off | Kernel board facts do not validate an arbitrary BIOS revision or prove hardware response |
| Unrecognized OMEN/Victus board, other HP, non-HP | Read-only generic sensors | Off; unrecognized board cannot opt into profile testing through marketing name alone | Discovery must never grant write support merely from `OMEN` in DMI |
| RGB-capable keyboard with community module | Adapter can read four-zone/brightness state | Off pending device/module-version validation | No module installed by this project; module warns about conflict with hp_wmi |
| Keyboard without Linux lighting interface | Other monitoring unaffected | No lighting control | Interface absence does not establish physical keyboard capabilities |
| Battery with threshold attribute | Attribute can be read | Off pending validation | No threshold attribute on reference Victus |
| Any graphics configuration | PCI/driver/runtime PM and available metrics | No MUX/Optimus switching | NVIDIA discrete classification; AMD/Intel topology remains unknown |

On reference 8C30, hp-wmi exposes `fan1_input`, `fan2_input`, `pwm1_enable`, but no
manual `pwm1` or `pwm2`. The kernel source supports maximum/automatic independently
from manual control; this build still blocks live fan commands until validated.
RPMs are displayed as reported; no calibrated tachometer comparison has occurred.
Revision 8 reports the readable Auto/Maximum policy independently of manual
target support. Real read-only daemon/direct CLI and independent sysfs reads
agree on Auto on this reference machine; this is not a fan transition or
recovery test. See `FAN_MONITORING.md`.

On this reference Victus, NVIDIA telemetry also passes a read-only transient
user-service probe with GPU devices hidden from the daemon. Desktop/CLI samples
come from a local read-only supplement, explicitly recorded in diagnostics.
See `GPU_MONITORING.md` for the paired query, driver/binary fingerprints and
the differences from a physical installation of the root system service.

Source table groups are exact ID lists, not numeric ranges. A board between two
listed hexadecimal IDs does not inherit support. Network monitoring is
model-independent read-only Linux procfs/sysfs data, verified on the reference
machine against independent counters. It reports the daemon/direct process's
network namespace and does not imply Internet reachability, speed-test accuracy
or traffic-priority support. See `NETWORK_MONITORING.md`.

Kernel/firmware upgrades may
change available interfaces. Run `omen-cli detect` to inspect the current gates;
`omen-cli diagnostics` writes a read-only JSON report to stdout.

Upstream community model claims (OMEN Space, OpenOMEN, OmenMon, OmenCore, RGB module) are
summarized in `REVERSE_ENGINEERING_REPORT.md`. They are not transplanted into our
hardware-tested matrix. No Windows-app version has been behaviorally compared.

The pinned OmenCore Windows keyboard database lists 8C30 as unverified and
backlight-only. It does not validate our physical keyboard or a Linux lighting
interface; see `OMENCORE_REVIEW.md` for the exact source and limits.

Revision 11's optional overlay is model-independent monitoring using these same
measurements and capability gates. Fresh read-only identity still matches
8C30/F.10/kernel 7.0.0-38-generic. Adding the overlay and fixed-mode fixture
thermal checks grants no live control capability or physical validation.

## Coarse mode capability (revision 12)

`fan_modes` reports Automatic/Maximum separately from manual `fan_control` and
`fan_curve`. Mode-only ordinary-file fixtures work without PWM targets. The
reference 8C30/F.10 interface is detected read-only; all live fan writes remain
blocked. See [FAN_MODES.md](FAN_MODES.md) for remaining production recovery gates.
