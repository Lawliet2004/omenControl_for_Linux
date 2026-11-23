#!/usr/bin/env python3
"""Independent fan-writer failure tests. Mutates private regular files only."""
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
DAEMON = ROOT / "target" / PROFILE / "omen-daemon"


def wait_for(predicate, timeout=8):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, "condition timed out"
        time.sleep(.025)


class Fixture:
    def __init__(self, directory, mock_flag="--mock-omen"):
        self.area = Path(directory)
        (self.area / "tmp").mkdir()
        self.log = (self.area / "daemon.log").open("w")
        self.helpers = set()
        self.child = subprocess.Popen(
            [str(DAEMON), mock_flag], stdout=subprocess.PIPE, stderr=self.log,
            text=True, env=dict(os.environ, TMPDIR=str(self.area / "tmp"),
                               OMEN_CONFIG_DIR=str(self.area / "user"),
                               OMEN_SYSTEM_CONFIG_DIR=str(self.area / "system")))
        assert select.select([self.child.stdout], [], [], 8)[0], "no fixture endpoint"
        self.endpoint = self.child.stdout.readline().strip()
        assert self.call("get_info")["mock"] is True
        self.hp = next((self.area / "tmp").glob("*/sys/class/hwmon/hwmon0"))
        assert (self.hp / "name").read_text().strip() == "hp"

    def request(self, operation, args=None):
        assert operation in {"get_info", "get_fan_state", "export_diagnostics", "set_fan_mode"}
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(10)
            stream.connect(self.endpoint)
            op = {"op": operation}
            if args is not None:
                op["args"] = args
            stream.sendall(json.dumps({"v": 1, "id": 41, "op": op}).encode() + b"\n")
            return json.loads(stream.makefile("rb").readline(65537))

    def call(self, operation):
        reply = self.request(operation)
        assert reply["ok"], reply
        return reply["data"]

    def diagnostics(self):
        data = self.call("export_diagnostics")
        supervisor = data.get("fan_supervisor")
        if supervisor and supervisor.get("pid"):
            self.helpers.add(supervisor["pid"])
        return data

    def arm(self, mode):
        assert self.call("get_info")["mock"] is True
        reply = self.request("set_fan_mode", {"mode": mode})
        assert reply["ok"], reply
        status = self.diagnostics()["fan_supervisor"]
        assert status["lease_active"] and not status["revoked"]
        assert status["pid"] != self.child.pid
        return status["pid"]

    def policy(self):
        return (self.hp / "pwm1_enable").read_text().strip()

    def close(self):
        if self.child.poll() is None:
            self.child.send_signal(signal.SIGCONT)
            self.child.terminate()
            try:
                self.child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=3)
        # The test process is a subreaper: reap only helpers it observed from
        # its own mock daemon, including after a deliberate parent SIGKILL.
        for pid in self.helpers:
            deadline = time.monotonic() + 5
            while True:
                try:
                    reaped, _ = os.waitpid(pid, os.WNOHANG)
                except ChildProcessError:
                    break  # Already reaped by the daemon.
                if reaped:
                    break
                assert time.monotonic() < deadline, f"owned fixture helper {pid} remained alive"
                time.sleep(.025)
        self.child.stdout.close()
        self.log.close()


def main():
    libc = ctypes.CDLL(None, use_errno=True)
    assert libc.prctl(36, 1, 0, 0, 0) == 0, "cannot become fixture child subreaper"
    result = {"result": "failed", "profile": PROFILE, "fixture_only": True,
              "hardware_control_writes": 0, "checks_passed": [],
              "daemon_sha256": hashlib.sha256(DAEMON.read_bytes()).hexdigest()}
    logs = []

    def passed(label):
        result["checks_passed"].append(label)
        print("PASS", label, flush=True)

    try:
        with tempfile.TemporaryDirectory(prefix="omen-guardian-", dir=ROOT / "target") as directory:
            base = Path(directory)

            def scenario(name, run, mock_flag="--mock-omen"):
                area = base / name
                area.mkdir()
                fixture = Fixture(area, mock_flag)
                try:
                    run(fixture)
                finally:
                    fixture.close()
                    logs.append(f"--- {name} ---\n" + (area / "daemon.log").read_text())

            def stalled_parent(fixture):
                first = fixture.arm({"kind": "manual", "duty_pct": 40})
                time.sleep(4.6)
                assert fixture.policy() == "1"
                passed("independent writer keeps a healthy fixed-duty lease alive beyond its four-second deadline")
                fixture.child.send_signal(signal.SIGSTOP)
                wait_for(lambda: fixture.policy() == "2", timeout=5.5)
                state = Path(f"/proc/{fixture.child.pid}/status").read_text()
                assert "State:\tT" in state and fixture.child.poll() is None
                passed("guardian restores Automatic while its daemon remains SIGSTOPped")
                fixture.child.send_signal(signal.SIGCONT)
                wait_for(lambda: fixture.diagnostics()["fan_policy"]["requested"] == {"kind": "auto"})
                time.sleep(1.1)
                assert fixture.policy() == "2"
                second = fixture.arm({"kind": "max"})
                assert first != second and fixture.policy() == "0"
                passed("resumed daemon cannot renew the revoked override; explicit rearming creates a fresh writer")
                # Normal shutdown removes the mock tree. Retain a read-only
                # descriptor so the final policy remains observable afterward.
                with (fixture.hp / "pwm1_enable").open() as final_policy:
                    fixture.child.terminate()
                    assert fixture.child.wait(timeout=6) == 0
                    assert final_policy.read().strip() == "2"
                assert not Path(fixture.endpoint).exists()
                passed("ordinary termination restores Automatic and removes the owned endpoint")

            scenario("stopped-parent", stalled_parent)

            def killed_parent(fixture):
                fixture.arm({"kind": "max"})
                started = time.monotonic()
                fixture.child.kill()
                fixture.child.wait(timeout=3)
                wait_for(lambda: fixture.policy() == "2", timeout=3)
                result["parent_sigkill_recovery_ms"] = round((time.monotonic() - started) * 1000, 1)
                passed("parent SIGKILL closes the private channel and the surviving writer restores its pinned fixture")

            scenario("killed-parent", killed_parent)

            def killed_helper(fixture):
                first = fixture.arm({"kind": "max"})
                os.kill(first, signal.SIGKILL)
                wait_for(lambda: fixture.policy() == "2")
                data = fixture.diagnostics()
                assert data["fan_policy"]["requested"] == {"kind": "auto"}
                assert not data["fan_policy"]["owns_override"]
                assert data["fan_supervisor"]["pid"] != first
                passed("a live daemon detects writer death and restores through a fresh independent writer")

            scenario("killed-helper", killed_helper)

            def lost_inode(fixture):
                fixture.arm({"kind": "max"})
                path = fixture.hp / "pwm1_enable"
                path.unlink()
                path.write_text("0")
                path.chmod(0o600)
                reply = fixture.request("set_fan_mode", {"mode": {"kind": "auto"}})
                assert not reply["ok"], reply
                policy = fixture.diagnostics()["fan_policy"]
                assert policy["owns_override"] and policy["recovery_error"]
                assert fixture.policy() == "0"
                passed("unlinked pinned actuator reports failed recovery and cannot redirect writes to a replacement path")

            scenario("lost-inode", lost_inode)
            def coarse_recovery(fixture):
                assert not (fixture.hp / "pwm1").exists()
                assert not (fixture.hp / "pwm2").exists()
                root = fixture.hp.parents[3]
                gpu = root / "sys/bus/pci/devices/0000:06:00.0"
                driver = root / "drivers/amdgpu"
                driver.mkdir(parents=True)
                (gpu / "driver").symlink_to(driver)
                (fixture.hp.parent / "hwmon2/device").symlink_to(gpu)
                fixture.arm({"kind": "max"})
                assert fixture.policy() == "0"
                time.sleep(4.6)
                assert fixture.policy() == "0"
                fixture.child.send_signal(signal.SIGSTOP)
                wait_for(lambda: fixture.policy() == "2", timeout=5.5)
                fixture.child.send_signal(signal.SIGCONT)
                wait_for(lambda: fixture.diagnostics()["fan_policy"]["requested"] == {"kind": "auto"})
                fixture.arm({"kind": "max"})
                fixture.child.kill()
                fixture.child.wait(timeout=3)
                wait_for(lambda: fixture.policy() == "2", timeout=3)
                assert (fixture.hp / "name").read_text().strip() == "hp"
                passed("mode-only Maximum lease renews, expires on stall and restores on parent SIGKILL without manual targets")

            scenario("coarse-recovery", coarse_recovery, "--mock")
            direct = subprocess.run([str(DAEMON), "--internal-fan-guard"],
                                    capture_output=True, text=True, timeout=3)
            assert direct.returncode != 0 and "invalid private guardian channel" in direct.stderr
            passed("internal helper refuses startup without its inherited private channel")
        result["result"] = "passed"
        result["cleanup_complete"] = True
    except BaseException as error:
        result["error"] = str(error)
        raise
    finally:
        result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence = ROOT / "docs/evidence"
        (evidence / f"fan-guardian-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
        (evidence / f"fan-guardian-{PROFILE}-daemon.log").write_text("\n".join(logs))


if __name__ == "__main__":
    main()
