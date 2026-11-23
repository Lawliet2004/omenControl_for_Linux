#!/usr/bin/env python3
"""Native GTK/AT-SPI settings and network interactions against fixture hardware."""
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
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))


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
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    result = {"result": "failed", "profile": PROFILE, "fixture_only": True, "live_hardware_writes": False,
              "interaction": "AT-SPI actions, editable text and native file-list selection; no injected application test actions",
              "picker": "real GTK fallback; private bus has no desktop portal",
              "requested_gtk_backend": os.environ.get("GDK_BACKEND", "auto"),
              "checks_passed": [], "network_checks_passed": [], "health_checks_passed": [],
              "binary_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest()}
    with tempfile.TemporaryDirectory(prefix="omen-native-ui-", dir=ROOT / "target") as directory:
        temp = Path(directory)
        children, logs = [], []
        for name in ["user", "system", "tmp"]:
            (temp / name).mkdir()
        user = temp / "user/config.toml"
        system = temp / "system/config.toml"
        user.write_text('version = 1\n[general]\nstart_minimized = false\npoll_interval_ms = 500\n'
                        '[games.per_game_profiles]\n"custom/audit" = "performance"\n'
                        '[[games.custom_games]]\nid = "custom/audit"\nname = "Audit fixture game"\n'
                        'source = "custom"\nlaunch_cmd = "/usr/bin/true"\n')
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
            daemon = launch([str(BIN / "omen-daemon"), "--mock-omen"], "daemon", True)
            endpoint = daemon.stdout.readline().strip()
            assert endpoint == str(temp / f"tmp/omen-mock-{daemon.pid}.sock")
            assert call(endpoint, "get_info")["mock"] is True
            env["OMEN_SOCK"] = endpoint
            gui = launch([str(BIN / "omen-gui"), "--page", "settings"], "gui")

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

            def activate(node, name):
                assert node.get_state_set().contains(Atspi.StateType.SENSITIVE), name
                action = node.get_action_iface()
                names = [action.get_action_name(index) for index in range(action.get_n_actions())]
                index = next((index for index, action_name in enumerate(names)
                              if action_name in ["click", "activate", "press"]), None)
                assert index is not None, (name, names)
                assert action.do_action(index), name

            def click(name, parent=None):
                node = wait_for(lambda: find("button", name, parent), "missing button: " + name)
                activate(node, name)

            def status(fragment):
                return any(node.get_role_name() == "label" and fragment in node.get_name() for node in walk())

            def label_starting(prefix):
                return next(node for node in walk() if node.get_role_name() == "label"
                            and node.get_name().startswith(prefix))

            def wait_status(fragment):
                wait_for(lambda: status(fragment), "missing operation status: " + fragment)

            def picker_closed(label):
                wait_for(lambda: not any(node.get_role_name() == "dialog" and node.get_name() == label
                                         for node in walk()), "native picker did not close: " + label)

            def save(label, path, title=None):
                click(label)
                title = title or label
                dialog = wait_for(lambda: find("dialog", title), "save dialog absent")
                entry = wait_for(lambda: next(node for node in walk(dialog) if node.get_role_name() == "text"
                                             and node.get_name() in ["Name:", "Name"]), "native save name field not ready")
                assert entry.get_editable_text_iface().set_text_contents(str(path))
                click("Save", dialog)
                picker_closed(title)
                wait_for(path.exists, "selected export file was not written")
                wait_status(title.rstrip("…") + ": saved")

            def select(node):
                current = node
                while current is not None:
                    parent = current.get_parent()
                    if parent is not None and parent.get_selection_iface() is not None:
                        assert parent.get_selection_iface().select_child(current.get_index_in_parent())
                        return
                    current = parent
                raise RuntimeError("no native selection interface")

            def open_backup(label, path):
                click(label)
                def visible_dialog():
                    node = find("dialog", label)
                    return node if node.get_state_set().contains(Atspi.StateType.SHOWING) else None
                dialog = wait_for(visible_dialog, "open dialog absent")
                row = wait_for(lambda: next(node for node in walk(dialog) if node.get_role_name() == "table row"
                                           and node.get_name().startswith(path.name + " ")), "export missing from native recent-file list")
                select(row)
                wait_for(lambda: row.get_state_set().contains(Atspi.StateType.SELECTED), "requested native file was not selected")
                wait_for(lambda: find("button", "Open", dialog).get_state_set().contains(Atspi.StateType.SENSITIVE), "selected file not ready")
                assert row.get_state_set().contains(Atspi.StateType.SELECTED), "native recent-file selection changed before acceptance"
                result.setdefault("observed_picker_selections", []).append({"dialog":label, "file":path.name, "selected":True})
                click("Open", dialog)
                picker_closed(label)

            def service_writes():
                return (temp / "daemon.log").read_text().count("op 'set_service_settings'")

            def network_passed(label):
                result["network_checks_passed"].append(label)
                print("PASS", label, flush=True)

            wait_status("Source: MOCK — fixture data, no real hardware")
            wait_status("Monitoring update received — last update")
            wait_status("Model:")
            wait_for(lambda: find("button", "Fixture"), "service health button absent")
            result["health_checks_passed"].append("health center shows fixture provenance, monitoring age, identity and connected service")
            print("PASS", result["health_checks_passed"][-1], flush=True)
            alert_name = "Notify when a temperature reaches the threshold"
            alerts = wait_for(lambda: find("check box", alert_name), "notification preference absent")
            assert not alerts.get_state_set().contains(Atspi.StateType.CHECKED)
            passed("temperature notifications are disabled by default")
            select(find("label", "System Vitals"))
            wait_status("Fixture network counters")
            sensors = call(endpoint, "get_sensors")
            assert sensors, "fixture must provide sensor readings"
            sensor = sensors[0]
            sensor_prefix = sensor["label"] + ": "
            wait_for(lambda: label_starting(sensor_prefix), "fixture sensor not displayed")
            search = wait_for(lambda: next(node for node in walk() if node.get_role_name() == "entry"
                                           and node.get_editable_text_iface() is not None), "sensor search absent")
            assert search.get_editable_text_iface().set_text_contents("no-such-sensor-omen-test")
            wait_status("No sensors match this search.")
            assert not any(node.get_role_name() == "label" and node.get_name().startswith(sensor_prefix) for node in walk())
            assert search.get_editable_text_iface().set_text_contents(sensor["id"])
            wait_for(lambda: label_starting(sensor_prefix), "search by sensor identifier did not find its reading")
            assert search.get_editable_text_iface().set_text_contents("")
            wait_for(lambda: label_starting(sensor_prefix), "clearing sensor search did not restore the reading")
            passed("sensor search hides unmatched readings, finds an identifier and restores readings when cleared")
            wait_for(lambda: label_starting("fixture0 · "), "fixture interface missing from Vitals")
            wait_for(lambda: label_starting("Receive: 0 B/s  ·  Send: 0 B/s"),
                     "fixture idle rates were not measured")
            network_passed("Vitals shows fixture interface identity and measured idle rates without live network data")
            fixture_root = next(path.parent.parent for path in (temp / "tmp").glob("*/sys/class") if (path / "net/fixture0").is_dir())
            net_proc = fixture_root / "proc/net/dev"
            network_readings = label_starting("Receive: ")
            original_network = net_proc.read_text()
            net_proc.write_text(original_network.replace("1048576 100 0 0", "2097152 100 2 3").replace("524288 50 0 0", "1572864 50 4 5"))
            wait_for(lambda: any(node.get_role_name() == "label" and node.get_name().startswith("Receive: ")
                                 and ("KiB/s" in node.get_name() or "MiB/s" in node.get_name()) for node in walk()),
                     "changed fixture counters did not produce a visible rate")
            wait_for(lambda: status("Errors RX/TX: 2 / 4"), "network error counters not shown")
            wait_for(lambda: status("Dropped RX/TX: 3 / 5"), "network drop counters not shown")
            network_passed("Vitals updates sampled rates, errors and drops from changed fixture counters")
            assert not network_readings.get_state_set().contains(Atspi.StateType.DEFUNCT)
            assert "Errors RX/TX: 2 / 4" in network_readings.get_name()
            assert network_readings == label_starting("Receive: ")
            network_passed("network updates retain the same native accessible text object instead of replacing its row")
            (fixture_root / "sys/class/net/fixture0/ifindex").write_text("8")
            wait_for(lambda: label_starting("Receive: ") != network_readings,
                     "a reused interface name retained the old native identity")
            network_passed("a changed kernel interface index replaces the old row even when its name is reused")
            net_proc.unlink()
            wait_status("Network monitoring unavailable:")
            network_passed("missing fixture counters show an unavailable reason without falling back to the host")
            (fixture_root / "sys/class/net/fixture0").rename(fixture_root / "sys/class/net/replacement0")
            (fixture_root / "sys/class/net/replacement0/ifindex").write_text("8")
            net_proc.write_text(original_network.replace("fixture0:", "replacement0:"))
            wait_for(lambda: label_starting("replacement0 · "), "replacement interface was not rendered after recovery")
            assert not any(node.get_role_name() == "label" and node.get_name().startswith("fixture0 · ") for node in walk())
            network_passed("recovered Vitals replaces removed interface rows with the new identity")
            select(find("label", "Settings & Diagnostics"))

            select(find("label", "Game Library"))
            wait_status("Audit fixture game")
            call(endpoint, "set_profile", {"profile": "balanced"})
            click("Launch")
            wait_status("Launch game: Launch requested, not verified")
            wait_for(lambda: call(endpoint, "get_profile") == "performance", "fixture launch did not apply bound profile")
            restore = wait_for(lambda: find("button", "Restore Balanced"), "manual game profile restoration absent")
            wait_for(lambda: restore.get_state_set().contains(Atspi.StateType.SENSITIVE), "restore action did not become available")
            click("Restore Balanced")
            wait_status("Restore game profile: restored Balanced")
            assert call(endpoint, "get_profile") == "balanced"
            passed("fixture game launch exposes an explicit restoration action that restores the prior profile")
            click("Launch")
            wait_for(lambda: call(endpoint, "get_profile") == "performance", "second fixture launch failed")
            call(endpoint, "set_profile", {"profile": "quiet"})
            wait_for(lambda: not restore.get_state_set().contains(Atspi.StateType.SENSITIVE), "external profile change left manual recovery enabled")
            assert call(endpoint, "get_profile") == "quiet"
            call(endpoint, "set_profile", {"profile": "balanced"})
            passed("a different profile selected after launch is preserved and disables the restoration action")
            select(find("label", "Dashboard"))
            click("Read history data")
            history = wait_for(lambda: find("text", "History readings"), "readable history absent")
            before_history = Atspi.Text.get_text(history.get_text_iface(), 0, -1)
            time.sleep(1.2)
            assert Atspi.Text.get_text(history.get_text_iface(), 0, -1) == before_history, "polling replaced the readable snapshot"
            click("Refresh history readings")
            wait_for(lambda: Atspi.Text.get_text(history.get_text_iface(), 0, -1) != before_history, "explicit history refresh did not obtain new acquisitions")
            passed("readable history remains stable during polling and refreshes only on request")
            select(find("label", "Settings & Diagnostics"))

            original_user = user.read_bytes()
            original_service = call(endpoint, "get_service_settings")
            click("Export user settings…")
            dialog = wait_for(lambda: find("dialog", "Export user settings…"), "cancel dialog absent")
            click("Cancel", dialog)
            picker_closed("Export user settings…")
            assert user.read_bytes() == original_user and not system.exists()
            passed("native export picker cancellation leaves user and service settings unchanged")

            user_backup = temp / "user backup λ.json"
            save("Export user settings…", user_backup)
            exported_user = json.loads(user_backup.read_text())
            assert exported_user["scope"] == "user" and set(exported_user["settings"]) == {"version", "general", "games"}
            assert user_backup.stat().st_mode & 0o777 == 0o600
            assert user.read_bytes() == original_user and service_writes() == 0
            passed("native user export writes private, scoped JSON without changing either configuration")

            click("Import user settings…")
            dialog = wait_for(lambda: find("dialog", "Import user settings…"), "import cancel dialog absent")
            click("Cancel", dialog)
            picker_closed("Import user settings…")
            assert user.read_bytes() == original_user and not system.exists()
            passed("native import picker cancellation leaves both configurations unchanged")
            edited_user = json.loads(user_backup.read_text())
            edited_user["settings"]["general"]["history_samples"] = 123
            edited_user["settings"]["general"]["alerts"]["enabled"] = True
            edited_user["settings"]["general"]["alerts"]["temperature_c"] = 81
            user_backup.write_text(json.dumps(edited_user))
            open_backup("Import user settings…", user_backup)
            wait_for(lambda: find("button", "Apply settings"), "user settings preview absent")
            assert status("Temperature notifications: enabled") and status("Threshold: 81 °C") and user.read_bytes() == original_user
            click("Cancel")
            assert user.read_bytes() == original_user and service_writes() == 0
            passed("user import preview displays notification settings and cancellation preserves both configurations")
            open_backup("Import user settings…", user_backup)
            wait_for(lambda: find("button", "Apply settings"), "second user settings preview absent")
            click("Apply settings")
            wait_status("Import user settings: saved")
            assert tomllib.loads(user.read_text())["general"]["history_samples"] == 123
            assert tomllib.loads(user.read_text())["general"]["alerts"]["enabled"] is True
            assert tomllib.loads(user.read_text())["general"]["alerts"]["temperature_c"] == 81
            assert not system.exists() and call(endpoint, "get_service_settings") == original_service
            passed("native user import saves the selected preferences only to the desktop file")
            saved_user = user.read_bytes()

            service_backup = temp / "service policies.json"
            save("Export service policies…", service_backup)
            exported_service = json.loads(service_backup.read_text())
            assert exported_service["scope"] == "service" and set(exported_service["settings"]) == {"version", "fan", "lighting", "power"}
            assert service_backup.stat().st_mode & 0o777 == 0o600
            assert service_writes() == 0 and user.read_bytes() == saved_user
            passed("native service export reads production IPC and writes a private service-only backup")

            user_backup.write_text(service_backup.read_text())
            open_backup("Import user settings…", user_backup)
            wait_status("Could not read user settings: invalid argument:")
            assert user.read_bytes() == saved_user and service_writes() == 0
            passed("scope-mismatched native user import displays an error without configuration changes")
            user_backup.write_text("{invalid JSON")
            open_backup("Import user settings…", user_backup)
            wait_status("Could not read user settings: invalid argument: settings JSON:")
            assert user.read_bytes() == saved_user and service_writes() == 0
            passed("malformed native import preserves existing configuration and reports failure")
            user_backup.write_text(" " * (1_048_576 + 1))
            open_backup("Import user settings…", user_backup)
            wait_status("settings backup exceeds 1 MiB")
            assert user.read_bytes() == saved_user and service_writes() == 0
            passed("oversized native import reports its size limit and preserves configuration")

            edited_service = json.loads(service_backup.read_text())
            edited_service["settings"]["power"].update(battery_aware=True, on_ac_profile="unchanged", on_battery_profile="quiet")
            service_backup.write_text(json.dumps(edited_service))
            open_backup("Import service policies…", service_backup)
            wait_for(lambda: find("button", "Apply policies"), "service preview absent")
            assert status("Battery automation: enabled") and status("AC profile: leave unchanged")
            assert service_writes() == 0 and call(endpoint, "get_service_settings") == original_service
            click("Cancel")
            wait_status("Service policy import cancelled")
            assert service_writes() == 0 and not system.exists() and user.read_bytes() == saved_user
            passed("service preview shows selected automation and cancellation never sends a mutation")
            open_backup("Import service policies…", service_backup)
            wait_for(lambda: find("button", "Apply policies"), "second service preview absent")
            click("Apply policies")
            wait_status("Import service policies: complete")
            wait_for(lambda: call(endpoint, "get_service_settings") == edited_service["settings"], "reviewed service policy was not applied")
            assert system.exists() and user.read_bytes() == saved_user and service_writes() == 1
            passed("explicit native Apply persists service policy through IPC while leaving desktop settings unchanged")

            select(find("label", "Performance Control"))
            def profile_button():
                return next(node for node in walk() if node.get_name() == "Balanced"
                            and node.get_role_name() in ["button", "toggle button"])
            wait_for(profile_button, "performance page absent")
            wait_for(lambda: not profile_button().get_state_set().contains(Atspi.StateType.SENSITIVE), "profile buttons did not reflect automation ownership")
            wait_status("battery automation owns profile selection")
            passed("profile controls become read-only after the imported battery policy takes ownership")

            select(find("label", "Settings & Diagnostics"))
            service_backup.write_text(json.dumps(exported_service))
            open_backup("Import service policies…", service_backup)
            wait_for(lambda: find("button", "Apply policies"), "restore service preview absent")
            click("Apply policies")
            wait_status("Import service policies: complete")
            wait_for(lambda: call(endpoint, "get_service_settings") == original_service, "service policy restore failed")
            assert service_writes() == 2 and user.read_bytes() == saved_user
            select(find("label", "Performance Control"))
            wait_for(lambda: profile_button().get_state_set().contains(Atspi.StateType.SENSITIVE), "profile buttons did not recover after automation released ownership")
            passed("restoring service policies re-enables profile controls without restarting the application")

            select(find("label", "Settings & Diagnostics"))
            diagnostics = temp / "diagnostics.json"
            click("Export diagnostics (JSON)")
            preview = wait_for(lambda: find("text", "Diagnostics report JSON"), "diagnostic review absent")
            reviewed_json = Atspi.Text.get_text(preview.get_text_iface(), 0, -1)
            assert json.loads(reviewed_json)["mock"] is True
            click("Close preview")
            assert not diagnostics.exists()
            click("Export diagnostics (JSON)")
            preview = wait_for(lambda: find("text", "Diagnostics report JSON"), "second diagnostic review absent")
            reviewed_json = Atspi.Text.get_text(preview.get_text_iface(), 0, -1)
            save("Save report…", diagnostics, "Export diagnostics")
            diagnostic_data = json.loads(diagnostics.read_text())
            assert diagnostic_data == json.loads(reviewed_json), "export differs from reviewed data"
            assert diagnostic_data["mock"] is True and "capabilities" in diagnostic_data
            assert diagnostic_data["compatibility_evidence"]["classification"] == "fixture"
            assert "client_telemetry" not in diagnostic_data
            assert all(sample["source"] != "nvidia_smi" for sample in diagnostic_data["sensors"])
            assert diagnostic_data["network"]["source"] == "fixture"
            assert diagnostics.stat().st_mode & 0o777 == 0o600
            passed("native diagnostics export identifies fixture hardware and writes private JSON")

            open_backup("Import service policies…", service_backup)
            wait_for(lambda: find("button", "Apply policies"), "disconnect preview absent")
            saved_system = system.read_bytes()
            daemon.terminate()
            assert daemon.wait(timeout=8) == 0
            click("Apply policies")
            wait_status("Import service policies: daemon unavailable:")
            assert system.read_bytes() == saved_system and user.read_bytes() == saved_user and service_writes() == 2
            passed("daemon loss during service preview reports failure without saving to either configuration")
            select(find("label", "Performance Control"))
            wait_for(lambda: not profile_button().get_state_set().contains(Atspi.StateType.SENSITIVE), "profile controls remained enabled after daemon loss")
            wait_status("capabilities unreadable")
            passed("daemon loss disables profile controls and explains the missing capability")
            select(find("label", "Settings & Diagnostics"))
            wait_for(lambda: find("button", "Attention needed"), "service health did not report daemon loss")
            wait_status("daemon unavailable")
            result["health_checks_passed"].append("service loss is reflected in the health button and diagnostics")
            print("PASS", result["health_checks_passed"][-1], flush=True)
            save("Export user settings…", temp / "offline-user.json")
            assert user.read_bytes() == saved_user and system.read_bytes() == saved_system
            passed("desktop settings export remains usable after the service disconnects")
            # Exercise the production application menu and the welcome panel
            # through AT-SPI, including persistence and reopening after dismissal.
            def menu_action(name):
                click("Application actions")
                def item():
                    for node in walk():
                        if node.get_name() != name:
                            continue
                        action = node.get_action_iface()
                        if action is not None and action.get_n_actions():
                            return action
                    return None
                action = wait_for(item, "application menu action missing: " + name)
                assert action.do_action(0), name
            menu_action("Show welcome")
            wait_status("Welcome to OMEN Control")
            click("Dismiss")
            wait_for(lambda: not status("Welcome to OMEN Control"), "welcome did not dismiss")
            assert tomllib.loads(user.read_text())["general"]["onboarding_dismissed"] is True
            menu_action("Show welcome")
            wait_status("Welcome to OMEN Control")
            click("Open Settings & Diagnostics")
            wait_for(lambda: find("button", "Export user settings…"), "welcome did not navigate to diagnostics")
            assert tomllib.loads(user.read_text())["general"]["onboarding_dismissed"] is True
            passed("application menu reopens the skippable welcome; native dismissal persists and diagnostics navigation works")
            result["result"] = "passed"
            result["gtk_display_type"] = next(line.split(" ",1)[1] for line in (temp / "gui.log").read_text().splitlines()
                                               if line.startswith("OMEN_UI_BACKEND "))
        except BaseException as error:
            result["error"] = str(error)
            if "walk" in locals():
                result["accessible_tree"] = [{"role": node.get_role_name(), "name": node.get_name()}
                                             for node in walk()]
                result["operation_labels"] = [node.get_name() for node in walk() if node.get_role_name() == "label"
                                              and node.get_name().startswith(("Export user settings:", "Import user settings:", "File selection", "Another action", "Import failed:"))]
                result["editable_fields"] = []
                for node in walk():
                    try:
                        if node.get_editable_text_iface() is not None:
                            result["editable_fields"].append({"role":node.get_role_name(),"name":node.get_name(),
                                "text":Atspi.Text.get_text(node.get_text_iface(),0,-1)[:200]})
                    except (GLib.Error, TypeError):
                        pass
            raise
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=8)
            for log in logs:
                log.close()
            for name in ["gui", "daemon", "session", "a11y"]:
                path = temp / (name + ".log")
                if path.exists():
                    (EVIDENCE / f"gui-settings-{PROFILE}-{name}.log").write_text(path.read_text())
            result["cleanup_complete"] = True
            result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            (EVIDENCE / f"gui-settings-{PROFILE}.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
