#!/usr/bin/env python3
"""GNotification delivery on a private bus; no real desktop notifications."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE


def server():
    import gi
    gi.require_version("Gio", "2.0")
    from gi.repository import Gio, GLib
    xml = '''<node><interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities"><arg type="as" direction="out"/></method>
    <method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
    <method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
    </interface></node>'''
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    count = 0
    output = Path(os.environ["OMEN_NOTIFICATION_RECORDS"])
    def method(_connection, _sender, _path, _interface, name, parameters, invocation):
        nonlocal count
        if name == "GetCapabilities":
            invocation.return_value(GLib.Variant("(as)", (["body", "actions"],)))
        elif name == "GetServerInformation":
            invocation.return_value(GLib.Variant("(ssss)", ("Fixture", "OMEN tests", "1", "1.2")))
        elif name == "CloseNotification":
            invocation.return_value(None)
        elif name == "Notify":
            count += 1
            values = parameters.unpack()
            with output.open("a") as file:
                file.write(json.dumps({"summary": values[3], "body": values[4], "actions": values[5]}) + "\n")
            invocation.return_value(GLib.Variant("(u)", (count,)))
    node = Gio.DBusNodeInfo.new_for_xml(xml)
    connection.register_object("/org/freedesktop/Notifications", node.interfaces[0], method, None, None)
    Gio.bus_own_name_on_connection(connection, "org.freedesktop.Notifications", Gio.BusNameOwnerFlags.NONE,
                                   lambda *_: print("READY", flush=True), None)
    GLib.MainLoop().run()


def main():
    assert os.geteuid() != 0
    checks = []
    out = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "target/audit-2026-10-27/notifications"))
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="omen-notifications-") as directory:
        temp = Path(directory)
        records = temp / "notifications.jsonl"
        config = temp / "user"
        config.mkdir()
        env = dict(os.environ, OMEN_CONFIG_DIR=str(config), OMEN_HOME=str(temp / "home"),
            OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"), OMEN_NOTIFICATION_RECORDS=str(records),
            TMPDIR=str(temp),
            XDG_DATA_HOME=str(temp / "data"), XDG_CONFIG_HOME=str(temp / "xdg-config"),
            GDK_BACKEND=os.environ.get("GDK_BACKEND", "x11"), GTK_A11Y="none", GSETTINGS_BACKEND="memory")
        children = []
        logs = []
        def launch(command, name, pipe=False):
            log = (temp / (name + ".log")).open("w")
            logs.append(log)
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE if pipe else log,
                stderr=log, text=True)
            children.append(process)
            return process
        def received():
            return [json.loads(line) for line in records.read_text().splitlines()] if records.exists() else []
        try:
            bus_config = temp / "bus.conf"
            bus_config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="signal"/><allow receive_type="error"/></policy></busconfig>')
            bus = launch(["dbus-daemon", "--config-file=" + str(bus_config), "--nofork", "--print-address=1"], "bus", True)
            env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
            notification_server = launch([sys.executable, __file__, "--server"], "notifications", True)
            assert notification_server.stdout.readline().strip() == "READY"
            applications = temp / "data/applications"
            applications.mkdir(parents=True)
            (applications / "com.omenlinux.Gui.desktop").write_text(
                (ROOT / "packaging/com.omenlinux.Gui.desktop").read_text().replace("Exec=omen-gui", "Exec=" + str(BIN / "omen-gui")))
            daemon = launch([str(BIN / "omen-daemon"), "--mock-omen"], "daemon", True)
            env["OMEN_SOCK"] = daemon.stdout.readline().strip()
            def start_gui(enabled):
                (config / "config.toml").write_text(
                    'version = 1\n[general]\npoll_interval_ms = 500\nonboarding_dismissed = true\n'
                    f'[general.alerts]\nenabled = {str(enabled).lower()}\ntemperature_c = 40\nhysteresis_c = 5\ncooldown_seconds = 30\n')
                return launch([str(BIN / "omen-gui"), "--new-instance"], "gui-" + str(enabled))
            disabled = start_gui(False)
            time.sleep(2)
            assert disabled.poll() is None
            assert not received(), "disabled notifications were delivered"
            disabled.terminate()
            disabled.wait(timeout=5)
            checks.append("notifications disabled by default/configuration deliver no message")
            enabled = start_gui(True)
            deadline = time.monotonic() + 10
            while not received() and time.monotonic() < deadline:
                assert enabled.poll() is None
                time.sleep(.1)
            messages = received()
            assert len(messages) == 1, (messages, (temp / "gui-True.log").read_text())
            assert "MOCK" in messages[0]["body"]
            assert "°C" in messages[0]["body"] and "not thermal protection" in messages[0]["body"]
            time.sleep(2)
            assert len(received()) == 1, "unchanged high temperature generated repeated notifications"
            checks.extend(["fresh fixture temperature delivers one real GNotification on the private bus",
                           "notification identifies fixture origin and informational purpose",
                           "unchanged high readings are suppressed by hysteresis and rate limiting"])
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=8)
            for log in logs:
                log.close()
            for log in temp.glob("*.log"):
                (out / log.name).write_text(log.read_text())
        result = {"result": "passed", "fixture_only": True, "real_desktop_notified": False,
            "profile": PROFILE, "binary_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest(),
            "checks_passed": checks}
        (out / f"notifications-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    server() if "--server" in sys.argv else main()
