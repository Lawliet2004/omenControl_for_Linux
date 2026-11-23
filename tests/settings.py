#!/usr/bin/env python3
"""Exercise scoped settings through the actual CLI and isolated mock daemon."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / os.environ.get("OMEN_TEST_PROFILE", "debug")


def call(path, op):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(path)
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": {"op": op}}).encode() + b"\n")
        result = json.loads(stream.makefile("rb").readline())
        assert result["ok"], result
        return result["data"]


def main():
    checks = []
    with tempfile.TemporaryDirectory(prefix="omen-settings-tests-") as temporary:
        temp = Path(temporary)
        user = temp / "user"
        system = temp / "system"
        user.mkdir()
        env = dict(os.environ, OMEN_CONFIG_DIR=str(user), OMEN_SYSTEM_CONFIG_DIR=str(system), OMEN_HOME=str(temp / "empty-home"))
        user_config = user / "config.toml"
        user_config.write_text('version = 1\n[general]\nstart_minimized = true\n[games.per_game_profiles]\n"steam/620" = "quiet"\n[[games.custom_games]]\nid = "custom/scoped-test"\nname = "Scoped settings fixture"\nsource = "custom"\nlaunch_cmd = "true"\n')
        child = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            path = child.stdout.readline().strip()
            assert call(path, "get_info")["mock"]
            original_service = call(path, "get_config")
            assert original_service["general"]["start_minimized"] is False
            assert original_service["games"]["per_game_profiles"] == {}
            assert not (system / "config.toml").exists()
            checks.append("missing service config never loads the desktop user's config")
            def cli(*args, success=True):
                result = subprocess.run([str(BIN / "omen-cli"), "--socket", path, "settings", *args], env=env, cwd=temp, capture_output=True, text=True)
                assert (result.returncode == 0) == success, result.stderr
                return result
            cli("export", "--scope", "user", "user.json")
            backup = json.loads((temp / "user.json").read_text())
            assert backup["scope"] == "user"
            assert "power" not in backup["settings"] and "fan" not in backup["settings"]
            assert (temp / "user.json").stat().st_mode & 0o777 == 0o600
            backup["settings"]["general"]["start_minimized"] = False
            (temp / "user.json").write_text(json.dumps(backup))
            cli("import", "--scope", "user", "user.json")
            assert tomllib.loads(user_config.read_text())["general"]["start_minimized"] is False
            assert call(path, "get_config") == original_service
            assert tomllib.loads(user_config.with_suffix(".toml.bak").read_text())["general"]["start_minimized"] is True
            games = subprocess.check_output([str(BIN / "omen-cli"), "--socket", path, "games"], env=env, text=True)
            assert "custom/scoped-test" in games
            checks.append("connected CLI user import changes only the user file and creates a backup")
            cli("export", "--scope", "service", "service.json")
            service = json.loads((temp / "service.json").read_text())
            assert set(service["settings"]) == {"version", "fan", "lighting", "power"}
            assert service["settings"]["power"]["battery_aware"] is False
            service["settings"]["fan"]["curve"]["points"][0]["duty_pct"] = 28
            service["settings"]["power"]["on_ac_profile"] = None
            (temp / "service.json").write_text(json.dumps(service))
            inspected = json.loads(cli("inspect", "--scope", "service", "service.json").stdout)
            assert inspected["settings"]["power"]["on_ac_profile"] == "unchanged"
            cli("import", "--scope", "service", "service.json", success=False)
            assert not (system / "config.toml").exists()
            before_user = user_config.read_bytes()
            cli("import", "--scope", "service", "--apply-service-policy", "service.json")
            after_service = call(path, "get_config")
            assert after_service["fan"]["curve"]["points"][0]["duty_pct"] == 28
            assert after_service["general"] == original_service["general"]
            assert after_service["games"] == original_service["games"]
            assert user_config.read_bytes() == before_user
            checks.append("reviewed service import uses typed IPC, writes the service file and preserves desktop data")
            before_service = (system / "config.toml").read_bytes()
            cli("import", "--scope", "user", "service.json", success=False)
            service["settings"]["games"] = {}
            (temp / "bad-service.json").write_text(json.dumps(service))
            cli("import", "--scope", "service", "--apply-service-policy", "bad-service.json", success=False)
            assert user_config.read_bytes() == before_user
            assert (system / "config.toml").read_bytes() == before_service
            checks.append("scope mismatch and desktop fields in service backups are rejected without file changes")
            child.send_signal(signal.SIGTERM)
            assert child.wait(timeout=5) == 0
            child = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            path = child.stdout.readline().strip()
            assert call(path, "get_config")["power"]["on_ac_profile"] == "unchanged"
            checks.append("nullable AC policy remains unchanged after daemon restart")
            # Valid-then-invalid file IO never reports success or destroys data.
            (temp / "too-large.json").write_text(" " * 1_048_577)
            cli("import", "--scope", "user", "too-large.json", success=False)
            user_config.write_text("invalid {")
            cli("export", "--scope", "user", "corrupt.json", success=False)
            assert user_config.read_text() == "invalid {"
            checks.append("oversized input and corrupt local config fail without replacing data")
        finally:
            child.send_signal(signal.SIGTERM)
            assert child.wait(timeout=5) == 0
    result = {"fixture_only": True, "live_hardware_writes": False, "checks_passed": checks}
    (ROOT / "docs/evidence/settings-integration.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
