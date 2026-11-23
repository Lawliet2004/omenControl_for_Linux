#!/usr/bin/env python3
"""Capture only the fan page widget tree with live read-only and fixture state."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/release"


def main():
    records = []
    with tempfile.TemporaryDirectory(prefix="omen-fan-capture-", dir=ROOT / "target") as directory:
        temp = Path(directory)
        env = dict(os.environ, GDK_BACKEND="wayland", GTK_A11Y="none", GSETTINGS_BACKEND="memory",
                   OMEN_REPORT_READY="1", OMEN_CONFIG_DIR=str(temp / "user"), OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"),
                   OMEN_HOME=str(temp / "games"), OMEN_SOCK=str(temp / "missing.sock"), TMPDIR=str(temp),
                   XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"), XDG_CACHE_HOME=str(temp / "cache"))

        def capture(name, fixture):
            path = ROOT / "docs/screenshots" / name
            completed = subprocess.run([str(BIN / "omen-gui"), "--page", "fan"],
                                       env=dict(env, OMEN_CAPTURE_PATH=str(path)),
                                       check=True, capture_output=True, text=True, timeout=20)
            display = next(line.split(" ", 1)[1] for line in completed.stderr.splitlines()
                           if line.startswith("OMEN_UI_BACKEND "))
            assert display == "GdkWaylandDisplay"
            pages = [line.split(" ", 1)[1] for line in completed.stderr.splitlines()
                     if line.startswith("OMEN_UI_PAGE ")]
            assert all(page == "fan" for page in pages), pages
            records.append({"path": str(path.relative_to(ROOT)), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                            "fixture": fixture, "gtk_display_type": display, "page_changes": pages})

        bus_config = temp / "session.conf"
        bus_config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/session.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
        with (temp / "session.log").open("w") as bus_log:
            bus = subprocess.Popen(["dbus-daemon", "--config-file=" + str(bus_config), "--nofork", "--print-address=1"],
                                   stdout=subprocess.PIPE, stderr=bus_log, text=True)
            try:
                env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
                assert env["DBUS_SESSION_BUS_ADDRESS"]
                capture("fan.png", False)
                with (temp / "daemon.log").open("w") as log:
                    daemon = subprocess.Popen([str(BIN / "omen-daemon"), "--mock"], env=env,
                                              stdout=subprocess.PIPE, stderr=log, text=True)
                    try:
                        env["OMEN_SOCK"] = daemon.stdout.readline().strip()
                        assert Path(env["OMEN_SOCK"]).name == f"omen-mock-{daemon.pid}.sock"
                        hp = next(temp.glob("*/sys/class/hwmon/hwmon0"))
                        assert (hp / "name").read_text().strip() == "hp"
                        assert not (hp / "pwm1").exists()
                        (hp / "pwm1_enable").write_text("0")
                        capture("fan-mode-only-fixture.png", True)
                    finally:
                        daemon.terminate()
                        assert daemon.wait(timeout=8) == 0
            finally:
                bus.terminate()
                assert bus.wait(timeout=8) == 0
    result = {"recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "method": "application-only GTK widget captures; no desktop framebuffer capture",
              "session_bus": "private minimal dbus-daemon without service activation; host tray and portal are not used",
              "hardware_control_writes": 0, "cleanup_complete": True, "screenshots": records,
              "gui_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest()}
    (ROOT / "docs/evidence/fan-captures.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
