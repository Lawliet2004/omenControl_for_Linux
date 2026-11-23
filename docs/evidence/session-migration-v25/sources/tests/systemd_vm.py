#!/usr/bin/env python3
"""Validate the installed systemd service in a disposable signed Ubuntu VM."""
import argparse
import contextlib
import datetime
import hashlib
import json
import os
import re
import shutil
import shlex
from pathlib import Path
import socket
import subprocess
import tempfile
import tarfile
import time

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--image", type=Path, default=ROOT / "target/vm-images/ubuntu-26.04-server-cloudimg-amd64.img")
    parser.add_argument("--runtime", type=Path, default=ROOT / "target/vm-runtime/root")
    parser.add_argument("--managed-game-helper", type=Path, help="Release native-session test executable for private-PPD installed-sandbox checks")
    parser.add_argument("--apt-cache-archive", type=Path, help="Optional cached .deb files; APT still verifies signed-index hashes")
    parser.add_argument("--offline-repository", type=Path, help="Local signed Ubuntu repository archive; guest networking is restricted to explicit SSH forwarding")
    parser.add_argument("--resolve-only", action="store_true", help="Resolve dependency URIs against the offline repository, then stop without installing or running application tests")
    parser.add_argument("--repository-component", action="append", choices=["main", "universe", "multiverse"], default=[])
    parser.add_argument("--extra-package", action="append", default=[], help="Additional guest-only distro package for a validation environment")
    parser.add_argument("--memory-mib", type=int, default=1024)
    parser.add_argument("--graphical-authorization", action="store_true", help="Exercise the installed GUI and real KDE polkit agent in a private virtual display")
    parser.add_argument("--flatpak-scope-probe", action="store_true", help="Observe real Flatpak scope/namespace boundaries in the disposable guest")
    parser.add_argument("--flatpak-migration-phase", choices=["ended", "interrupted"], default="interrupted", help="Expected installed session result after the Flatpak peer leaves its scope; use ended for the historical revision 24 baseline")
    parser.add_argument("--source-switch", action="store_true", help="Exercise installed default source transitions in the private guest desktop")
    parser.add_argument("--real-game", action="store_true", help="Exercise native Lutris/OpenTTD with a private profile provider in the guest desktop")
    parser.add_argument("--keep-failed", action="store_true", help="Debug only: stop the failed VM but retain its disk and inputs for inspection")
    parser.add_argument("--apt-mirror", default="https://archive.ubuntu.com/ubuntu", help="HTTPS Ubuntu archive mirror for the guest only")
    args = parser.parse_args()
    assert 512 <= args.memory_mib <= 4096
    assert not args.graphical_authorization or (args.offline_repository and args.memory_mib >= 2048)
    assert not args.flatpak_scope_probe or args.managed_game_helper
    assert not args.source_switch or (args.offline_repository and args.memory_mib >= 2048)
    assert not args.real_game or (args.offline_repository and args.memory_mib >= 2048 and args.managed_game_helper)
    assert all(re.fullmatch(r"[a-z0-9][a-z0-9+.-]*(?::[a-z0-9]+)?", name) for name in args.extra_package)
    components = sorted(set(args.repository_component or ["main"]))
    extra_packages = ' ' + shlex.join(args.extra_package) if args.extra_package else ''
    assert not args.resolve_only or args.offline_repository, "dependency resolution mode requires an offline repository"
    if args.offline_repository:
        with tarfile.open(args.offline_repository) as repository:
            members = repository.getmembers()
            assert len(members) <= 2048 and sum(m.size for m in members) <= 512 * 1024 * 1024
            for member in members:
                path = Path(member.name)
                assert not path.is_absolute() and '..' not in path.parts
                assert member.isdir() or member.isfile(), "repository contains a link or special file"
                assert path.parts and path.parts[0] in ['dists', 'pool']
            names = {m.name for m in members}
            for suite in ['resolute', 'resolute-updates', 'resolute-security']:
                assert 'dists/' + suite + '/InRelease' in names
                for component in components:
                    assert 'dists/' + suite + '/' + component + '/binary-amd64/Packages' in names
    if args.apt_cache_archive:
        with tarfile.open(args.apt_cache_archive) as cache:
            assert all(m.isfile() and '/' not in m.name and m.name.endswith('.deb') for m in cache.getmembers())
    assert re.fullmatch(r"https://[A-Za-z0-9.-]+/[A-Za-z0-9/._-]+", args.apt_mirror)
    assert os.geteuid() != 0, "run QEMU as the desktop user"
    image, package, runtime = args.image.resolve(), args.package.resolve(), args.runtime.resolve()
    assert all(',' not in str(path) for path in [image, package, runtime, ROOT])
    subprocess.run(["gpgv", "--keyring", "/usr/share/keyrings/ubuntu-cloudimage-keyring.gpg",
                    str(image.parent / "SHA256SUMS.gpg"), str(image.parent / "SHA256SUMS")], check=True)
    expected = next(line.split()[0] for line in (image.parent / "SHA256SUMS").read_text().splitlines() if line.split()[-1].lstrip('*') == image.name)
    assert hashlib.file_digest(image.open("rb"), "sha256").hexdigest() == expected
    env = dict(os.environ, LD_LIBRARY_PATH=str(runtime / "usr/lib/x86_64-linux-gnu"),
               QEMU_MODULE_DIR=str(runtime / "usr/lib/x86_64-linux-gnu/qemu"))
    qemu = runtime / "usr/bin/qemu-system-x86_64"
    qemu_img = runtime / "usr/bin/qemu-img"
    result = {"started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "package_sha256": hashlib.file_digest(package.open("rb"), "sha256").hexdigest(),
              "image_sha256": expected, "qemu_version": subprocess.check_output([str(qemu), "--version"], env=env, text=True).splitlines()[0],
              "host_uid": os.geteuid(), "host_package_installed": False,
              "apt_mirror": args.apt_mirror,
              "offline_repository_sha256": hashlib.file_digest(args.offline_repository.open("rb"), "sha256").hexdigest() if args.offline_repository else None,
              "guest_network_restricted": bool(args.offline_repository),
              "dependency_resolution_only": args.resolve_only,
              "extra_guest_packages": args.extra_package,
              "repository_components": components,
              "graphical_authorization_requested": args.graphical_authorization,
              "real_game_requested": args.real_game,
              "source_switch_requested": args.source_switch,
              "flatpak_scope_probe_requested": args.flatpak_scope_probe,
              "flatpak_migration_phase_expected": args.flatpak_migration_phase,
              "application_tests_executed": False,
              "apt_cache_sha256": hashlib.file_digest(args.apt_cache_archive.open("rb"), "sha256").hexdigest() if args.apt_cache_archive else None,
              "hardware_writes": False, "physical_hardware_validated": False,
              "result": "failed", "vm_exited": False, "temporary_files_removed": False}
    work_paths = []
    def finish():
        result["temporary_files_removed"] = bool(work_paths) and not work_paths[0].exists()
        result["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (EVIDENCE / "vm-systemd.json").write_text(json.dumps(result, indent=2) + "\n")
    with contextlib.ExitStack() as cleanup:
        cleanup.callback(finish)
        directory = tempfile.mkdtemp(prefix="omen-systemd-vm-", dir=ROOT / "target")
        work = Path(directory)
        work_paths.append(work)
        def remove_work():
            if args.keep_failed and result["result"] != "passed":
                result["retained_work_directory"] = str(work)
            else:
                shutil.rmtree(work)
        cleanup.callback(remove_work)
        key = work / "id_ed25519"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
        seed = work / "seed"
        seed.mkdir()
        userdata = {"users": ["default"], "ssh_authorized_keys": [key.with_suffix(".pub").read_text().strip()],
                    "ssh_pwauth": False, "disable_root": True, "package_update": False}
        (seed / "user-data").write_text("#cloud-config\n" + json.dumps(userdata, indent=2) + "\n")
        (seed / "meta-data").write_text("instance-id: " + work.name + "\nlocal-hostname: omen-systemd-test\n")
        seed_inputs = [str(seed / "user-data"), str(seed / "meta-data")]
        if args.offline_repository:
            # The guest needs only DHCP and explicit host-forwarded SSH. Do not
            # make boot depend on DNS/outside connectivity that QEMU forbids.
            (seed / "network-config").write_text(json.dumps({"version": 2, "ethernets": {
                "fixture": {"match": {"macaddress": "52:54:00:12:34:56"},
                    "dhcp4": True, "dhcp6": False, "optional": True, "accept-ra": False,
                    "dhcp4-overrides": {"use-dns": False, "use-ntp": False}}}}, indent=2) + "\n")
            seed_inputs.append(str(seed / "network-config"))
        iso = work / "seed.iso"
        subprocess.run(["genisoimage", "-quiet", "-output", str(iso), "-volid", "cidata", "-joliet", "-rock",
                        *seed_inputs], check=True)
        disk = work / "disk.qcow2"
        subprocess.run([str(qemu_img), "create", "-q", "-f", "qcow2", "-F", "qcow2", "-b", str(image), str(disk), "8G"], env=env, check=True)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        options = ["-i", str(key), "-o", "BatchMode=yes", "-o", "ConnectTimeout=3",
                   "-o", "StrictHostKeyChecking=accept-new", "-o", "UserKnownHostsFile=" + str(work / "known_hosts"), "-o", "LogLevel=ERROR"]
        ssh_base = ["ssh", *options, "-p", str(port), "ubuntu@127.0.0.1"]
        command = [str(qemu), "-no-user-config", "-nodefaults", "-machine", "q35,accel=kvm,dump-guest-core=off",
                   "-cpu", "host", "-m", str(args.memory_mib), "-smp", "2", "-display", "none", "-vga", "none", "-monitor", "none",
                   "-L", str(runtime / "usr/share/qemu"), "-bios", str(runtime / "usr/share/seabios/bios-256k.bin"),
                   "-drive", f"file={disk},if=virtio,format=qcow2", "-drive", f"file={iso},media=cdrom,readonly=on,format=raw",
                   "-netdev", "user,id=net0," + ("restrict=on," if args.offline_repository else "") + f"hostfwd=tcp:127.0.0.1:{port}-:22", "-device", "virtio-net-pci,netdev=net0,romfile=,mac=52:54:00:12:34:56",
                   "-device", "virtio-rng-pci", "-serial", "file:" + str(work / "serial.log"),
                   "-sandbox", "on,obsolete=deny,elevateprivileges=deny,spawn=deny,resourcecontrol=deny"]
        if args.graphical_authorization or args.real_game or args.source_switch:
            command += ["-device", "virtio-vga,xres=1280,yres=900,romfile=" + str(runtime / "usr/share/seabios/vgabios-virtio.bin"),
                        "-qmp", "unix:" + str(work / "qmp") + ",server=on,wait=off"]
        with (work / "qemu.log").open("w") as qemu_log, (EVIDENCE / "vm-systemd-run.log").open("w") as log:
            vm = subprocess.Popen(command, env=env, stdout=qemu_log, stderr=qemu_log)
            result["qemu_pid"] = vm.pid
            result["qemu_arguments"] = command[1:]

            def ssh(remote, timeout=120, check=True, input=None, capture=True):
                log.write("+ " + remote + "\n")
                log.flush()
                streams = {"capture_output": True} if capture else {"stdout": log, "stderr": subprocess.STDOUT}
                call = subprocess.run([*ssh_base, remote], input=input, text=True, timeout=timeout, **streams)
                if capture:
                    log.write(call.stdout + call.stderr)
                    log.flush()
                if check:
                    call.check_returncode()
                return call

            def ready(previous_boot=None):
                deadline = time.monotonic() + 180
                while time.monotonic() < deadline:
                    if vm.poll() is not None:
                        raise RuntimeError("QEMU exited before SSH readiness")
                    try:
                        call = subprocess.run([*ssh_base, "cat /proc/sys/kernel/random/boot_id"], capture_output=True, text=True, timeout=8)
                        if call.returncode == 0 and (previous_boot is None or call.stdout.strip() != previous_boot):
                            return call.stdout.strip()
                    except subprocess.TimeoutExpired:
                        pass
                    time.sleep(.5)
                raise RuntimeError("VM SSH readiness timed out")

            def copy(source, target):
                subprocess.run(["scp", "-q", *options, "-P", str(port), str(source), "ubuntu@127.0.0.1:" + target], check=True, timeout=60)

            def capture_guest(name):
                assert re.fullmatch(r"[a-z0-9-]{1,64}", name)
                folder = EVIDENCE / "graphical-captures"
                folder.mkdir(exist_ok=True)
                destination = (folder / (name + ".png")).resolve()
                with socket.socket(socket.AF_UNIX) as monitor:
                    monitor.settimeout(8)
                    monitor.connect(str(work / "qmp"))
                    stream = monitor.makefile('rwb')
                    assert 'QMP' in json.loads(stream.readline())
                    for serial, request in enumerate([{"execute": "qmp_capabilities"},
                            {"execute": "screendump", "arguments": {"filename": str(destination), "format": "png"}}]):
                        request['id'] = serial
                        stream.write(json.dumps(request).encode() + b'\n')
                        stream.flush()
                        while True:
                            response = json.loads(stream.readline())
                            if response.get('id') == serial:
                                assert 'return' in response, response
                                break
                result.setdefault('graphical_captures', []).append({'path': str(destination.relative_to(ROOT)),
                    'sha256': hashlib.file_digest(destination.open('rb'), 'sha256').hexdigest()})

            def graphical_tests(source="graphical_polkit_vm_inner.py", guard="OMEN_GRAPHICAL_VM", deadline_seconds=1100):
                import selectors
                copy(ROOT / "tests" / source, "/var/tmp/omen-graphical-tests.py")
                stage = subprocess.Popen([*ssh_base, "sudo -n env " + guard + "=1 python3 /var/tmp/omen-graphical-tests.py"],
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
                selector = selectors.DefaultSelector()
                selector.register(stage.stdout, selectors.EVENT_READ)
                deadline = time.monotonic() + deadline_seconds
                pending = b''
                try:
                    while True:
                        if time.monotonic() >= deadline:
                            raise RuntimeError('graphical test stage exceeded its deadline')
                        if not selector.select(1):
                            continue
                        chunk = os.read(stage.stdout.fileno(), 65536)
                        if not chunk:
                            if pending:
                                log.write(pending.decode(errors='replace'))
                            break
                        pending += chunk
                        while b'\n' in pending:
                            raw, pending = pending.split(b'\n', 1)
                            line = raw.decode(errors='replace')
                            if line.startswith('OMEN_VM_CAPTURE '):
                                capture_guest(line.strip().removeprefix('OMEN_VM_CAPTURE '))
                                stage.stdin.write(b'OK\n')
                                stage.stdin.flush()
                            else:
                                log.write(line + '\n')
                                log.flush()
                    stage.wait(timeout=10)
                    assert stage.returncode == 0, 'graphical test worker failed'
                finally:
                    selector.close()
                    if stage.poll() is None:
                        stage.terminate()
                        stage.wait(timeout=10)

            try:
                boot = ready()
                assert boot != Path("/proc/sys/kernel/random/boot_id").read_text().strip()
                cloud_init = ssh("sudo -n cloud-init status --wait --format json", timeout=180, check=False)
                (EVIDENCE / "vm-cloud-init-status.json").write_text(cloud_init.stdout)
                cloud_init.check_returncode()
                assert ssh("systemd-detect-virt --vm").stdout.strip() in ["kvm", "qemu"]
                copy(package, "/var/tmp/omen-current.deb")
                copy(ROOT / "tests/systemd_vm_inner.py", "/var/tmp/omen-vm-tests.py")
                copy(ROOT / "tests/coarse_recovery_vm_inner.py", "/var/tmp/omen-coarse-tests.py")
                copy(ROOT / "tests/nvidia_reader_vm_inner.py", "/var/tmp/omen-nvidia-tests.py")
                if args.managed_game_helper:
                    copy(args.managed_game_helper.resolve(), "/var/tmp/omen-managed-helper")
                    copy(ROOT / "tests/managed_games_vm_inner.py", "/var/tmp/omen-managed-tests.py")
                if args.apt_cache_archive:
                    copy(args.apt_cache_archive.resolve(), "/var/tmp/omen-apt-cache.tar")
                    ssh("sudo -n tar -xf /var/tmp/omen-apt-cache.tar -C /var/cache/apt/archives")
                setup = '''set -eu
export DEBIAN_FRONTEND=noninteractive
sed -i -e 's|^URIs:.*|URIs: APT_MIRROR|' -e 's/^Components:.*/Components: main/' /etc/apt/sources.list.d/ubuntu.sources
printf 'Acquire::https::Timeout "30";\nAcquire::Retries "2";\nAcquire::Languages "none";\n' > /etc/apt/apt.conf.d/99-omen-vm
apt-get -o APT::Update::Error-Mode=any update
apt-get install -y --no-install-recommends /var/tmp/omen-current.deb
'''.replace("APT_MIRROR", args.apt_mirror)
                if extra_packages:
                    setup = setup.replace('/var/tmp/omen-current.deb\n', '/var/tmp/omen-current.deb' + extra_packages + '\n')
                if args.offline_repository:
                    copy(args.offline_repository.resolve(), "/var/tmp/omen-apt-repository.tar")
                    setup = '''set -eu
export DEBIAN_FRONTEND=noninteractive
mkdir -p /var/tmp/omen-apt-repository
tar -xf /var/tmp/omen-apt-repository.tar -C /var/tmp/omen-apt-repository
for source in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do
    if [ -f "$source" ]; then mv "$source" "$source.omen-disabled"; fi
done
cat > /etc/apt/sources.list.d/omen-offline.sources <<'SOURCES'
Types: deb
URIs: file:/var/tmp/omen-apt-repository
Suites: resolute resolute-updates resolute-security
Components: main
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
Targets: Packages
SOURCES
printf 'Acquire::Languages "none";\nAcquire::By-Hash "false";\n' > /etc/apt/apt.conf.d/99-omen-offline
apt-get -o APT::Update::Error-Mode=any update
'''
                    setup = setup.replace('Components: main\n', 'Components: ' + ' '.join(components) + '\n')
                    if not args.resolve_only:
                        setup += 'apt-get install -y --no-install-recommends /var/tmp/omen-current.deb' + extra_packages + '\n'
                ssh("sudo -n sh -se", input=setup, timeout=600, capture=False)
                if args.resolve_only:
                    resolution = ssh("sudo -n apt-get --print-uris --no-install-recommends install /var/tmp/omen-current.deb" + extra_packages, timeout=60)
                    (EVIDENCE / "vm-dependency-resolution.txt").write_text(resolution.stdout)
                    result.update(result="resolved", application_tests_executed=False)
                    return
                result["application_tests_executed"] = True
                ssh("sudo -n python3 /var/tmp/omen-vm-tests.py initial", timeout=240, capture=False)
                ssh("sudo -n python3 /var/tmp/omen-coarse-tests.py", timeout=180, capture=False)
                ssh("sudo -n python3 /var/tmp/omen-nvidia-tests.py", timeout=180, capture=False)
                if args.managed_game_helper:
                    ssh("sudo -n install -D -m755 /var/tmp/omen-managed-helper /usr/local/libexec/omen-managed-fixture")
                    ssh("sudo -n python3 /var/tmp/omen-managed-tests.py", timeout=180, capture=False)
                if args.flatpak_scope_probe:
                    ssh("sudo -n install -D -m755 /var/tmp/omen-managed-helper /usr/local/libexec/omen-managed-fixture")
                    copy(ROOT / "tests/flatpak_scope_vm_inner.py", "/var/tmp/omen-flatpak-scope-probe.py")
                    ssh("sudo -n python3 /var/tmp/omen-flatpak-scope-probe.py --migration-phase " + args.flatpak_migration_phase, timeout=240, capture=False)
                if args.graphical_authorization:
                    graphical_tests()
                if args.source_switch:
                    graphical_tests("source_switch_vm_inner.py", "OMEN_SOURCE_SWITCH_VM", 420)
                if args.real_game:
                    ssh("sudo -n install -D -m755 /var/tmp/omen-managed-helper /usr/local/libexec/omen-managed-fixture")
                    graphical_tests("real_game_vm_inner.py", "OMEN_REAL_GAME_VM", 900)
                    artifacts = ssh("sudo -n cat /var/tmp/omen-real-game-artifacts.json").stdout
                    (EVIDENCE / "real-game-artifacts.json").write_text(artifacts)
                (EVIDENCE / "vm-coarse-journal.log").write_text(ssh("sudo -n journalctl --no-pager -u omen-fan-recovery@hwmon0.service -u omen-coarse-controller-fixture.service").stdout)
                (EVIDENCE / "vm-systemd-initial-journal.log").write_text(ssh("sudo -n journalctl --no-pager -u omen-linux.service").stdout)
                ssh("sudo -n systemctl reboot", check=False)
                boot = ready(boot)
                ssh("sudo -n python3 /var/tmp/omen-vm-tests.py after_reboot", timeout=120, capture=False)
                ssh("sudo -n systemctl reboot", check=False)
                ready(boot)
                ssh("sudo -n python3 /var/tmp/omen-vm-tests.py after_disabled_reboot", timeout=240, capture=False)
                result.update(json.loads(ssh("sudo -n cat /var/tmp/omen-vm-result.json").stdout))
                result["result"] = "passed"
            finally:
                keep = False
                if vm.poll() is None:
                    try:
                        partial = ssh("sudo -n cat /var/tmp/omen-vm-result.json", timeout=15, check=False)
                        if partial.returncode == 0 and result["result"] != "passed":
                            result["partial_result"] = json.loads(partial.stdout)
                        if args.graphical_authorization or args.real_game or args.source_switch:
                            desktop_journal = ssh("sudo -n journalctl --no-pager -u sddm.service -u user@1000.service", timeout=20, check=False)
                            (EVIDENCE / "vm-desktop-journal.log").write_text(desktop_journal.stdout + desktop_journal.stderr)
                            auth_journal = ssh("sudo -n journalctl --no-pager -u polkit.service", timeout=15, check=False)
                            (EVIDENCE / "vm-polkit-journal.log").write_text(auth_journal.stdout + auth_journal.stderr)
                        if args.real_game:
                            game_artifacts = ssh("sudo -n cat /var/tmp/omen-real-game-artifacts.json", timeout=15, check=False)
                            if game_artifacts.returncode == 0:
                                (EVIDENCE / "real-game-artifacts.json").write_text(game_artifacts.stdout)
                        coarse_journal = ssh("sudo -n journalctl --no-pager -u omen-fan-recovery@hwmon0.service -u omen-coarse-controller-fixture.service", timeout=15, check=False)
                        (EVIDENCE / "vm-coarse-journal.log").write_text(coarse_journal.stdout + coarse_journal.stderr)
                        journal = ssh("sudo -n journalctl --no-pager -u omen-linux.service", timeout=15, check=False)
                        (EVIDENCE / "vm-systemd-journal.log").write_text(journal.stdout + journal.stderr)
                        if not keep:
                            ssh("sudo -n systemctl poweroff", timeout=15, check=False)
                            vm.wait(timeout=30)
                    except (subprocess.SubprocessError, OSError):
                        if not keep:
                            vm.terminate()
                            try:
                                vm.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                vm.kill()
                                vm.wait()
                result["vm_exit_code"] = vm.returncode
                result["vm_exited"] = vm.poll() is not None
                for name in ["serial", "qemu"]:
                    path = work / (name + ".log")
                    if path.exists():
                        (EVIDENCE / ("vm-" + name + ".log")).write_bytes(path.read_bytes())
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
