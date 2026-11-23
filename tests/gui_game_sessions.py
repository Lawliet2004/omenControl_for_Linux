#!/usr/bin/env python3
"""Native session UI on private hardware/AT-SPI fixtures and real user scopes.

GTK uses the existing desktop session bus so systemd-run can reach the user
manager; the app is NON_UNIQUE, shortcuts/notifications are disabled, and the
power bus and accessibility bus are private. Captures contain only GTK trees.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE
HARNESS = Path(os.environ["OMEN_NATIVE_GAME_TEST_BIN"])
OUT = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "target/managed-game-ui"))


def call(endpoint, operation, arguments=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(6)
        stream.connect(str(endpoint))
        request = {"v": 1, "id": 1, "op": {"op": operation}}
        if arguments is not None:
            request["op"]["args"] = arguments
        stream.sendall(json.dumps(request).encode() + b"\n")
        reply = json.loads(stream.makefile("rb").readline())
        assert reply["ok"], reply
        return reply["data"]


def main():
    assert os.geteuid() != 0
    OUT.mkdir(parents=True, exist_ok=True)
    captures = Path(os.environ.get("OMEN_CAPTURE_DIR", OUT / "captures")).resolve()
    captures.mkdir(parents=True, exist_ok=True)
    result = {"result": "failed", "fixture_only": True, "physical_hardware_writes": False,
              "system_power_bus_used": False, "real_shortcuts_or_notifications_enabled": False,
              "profile": PROFILE, "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "gui_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest(),
              "cli_sha256": hashlib.sha256((BIN / "omen-cli").read_bytes()).hexdigest(),
              "checks_passed": [], "captures": []}
    with tempfile.TemporaryDirectory(prefix="omen-game-ui-", dir=ROOT / "target") as directory:
        temp = Path(directory)
        children, logs = [], []
        for name in ["user", "system", "service", "tmp"]:
            (temp / name).mkdir(mode=0o700)
        config = temp / "user/config.toml"
        config.write_text('version = 1\n[general]\npoll_interval_ms = 500\nonboarding_dismissed = true\n'
                          '[games.per_game_profiles]\n"custom/gui" = "performance"\n'
                          '[[games.custom_games]]\nid = "custom/gui"\nname = "GUI session fixture"\n'
                          'source = "custom"\nlaunch_cmd = "/bin/sleep 7"\n')
        env = dict(os.environ, OMEN_CONFIG_DIR=str(config.parent),
                   OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"), OMEN_HOME=str(temp / "games"),
                   OMEN_REPORT_READY="1", TMPDIR=str(temp / "tmp"), GSETTINGS_BACKEND="memory",
                   GTK_A11Y="atspi", XDG_CONFIG_HOME=str(temp / "xdg-config"),
                   XDG_DATA_HOME=str(temp / "data"), XDG_CACHE_HOME=str(temp / "cache"),
                   LC_ALL="C.UTF-8")

        def launch(command, name, pipe=False, extra=None):
            log = (temp / (name + ".log")).open("w")
            logs.append(log)
            child = subprocess.Popen(command, env=dict(env, **(extra or {})),
                                     stdout=subprocess.PIPE if pipe else log, stderr=log, text=True)
            children.append(child)
            return child

        def passed(text):
            result["checks_passed"].append(text)
            print("PASS", text, flush=True)

        gui = None
        try:
            bus = launch(["dbus-daemon", "--session", "--nofork", "--print-address=1",
                          "--address=unix:path=" + str(temp / "power-bus")], "power-bus", True)
            address = bus.stdout.readline().strip()
            assert address.startswith("unix:")
            env["DBUS_SYSTEM_BUS_ADDRESS"] = address
            ppd = launch([str(HARNESS), "ppd-service", address], "ppd", True)
            assert ppd.stdout.readline().strip() == "READY"
            endpoint = temp / "service/ctl.sock"
            service = launch([str(HARNESS), "fixture-service", str(endpoint),
                              str(temp / "service/sessions.json"), address, "mock"], "service", True)
            assert service.stdout.readline().strip() == "READY"
            env["OMEN_SOCK"] = str(endpoint)
            env["AT_SPI_BUS_ADDRESS"] = "unix:path=" + str(temp / "a11y-bus")
            a11y = launch(["dbus-daemon", "--config-file=/usr/share/defaults/at-spi2/accessibility.conf",
                           "--address=" + env["AT_SPI_BUS_ADDRESS"], "--nofork", "--print-address=1"], "a11y", True)
            assert a11y.stdout.readline().strip()
            os.environ["AT_SPI_BUS_ADDRESS"] = env["AT_SPI_BUS_ADDRESS"]
            import gi
            gi.require_version("Atspi", "2.0")
            from gi.repository import Atspi, GLib
            Atspi.init()
            Atspi.set_timeout(2000, 5000)
            context = GLib.MainContext.default()

            def wait(predicate, message, timeout=15):
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    while context.pending():
                        context.iteration(False)
                    try:
                        value = predicate()
                        if value:
                            return value
                    except (GLib.Error, StopIteration, ConnectionError):
                        pass
                    time.sleep(.04)
                raise RuntimeError(message)

            def attach(process):
                def find():
                    desktop = Atspi.get_desktop(0)
                    return next(node for i in range(desktop.get_child_count())
                                if (node := desktop.get_child_at_index(i)) is not None
                                and node.get_process_id() == process.pid)
                app = wait(find, "own GUI missing from accessibility bus")
                app.set_cache_mask(Atspi.Cache.NONE)
                app.clear_cache()
                return app

            def walk(node):
                if node is None:
                    return
                try:
                    if node.get_state_set().contains(Atspi.StateType.DEFUNCT):
                        return
                    yield node
                    for i in range(node.get_child_count()):
                        yield from walk(node.get_child_at_index(i))
                except GLib.Error:
                    return

            def click(app, name):
                node = wait(lambda: next(n for n in walk(app) if n.get_role_name() == "button"
                                        and n.get_name() == name and n.get_state_set().contains(Atspi.StateType.SENSITIVE)), "missing button " + name)
                assert node.get_state_set().contains(Atspi.StateType.SENSITIVE), name
                action = node.get_action_iface()
                index = next(i for i in range(action.get_n_actions())
                             if action.get_action_name(i) in ["click", "activate", "press"])
                assert action.do_action(index)
                return node

            def text(app, fragment):
                return any(fragment in n.get_name() for n in walk(app) if n.get_role_name() == "label")

            gui = launch([str(BIN / "omen-gui"), "--new-instance", "--page", "games"], "gui")
            app = attach(gui)
            wait(lambda: text(app, "No tracked sessions yet"), "initial session empty state absent")
            setup_button = click(app, "Profile setup")
            result["setup_accessible_description"] = setup_button.get_description()
            result["setup_accessible_relations"] = [{"type": int(relation.get_relation_type()),
                "targets": [relation.get_target(i).get_name() for i in range(relation.get_n_targets())]}
                for relation in setup_button.get_relation_set()]
            click(app, "Enable automatic restore")
            wait(lambda: "custom/gui" in tomllib.loads(config.read_text())["games"].get("managed_game_ids", []), "mode was not saved")
            assert not call(endpoint, "get_game_sessions")["sessions"]
            passed("native setup saves automatic mode in user settings without starting a hold")
            click(app, "Launch")
            wait(lambda: any(s["profile_held"] for s in call(endpoint, "get_game_sessions")["sessions"]), "GUI launch did not acquire a hold")
            wait(lambda: text(app, "Tracking process tree · requested Performance · profile hold active"), "active session row absent")
            assert call(endpoint, "get_profile") == "performance"
            passed("native Launch runs the custom wrapper and renders active ownership with the effective profile")
            click(app, "Use manual restore")
            wait(lambda: not tomllib.loads(config.read_text())["games"]["managed_game_ids"], "manual mode was not saved")
            assert any(s["profile_held"] for s in call(endpoint, "get_game_sessions")["sessions"])
            passed("changing the preference affects future launches and preserves the active session until explicitly released")
            click(app, "Release profile hold")
            wait(lambda: call(endpoint, "get_profile") == "balanced", "UI release did not release the hold")
            assert any(s["phase"] == "tracking" for s in call(endpoint, "get_game_sessions")["sessions"])
            wait(lambda: all(s["phase"] == "ended" for s in call(endpoint, "get_game_sessions")["sessions"]), "released session did not reach actual exit")
            passed("native release restores provider selection without stopping the game, and actual exit moves it to history")
            click(app, "Enable automatic restore")
            wait(lambda: "custom/gui" in tomllib.loads(config.read_text())["games"]["managed_game_ids"], "second automatic mode was not saved")
            click(app, "Launch")
            wait(lambda: any(s["profile_held"] for s in call(endpoint, "get_game_sessions")["sessions"]), "second GUI launch failed")
            gui.kill()
            gui.wait(timeout=8)
            assert call(endpoint, "get_profile") == "performance"
            wait(lambda: all(s["phase"] == "ended" for s in call(endpoint, "get_game_sessions")["sessions"]), "GUI death interrupted daemon lifetime tracking")
            wait(lambda: call(endpoint, "get_profile") == "balanced", "GUI death left an orphan profile")
            passed("SIGKILL of the GUI preserves the running session and the daemon releases its hold after real exit")

            capture_cases = [] if os.environ.get("OMEN_SKIP_CAPTURES") == "1" else [
                ("games-active-wide-light", 1240, 1100, "prefer-light", False),
                ("games-setup-narrow-dark", 600, 1100, "prefer-dark", True),
            ]
            for name, width, height, theme, setup in capture_cases:
                command = launch([str(BIN / "omen-cli"), "--socket", str(endpoint), "game-run", "custom/gui", "--", "/bin/sleep", "7"], name + "-command")
                wait(lambda: any(s["profile_held"] for s in call(endpoint, "get_game_sessions")["sessions"]), "capture session failed")
                path = captures / (name + ".png")
                window = launch([str(BIN / "omen-gui"), "--new-instance", "--page", "games"], name,
                                extra={"OMEN_CAPTURE_PATH": str(path), "OMEN_CAPTURE_WIDTH": str(width),
                                       "OMEN_CAPTURE_HEIGHT": str(height), "ADW_DEBUG_COLOR_SCHEME": theme,
                                       "GDK_BACKEND": "x11"})
                captured_app = attach(window)
                if setup:
                    button = click(captured_app, "Profile setup")
                    wait(lambda: text(captured_app, "Automatic restore is enabled"), "capture setup did not expand", timeout=4)
                    component = button.get_component_iface()
                    if component is not None:
                        try:
                            component.scroll_to(Atspi.ScrollType.ANYWHERE)
                        except GLib.Error:
                            # GTK may not implement Component.ScrollTo on a
                            # button. Use its real visible vertical scrollbar.
                            for node in walk(captured_app):
                                states = node.get_state_set()
                                if node.get_role_name() == "scroll bar" and states.contains(Atspi.StateType.VERTICAL) and states.contains(Atspi.StateType.SHOWING):
                                    value = node.get_value_iface()
                                    if value is not None:
                                        value.set_current_value(value.get_maximum_value())
                window.wait(timeout=15)
                assert window.returncode == 0 and path.is_file()
                result["captures"].append({"path": str(path.relative_to(ROOT)), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                           "requested_size": [width, height], "actual_size": list(struct.unpack(">II", path.read_bytes()[16:24])), "theme": theme, "fixture_only": True})
                command.wait(timeout=12)
                wait(lambda: all(not s["profile_held"] for s in call(endpoint, "get_game_sessions")["sessions"]), "capture session cleanup failed")
            if capture_cases:
                passed("wide/light and narrow/dark app-only GTK captures cover active sessions and inline setup")
            result["result"] = "passed"
        except Exception as error:
            result["error"] = str(error)
            if gui is not None:
                result["gui_exit_code"] = gui.poll()
            if "app" in locals():
                result["accessible_nodes"] = [{"role": n.get_role_name(), "name": n.get_name()} for n in walk(app)]
            raise
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=8)
            for log in logs:
                log.close()
            for log in temp.glob("*.log"):
                (OUT / log.name).write_text(log.read_text())
            result["cleanup_complete"] = True
            (OUT / f"gui-game-sessions-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
