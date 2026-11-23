# GPU monitoring across the service sandbox

Revision `0.1.0-5` fixes missing NVIDIA readings when a client connects to the
device-isolated daemon. The daemon retains `PrivateDevices=yes`. Its private
`/dev` excludes physical GPU nodes, so NVIDIA's driver query fails there even
though PCI discovery and the `nvidia-smi` executable are visible.

This follows the documented [systemd device isolation](https://freedesktop.org/software/systemd/man/latest/systemd.exec.html#PrivateDevices=).
The NVIDIA reader uses only the documented [selective query interface](https://docs.nvidia.com/deploy/nvidia-smi/index.html),
with fixed fields and a PCI bus ID. It sends no management command.

## Read path and limits

The shared `omen-client::monitoring::MonitoringClient` supplements successful
daemon reads in the unprivileged desktop/CLI process when NVIDIA samples are
missing. Existing daemon measurements remain authoritative. Failed responses,
root clients, mock identity, unknown identity and fixture samples receive no
local supplement. All control requests keep their existing daemon authorization
and capability checks. The daemon does not depend on `omen-client`.

The GUI identifies the local read-only fallback in its source label. Combined
diagnostics record the exact supplemented sensor IDs under `client_telemetry`;
raw IPC diagnostics continue to describe the daemon's own environment. The
daemon's NVIDIA capability now checks device-node access instead of declaring
telemetry available solely from an executable and bound PCI driver.

Local sampling caches successes and failures for five seconds, retains original
measurement timestamps, and discards the cache when the selected GPU disappears
or is reported suspended. It does not query a known suspended GPU. Each command
has a two-second deadline. Invalid/non-finite numbers, unsupported fields and
invalid VRAM totals are omitted rather than displayed as zero.

The existing single NVIDIA metric set refers to the first bound NVIDIA display
adapter in PCI order. `--id=PCI_ADDRESS` now selects that adapter explicitly,
avoiding an all-device query. Multiple NVIDIA metric sets and calibration
against independent physical instruments remain unimplemented/unverified.
Driver, permissions and metric support can still make individual values absent.
The local measurements are used for display/diagnostics; they do not feed the
daemon's fan-control loop. Live fan controls remain release-blocked.

## Validation

`tests/live_gpu.py` uses an already-active real NVIDIA GPU and a transient
systemd **user** service, without installing the package. A paired query with
physical devices visible succeeds. With device isolation enabled, the actual
daemon's namespace has no `/dev/nvidiactl` and a read-only sysfs view. Raw daemon
diagnostics return no NVIDIA samples; the connected CLI obtains the GPU name
and five numeric measurements from the local reader, and identifies their
origin. An app-only dashboard capture shows the connected GUI readings.

The baseline failed with the missing measurements and an incorrectly available
daemon capability. The fixed run passes. Fingerprints and observations are in:

* `evidence/gpu-monitoring-before.json` and its service journal.
* `evidence/gpu-monitoring.json`, including the paired query and namespace data.
* `evidence/gpu-monitoring-gui.json` and `screenshots/dashboard-sandboxed-service.png`.

The probe runs as UID 1000, uses a private mode-0700 directory in `/tmp`, and sets
`PrivateTmp=no` so that directory remains reachable. Other recorded namespace
settings include `ProtectHome=yes`, `ProtectSystem=strict` and the read-only sysfs
bind. It is not an installation of the root system service or proof of its full
physical deployment. The packaged root unit is separately tested in the VM.
All transient services and files are removed. No profile/fan/RGB/charging/graphics
control write is requested. The optional five-second resource window records both
GUI and daemon CPU/RSS, including reaped helper CPU; see TEST_REPORT.md for its
short-window and concurrent-workload limits. Numerical agreement across different sampling times
is not a calibration or thermal-stability test.

```bash
# Requires an existing user systemd manager, user namespaces, NVIDIA driver
# utilities and an already-active NVIDIA display GPU. No privilege elevation.
python3 tests/live_gpu.py
# Optional app-only capture in an existing graphical session:
python3 tests/live_gpu.py --capture
# Optional short connected monitoring resource window:
python3 tests/live_gpu.py --benchmark
```

Seven added Rust cases cover device-node detection, invalid CSV values, cache
reuse/expiry/suspend/adapter changes, mock/root/failure isolation, retention of
backend measurements and timestamps, diagnostics provenance, and capability
scope. Native settings diagnostics also assert that fixture reports contain no
live NVIDIA supplement.
