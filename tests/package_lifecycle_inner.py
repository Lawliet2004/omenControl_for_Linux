#!/usr/bin/env python3
"""Container payload for package_lifecycle.py. Never run on the host."""
import hashlib
import json
import os
import pwd
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import time

UNIT = "omen-linux.service"
CONFIG = Path("/etc/omen-linux/config.toml")
LINK = Path("/etc/systemd/system/multi-user.target.wants/omen-linux.service")
STATE = Path("/var/lib/systemd/deb-systemd-helper-enabled/omen-linux.service.dsh-also")
POLICY = Path("/usr/share/polkit-1/actions/com.omenlinux.policy")
CURRENT = "/test/current.deb"
BASELINE = "/test/baseline.deb"
CHECKS = []


def run(*args, env=None, check=True):
    print("+ " + shlex.join(args), file=sys.stderr, flush=True)
    result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=120)
    print(result.stdout + result.stderr, file=sys.stderr, end="", flush=True)
    if check:
        result.check_returncode()
    return result


def passed(message):
    CHECKS.append(message)
    print("PASS " + message, file=sys.stderr, flush=True)


def wait_for(predicate, message):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.1)
    raise RuntimeError(message)


CLIENT = '''import json,os,socket,sys
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(65);s.connect('/run/omen-linux/ctl.sock')
 s.sendall(sys.argv[1].encode()+b'\\n')
 print(json.dumps({'uid':os.geteuid(),'response':json.loads(s.makefile('rb').readline())}))
'''


def request(op, args=None, user=True):
    operation = {"op": op}
    if args is not None:
        operation["args"] = args
    def drop_user():
        os.setgroups([])
        os.setgid(1000)
        os.setuid(1000)
    result = subprocess.run(["python3", "-c", CLIENT, json.dumps({"v": 1, "id": 1, "op": operation})],
                            preexec_fn=drop_user if user else None,
                            capture_output=True, text=True, timeout=70)
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert payload["uid"] == (1000 if user else 0)
    return payload["response"]


def error(response, code):
    return not response["ok"] and response["error"]["code"] == code


def authorization():
    try:
        account = pwd.getpwuid(1000)
    except KeyError:
        run("useradd", "--uid", "1000", "--create-home", "omen-test")
        account = pwd.getpwuid(1000)
    username = json.dumps(account.pw_name)
    processes = []
    logs = []
    rule = Path("/etc/polkit-1/rules.d/00-omen-lifecycle.rules")
    original = CONFIG.read_bytes()
    try:
        assert not Path("/run/dbus/system_bus_socket").exists()
        log = Path("/test/daemon.log").open("w")
        logs.append(log)
        daemon = subprocess.Popen(["/usr/bin/omen-daemon"], stdout=log, stderr=log)
        processes.append(daemon)
        wait_for(lambda: Path("/run/omen-linux/ctl.sock").is_socket(), "daemon socket not ready")
        info = request("get_info")
        assert info["ok"] and info["data"]["mock"] is False
        network = request("get_network")
        assert network["ok"] and network["data"]["source"] == "kernel" and network["data"]["interfaces"]
        time.sleep(.3)
        network = request("get_network")
        assert network["ok"] and network["data"]["interval_ms"] is not None
        assert all(i["ifindex"] > 0 for i in network["data"]["interfaces"])
        passed("installed daemon exposes container-network counters and sampled rates to UID 1000 without authorization grants")
        settings = request("get_service_settings")["data"]
        assert settings["power"]["battery_aware"] is False
        settings["power"]["on_ac_profile"] = "unchanged"
        assert error(request("set_service_settings", {"settings": settings}), "permission_denied")
        assert CONFIG.read_bytes() == original
        passed("installed production daemon permits UID 1000 reads and fails closed without a system bus")

        Path("/run/dbus").mkdir(exist_ok=True)
        for command, name in [
            (["dbus-daemon", "--system", "--nofork", "--nopidfile"], "dbus"),
            (["/usr/lib/polkit-1/polkitd", "--no-debug"], "polkit"),
        ]:
            log = Path(f"/test/{name}.log").open("w")
            logs.append(log)
            processes.append(subprocess.Popen(command, stdout=log, stderr=log))
            if name == "dbus":
                wait_for(lambda: Path("/run/dbus/system_bus_socket").is_socket(), "private system bus not ready")
        wait_for(lambda: run("busctl", "--system", "--no-pager", "status", "org.freedesktop.PolicyKit1", check=False).returncode == 0,
                 "polkit did not register on the container bus")
        assert error(request("set_service_settings", {"settings": settings}), "permission_denied")
        assert error(request("launch_game", {"id": "custom/forbidden"}), "permission_denied")
        assert CONFIG.read_bytes() == original
        passed("installed polkit actions deny non-session UID 1000 admin/control requests without an authentication agent")

        rule.write_text('polkit.addRule(function(action, subject) { if (subject.user == ' + username + ' && action.id == "com.omenlinux.control") return polkit.Result.YES; });\n')
        wait_for(lambda: error(request("launch_game", {"id": "custom/forbidden"}), "unsupported"), "control rule not loaded")
        assert error(request("set_service_settings", {"settings": settings}), "permission_denied")
        passed("a container-only control grant reaches the daemon refusal and cannot authorize admin settings")
        rule.write_text('polkit.addRule(function(action, subject) { if (subject.user == ' + username + ' && action.id == "com.omenlinux.admin") return polkit.Result.YES; });\n')
        wait_for(lambda: request("set_service_settings", {"settings": settings})["ok"], "admin rule not loaded")
        assert CONFIG.read_bytes() != original
        assert request("get_service_settings")["data"] == settings
        assert CONFIG.stat().st_mode & 0o777 == 0o600
        passed("a container-only admin grant saves disabled automation through production IPC as UID 1000")
        rule.unlink()
        wait_for(lambda: error(request("set_service_settings", {"settings": settings}), "permission_denied"), "grant revocation not observed")
        passed("removing the temporary polkit rule revokes subsequent admin requests")
        for child in reversed(processes[1:]):
            child.terminate()
            child.wait(timeout=5)
        assert request("get_info")["ok"]
        assert error(request("set_service_settings", {"settings": settings}), "permission_denied")
        passed("loss of the authorization bus leaves reads working and mutations denied")
    finally:
        rule.unlink(missing_ok=True)
        for child in reversed(processes):
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=8)
        for log in logs:
            log.close()
        CONFIG.write_bytes(original)
        CONFIG.chmod(0o600)
    assert not Path("/run/omen-linux/ctl.sock").exists()
    assert "skipping authorization" not in Path("/test/daemon.log").read_text()
    passed("SIGTERM removes the production socket; no mock authorization bypass was used")
    return Path(account.pw_dir)


def main():
    assert os.geteuid() == 0 and Path("/.dockerenv").is_file(), "container payload only"
    assert os.environ.get("OMEN_LIFECYCLE_CONTAINER") == "1"
    status = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines() if ":" in line)
    assert not int(status["CapEff"].strip(), 16) & (1 << 21), "SYS_ADMIN must be absent"
    assert status["NoNewPrivs"].strip() == "1"
    assert os.statvfs("/sys").f_flag & os.ST_RDONLY, "sysfs must be read-only"
    assert not Path("/run/systemd/system").exists(), "this test requires an offline systemd container"
    assert not Path("/var/run/docker.sock").exists()
    expected = run("dpkg-deb", "-f", CURRENT, "Version").stdout.strip()
    baseline = run("dpkg-deb", "-f", BASELINE, "Version").stdout.strip()
    assert run("dpkg", "--compare-versions", expected, "gt", baseline, check=False).returncode == 0
    assert run("dpkg-query", "-W", "-f=${Version}", "omen-linux").stdout == expected
    assert LINK.is_symlink() and STATE.is_file() and POLICY.is_file()
    assert CONFIG.stat().st_mode & 0o777 == 0o600
    assert "not found" not in run("ldd", "/usr/bin/omen-gui").stdout
    assert run("omen-cli", "detect").stdout.strip()
    run("desktop-file-validate", "/usr/share/applications/com.omenlinux.Gui.desktop")
    passed("fresh Ubuntu package install resolves distro libraries, creates private config and enables the unit offline")

    home = authorization()
    user = home / ".config/omen-linux/config.toml"
    user.parent.mkdir(parents=True)
    user.write_text("# user settings must survive package removal\nversion = 1\n")
    user.chmod(0o600)
    os.chown(user, 1000, 1000)
    saved_user = user.read_bytes()
    CONFIG.write_text("# administrator settings must survive upgrade/remove\nversion = 1\n[power]\nbattery_aware = false\n")
    saved = CONFIG.read_bytes()
    run("systemctl", "disable", UNIT)
    run("dpkg", "-i", CURRENT)
    assert not LINK.is_symlink() and CONFIG.read_bytes() == saved
    passed("same-version reinstall preserves a disabled unit and administrator configuration")
    run("systemctl", "enable", UNIT)
    run("dpkg", "-i", CURRENT)
    assert LINK.is_symlink() and CONFIG.read_bytes() == saved
    passed("same-version reinstall preserves an enabled unit without rewriting configuration")
    run("systemctl", "mask", UNIT)
    run("dpkg", "-i", CURRENT)
    mask = Path("/etc/systemd/system/omen-linux.service")
    assert mask.is_symlink() and os.readlink(mask) == "/dev/null"
    passed("same-version reinstall preserves an administrator mask")
    run("systemctl", "unmask", UNIT)
    run("systemctl", "disable", UNIT)

    # A recorder exercises the running-manager maintscript branch without
    # pretending that a systemd PID 1 or its sandbox was tested.
    fake = Path("/test/bin")
    fake.mkdir()
    recorder = fake / "systemctl"
    recorder.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> /test/systemctl.calls\nexit 99\n')
    recorder.chmod(0o755)
    policy = Path("/usr/sbin/policy-rc.d")
    prior = policy.read_bytes() if policy.exists() else None
    policy.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> /test/policy.calls\nexit 101\n')
    policy.chmod(0o755)
    Path("/run/systemd/system").mkdir(parents=True)
    try:
        run("dpkg", "-i", CURRENT, env=dict(os.environ, PATH=str(fake) + ":" + os.environ["PATH"]))
        calls = Path("/test/systemctl.calls").read_text().splitlines()
        assert calls == ["--system daemon-reload"], calls
        assert "omen-linux.service restart" in Path("/test/policy.calls").read_text()
        assert not LINK.is_symlink()
        passed("policy-rc.d denial prevents runtime restart; upgrades do not disable or enable the unit directly")
    finally:
        Path("/run/systemd/system").rmdir()
        if prior is None:
            policy.unlink()
        else:
            policy.write_bytes(prior)

    run("dpkg", "--remove", "omen-linux")
    assert CONFIG.read_bytes() == saved and user.read_bytes() == saved_user
    assert not POLICY.exists() and not Path("/usr/bin/omen-daemon").exists()
    run("dpkg", "-i", CURRENT)
    assert not LINK.is_symlink() and CONFIG.read_bytes() == saved
    passed("remove/reinstall retains service-disable state, system configuration and desktop settings")
    CONFIG.with_suffix(".toml.bak").write_text("backup")
    run("dpkg", "--purge", "omen-linux")
    assert not CONFIG.exists() and not CONFIG.with_suffix(".toml.bak").exists()
    assert not LINK.is_symlink() and not STATE.exists() and not POLICY.exists()
    assert user.read_bytes() == saved_user
    passed("purge removes package settings, policy and helper links while retaining desktop settings")

    # Exercise a real version increase from the pre-helper package, whose
    # missing helper state must not turn a disabled installation back on.
    run("dpkg", "-i", BASELINE)
    assert not STATE.exists()
    run("systemctl", "enable", UNIT)
    run("systemctl", "disable", UNIT)
    CONFIG.write_bytes(saved)
    run("dpkg", "-i", CURRENT)
    assert not LINK.is_symlink() and STATE.exists() and CONFIG.read_bytes() == saved
    assert run("dpkg-query", "-W", "-f=${Version}", "omen-linux").stdout == expected
    passed("real legacy-version upgrade bootstraps helper state without enabling an administrator-disabled service")
    run("dpkg", "--purge", "omen-linux")
    for choice in ["enabled", "masked"]:
        run("dpkg", "-i", BASELINE)
        assert not STATE.exists()
        run("systemctl", "enable" if choice == "enabled" else "mask", UNIT)
        CONFIG.write_bytes(saved)
        run("dpkg", "-i", CURRENT)
        assert STATE.exists() and CONFIG.read_bytes() == saved
        if choice == "enabled":
            assert LINK.is_symlink()
        else:
            assert mask.is_symlink() and os.readlink(mask) == "/dev/null"
            run("systemctl", "unmask", UNIT)
        passed(f"real legacy-version upgrade preserves an administrator-{choice} service")
        run("dpkg", "--purge", "omen-linux")
    run("dpkg", "-i", CURRENT)
    assert LINK.is_symlink() and CONFIG.stat().st_mode & 0o777 == 0o600
    passed("purge followed by a fresh install restores the default enablement and private configuration")
    if os.environ.get("OMEN_GAME_FORMAT_FIXTURES") == "1":
        work = Path("/test/work")
        work.mkdir(mode=0o700)
        os.chown(work, 1000, 1000)
        temporary = work / "temporary"
        temporary.mkdir(mode=0o700)
        os.chown(temporary, 1000, 1000)
        def desktop_user():
            os.setgroups([])
            os.setgid(1000)
            os.setuid(1000)
        env = dict(os.environ, OMEN_TEST_BIN_DIR="/usr/bin", OMEN_TEST_TEMP_DIR=str(temporary),
                   OMEN_TEST_EVIDENCE_DIR=str(work / "formats"), OMEN_TEST_PROFILE="installed")
        checked = subprocess.run([sys.executable, "/test/game_formats.py"], env=env,
                                 preexec_fn=desktop_user, text=True, capture_output=True, timeout=120)
        Path("/test/installed-game-formats.log").write_text(checked.stdout + checked.stderr)
        assert checked.returncode == 0, checked.stdout + checked.stderr
        formats = json.loads((work / "formats/game-formats.json").read_text())
        assert formats["result"] == "passed" and formats["cleanup_complete"]
        assert len(formats["checks_passed"]) == 15
        passed("installed launcher-format discovery, routing and recovery pass as the desktop UID on private fixtures")
    run("dpkg", "--purge", "omen-linux")
    return {"checks_passed": CHECKS, "current_version": expected, "baseline_version": baseline,
            "fixture_policy_only": True, "hardware_writes": False,
            "systemd_pid1_tested": False, "gui_authentication_prompt_tested": False,
            "test_payload_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "configuration_sha256": hashlib.sha256(saved).hexdigest()}


if __name__ == "__main__":
    result = main()
    Path("/test/result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
