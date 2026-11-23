#!/usr/bin/env python3
"""Native GTK/AT-SPI Automatic/Maximum actions against mode-only fixtures."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE
INDEPENDENT = os.environ.get("OMEN_TEST_INDEPENDENT") == "1"
SUFFIX = "-independent" if INDEPENDENT else ""


def call(path, operation, args=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(path)
        op = {"op": operation}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": op}).encode() + b"\n")
        response = json.loads(stream.makefile("rb").readline())
        assert response["ok"], response
        return response["data"]


def main():
    assert os.geteuid() != 0, "run native GUI tests as the desktop user"
    result = {"result": "failed", "profile": PROFILE, "fixture_only": True, "independent_writer": INDEPENDENT, "live_hardware_writes": False,
              "interaction": "native AT-SPI button actions; private bus; no injected application test actions",
              "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "checks_passed": [],
              "binary_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest()}
    with tempfile.TemporaryDirectory(prefix="omen-fan-ui-") as directory:
        temp = Path(directory)
        children, logs = [], []
        for name in ["user", "system", "tmp"]:
            (temp / name).mkdir()
        user = temp / "user/config.toml"
        system = temp / "system/config.toml"
        user.write_text("version = 1\n[general]\nstart_minimized = false\npoll_interval_ms = 500\n")
        env = dict(os.environ, OMEN_CONFIG_DIR=str(user.parent), OMEN_SYSTEM_CONFIG_DIR=str(system.parent),
                   OMEN_REPORT_READY="1", OMEN_HOME=str(temp / "games"), TMPDIR=str(temp / "tmp"),
                   GSETTINGS_BACKEND="memory", GTK_A11Y="atspi", GDK_DEBUG="no-portals",
                   XDG_CONFIG_HOME=str(temp / "xdg-config"), XDG_DATA_HOME=str(temp / "xdg-data"),
                   XDG_CACHE_HOME=str(temp / "cache"), LC_ALL="C.UTF-8")

        def launch(command, name, pipe=False):
            log = (temp / (name + ".log")).open("w")
            logs.append(log)
            child = subprocess.Popen(command, env=env, cwd=temp, stdout=subprocess.PIPE if pipe else log,
                                     stderr=log, text=True)
            children.append(child)
            return child

        def passed(message):
            result["checks_passed"].append(message)
            print("PASS", message, flush=True)

        try:
            config = temp / "session.conf"
            config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/session.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
            session = launch(["dbus-daemon", "--config-file=" + str(config), "--nofork", "--print-address=1"], "session", True)
            env["DBUS_SESSION_BUS_ADDRESS"] = session.stdout.readline().strip()
            env["AT_SPI_BUS_ADDRESS"] = "unix:path=" + str(temp / "a11y.sock")
            accessibility = launch(["dbus-daemon", "--config-file=/usr/share/defaults/at-spi2/accessibility.conf",
                                    "--address=" + env["AT_SPI_BUS_ADDRESS"], "--nofork", "--print-address=1"], "a11y", True)
            assert accessibility.stdout.readline().strip()
            os.environ.update({key: env[key] for key in ["DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS"]})
            daemon = launch([str(BIN / "omen-daemon"), "--mock"] + (["--mock-recovery", "auto"] if INDEPENDENT else []), "daemon", True)
            endpoint = daemon.stdout.readline().strip()
            assert endpoint == str(temp / f"tmp/omen-mock-{daemon.pid}.sock")
            assert call(endpoint, "get_info")["mock"] is True
            env["OMEN_SOCK"] = endpoint
            hp = next((temp / "tmp").glob("*/sys/class/hwmon/hwmon0"))
            root = hp.parents[3]
            gpu = root / "sys/bus/pci/devices/0000:06:00.0"
            driver = root / "drivers/amdgpu"
            driver.mkdir(parents=True)
            (gpu / "driver").symlink_to(driver)
            (hp.parent / "hwmon2/device").symlink_to(gpu)
            if INDEPENDENT:
                recovery = launch([str(BIN / "omen-daemon"), "--coarse-recovery-fixture", str(root), "--instance", "hwmon0"], "recovery", True)
                assert recovery.stdout.readline().strip() == str(root / "run/omen-fan-recovery/control.sock")
            gui = launch([str(BIN / "omen-gui"), "--page", "fan"], "gui")

            import gi
            gi.require_version("Atspi", "2.0")
            from gi.repository import Atspi, GLib
            Atspi.init()
            Atspi.set_timeout(2000, 5000)
            context = GLib.MainContext.default()

            def wait_for(predicate, message, timeout=12):
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    while context.pending():
                        context.iteration(False)
                    try:
                        value = predicate()
                        if value:
                            return value
                    except (GLib.Error, StopIteration):
                        pass
                    time.sleep(.05)
                raise RuntimeError(message)

            def own_application():
                desktop = Atspi.get_desktop(0)
                for index in range(desktop.get_child_count()):
                    candidate = desktop.get_child_at_index(index)
                    if candidate.get_process_id() == gui.pid:
                        return candidate
                return None
            app = wait_for(own_application, "test application absent from private accessibility bus")
            app.set_cache_mask(Atspi.Cache.NONE)
            app.clear_cache()

            def walk(node=None, depth=0):
                node = app if node is None else node
                if depth > 40:
                    return
                try:
                    if node.get_state_set().contains(Atspi.StateType.DEFUNCT):
                        return
                    yield node
                    for index in range(node.get_child_count()):
                        yield from walk(node.get_child_at_index(index), depth + 1)
                except GLib.Error:
                    return

            def find(role, name, parent=None):
                return next(node for node in walk(parent) if node.get_role_name() == role and node.get_name() == name)

            def click(name, parent=None):
                node = wait_for(lambda: find("button", name, parent), "missing button: " + name)
                assert node.get_state_set().contains(Atspi.StateType.SENSITIVE), name
                action = node.get_action_iface()
                index = next(index for index in range(action.get_n_actions()) if action.get_action_name(index) == "click")
                assert action.do_action(index), name

            def status(fragment):
                return any(node.get_role_name() == "label" and fragment in node.get_name() for node in walk())

            def wait_status(fragment):
                wait_for(lambda: status(fragment), "missing operation status: " + fragment)

            assert not (hp / "pwm1").exists() and not (hp / "pwm2").exists()
            wait_for(lambda: find("button", "Maximum"), "firmware controls absent")
            assert not any(node.get_name() == "Apply manual speed" for node in walk())
            passed("mode-only fixture offers Automatic/Maximum without manual percentage controls")
            click("Maximum")
            wait_for(lambda: (hp / "pwm1_enable").read_text().strip() == "0", "Maximum did not reach fixture")
            wait_status("Set fans to Maximum")
            click("Automatic")
            wait_for(lambda: (hp / "pwm1_enable").read_text().strip() == "2", "Automatic did not restore fixture")
            passed("native buttons switch Maximum then Automatic through production IPC")
            conflict = root / "proc/43123"
            conflict.mkdir(parents=True)
            (conflict / "comm").write_text("omencore-cli\n")
            wait_for(lambda: all(not find("button", name).get_state_set().contains(Atspi.StateType.SENSITIVE)
                                 for name in ["Automatic", "Maximum"]), "capability loss did not disable mode actions")
            wait_status("competing daemon")
            assert (hp / "pwm1_enable").read_text().strip() == "2"
            passed("fresh competing-controller capability disables both actions with a visible reason")
            (conflict / "comm").unlink()
            conflict.rmdir()
            wait_for(lambda: find("button", "Maximum").get_state_set().contains(Atspi.StateType.SENSITIVE),
                     "controls did not recover with fresh capability")
            click("Maximum")
            wait_for(lambda: (hp / "pwm1_enable").read_text().strip() == "0", "rearmed Maximum failed")
            (hp.parent / "hwmon2/temp1_input").write_text("invalid")
            wait_for(lambda: (hp / "pwm1_enable").read_text().strip() == "2", "thermal loss did not restore Automatic")
            passed("rearmed Maximum returns to Automatic when identified GPU temperature is lost")
            result["result"] = "passed"
            result["gtk_display_type"] = next(line.split(" ",1)[1] for line in (temp / "gui.log").read_text().splitlines()
                                               if line.startswith("OMEN_UI_BACKEND "))
        except BaseException as error:
            result["error"] = str(error)
            raise
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=8)
            for log in logs:
                log.close()
            for name in ["gui", "daemon", "session", "a11y", "recovery"]:
                path = temp / (name + ".log")
                if path.exists():
                    (ROOT / "docs/evidence" / f"fan-modes-{PROFILE}{SUFFIX}-{name}.log").write_text(path.read_text())
            result["cleanup_complete"] = True
            result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            (ROOT / "docs/evidence" / f"fan-modes-{PROFILE}{SUFFIX}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
