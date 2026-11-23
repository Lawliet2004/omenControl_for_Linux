#!/usr/bin/env python3
"""Exercise real daemon/CLI executables against isolated fixture hardware."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / os.environ.get("OMEN_TEST_PROFILE", "debug")


def call(path, operation, args=None, version=1):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(path)
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": version, "id": 17, "op": op}).encode() + b"\n")
        return json.loads(stream.makefile("rb").readline())


def run():
    checks = []
    with tempfile.TemporaryDirectory(prefix="omen-smoke-") as temp:
        env = dict(os.environ, OMEN_CONFIG_DIR=temp, OMEN_SYSTEM_CONFIG_DIR=temp)
        child = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"],
                                 stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True)
        try:
            path = child.stdout.readline().strip()
            assert path and Path(path).is_socket(), "daemon did not create its socket"
            assert Path(path).stat().st_mode & 0o777 == 0o600
            assert call(path, "get_info")["data"]["mock"] is True
            checks.append("private mock socket and explicit mock identity")
            assert not call(path, "ping", version=999)["ok"]
            checks.append("unsupported protocol version denied")
            assert call(path, "set_fan_mode", {"mode": {"kind": "curve"}})["ok"]
            assert call(path, "get_fan_state")["data"]["mode"]["kind"] == "manual"
            assert call(path, "set_fan_mode", {"mode": {"kind": "auto"}})["ok"]
            time.sleep(1.1)
            assert call(path, "get_fan_state")["data"]["mode"]["kind"] == "auto"
            checks.append("curve applies synchronously; Auto survives subsequent ticks")
            assert not call(path, "launch_game", {"id": "custom/anything"})["ok"]
            checks.append("privileged game execution refused")
            output = subprocess.check_output([str(BIN / "omen-cli"), "--socket", path, "fan"], text=True, env=env)
            assert json.loads(output)["mode"]["kind"] == "auto"
            checks.append("real CLI consumes daemon fan-state response")
            subprocess.run(["python3", str(ROOT / "scripts/profile_test.py"), "--socket", path,
                            "--output", f"{temp}/profile-fixture.json", "--fixture-test",
                            "--approve-balanced-restore"], env=env, check=True, stdout=subprocess.DEVNULL)
            assert json.loads(Path(f"{temp}/profile-fixture.json").read_text())["restored"]
            checks.append("profile-test supervisor restores original fixture state")
            child.send_signal(signal.SIGTERM)
            assert child.wait(timeout=3) == 0
            assert not Path(path).exists()
            checks.append("SIGTERM exits cleanly and removes owned socket")
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
    print(json.dumps({"fixture_only": True, "checks_passed": checks}, indent=2))


if __name__ == "__main__":
    run()
