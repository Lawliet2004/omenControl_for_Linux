#!/usr/bin/env python3
"""Run only inside the disposable Ubuntu VM managed by systemd_vm.py."""
import json
import hashlib
import os
from pathlib import Path
import pwd
import subprocess
import sys
import time

UNIT = "omen-linux.service"
SOCKET = Path("/run/omen-linux/ctl.sock")
CONFIG = Path("/etc/omen-linux/config.toml")
RESULT = Path("/var/tmp/omen-vm-result.json")
state = json.loads(RESULT.read_text()) if RESULT.exists() else {"checks_passed": []}


def run(*args, check=True):
    result = subprocess.run(args, capture_output=True, text=True, timeout=90)
    print("+", " ".join(args), result.returncode, result.stdout.strip(), result.stderr.strip(), flush=True)
    if check:
        result.check_returncode()
    return result


def passed(message):
    state["checks_passed"].append(message)
    RESULT.write_text(json.dumps(state, indent=2) + "\n")
    print("PASS", message, flush=True)


def wait_for(predicate, message):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (OSError, ValueError, subprocess.SubprocessError):
            pass
        time.sleep(.2)
    raise RuntimeError(message)


CLIENT = '''import json,socket,sys
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(65);s.connect('/run/omen-linux/ctl.sock');s.sendall(sys.argv[1].encode()+b'\\n')
 print(s.makefile('r').readline())
'''


def request(op, args=None, user=False):
    operation = {"op": op}
    if args is not None:
        operation["args"] = args
    account = pwd.getpwnam("ubuntu")
    def drop():
        os.setgroups([])
        os.setgid(account.pw_gid)
        os.setuid(account.pw_uid)
    result = subprocess.run(["python3", "-c", CLIENT, json.dumps({"v": 1, "id": 1, "op": operation})],
                            preexec_fn=drop if user else None, capture_output=True, text=True, timeout=70)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


def active():
    return subprocess.run(["systemctl", "is-active", "--quiet", UNIT]).returncode == 0


def ready():
    wait_for(lambda: active() and SOCKET.is_socket() and request("get_info")["ok"], "service not ready")
    assert request("get_info")["data"]["mock"] is False


def pid():
    return int(run("systemctl", "show", "--property=MainPID", "--value", UNIT).stdout)


def denied(response):
    return not response["ok"] and response["error"]["code"] == "permission_denied"


def initial():
    state["initial_boot_id"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    state["kernel"] = run("uname", "-r").stdout.strip()
    state["systemd_version"] = run("systemctl", "--version").stdout.splitlines()[0]
    ready()
    service_pid = pid()
    assert Path(f"/proc/{service_pid}/cmdline").read_bytes().split(b'\0') == [b'/usr/bin/omen-daemon', b'']
    assert request("get_info", user=True)["ok"]
    assert SOCKET.stat().st_mode & 0o777 == 0o666
    assert CONFIG.stat().st_mode & 0o777 == 0o600
    passed("real systemd starts the installed read-only daemon; UID 1000 connects to its production socket")
    network = request("get_network", user=True)
    assert network["ok"] and network["data"]["source"] == "kernel" and network["data"]["interfaces"]
    time.sleep(.3)
    network = request("get_network", user=True)
    assert network["ok"] and network["data"]["interval_ms"] is not None
    assert all(i["ifindex"] > 0 for i in network["data"]["interfaces"])
    state["network_monitoring"] = network["data"]
    passed("sandboxed installed service exposes real per-interface network counters and sampled rates to UID 1000")
    status = dict(line.split(":", 1) for line in Path(f"/proc/{service_pid}/status").read_text().splitlines() if ":" in line)
    assert status["NoNewPrivs"].strip() == "1"
    assert int(status["CapBnd"].strip(), 16) == 2 and int(status["CapEff"].strip(), 16) == 2
    assert status["Seccomp"].strip() == "2"
    passed("running process has NoNewPrivileges, seccomp and only CAP_DAC_OVERRIDE")
    run("systemctl", "cat", UNIT)
    state["loaded_sandbox_properties"] = run("systemctl", "show", UNIT, "--property=FragmentPath,ProtectSystem,ProtectHome,ReadOnlyPaths,ReadWritePaths,ProtectKernelTunables,PrivateTmp").stdout
    state["manager_mount_namespace"] = os.readlink("/proc/1/ns/mnt")
    state["service_mount_namespace"] = os.readlink(f"/proc/{service_pid}/ns/mnt")
    state["sysfs_mountinfo"] = [line for line in Path(f"/proc/{service_pid}/mountinfo").read_text().splitlines()
                               if line.split()[4].startswith("/sys")]
    mounts = {}
    for path, writable in [("/sys", False), ("/etc/hostname", False), ("/etc/omen-linux", True), ("/run/omen-linux", True)]:
        # Entering only the mount namespace can retain the caller's root path.
        # Use the target root and independently check its proc-root view.
        options = run("nsenter", "--target", str(service_pid), "--mount", "--root", "--wd=/", "--", "findmnt", "--noheadings", "--output", "VFS-OPTIONS", "--target", path).stdout.strip()
        readonly = bool(os.statvfs(f"/proc/{service_pid}/root{path}").f_flag & os.ST_RDONLY)
        mounts[path] = {"options": options, "statvfs_readonly": readonly, "expected_writable": writable}
    state["service_mount_options"] = mounts
    RESULT.write_text(json.dumps(state, indent=2) + "\n")
    for path, mount in mounts.items():
        assert mount["statvfs_readonly"] == (not mount["expected_writable"]), (path, mount)
        # findmnt can include a covered lower mount and its visible overmount.
        # statvfs above establishes the access mode of the visible path.
        expected = "rw" if mount["expected_writable"] else "ro"
        assert any(expected in row.split(',') for row in mount["options"].splitlines()), (path, mount)
    marker = Path("/home/ubuntu/omen-vm-private-marker")
    temporary = Path("/tmp/omen-vm-outside-marker")
    try:
        marker.write_text("outside the service namespace")
        temporary.write_text("outside the service namespace")
        for path in [marker, temporary]:
            assert not Path(f"/proc/{service_pid}/root{path}").exists()
    finally:
        marker.unlink(missing_ok=True)
        temporary.unlink(missing_ok=True)
    passed("actual mount namespace enforces read-only system/sysfs, writable service paths, hidden home and private tmp")

    settings = request("get_service_settings")["data"]
    assert settings["power"]["battery_aware"] is False
    settings["power"]["on_ac_profile"] = "unchanged"
    assert denied(request("set_service_settings", {"settings": settings}, user=True))
    rule = Path("/etc/polkit-1/rules.d/00-omen-vm-test.rules")
    try:
        rule.write_text('polkit.addRule(function(action, subject) { if (subject.user == "ubuntu" && action.id == "com.omenlinux.admin") return polkit.Result.YES; });\n')
        wait_for(lambda: request("set_service_settings", {"settings": settings}, user=True)["ok"], "sandboxed admin save failed")
        assert request("get_service_settings")["data"] == settings
        assert CONFIG.stat().st_mode & 0o777 == 0o600
    finally:
        rule.unlink(missing_ok=True)
    wait_for(lambda: denied(request("set_service_settings", {"settings": settings}, user=True)), "admin grant not revoked")
    passed("sandboxed production pkcheck/polkit denies, grants a VM-only admin save and observes rule revocation")
    run("systemctl", "stop", UNIT)
    assert not active() and not SOCKET.exists() and not SOCKET.parent.exists()
    passed("systemd stop removes the socket and RuntimeDirectory")
    run("systemctl", "start", UNIT)
    ready()
    before = pid()
    run("systemctl", "restart", UNIT)
    ready()
    assert pid() != before
    passed("systemd start/restart creates a new responding daemon")
    before = pid()
    run("systemctl", "kill", "--kill-whom=main", "--signal=SIGKILL", UNIT)
    wait_for(lambda: active() and pid() != before and SOCKET.is_socket() and request("get_info")["ok"], "on-failure restart failed")
    passed("Restart=on-failure recovers the read-only service and socket after guest-only SIGKILL")


def after_reboot():
    assert Path("/proc/sys/kernel/random/boot_id").read_text().strip() != state["initial_boot_id"]
    ready()
    assert request("get_service_settings")["data"]["power"]["on_ac_profile"] == "unchanged"
    passed("a real guest reboot starts the enabled service and retains system configuration")
    run("systemctl", "disable", "--now", UNIT)
    assert not active() and not SOCKET.exists()
    state["disabled_boot_id"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    passed("administrator disable --now stops the service and removes its socket")


def after_disabled_reboot():
    assert Path("/proc/sys/kernel/random/boot_id").read_text().strip() != state["disabled_boot_id"]
    assert not active() and not SOCKET.exists()
    passed("a guest reboot respects administrator-disabled service state")
    saved = CONFIG.read_bytes()
    run("dpkg", "-i", "/var/tmp/omen-current.deb")
    assert not active() and not SOCKET.exists() and CONFIG.read_bytes() == saved
    passed("real systemd package reinstall preserves a disabled service and configuration")
    run("systemctl", "mask", UNIT)
    run("dpkg", "-i", "/var/tmp/omen-current.deb")
    assert run("systemctl", "is-enabled", UNIT, check=False).stdout.strip() == "masked"
    assert not active() and not SOCKET.exists()
    passed("real systemd package reinstall preserves an administrator mask")
    run("systemctl", "unmask", UNIT)
    run("systemctl", "enable", "--now", UNIT)
    ready()
    before = pid()
    run("dpkg", "-i", "/var/tmp/omen-current.deb")
    ready()
    assert pid() != before and CONFIG.read_bytes() == saved
    passed("enabled running-service reinstall performs a real restart and retains configuration")
    run("dpkg", "--remove", "omen-linux")
    assert not active() and not SOCKET.exists() and CONFIG.read_bytes() == saved
    passed("dpkg removal stops the actual service and preserves system configuration")
    run("dpkg", "-i", "/var/tmp/omen-current.deb")
    ready()
    passed("reinstall after removal starts the formerly enabled service")
    run("dpkg", "--purge", "omen-linux")
    assert not active() and not SOCKET.exists() and not CONFIG.exists()
    assert not Path("/usr/share/polkit-1/actions/com.omenlinux.policy").exists()
    assert not Path("/etc/systemd/system/multi-user.target.wants/omen-linux.service").is_symlink()
    passed("dpkg purge stops the service and removes its config, policy and enablement link")


if __name__ == "__main__":
    assert os.geteuid() == 0 and Path("/proc/1/comm").read_text().strip() == "systemd"
    assert run("systemd-detect-virt", "--vm").stdout.strip() in ["kvm", "qemu"]
    assert "QEMU" in Path("/sys/class/dmi/id/sys_vendor").read_text()
    state["test_payload_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    assert pwd.getpwnam("ubuntu").pw_uid == 1000
    {"initial": initial, "after_reboot": after_reboot, "after_disabled_reboot": after_disabled_reboot}[sys.argv[1]]()
    RESULT.write_text(json.dumps(state, indent=2) + "\n")
