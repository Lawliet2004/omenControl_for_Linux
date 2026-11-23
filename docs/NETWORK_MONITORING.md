# Read-only network monitoring

Revision `0.1.0-7` adds per-interface receive/send rates, link state and cumulative
error/drop counters to System Vitals, the CLI and diagnostics. It reads the
process's network namespace. It generates no test traffic, captures no packets,
opens no network socket and changes no network policy.

```bash
omen-cli network
omen-cli network --json --interval-ms 1000
```

The CLI takes two acquisitions, waiting 250–10000 ms. Rates use their actual
interval. Connected clients use production `get_network` IPC; direct clients use
the same read-only backend. An older daemon reports the additive operation
unsupported, without a fallback to local counters from another source/namespace.
Existing protocol operations and authorization remain unchanged.

## Counter semantics

A batched `/proc/net/dev` read supplies byte/error/drop counters. Read-only
`/sys/class/net` supplies ifindex, directory identity, link state and loopback
flags. See the [Linux interface statistics ABI](https://docs.kernel.org/networking/statistics.html).
Procfs folds RX missed errors into RX drops; the JSON field is explicitly
`rx_dropped_or_missed`, and the UI explains it.

Interfaces are shown separately in name order. VPNs, bridges and physical
interfaces may count the same traffic, so no system total is fabricated. Link
state does not establish Internet connectivity or available bandwidth. Drops
and errors are cumulative kernel reports, not application packet-loss rates.
No IP/MAC addresses, routes, process attribution or packet contents are collected.

Rates use bytes per second and binary KiB/MiB units. First samples and resets
say Collecting; missing byte counters are Unavailable. Unchanged valid counters
produce a measured zero. A counter decrease or interface identity change rebases
that direction. Whole-read failure clears the baseline; recovery starts fresh.

Sampling uses Linux `CLOCK_BOOTTIME`, which includes suspend time; see
[Linux timekeeping](https://docs.kernel.org/core-api/timekeeping.html).
Gaps over 30 seconds suppress rates and rebase. This is not a physical
suspend/resume validation. Successful reads cache for 250 ms and retain their
original timestamp/rates. Missing interfaces lose their previous baseline.
ifindex/directory identity are checked around the non-atomic procfs/sysfs reads;
this does not guarantee detection of arbitrary same-value/ABA replacement.

Input is bounded at 1 MiB and reports at most 128 readable interfaces, with
explicit omitted/unreadable counts. Unknown metadata is not guessed. Fixture
backends report `source: fixture` and never substitute host interfaces. The
daemon's existing device/filesystem restrictions remain enabled. There are no
traffic-priority or privileged network-control commands.

## Selecting readings

The GUI retains the same native reading object while the interface identity is
unchanged. A focused copy selection holds that reading's text stable, with a
visible **Reading held for selection** notice when newer values arrive. Clearing
the selection or leaving the reading resumes current values on the next poll.
Collection continues, and missing data or a changed interface identity removes
the held row immediately. This is a temporary selectable snapshot, not a stopped
network monitor.

## Validation

Five added Rust tests cover formatting and missing/idle states, exact deltas,
cache timestamps, resets, gaps, invalid/missing input, disappearance/replacement,
same-index recreation, loopback metadata and input/interface bounds.

`tests/network.py` passes nine scenarios per build using real daemon/CLI
processes and production IPC. Fixture changes verify arithmetic, resets,
identity changes, failure/recovery, CLI timing bounds and diagnostics. An
extracted revision 6 daemon verifies unsupported-operation behavior. The live
read-only scenario starts an unprivileged daemon with controls disabled and
brackets returned byte counters with independent sysfs reads for four stable
interfaces. It generates no traffic and performs no control writes. Evidence:
`evidence/network-{debug,release}.json` and associated logs.

`tests/gui_settings.py` adds four native AT-SPI Vitals workflows per build:
fixture idle rates, rate/error/drop updates, missing-data reason, and interface
row replacement after recovery. Its 16 settings checks remain separate.
`scripts/capture-network.py` captures only the app's GTK tree on Wayland for live
read-only and fixture data. Both images were inspected; fingerprints are in
`evidence/network-captures.json` and images in `screenshots/vitals-network*.png`.

Container and VM suites also check UID 1000 network reads through the installed
daemon without authorization grants. Those use virtual network namespaces,
not physical throughput validation. No speed test, calibration, packet capture,
physical interface churn or host suspend experiment was run.

```bash
python3 tests/network.py
OMEN_TEST_PROFILE=release python3 tests/network.py
GDK_BACKEND=wayland python3 tests/gui_settings.py
```
