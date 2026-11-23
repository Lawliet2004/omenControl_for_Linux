#!/usr/bin/env python3
"""Production curve/failure/scheduling tests on private ordinary-file fixtures only."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE


def request(endpoint, operation, args=None):
    assert operation in {"get_info", "get_fan_state", "export_diagnostics", "set_fan_mode"}
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(6)
        stream.connect(str(endpoint))
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 37, "op": op}).encode() + b"\n")
        return json.loads(stream.makefile("rb").readline(65537))


def main():
    result = {"result": "failed", "profile": PROFILE, "fixture_only": True,
              "hardware_control_writes": 0, "checks_passed": [],
              "daemon_sha256": hashlib.sha256((BIN / "omen-daemon").read_bytes()).hexdigest()}
    child = None
    paused = False
    fifo = None
    with tempfile.TemporaryDirectory(prefix="omen-thermal-", dir=ROOT / "target") as directory:
        temp = Path(directory)
        (temp / "tmp").mkdir()
        (temp / "system").mkdir()
        (temp / "system/config.toml").write_text(
            'version = 1\n[power]\nbattery_aware = true\non_battery_profile = "balanced"\n')
        env = dict(os.environ, OMEN_CONFIG_DIR=str(temp / "user"), OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"),
                   TMPDIR=str(temp / "tmp"))
        with (temp / "daemon.log").open("w") as log:
            try:
                child = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"], env=env,
                                         stdout=subprocess.PIPE, stderr=log, text=True)
                endpoint = child.stdout.readline().strip()

                def call(operation):
                    reply = request(endpoint, operation)
                    assert reply["ok"], reply
                    return reply["data"]

                assert call("get_info")["mock"] is True
                hp = next((temp / "tmp").glob("*/sys/class/hwmon/hwmon0"))
                root = hp.parents[3]
                assert (hp / "name").read_text().strip() == "hp"
                gpu = root / "sys/bus/pci/devices/0000:06:00.0"
                (gpu / "power").mkdir(parents=True)
                (gpu / "class").write_text("0x030000")
                (gpu / "vendor").write_text("0x1002")
                (gpu / "power/runtime_status").write_text("active")
                driver = root / "drivers/amdgpu"
                driver.mkdir(parents=True)
                (gpu / "driver").symlink_to(driver)
                thermal = root / "sys/class/hwmon/hwmon2"
                thermal.mkdir()
                (thermal / "name").write_text("amdgpu")
                (thermal / "temp1_input").write_text("45000")
                (thermal / "device").symlink_to(gpu)

                def passed(label):
                    result["checks_passed"].append(label)
                    print("PASS", label, flush=True)

                def wait_for(predicate, timeout=5):
                    deadline = time.monotonic() + timeout
                    while not predicate():
                        assert child.poll() is None, "fixture daemon exited unexpectedly"
                        assert time.monotonic() < deadline, "condition timed out"
                        time.sleep(.05)

                def arm():
                    assert call("get_info")["mock"] is True
                    reply = request(endpoint, "set_fan_mode", {"mode": {"kind": "curve"}})
                    assert reply["ok"], reply

                def auto():
                    return call("get_fan_state")["mode"] == {"kind": "auto"}

                for fixed in [{"kind": "max"}, {"kind": "manual", "duty_pct": 80}]:
                    (thermal / "device").unlink()
                    (hp / "pwm1_enable").write_text("0")
                    reply = request(endpoint, "set_fan_mode", {"mode": fixed})
                    assert not reply["ok"] and (hp / "pwm1_enable").read_text().strip() == "0"
                    (thermal / "device").symlink_to(gpu)
                    reply = request(endpoint, "set_fan_mode", {"mode": fixed})
                    assert reply["ok"], reply
                    policy = call("export_diagnostics")["fan_policy"]
                    assert policy["guard"] and policy["requested"] == fixed
                    (thermal / "temp1_input").write_text("invalid")
                    wait_for(auto)
                    (thermal / "temp1_input").write_text("45000")
                    time.sleep(1.1)
                    assert auto()
                    passed(f"fixed {fixed['kind']} requires CPU/GPU preflight, recovers on data loss and does not resume without rearming")

                arm()
                diagnostic = call("export_diagnostics")
                assert diagnostic["fan_policy"]["guard"]["frame"]["hottest_c"] == 61.8
                assert any(sample.get("device_id") == "0000:06:00.0" for sample in diagnostic["sensors"])
                passed("curve arms with identified CPU and GPU inputs; diagnostics retain the guarded cohort")

                (thermal / "temp1_input").write_text("90000")
                wait_for(lambda: (call("get_fan_state")["duty_pct"] or 0) >= 99)
                passed("a hot GPU raises the real fixture adapter to full curve duty while CPU temperature stays fixed")

                (thermal / "temp1_input").write_text("invalid")
                wait_for(auto)
                policy = call("export_diagnostics")["fan_policy"]
                assert policy["guard"] is None and policy["last_error"] and not policy["owns_override"]
                (thermal / "temp1_input").write_text("45000")
                time.sleep(1.1)
                assert auto()
                passed("invalid GPU data stops and restores the curve; returning data does not rearm it")

                (thermal / "device").unlink()
                (hp / "pwm1_enable").write_text("0")
                failed = request(endpoint, "set_fan_mode", {"mode": {"kind": "curve"}})
                assert not failed["ok"] and (hp / "pwm1_enable").read_text().strip() == "0"
                (thermal / "device").symlink_to(gpu)
                passed("missing GPU association fails preflight without claiming or restoring an external fixture override")

                arm()
                (gpu / "power/runtime_status").write_text("suspended")
                wait_for(auto)
                (gpu / "power/runtime_status").write_text("active")
                time.sleep(1.1)
                assert auto()
                passed("GPU power-state changes invalidate the pinned cohort and require explicit rearming")

                arm()
                child.send_signal(signal.SIGSTOP)
                paused = True
                time.sleep(3.4)
                child.send_signal(signal.SIGCONT)
                paused = False
                wait_for(auto)
                passed("a real process scheduling gap is detected after continuation and returns the fixture to Auto")

                arm()
                # Invalidate the already-pinned controller, so both the policy
                # worker and its independent writer must refuse the descriptor.
                (hp / "name").write_text("other")
                wait_for(lambda: call("export_diagnostics")["fan_policy"]["recovery_error"] is not None)
                assert call("export_diagnostics")["fan_policy"]["owns_override"] is True
                (hp / "name").write_text("hp")
                wait_for(auto)
                # The independent writer may restore before the daemon has
                # consumed its acknowledgement and released logical ownership.
                wait_for(lambda: call("export_diagnostics")["fan_policy"]["owns_override"] is False)
                passed("a blocked Auto recovery remains owned and retries after pinned controller identity returns")

                arm()
                fifo = root / "sys/firmware/acpi/platform_profile"
                assert fifo.read_text().strip() == "balanced"
                fifo.unlink()
                os.mkfifo(fifo, 0o600)
                (root / "sys/class/power_supply/BAT0/capacity").write_text("10")

                def power_is_blocked():
                    for task in Path(f"/proc/{child.pid}/task").iterdir():
                        try:
                            if (task / "comm").read_text().strip() == "power-policy":
                                state = (task / "wchan").read_text().strip()
                                if state in ["wait_for_partner", "pipe_read"]:
                                    result["blocked_power_worker"] = {"tid": int(task.name), "wchan": state}
                                    return True
                        except OSError:
                            pass
                    return False

                wait_for(power_is_blocked, timeout=13)
                (thermal / "temp1_input").write_text("90000")
                wait_for(lambda: (call("get_fan_state")["duty_pct"] or 0) >= 99)
                passed("the named power worker is blocked on fixture profile IO while independent fan ticks still follow GPU heat")

                child.terminate()
                deadline = time.monotonic() + 4
                while (hp / "pwm1_enable").read_text().strip() != "2":
                    assert time.monotonic() < deadline
                    time.sleep(.02)
                assert child.poll() is None
                descriptor = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
                os.write(descriptor, b"performance\n")
                os.close(descriptor)
                fifo = None
                assert child.wait(timeout=6) == 0
                assert not Path(endpoint).exists()
                passed("shutdown restores Auto before profile IO drains and performs no late profile write after the blocked read returns")
                result["result"] = "passed"
            except BaseException as error:
                result["error"] = str(error)
                raise
            finally:
                if child is not None and child.poll() is None:
                    if paused:
                        child.send_signal(signal.SIGCONT)
                    child.terminate()
                    if fifo is not None:
                        try:
                            descriptor = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
                            os.write(descriptor, b"balanced\n")
                            os.close(descriptor)
                        except OSError:
                            pass
                    try:
                        child.wait(timeout=6)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=3)
                log.flush()
                (ROOT / "docs/evidence" / f"thermal-{PROFILE}-daemon.log").write_text((temp / "daemon.log").read_text())
                result["cleanup_complete"] = child is None or child.poll() is not None
                result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
                (ROOT / "docs/evidence" / f"thermal-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
