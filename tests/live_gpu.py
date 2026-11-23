#!/usr/bin/env python3
"""Read-only GPU regression on an actual user systemd service; no host install."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record", default="gpu-monitoring.json")
    parser.add_argument("--capture", action="store_true", help="Capture only the connected GUI's own dashboard widget tree")
    parser.add_argument("--benchmark", action="store_true", help="Measure a five-second connected GUI/daemon monitoring window")
    args = parser.parse_args()
    assert Path(args.record).name == args.record and args.record.endswith(".json")
    assert os.geteuid() != 0
    binary = ROOT / "target/release"
    unit = "omen-gpu-readonly-" + uuid.uuid4().hex[:10]
    result = {"result": "failed", "hardware_writes": False, "host_package_installed": False,
              "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "method": "transient systemd user service with real GPU/sysfs and PrivateDevices; production read-only IPC and CLI",
              "uid": os.geteuid(), "binary_sha256": {name: hashlib.sha256((binary / name).read_bytes()).hexdigest()
                                                     for name in ["omen-cli", "omen-daemon"]}}
    result["kernel"] = os.uname().release
    result["nvidia_driver_version"] = Path("/sys/module/nvidia/version").read_text().strip()
    # ProtectHome also hides /run/user. Keep the user-owned executable outside
    # that hierarchy; this probe's private 0700 directory replaces PrivateTmp.
    runtime = Path("/tmp")
    with tempfile.TemporaryDirectory(prefix="omen-gpu-probe-", dir=runtime) as directory:
        temp = Path(directory)
        started = False
        try:
            candidates = []
            for pci in sorted(Path("/sys/bus/pci/devices").iterdir()):
                if (pci / "vendor").read_text().strip() != "0x10de":
                    continue
                if not (pci / "class").read_text().strip().startswith("0x03"):
                    continue
                candidates.append({"pci": pci.name, "runtime_status": (pci / "power/runtime_status").read_text().strip(),
                                   "driver": (pci / "driver").resolve().name})
            result["gpus"] = candidates
            assert candidates and candidates[0]["runtime_status"] == "active" and candidates[0]["driver"] == "nvidia", "requires an already-active NVIDIA GPU; will not wake a suspended device"
            reference_args = ["/usr/bin/nvidia-smi", "--id=" + candidates[0]["pci"],
                              "--query-gpu=name,temperature.gpu,utilization.gpu,power.draw,clocks.sm,memory.used,memory.total",
                              "--format=csv,noheader,nounits"]
            reference = subprocess.run(reference_args, capture_output=True, text=True, timeout=5)
            result["independent_reference"] = {"args": reference_args, "exit_code": reference.returncode, "stdout": reference.stdout, "stderr": reference.stderr}
            assert reference.returncode == 0 and reference.stdout.strip()
            shutil.copyfile(binary / "omen-daemon", temp / "omen-daemon")
            (temp / "omen-daemon").chmod(0o700)
            (temp / "config").mkdir()
            env = dict(os.environ, OMEN_CONFIG_DIR=str(temp / "config"), OMEN_SYSTEM_CONFIG_DIR=str(temp / "config"),
                       OMEN_SOCK=str(temp / "ctl.sock"))
            properties = ["PrivateDevices=yes", "ProtectSystem=strict", "ProtectHome=yes", "BindReadOnlyPaths=/sys",
                          "ProtectKernelTunables=yes", "ReadWritePaths=" + str(temp), "PrivateTmp=no",
                          "RestrictAddressFamilies=AF_UNIX", "ProtectKernelModules=yes", "ProtectKernelLogs=yes",
                          "ProtectControlGroups=yes", "NoNewPrivileges=yes", "LockPersonality=yes", "RestrictSUIDSGID=yes", "UMask=0077"]
            result["properties"] = properties
            # Paired read-only control: the same sandbox settings with physical
            # devices visible must still be able to query the active GPU.
            control = ["systemd-run", "--user", "--unit=" + unit + "-control", "--wait", "--pipe", "--collect"]
            control += ["--property=" + ("PrivateDevices=no" if prop == "PrivateDevices=yes" else prop) for prop in properties]
            control += reference_args
            observed = subprocess.run(control, capture_output=True, text=True, timeout=15)
            result["device_visibility_control"] = {"exit_code": observed.returncode, "stdout": observed.stdout, "stderr": observed.stderr}
            assert observed.returncode == 0 and observed.stdout.strip(), "control query failed with GPU device nodes visible"
            probe = "import json,os;from pathlib import Path; p=Path(" + repr(str(temp)) + ");(p/'namespace.json').write_text(json.dumps({'uid':os.geteuid(),'nvidiactl_visible':Path('/dev/nvidiactl').exists(),'sysfs_readonly':bool(os.statvfs('/sys').f_flag & os.ST_RDONLY),'mount_namespace':os.readlink('/proc/self/ns/mnt')}));os.execv(str(p/'omen-daemon'),[str(p/'omen-daemon')])"
            command = ["systemd-run", "--user", "--unit=" + unit, "--collect"]
            command += ["--property=" + prop for prop in properties]
            command += ["--setenv=" + key + "=" + env[key] for key in ["OMEN_CONFIG_DIR", "OMEN_SYSTEM_CONFIG_DIR", "OMEN_SOCK"]]
            command += ["/usr/bin/python3", "-c", probe]
            subprocess.run(command, check=True, capture_output=True, text=True)
            started = True
            deadline = time.monotonic() + 10
            while not Path(env["OMEN_SOCK"]).exists():
                assert time.monotonic() < deadline, "transient daemon did not become ready"
                time.sleep(.05)
            def call(operation):
                assert operation in ["get_info", "get_capabilities", "get_sensors", "export_diagnostics"]
                with socket.socket(socket.AF_UNIX) as stream:
                    stream.settimeout(5)
                    stream.connect(env["OMEN_SOCK"])
                    stream.sendall(json.dumps({"v": 1, "id": 1, "op": {"op": operation}}).encode() + b"\n")
                    reply = json.loads(stream.makefile("rb").readline())
                    assert reply["ok"], reply
                    return reply["data"]
            result["namespace"] = json.loads((temp / "namespace.json").read_text())
            assert result["namespace"]["nvidiactl_visible"] is False and result["namespace"]["sysfs_readonly"] is True
            result["daemon_info"] = call("get_info")
            assert result["daemon_info"]["mock"] is False
            raw = call("export_diagnostics")
            result["daemon_nvidia_capability"] = raw["capabilities"]["nvidia_telemetry"]
            result["daemon_nvidia_samples"] = [s for s in raw["sensors"] if s["source"] == "nvidia_smi"]
            assert not result["daemon_nvidia_samples"]
            assert result["daemon_nvidia_capability"]["status"] == "unavailable"
            assert raw["config"]["power"]["battery_aware"] is False
            assert raw["capabilities"]["performance_profiles"]["status"] != "available"
            cli = subprocess.run([str(binary / "omen-cli"), "--socket", env["OMEN_SOCK"], "diagnostics"],
                                 env=env, capture_output=True, text=True, timeout=12)
            assert cli.returncode == 0, cli.stderr
            client = json.loads(cli.stdout)
            result["client_nvidia_capability"] = client["capabilities"]["nvidia_telemetry"]
            result["client_nvidia_samples"] = [s for s in client["sensors"] if s["source"] == "nvidia_smi"]
            result["client_telemetry"] = client.get("client_telemetry")
            assert result["client_nvidia_capability"]["status"] == "available"
            assert any(s["id"] == "nvidia/temp" and s["value"] is not None for s in result["client_nvidia_samples"]), "connected CLI lost NVIDIA measurements behind the device sandbox"
            assert result["client_telemetry"], "local readings must identify their origin"
            if args.capture:
                capture = ROOT / "docs/screenshots/dashboard-sandboxed-service.png"
                childenv = dict(env, OMEN_REPORT_READY="1", OMEN_CAPTURE_PATH=str(capture))
                captured = subprocess.run([str(binary / "omen-gui"), "--page", "dashboard"], env=childenv,
                                         check=True, capture_output=True, text=True, timeout=20)
                result["gui_capture"] = {"path": str(capture.relative_to(ROOT)),
                                         "sha256": hashlib.sha256(capture.read_bytes()).hexdigest(),
                                         "gtk_display_type": next(line.split(" ",1)[1] for line in captured.stderr.splitlines()
                                                                  if line.startswith("OMEN_UI_BACKEND ")),
                                         "gui_sha256": hashlib.sha256((binary / "omen-gui").read_bytes()).hexdigest()}
            if args.benchmark:
                resources = runpy.run_path(str(ROOT / "scripts/benchmark.py"))["resources"]
                childenv = dict(env, OMEN_REPORT_READY="1")
                gui = subprocess.Popen([str(binary / "omen-gui"), "--new-instance", "--page", "dashboard"], env=childenv,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                try:
                    time.sleep(2)
                    assert gui.poll() is None
                    daemon_pid = int(subprocess.check_output(["systemctl", "--user", "show", unit, "--property=MainPID", "--value"]))
                    before = {"gui": resources(gui.pid), "daemon": resources(daemon_pid)}
                    start = time.monotonic()
                    time.sleep(5)
                    elapsed = time.monotonic() - start
                    after = {"gui": resources(gui.pid), "daemon": resources(daemon_pid)}
                    hz = os.sysconf("SC_CLK_TCK")
                    result["connected_monitoring"] = {
                        "method": "five-second warm window; GUI connected to device-isolated user daemon; percent of one logical core; includes local read-only NVIDIA fallback",
                        "seconds": elapsed,
                        "gui_sha256": hashlib.sha256((binary / "omen-gui").read_bytes()).hexdigest(),
                        "processes": {name: {"rss_kib": after[name][2],
                                             "cpu_pct_one_core": (after[name][0] - before[name][0]) / hz / elapsed * 100,
                                             "reaped_children_cpu_pct_one_core": (after[name][1] - before[name][1]) / hz / elapsed * 100}
                                      for name in before}}
                finally:
                    if gui.poll() is None:
                        gui.terminate()
                        gui.wait(timeout=8)
            result["result"] = "passed"
        except BaseException as error:
            result["error"] = str(error)
        finally:
            if started:
                journal = subprocess.run(["journalctl", "--user", "--unit=" + unit, "--no-pager", "-n", "100"], capture_output=True, text=True)
                (ROOT / "docs/evidence" / (Path(args.record).stem + ".log")).write_text(journal.stdout + journal.stderr)
                subprocess.run(["systemctl", "--user", "stop", unit], capture_output=True)
                assert subprocess.run(["systemctl", "--user", "is-active", unit], capture_output=True).returncode != 0
            result["service_stopped"] = True
            result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    result["temporary_files_removed"] = not temp.exists()
    (ROOT / "docs/evidence" / args.record).write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if result["result"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
