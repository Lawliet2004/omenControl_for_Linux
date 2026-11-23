#!/usr/bin/env python3
"""Fan telemetry through production IPC/CLI; only private fixtures are mutated."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE


def request(endpoint, operation, args=None, fixture=False):
    allowed = {"get_info", "get_fan_state", "get_capabilities"}
    assert operation in allowed or (fixture and operation == "set_fan_mode")
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(6)
        stream.connect(str(endpoint))
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 29, "op": op}).encode() + b"\n")
        line = stream.makefile("rb").readline(65537)
        assert len(line) <= 65536
        return json.loads(line)


def call(endpoint, operation):
    response = request(endpoint, operation)
    assert response["ok"], response
    return response["data"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live-readonly", action="store_true", help="Also compare read-only mode on the reference HP host")
    args = parser.parse_args()
    result = {
        "result": "failed", "profile": PROFILE, "hardware_control_writes": 0,
        "binary_sha256": {name: hashlib.sha256((BIN / name).read_bytes()).hexdigest()
                          for name in ["omen-daemon", "omen-cli"]},
        "checks_passed": [], "live_operations": ["get_info", "get_fan_state", "get_capabilities"],
    }
    children, logs = [], []
    with tempfile.TemporaryDirectory(prefix="omen-fan-monitoring-", dir=ROOT / "target") as directory:
        temp = Path(directory)

        def start(name, mock_flag):
            area = temp / name
            (area / "tmp").mkdir(parents=True)
            env = dict(os.environ, TMPDIR=str(area / "tmp"), OMEN_CONFIG_DIR=str(area / "user"),
                       OMEN_SYSTEM_CONFIG_DIR=str(area / "system"), OMEN_SOCK=str(area / "control.sock"))
            log = (area / "daemon.log").open("w")
            logs.append(log)
            child = subprocess.Popen([str(BIN / "omen-daemon")] + ([mock_flag] if mock_flag else []),
                                     env=env, stdout=subprocess.PIPE, stderr=log, text=True)
            children.append(child)
            if mock_flag:
                endpoint = child.stdout.readline().strip()
            else:
                endpoint = env["OMEN_SOCK"]
                deadline = time.monotonic() + 8
                while not Path(endpoint).is_socket():
                    assert child.poll() is None and time.monotonic() < deadline
                    time.sleep(.02)
            assert call(endpoint, "get_info")["mock"] is bool(mock_flag)
            return child, endpoint, env, area

        def fixture_hp(area):
            matches = list((area / "tmp").glob("*/sys/class/hwmon/hwmon0"))
            assert len(matches) == 1 and (matches[0] / "name").read_text().strip() == "hp"
            return matches[0]

        def passed(label):
            result["checks_passed"].append(label)
            print("PASS", label, flush=True)

        try:
            _, endpoint, env, area = start("coarse", "--mock")
            hp = fixture_hp(area)
            state = call(endpoint, "get_fan_state")
            assert state["mode"] == {"kind": "auto"} and state["duty_pct"] is None
            caps = call(endpoint, "get_capabilities")
            assert caps["fan_control"]["status"] != "available" and caps["fan_curve"]["status"] != "available"
            command = [str(BIN / "omen-cli"), "--socket", endpoint, "fan"]
            cli = json.loads(subprocess.run(command, env=env, capture_output=True, text=True,
                                            check=True, timeout=8).stdout)
            assert cli == state
            passed("mode-only Victus exposes Auto through production IPC and CLI without granting manual control")

            for raw, kind in [("0", "max"), ("2", "auto")]:
                (hp / "pwm1_enable").write_text(raw)
                changed = call(endpoint, "get_fan_state")
                assert changed["mode"] == {"kind": kind} and changed["duty_pct"] is None
                assert changed["cpu_rpm"] == 2400 and changed["gpu_rpm"] == 2300
            passed("fixture mode-only changes are observed without inventing PWM targets")

            for raw in ["99", "invalid", "1"]:
                (hp / "pwm1_enable").write_text(raw)
                unknown = call(endpoint, "get_fan_state")
                assert unknown["mode"] is None and unknown["duty_pct"] is None
                assert unknown["cpu_rpm"] == 2400
            (hp / "pwm1_enable").unlink()
            assert call(endpoint, "get_fan_state")["mode"] is None
            (hp / "pwm1_enable").write_text("2")
            (hp / "pwm1_enable").chmod(0o600)
            passed("invalid/missing policy and missing manual duty remain unknown while RPM survives")

            assert call(endpoint, "get_info")["mock"] is True
            before = (hp / "pwm1_enable").read_bytes()
            denied = request(endpoint, "set_fan_mode", {"mode": {"kind": "manual", "duty_pct": 50}}, fixture=True)
            assert not denied["ok"] and (hp / "pwm1_enable").read_bytes() == before
            fixture_root = hp.parents[3]
            gpu = fixture_root / "sys/bus/pci/devices/0000:06:00.0"
            driver = fixture_root / "drivers/amdgpu"
            driver.mkdir(parents=True)
            (gpu / "driver").symlink_to(driver)
            (hp.parent / "hwmon2/device").symlink_to(gpu)
            maximum = request(endpoint, "set_fan_mode", {"mode": {"kind": "max"}}, fixture=True)
            assert maximum["ok"], maximum
            assert (hp / "pwm1_enable").read_text().strip() == "0"
            automatic = request(endpoint, "set_fan_mode", {"mode": {"kind": "auto"}}, fixture=True)
            assert automatic["ok"], automatic
            assert (hp / "pwm1_enable").read_text().strip() == "2"
            assert not (hp / "pwm1").exists() and not (hp / "pwm2").exists()
            passed("mode-only fixture switches Maximum/Automatic through IPC while refusing manual duty")

            (hp / "name").write_text("not-hp")
            assert not request(endpoint, "get_fan_state")["ok"]
            (hp / "name").write_text("hp")
            assert call(endpoint, "get_fan_state")["mode"] == {"kind": "auto"}
            passed("device disappearance fails explicitly and recovers without a host-data fallback")

            _, writable, _, other_area = start("manual", "--mock-omen")
            other_hp = fixture_hp(other_area)
            (other_hp / "pwm1_enable").write_text("1")
            for raw in ["256", "bad"]:
                (other_hp / "pwm1").write_text(raw)
                invalid = call(writable, "get_fan_state")
                assert invalid["mode"] is None and invalid["duty_pct"] is None
            (other_hp / "pwm1").write_text("128")
            assert call(writable, "get_fan_state")["mode"] == {"kind": "manual", "duty_pct": 50}
            (other_hp / "pwm1_enable").write_text("2")
            passed("malformed/out-of-range manual readings are absent and valid duty recovers")

            fixture_root = other_hp.parents[3]
            assert (fixture_root / "sys/class/dmi/id/board_name").is_file()
            conflict = fixture_root / "proc/43123"
            conflict.mkdir(parents=True)
            (conflict / "comm").write_text("omencore-cli\n")
            conflict_caps = call(writable, "get_capabilities")
            assert conflict_caps["fan_control"]["status"] != "available"
            assert "omencore-cli" in conflict_caps["fan_control"]["detail"]
            assert call(writable, "get_info")["mock"] is True
            protected = {path.name: path.read_bytes() for path in other_hp.glob("pwm*")}
            assert not request(writable, "set_fan_mode", {"mode": {"kind": "max"}}, fixture=True)["ok"]
            assert protected == {path.name: path.read_bytes() for path in other_hp.glob("pwm*")}
            shutil.rmtree(conflict)
            assert call(writable, "get_capabilities")["fan_control"]["status"] == "available"
            passed("OmenCore process presence blocks fixture writes before mutation and removal releases the gate")

            if args.live_readonly:
                _, live, live_env, live_area = start("readonly", None)
                live_caps = call(live, "get_capabilities")
                assert all(live_caps[key]["status"] != "available" for key in ["fan_modes", "fan_control", "fan_curve"])
                hp_devices = [path for path in Path("/sys/class/hwmon").glob("hwmon*")
                              if (path / "name").read_text().strip() == "hp"]
                assert len(hp_devices) == 1
                live_hp = hp_devices[0]
                before = (live_hp / "pwm1_enable").read_text().strip()
                actual = call(live, "get_fan_state")
                after = (live_hp / "pwm1_enable").read_text().strip()
                assert before == after and before in ["0", "2"]
                assert actual["mode"] == {"kind": {"0": "max", "2": "auto"}[before]}
                if not (live_hp / "pwm1").exists():
                    assert actual["duty_pct"] is None
                direct_command = [str(BIN / "omen-cli"), "--socket", str(live_area / "missing.sock"), "fan"]
                direct = json.loads(subprocess.run(direct_command, env=live_env, capture_output=True,
                                                   text=True, check=True, timeout=8).stdout)
                final = (live_hp / "pwm1_enable").read_text().strip()
                assert final == before and direct["mode"] == actual["mode"]
                result["live_readonly"] = {"uid": os.geteuid(), "kernel": os.uname().release,
                                           "hwmon": str(live_hp), "policy_before": before,
                                           "policy_after": final, "daemon_state": actual, "direct_cli_state": direct,
                                           "pwm1_present": (live_hp / "pwm1").exists(),
                                           "scope": "Kernel-reported mode only; RPM calibration, physical response and recovery are not tested."}
                assert "op 'set_" not in (live_area / "daemon.log").read_text()
                passed("real unprivileged daemon and direct CLI agree with independent sysfs mode reads; controls stay disabled")
            result["result"] = "passed"
        except BaseException as error:
            result["error"] = str(error)
            raise
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    assert child.wait(timeout=8) == 0
            for log in logs:
                log.close()
            for path in temp.glob("*/daemon.log"):
                (ROOT / "docs/evidence" / f"fan-monitoring-{PROFILE}-{path.parent.name}-daemon.log").write_text(path.read_text())
            result["cleanup_complete"] = True
            result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            (ROOT / "docs/evidence" / f"fan-monitoring-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
