#!/usr/bin/env python3
"""Real launcher processes and production IPC, exclusively on fixture hardware."""
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / os.environ.get("OMEN_TEST_PROFILE", "debug")
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))


def call(path, operation, args=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(path)
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": op}).encode() + b"\n")
        result = json.loads(stream.makefile("rb").readline())
        assert result["ok"], result
        return result["data"]


def main():
    if os.geteuid() == 0:
        raise RuntimeError("Run this desktop-launch integration test as an unprivileged user")
    checks = []
    with tempfile.TemporaryDirectory(prefix="omen-launch-tests-") as temporary:
        temp = Path(temporary)
        user = temp / "user"
        home = temp / "home"
        launchers = temp / "bin"
        fixture_tmp = temp / "fixture-tmp"
        for directory in [user, launchers, fixture_tmp]:
            directory.mkdir()
        apps = home / ".steam/steam/steamapps"
        apps.mkdir(parents=True)
        (apps / "appmanifest_620.acf").write_text('"appid" "620"\n"name" "Steam fixture"\n')
        flat_apps = home / ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps"
        flat_apps.mkdir(parents=True)
        (flat_apps / "appmanifest_620.acf").write_text('"AppState" {"appid" "620" "name" "Flatpak Steam fixture" "StateFlags" "4"}')
        for heroic in [home / ".config/heroic", home / ".var/app/com.heroicgameslauncher.hgl/config/heroic"]:
            installed = heroic / "legendaryConfig/legendary/installed.json"
            installed.parent.mkdir(parents=True)
            installed.write_text(json.dumps({"Quail": {"title": "Heroic fixture", "is_dlc": False}}))
        for path in [home / ".local/share/lutris/pga.db", home / ".var/app/net.lutris.Lutris/data/lutris/pga.db"]:
            path.parent.mkdir(parents=True)
            with sqlite3.connect(path) as db:
                db.executescript("CREATE TABLE games (id INTEGER, name TEXT, installed INTEGER); INSERT INTO games VALUES (9, 'Lutris fixture', 1), (10, 'Not installed', 0);")
        helper = temp / "fixture-writer.py"
        helper.write_text('''import json,os,signal,socket,sys
def call(name,args=None):
 with socket.socket(socket.AF_UNIX) as s:
  s.connect(os.environ["OMEN_SOCK"])
  op={"op":name}
  if args is not None:op["args"]=args
  s.sendall(json.dumps({"v":1,"id":1,"op":op}).encode()+b"\\n")
  response=json.loads(s.makefile("rb").readline())
  assert response["ok"],response
  return response["data"]
assert call("get_info")["mock"],"refusing a live endpoint"
if sys.argv[1]=="external":call("set_profile",{"profile":"quiet"})
else:os.kill(int(os.environ["OMEN_TEST_DAEMON_PID"]),signal.SIGKILL)
sys.exit(37)
''')
        config = 'version = 1\n[games.per_game_profiles]\n"steam/620"="performance"\n"custom/fail"="performance"\n"custom/external"="performance"\n"custom/lost"="performance"\n'
        for identifier in ["steam/flatpak/620", "heroic/legendary/Quail", "heroic/flatpak/legendary/Quail", "lutris/9", "lutris/flatpak/9"]:
            config += f'{json.dumps(identifier)}="performance"\n'
        for identifier, command in [
            ("custom/fail", "exit 37"),
            ("custom/external", f"exec /usr/bin/python3 {shlex.quote(str(helper))} external"),
            ("custom/lost", f"exec /usr/bin/python3 {shlex.quote(str(helper))} lost"),
            ("custom/unbound", "printf 'not JSON'; exit 0"),
            ("custom/running", "exec /usr/bin/sleep 0.8"),
        ]:
            config += f'[[games.custom_games]]\nid={json.dumps(identifier)}\nname={json.dumps(identifier)}\nsource="custom"\nlaunch_cmd={json.dumps(command)}\n'
        (user / "config.toml").write_text(config)
        env = dict(os.environ, OMEN_CONFIG_DIR=str(user), OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"),
                   OMEN_HOME=str(home), TMPDIR=str(fixture_tmp), OMEN_LAUNCH_LOG=str(temp / "submission"))
        logpath = temp / "daemon.log"
        log = logpath.open("w")
        daemon = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"], env=env, stdout=subprocess.PIPE, stderr=log, text=True)
        try:
            path = daemon.stdout.readline().strip()
            assert call(path, "get_info")["mock"]
            env.update(OMEN_SOCK=path, OMEN_TEST_DAEMON_PID=str(daemon.pid))
            def reset(profile="balanced"):
                call(path, "set_profile", {"profile": profile})
            def cli(identifier, success=False, search=None):
                child_env = dict(env, PATH=str(launchers) if search is None else search)
                result = subprocess.run([str(BIN / "omen-cli"), "--socket", path, "game-launch", identifier, "--json"],
                                        env=child_env, capture_output=True, text=True, timeout=15)
                assert (result.returncode == 0) == success, (result.stdout, result.stderr)
                return json.loads(result.stdout)
            def steam(text):
                executable = launchers / "steam"
                executable.write_text(text)
                executable.chmod(0o700)
            reset()
            writes = logpath.read_text().count("op 'set_profile'")
            missing = cli("steam/620")
            assert missing["error"]["stage"] == "preparation"
            assert logpath.read_text().count("op 'set_profile'") == writes
            checks.append("missing launcher is rejected before any profile write")
            steam("#!/nonexistent/omen-test-interpreter\n")
            failed_spawn = cli("steam/620")
            assert failed_spawn["error"]["stage"] == "launcher"
            assert failed_spawn["error"]["recovery"] == {"kind": "restored", "profile": "balanced"}
            assert call(path, "get_profile") == "balanced"
            checks.append("spawn failure after profile application restores and verifies Balanced")
            reset("performance")
            writes = logpath.read_text().count("op 'set_profile'")
            already_active = cli("steam/620")
            assert already_active["error"]["recovery"]["kind"] == "not_needed"
            assert call(path, "get_profile") == "performance"
            assert logpath.read_text().count("op 'set_profile'") == writes
            checks.append("an already-active profile is not mutated or claimed for rollback")
            reset()
            nonzero = cli("custom/fail")
            assert "37" in nonzero["error"]["cause"]["message"]
            assert nonzero["error"]["recovery"]["kind"] == "restored"
            assert call(path, "get_profile") == "balanced"
            checks.append("early nonzero launcher exit is reported and rolls back the applied profile")
            external = cli("custom/external")
            assert external["error"]["recovery"]["kind"] == "unverified"
            assert call(path, "get_profile") == "quiet"
            checks.append("a second IPC writer revokes recovery ownership and its profile is preserved")
            reset()
            steam('#!/bin/sh\nprintf "%s\\n" "$1" > "$OMEN_LAUNCH_LOG"\n/usr/bin/id -u >> "$OMEN_LAUNCH_LOG"\nexit 0\n')
            submitted = cli("steam/620", success=True)
            assert submitted["launch"]["profile_was_applied"]
            assert submitted["launch"]["launcher"]["game_execution_verified"] is False
            assert submitted["launch"]["launcher"]["launcher_state"] == "request_completed"
            assert (temp / "submission").read_text().splitlines() == ["steam://rungameid/620", str(os.geteuid())]
            assert call(path, "get_profile") == "performance"
            checks.append("successful fixture Steam submission uses one URI argument and the desktop UID")
            reset()
            writes = logpath.read_text().count("op 'set_profile'")
            missing_flatpak = cli("steam/flatpak/620")
            assert missing_flatpak["error"]["stage"] == "preparation"
            assert logpath.read_text().count("op 'set_profile'") == writes
            checks.append("missing Flatpak launcher fails before a bound profile is changed")
            for binary in ["heroic", "lutris", "flatpak"]:
                helper_launcher = launchers / binary
                helper_launcher.write_text('#!/bin/sh\nprintf "%s\\n" "$0" "$@" > "$OMEN_LAUNCH_LOG"\n/usr/bin/id -u >> "$OMEN_LAUNCH_LOG"\nexit 0\n')
                helper_launcher.chmod(0o700)
            for identifier, binary, arguments in [
                ("steam/flatpak/620", "flatpak", ["run", "com.valvesoftware.Steam", "steam://rungameid/620"]),
                ("heroic/legendary/Quail", "heroic", ["heroic://launch/legendary/Quail"]),
                ("heroic/flatpak/legendary/Quail", "flatpak", ["run", "com.heroicgameslauncher.hgl", "heroic://launch/legendary/Quail"]),
                ("lutris/9", "lutris", ["lutris:rungameid/9"]),
                ("lutris/flatpak/9", "flatpak", ["run", "net.lutris.Lutris", "lutris:rungameid/9"]),
            ]:
                reset()
                submitted = cli(identifier, success=True)
                assert submitted["launch"]["profile_was_applied"]
                assert not submitted["launch"]["launcher"]["game_execution_verified"]
                assert (temp / "submission").read_text().splitlines() == [str(launchers / binary), *arguments, str(os.geteuid())]
                assert call(path, "get_profile") == "performance"
                checks.append(f"{identifier}: discovered fixture submits exact arguments as desktop UID")
            reset()
            settings = call(path, "get_service_settings")
            settings["power"].update(battery_aware=True, on_ac_profile="unchanged")
            call(path, "set_service_settings", {"settings": settings})
            blocked = cli("steam/620")
            assert blocked["error"]["cause"]["code"] == "safety_lockout"
            assert call(path, "get_profile") == "balanced"
            settings["power"]["battery_aware"] = False
            call(path, "set_service_settings", {"settings": settings})
            checks.append("battery automation and bound per-game profiles cannot compete")
            lost = cli("custom/lost")
            assert lost["error"]["recovery"]["kind"] == "unverified"
            assert lost["error"]["original_profile"] == "balanced"
            assert lost["error"]["requested_profile"] == "performance"
            assert daemon.wait(timeout=5) == -signal.SIGKILL
            checks.append("lost daemon during rollback reports recovery as unverified")
            unbound = cli("custom/unbound", success=True)
            assert unbound["launch"]["original_profile"] is None
            assert not unbound["launch"]["profile_was_applied"]
            checks.append("unbound game commands work without a daemon and cannot pollute JSON stdout")
            running = cli("custom/running", success=True)
            receipt = running["launch"]["launcher"]
            assert receipt["launcher_state"] == "running" and not receipt["game_execution_verified"]
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                status = Path(f"/proc/{receipt['process_id']}/stat")
                try:
                    state = status.read_text().rsplit(")", 1)[1].split()[0]
                except (FileNotFoundError, ProcessLookupError):
                    break
                if state == "Z":
                    break
                time.sleep(.05)
            else:
                raise RuntimeError("fixture sleep process did not exit")
            checks.append("longer-running launcher returns a process receipt without claiming game execution")
        finally:
            if daemon.poll() is None:
                daemon.send_signal(signal.SIGTERM)
                daemon.wait(timeout=5)
            log.close()
    result = {"fixture_only": True, "live_hardware_writes": False, "desktop_uid": os.geteuid(), "checks_passed": checks}
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    (EVIDENCE / "game-launch-integration.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
