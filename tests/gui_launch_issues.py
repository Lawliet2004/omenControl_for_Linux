#!/usr/bin/env python3
"""Late/pre-registration failures in native GTK, without host hardware writes.

Only reads reach a real mock daemon. A private transport proxy refuses managed
registration after the GUI's submission receipt, before any command or hold.
The actual user systemd manager is used for the command's dedicated scope.
"""
import hashlib
import json
import os
from pathlib import Path
import socket
import socketserver
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE
OUT = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "target/launch-issues/ui"))


def call(endpoint, operation):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(6)
        stream.connect(str(endpoint))
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": {"op": operation}}).encode() + b"\n")
        reply = json.loads(stream.makefile("rb").readline())
        assert reply["ok"], reply
        return reply["data"]


def main():
    assert os.geteuid() != 0
    OUT.mkdir(parents=True, exist_ok=True)
    result = {"result": "failed", "fixture_only": True, "physical_hardware_writes": False,
              "system_power_bus_used": False, "profile": PROFILE,
              "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "gui_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest(),
              "cli_sha256": hashlib.sha256((BIN / "omen-cli").read_bytes()).hexdigest(),
              "checks_passed": [], "captures": []}
    with tempfile.TemporaryDirectory(prefix="omen-issues-ui-", dir=ROOT / "target") as directory:
        temp = Path(directory)
        for name in ["user", "system", "tmp"]:
            (temp / name).mkdir(mode=0o700)
        config = temp / "user/config.toml"
        sentinel = temp / "must-not-execute"
        config.write_text('version = 1\n[general]\npoll_interval_ms = 500\nonboarding_dismissed = true\n'
                          '[games]\nmanaged_game_ids = ["custom/denied"]\n'
                          '[games.per_game_profiles]\n"custom/denied" = "performance"\n'
                          '[[games.custom_games]]\nid = "custom/denied"\nname = "Launch recovery fixture"\n'
                          'source = "custom"\nlaunch_cmd = "/usr/bin/touch ' + str(sentinel) + '"\n')
        original_config = config.read_bytes()
        env = dict(os.environ, OMEN_CONFIG_DIR=str(config.parent), OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"),
                   OMEN_HOME=str(temp / "home"), TMPDIR=str(temp / "tmp"), GTK_A11Y="atspi",
                   GSETTINGS_BACKEND="memory", XDG_CONFIG_HOME=str(temp / "xdg-config"),
                   XDG_DATA_HOME=str(temp / "data"), XDG_CACHE_HOME=str(temp / "cache"),
                   DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(temp / "no-system-bus"), LC_ALL="C.UTF-8")
        children, logs = [], []
        proxy = None

        def launch(command, name, pipe=False, extra=None):
            log = (temp / (name + ".log")).open("w")
            logs.append(log)
            child = subprocess.Popen(command, env=dict(env, **(extra or {})), text=True,
                                     stdout=subprocess.PIPE if pipe else log, stderr=log)
            children.append(child)
            return child

        def passed(description):
            result["checks_passed"].append(description)
            print("PASS", description, flush=True)

        try:
            service = launch([str(BIN / "omen-daemon"), "--mock-omen"], "daemon", True)
            endpoint = Path(service.stdout.readline().strip())
            assert endpoint.is_socket()

            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    for line in self.rfile:
                        request = json.loads(line)
                        operation = request["op"]["op"]
                        if operation == "start_managed_game":
                            time.sleep(1.2)
                            reply = {"id": request["id"], "ok": False,
                                     "error": {"code": "permission_denied", "message": "SECRET_PRIVATE_ERROR"}}
                        elif operation.startswith("get_") or operation == "ping":
                            with socket.socket(socket.AF_UNIX) as forward:
                                forward.settimeout(5)
                                forward.connect(str(endpoint))
                                forward.sendall(line)
                                reply = json.loads(forward.makefile("rb").readline())
                        else:
                            raise AssertionError("Unexpected write through fixture proxy: " + operation)
                        self.wfile.write(json.dumps(reply).encode() + b"\n")
                        self.wfile.flush()

            class Server(socketserver.ThreadingUnixStreamServer):
                daemon_threads = True
            proxy_path = temp / "proxy.sock"
            proxy = Server(str(proxy_path), Handler)
            thread = threading.Thread(target=proxy.serve_forever, daemon=True)
            thread.start()
            env["OMEN_SOCK"] = str(proxy_path)
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
                    except (GLib.Error, StopIteration, ConnectionError, json.JSONDecodeError):
                        pass
                    time.sleep(.04)
                raise RuntimeError(message)

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

            def attach(process):
                def find():
                    desktop = Atspi.get_desktop(0)
                    return next(n for i in range(desktop.get_child_count())
                                if (n := desktop.get_child_at_index(i)) is not None
                                and n.get_process_id() == process.pid)
                app = wait(find, "own GUI missing from accessibility bus")
                app.set_cache_mask(Atspi.Cache.NONE)
                app.clear_cache()
                return app

            def text(app, fragment):
                return any(fragment in n.get_name() for n in walk(app) if n.get_role_name() == "label")

            def click(app, name):
                node = wait(lambda: next(n for n in walk(app) if n.get_role_name() == "button"
                                        and n.get_name() == name and n.get_state_set().contains(Atspi.StateType.SENSITIVE)),
                            "missing button " + name)
                action = node.get_action_iface()
                i = next(i for i in range(action.get_n_actions())
                         if action.get_action_name(i) in ["click", "activate", "press"])
                assert action.do_action(i)

            def start_gui(name="gui", extra=None):
                process = launch([str(BIN / "omen-gui"), "--new-instance", "--page", "games"], name, extra=extra)
                return process, attach(process)

            def cli(*arguments):
                return subprocess.run([str(BIN / "omen-cli"), *arguments], env=env, text=True,
                                      capture_output=True, timeout=15)

            gui, app = start_gui()
            wait(lambda: text(app, "No tracked sessions yet"), "initial session state absent")
            click(app, "Launch")
            wait(lambda: text(app, "Authorization was refused"), "late denial not visible in the GUI")
            assert not sentinel.exists()
            assert not call(endpoint, "get_game_sessions")["sessions"]
            assert call(endpoint, "get_profile") == "balanced"
            assert config.read_bytes() == original_config
            passed("a denial after the submission receipt appears in native Launch issues without execution or profile changes")
            issues_path = config.parent / "launch-issues/issues.json"
            data = json.loads(issues_path.read_text())
            assert len(data["issues"]) == 1 and data["issues"][0]["reason"]["kind"] == "authorization"
            assert "SECRET" not in issues_path.read_text() and str(sentinel) not in issues_path.read_text()
            assert (issues_path.stat().st_mode & 0o777) == 0o600
            assert (issues_path.parent.stat().st_mode & 0o777) == 0o700
            listed = cli("game-launch-issues", "--json")
            assert listed.returncode == 0 and len(json.loads(listed.stdout)) == 1
            passed("worker and GUI observers coalesce to one private reason without captured errors, command arguments or output")
            click(app, "Clear launch issues")
            wait(lambda: json.loads(issues_path.read_text())["issues"] == [], "clear did not reset saved reports")
            assert config.read_bytes() == original_config and call(endpoint, "get_profile") == "balanced"
            passed("native Clear launch issues resets desktop reports and leaves games, daemon history and profiles unchanged")
            gui.terminate()
            gui.wait(timeout=8)
            outcome = cli("game-run", "custom/denied", "--", "/usr/bin/touch", str(sentinel))
            assert outcome.returncode != 0 and not sentinel.exists()
            result["closed_wrapper_stderr"] = outcome.stderr
            result["closed_wrapper_saved_issues"] = json.loads(issues_path.read_text())
            assert len(result["closed_wrapper_saved_issues"]["issues"]) == 1, outcome.stderr
            gui, app = start_gui("reopened")
            wait(lambda: text(app, "Authorization was refused"), "failure while GUI closed was not recovered")
            passed("a wrapper failure while the GUI is closed is visible when Game Library reopens")
            issues_path.write_text("deliberately damaged fixture history")
            click(app, "Launch")
            wait(lambda: text(app, "Saved launch issues unavailable"), "damaged history was not explained")
            wait(lambda: any(n.get_name() == "Recent desktop reports (2)" for n in walk(app)), "live fallback missing")
            assert issues_path.read_text() == "deliberately damaged fixture history"
            click(app, "Clear launch issues")
            wait(lambda: json.loads(issues_path.read_text())["issues"] == [], "explicit damaged-data reset failed")
            passed("damaged saved data is preserved, late failures remain live in the GUI, and explicit reset recovers")
            gui.terminate()
            gui.wait(timeout=8)
            outcome = cli("game-run", "custom/denied", "--", "/usr/bin/touch", str(sentinel))
            assert outcome.returncode != 0
            captures = Path(os.environ.get("OMEN_CAPTURE_DIR", OUT / "captures")).resolve()
            if os.environ.get("OMEN_SKIP_CAPTURES") != "1":
                captures.mkdir(parents=True, exist_ok=True)
                for name, width, height, theme in [("launch-issues-wide-light", 1240, 1100, "prefer-light"),
                                                    ("launch-issues-narrow-dark", 600, 1100, "prefer-dark")]:
                    path = captures / (name + ".png")
                    window, captured = start_gui(name, {"OMEN_CAPTURE_PATH": str(path),
                        "OMEN_CAPTURE_WIDTH": str(width), "OMEN_CAPTURE_HEIGHT": str(height),
                        "ADW_DEBUG_COLOR_SCHEME": theme, "GDK_BACKEND": "x11"})
                    wait(lambda: text(captured, "Authorization was refused"), "capture did not load history")
                    window.wait(timeout=15)
                    assert window.returncode == 0 and path.is_file()
                    result["captures"].append({"path": str(path.relative_to(ROOT)), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "requested_size": [width, height], "actual_size": list(struct.unpack(">II", path.read_bytes()[16:24])), "theme": theme})
                passed("app-only wide/light and narrow/dark captures cover persisted launch recovery")
            cleared = cli("game-launch-issues-clear")
            assert cleared.returncode == 0 and json.loads(issues_path.read_text())["issues"] == []
            assert config.read_bytes() == original_config and not sentinel.exists()
            assert not call(endpoint, "get_game_sessions")["sessions"] and call(endpoint, "get_profile") == "balanced"
            passed("CLI inspection and clearing require no hardware discovery, game execution or profile write")
            result["result"] = "passed"
        except Exception as error:
            result["error"] = str(error)
            if "app" in locals():
                result["accessible_nodes"] = [{"role": n.get_role_name(), "name": n.get_name()} for n in walk(app)]
            raise
        finally:
            if proxy is not None:
                proxy.shutdown()
                proxy.server_close()
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=8)
            for log in logs:
                log.close()
            for log in temp.glob("*.log"):
                (OUT / log.name).write_text(log.read_text())
            result["cleanup_complete"] = True
            (OUT / f"gui-launch-issues-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
