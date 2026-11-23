#!/usr/bin/env python3
"""Real GTK + session-bus tray integration; all profile writes use mock hardware."""
import json
import hashlib
import os
from pathlib import Path
import signal
import socket as unix_socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / os.environ.get("OMEN_TEST_PROFILE", "debug")
BUS = ["busctl", "--user", "--json=short", "--timeout=2", "--"]


def wait_for(predicate, description, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.05)
    raise RuntimeError(f"Timed out: {description}")


def bus(*args, env=None):
    output = subprocess.check_output(BUS + list(args), env=env, stderr=subprocess.DEVNULL)
    return json.loads(output) if output.strip() else None


def ipc(path, operation, args=None):
    with unix_socket.socket(unix_socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(path)
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": op}).encode() + b"\n")
        response = json.loads(stream.makefile("rb").readline())
        assert response["ok"], response
        return response["data"]


class App:
    def __init__(self, directory, minimized=False, socket=None, address=None, page=None, config_text=None):
        self.directory = directory
        directory.mkdir()
        (directory / "config.toml").write_text(config_text or f"version = 1\n[general]\nstart_minimized = {str(minimized).lower()}\n")
        self.logpath = directory / "gui.log"
        self.log = self.logpath.open("w")
        self.env = dict(os.environ, OMEN_REPORT_READY="1", OMEN_CONFIG_DIR=str(directory),
                        OMEN_SYSTEM_CONFIG_DIR=str(directory), OMEN_SOCK=socket or str(directory / "missing.sock"))
        if address:
            self.env["DBUS_SESSION_BUS_ADDRESS"] = address
        command = [str(BIN / "omen-gui")]
        if address is None:
            command.append("--new-instance")
        if page:
            command += ["--page", page]
        self.child = subprocess.Popen(command, env=self.env, stdout=self.log, stderr=self.log)
        self.service = f"org.kde.StatusNotifierItem-{self.child.pid}-1"

    def text(self):
        return self.logpath.read_text()

    def visible(self):
        lines = [line for line in self.text().splitlines() if line.startswith("OMEN_UI_VISIBLE ")]
        return lines[-1].endswith("true") if lines else None

    def ready(self):
        wait_for(lambda: "OMEN_TRAY_ONLINE" in self.text(), "tray registration")
        wait_for(lambda: "OMEN_UI_MAPPED" in self.text(), "GTK window map")

    def layout(self):
        path = bus("get-property", self.service, "/StatusNotifierItem", "org.kde.StatusNotifierItem", "Menu", env=self.env)["data"]
        layout = bus("call", self.service, path, "com.canonical.dbusmenu", "GetLayout", "iias", "0", "-1", "0", env=self.env)["data"][1]
        return path, layout

    def labels(self):
        def walk(node):
            identifier, props, children = node
            yield props.get("label", {}).get("data", ""), identifier, props
            for child in children:
                yield from walk(child["data"])
        return list(walk(self.layout()[1]))

    def click(self, label):
        _, identifier, props = next(item for item in self.labels() if item[0] == label)
        assert props.get("enabled", {}).get("data", True), label
        bus("call", self.service, self.layout()[0], "com.canonical.dbusmenu", "Event", "isvu",
            str(identifier), "clicked", "i", "0", "0", env=self.env)

    def quit(self):
        self.click("Quit OMEN Control")
        assert self.child.wait(timeout=8) == 0

    def close(self):
        if self.child.poll() is None:
            self.child.terminate()
            self.child.wait(timeout=8)
        self.log.close()


def watcher():
    """Minimal protocol fixture, exclusively on the test's private session bus."""
    from gi.repository import Gio, GLib
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    xml = '''<node><interface name="org.kde.StatusNotifierWatcher">
      <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
      <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
      <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
      <property name="ProtocolVersion" type="i" access="read"/>
    </interface></node>'''
    items = []
    def method(_c, _s, _p, _i, _m, params, invocation):
        items.append(params.unpack()[0] + "/StatusNotifierItem")
        invocation.return_value(None)
    def prop(_c, _s, _p, _i, name):
        return {"IsStatusNotifierHostRegistered": GLib.Variant("b", True),
                "RegisteredStatusNotifierItems": GLib.Variant("as", items),
                "ProtocolVersion": GLib.Variant("i", 0)}[name]
    connection.register_object("/StatusNotifierWatcher", Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], method, prop, None)
    Gio.bus_own_name_on_connection(connection, "org.kde.StatusNotifierWatcher", Gio.BusNameOwnerFlags.NONE,
                                   lambda *_: print("READY", flush=True), None)
    GLib.MainLoop().run()


def start_watcher(address):
    child = subprocess.Popen([sys.executable, __file__, "--watcher"],
                             env=dict(os.environ, DBUS_SESSION_BUS_ADDRESS=address),
                             stdout=subprocess.PIPE, text=True)
    assert child.stdout.readline().strip() == "READY"
    return child


def main():
    checks = []
    # The real desktop host must exist; no success claim from a skipped scenario.
    assert bus("get-property", "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
               "org.kde.StatusNotifierWatcher", "IsStatusNotifierHostRegistered")["data"]
    with tempfile.TemporaryDirectory(prefix="omen-tray-tests-") as temporary:
        temp = Path(temporary)
        app = App(temp / "desktop")
        try:
            app.ready()
            assert "Set power profile" not in [item[0] for item in app.labels()]
            app.click("Hide window")
            wait_for(lambda: app.visible() is False, "window hidden")
            bus("call", app.service, "/StatusNotifierItem", "org.kde.StatusNotifierItem", "Activate", "ii", "0", "0")
            wait_for(lambda: app.visible() is True, "activation reopens window")
            app.click("System vitals")
            wait_for(lambda: "OMEN_UI_PAGE vitals" in app.text(), "tray navigation reaches GTK page")
            app.quit()
            checks.append("real KDE tray: read-only menu, Hide, Activate, page navigation, clean Quit")
        finally:
            app.close()
        app = App(temp / "minimized", minimized=True)
        try:
            app.ready()
            wait_for(lambda: app.visible() is False, "start-minimized preference")
            app.click("Show OMEN Control")
            wait_for(lambda: app.visible() is True, "show minimized window")
            app.quit()
            checks.append("startup preference hides only after tray registration; menu restores window")
        finally:
            app.close()
        # A private session bus has no tray watcher. Never stop the user's shell.
        private_bus_config = temp / "session.conf"
        private_bus_config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/session.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
        daemon = subprocess.Popen(["dbus-daemon", "--config-file="+str(private_bus_config), "--nofork", "--print-address=1"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            address = daemon.stdout.readline().strip()
            app = App(temp / "no-host", minimized=True, address=address)
            try:
                wait_for(lambda: "OMEN_TRAY_OFFLINE" in app.text(), "missing tray host detected")
                assert app.visible() is True
                checks.append("no tray host: start-minimized falls back to a visible GTK window")
            finally:
                app.close()
            host = start_watcher(address)
            try:
                app = App(temp / "host-restart", minimized=True, address=address)
                try:
                    app.ready()
                    wait_for(lambda: app.visible() is False, "private-host startup hide")
                    host.terminate()
                    host.wait(timeout=5)
                    wait_for(lambda: "OMEN_TRAY_OFFLINE" in app.text(), "host loss detected")
                    wait_for(lambda: app.visible() is True, "host loss reveals hidden window")
                    host = start_watcher(address)
                    wait_for(lambda: app.text().count("OMEN_TRAY_ONLINE") >= 2, "host re-registration")
                    assert app.visible() is True
                    app.quit()
                    checks.append("private-bus host loss reveals window; host restart re-registers without hiding it again")
                finally:
                    app.close()
            finally:
                if host.poll() is None:
                    host.terminate()
                    host.wait(timeout=5)
        finally:
            daemon.terminate()
            daemon.wait(timeout=5)
        env = dict(os.environ, OMEN_CONFIG_DIR=str(temp / "fixture"), OMEN_SYSTEM_CONFIG_DIR=str(temp / "fixture"))
        fixture = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True)
        try:
            socket = fixture.stdout.readline().strip()
            app = App(temp / "mock-gui", socket=socket)
            try:
                app.ready()
                wait_for(lambda: "Set power profile" in [item[0] for item in app.labels()], "tray power profile menu item")
                app.click("Performance")
                def current():
                    result = subprocess.check_output([str(BIN / "omen-cli"), "--socket", socket, "profile"], text=True)
                    return result.strip().lower()
                wait_for(lambda: current() == "performance", "tray profile request through production IPC on mock hardware")
                app.click("Balanced")
                wait_for(lambda: current() == "balanced", "fixture profile restore")
                settings = ipc(socket, "get_service_settings")
                settings["power"].update(battery_aware=True, on_ac_profile="unchanged")
                ipc(socket, "set_service_settings", {"settings": settings})
                def read_only_menu():
                    labels = [item[0] for item in app.labels()]
                    return "Set power profile" not in labels and any("read-only" in label for label in labels)
                wait_for(read_only_menu, "tray removes profile actions when automation takes ownership")
                assert current() == "balanced"
                settings["power"]["battery_aware"] = False
                ipc(socket, "set_service_settings", {"settings": settings})
                wait_for(lambda: "Set power profile" in [item[0] for item in app.labels()], "tray restores profile actions after automation releases ownership")
                assert current() == "balanced"
                checks.append("fixture only: tray capability changes remove and restore profile actions without a profile or visibility change")
                app.quit()
                checks.append("fixture only: tray profile changes reach daemon and restore Balanced")
            finally:
                app.close()
        finally:
            fixture.send_signal(signal.SIGTERM)
            assert fixture.wait(timeout=5) == 0
        for path in temp.glob("*/gui.log"):
            destination = ROOT / "docs/evidence" / f"tray-{path.parent.name}.log"
            destination.write_text(path.read_text())
    profile = os.environ.get("OMEN_TEST_PROFILE", "debug")
    result = {"host": "running KDE StatusNotifierWatcher", "profile": profile,
              "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "gui_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest(),
              "live_hardware_writes": False, "checks_passed": checks}
    (ROOT / "docs/evidence/tray-integration.json").write_text(json.dumps(result, indent=2) + "\n")
    (ROOT / "docs/evidence" / f"tray-integration-{profile}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    watcher() if "--watcher" in sys.argv else main()
