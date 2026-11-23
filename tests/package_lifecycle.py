#!/usr/bin/env python3
"""Install/upgrade/remove in a disposable Ubuntu Docker container without --privileged."""
import argparse
import datetime
import hashlib
import io
import json
import os
import re
from pathlib import Path
import subprocess
import tarfile
import uuid

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))
DOCKER = ["docker", "--host", "unix:///var/run/docker.sock"]


def command(*args, **kwargs):
    kwargs.setdefault("timeout", 900)
    return subprocess.run([*DOCKER, *args], check=True, **kwargs)


def output(*args):
    return command(*args, capture_output=True, text=True).stdout.strip()


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--baseline", required=True, type=Path, help="Older pre-helper .deb for the real upgrade test")
    parser.add_argument("--image", default="ubuntu:26.04", help="Existing local base image; never pulled automatically")
    parser.add_argument("--ca-bundle", type=Path, default=Path("/etc/ssl/certs/ca-certificates.crt"), help="Public CA bundle for HTTPS apt in the minimal base image")
    parser.add_argument("--keep-failed", action="store_true", help="Debug only: retain a failed container; its ID is recorded for explicit cleanup")
    parser.add_argument("--apt-mirror", default="https://archive.ubuntu.com/ubuntu", help="HTTPS Ubuntu archive mirror for the container only")
    parser.add_argument("--apt-cache-archive", type=Path, help="Optional cached .deb files; APT still verifies signed-index hashes")
    parser.add_argument("--resolve-only", action="store_true", help="Resolve exact container dependencies from a signed offline repository without installing")
    parser.add_argument("--offline-repository", type=Path, help="Signed Ubuntu repository archive; container networking is disabled and APT verifies its indexes")
    parser.add_argument("--offline-dependencies", action="store_true", help="Use an explicitly selected image with existing distro dependencies; purge its old OMEN package first and forbid downloads")
    parser.add_argument("--game-format-fixtures", action="store_true", help="Run the installed CLI/daemon launcher-format fixtures as the desktop UID")
    args = parser.parse_args()
    assert not args.resolve_only or args.offline_repository
    if args.offline_repository:
        assert not args.offline_dependencies
        with tarfile.open(args.offline_repository) as archive:
            members=archive.getmembers()
            assert len(members)<=2048 and sum(m.size for m in members)<=512*1024*1024
            assert all(not Path(m.name).is_absolute() and '..' not in Path(m.name).parts
                and (m.isdir() or m.isfile()) and Path(m.name).parts[0] in ['dists','pool'] for m in members)
    if args.apt_cache_archive:
        with tarfile.open(args.apt_cache_archive) as cache:
            assert all(m.isfile() and '/' not in m.name and m.name.endswith('.deb') for m in cache.getmembers())
    assert re.fullmatch(r"https://[A-Za-z0-9.-]+/[A-Za-z0-9/._-]+", args.apt_mirror)
    package, baseline = args.package.resolve(), args.baseline.resolve()
    for path in [package, baseline]:
        assert path.is_file(), path
    control = subprocess.check_output(["dpkg-deb", "--ctrl-tarfile", str(baseline)])
    with tarfile.open(fileobj=io.BytesIO(control)) as archive:
        postinst = next(m for m in archive.getmembers() if m.name.removeprefix("./") == "postinst")
        assert b"deb-systemd-helper" not in archive.extractfile(postinst).read(), "baseline must predate Debian helper-state migration (use revision 1)"
    image = json.loads(output("image", "inspect", args.image))[0]
    name = "omen-lifecycle-" + uuid.uuid4().hex[:10]
    container = output("run", "--detach", "--init", "--pull=never", "--name", name,
                       "--label", "com.omenlinux.test=package-lifecycle",
                       "--cap-drop=SYS_ADMIN", "--security-opt=no-new-privileges",
                       "--pids-limit=256", "--memory=1536m", "--cpus=2", *(["--network=none"] if args.offline_repository else []), image["Id"], "sleep", "infinity")
    result = {"started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "base_image": image["Id"], "base_digests": image["RepoDigests"],
              "package_sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
              "baseline_sha256": hashlib.sha256(baseline.read_bytes()).hexdigest(),
              "host_package_installed": False,
              "dependency_resolution_only": args.resolve_only, "container_removed": False,
              "apt_mirror": args.apt_mirror,
              "offline_dependency_reuse": args.offline_dependencies,
              "offline_signed_repository_sha256": hashlib.file_digest(args.offline_repository.open("rb"), "sha256").hexdigest() if args.offline_repository else None,
              "installed_game_format_fixtures": args.game_format_fixtures,
              "apt_cache_sha256": hashlib.file_digest(args.apt_cache_archive.open("rb"), "sha256").hexdigest() if args.apt_cache_archive else None,
              "isolation": "default Docker namespaces/devices; SYS_ADMIN absent; no-new-privileges; no explicit host mounts, bus, sockets or device passthrough",
              "result": "failed"}
    try:
        state = json.loads(output("inspect", container))[0]
        assert state["Config"]["Labels"]["com.omenlinux.test"] == "package-lifecycle"
        assert not state["HostConfig"]["Privileged"] and not state["Mounts"]
        assert not state["HostConfig"]["Devices"] and not state["HostConfig"]["PidMode"]
        if args.offline_dependencies:
            cleanup = '''set -eu
if dpkg-query -W -f='${db:Status-Status}' omen-linux 2>/dev/null | grep -qx installed; then dpkg --purge omen-linux; fi
rm -rf /test /run/omen-linux /var/lib/omen-linux
rm -f /etc/polkit-1/rules.d/00-omen-lifecycle.rules /run/dbus/system_bus_socket /run/dbus/pid
test ! -d /run/systemd/system
'''
            with (EVIDENCE / "dependency-reuse-cleanup.log").open("w") as log:
                command("exec", "-i", container, "sh", input=cleanup, text=True, stdout=log, stderr=subprocess.STDOUT)
        command("exec", container, "mkdir", "-p", "/test")
        command("cp", str(package), container + ":/test/current.deb")
        command("cp", str(baseline), container + ":/test/baseline.deb")
        command("cp", str(ROOT / "tests/package_lifecycle_inner.py"), container + ":/test/lifecycle.py")
        if args.game_format_fixtures:
            for filename in ["game_formats.py", "game_format_fixtures.py"]:
                command("cp", str(ROOT / "tests" / filename), container + ":/test/" + filename)
        command("cp", str(args.ca_bundle.resolve()), container + ":/test/ca-certificates.crt")
        if args.apt_cache_archive:
            command("cp", str(args.apt_cache_archive.resolve()), container + ":/test/apt-cache.tar")
            command("exec", container, "tar", "-xf", "/test/apt-cache.tar", "-C", "/var/cache/apt/archives")
        setup = '''set -eu
export DEBIAN_FRONTEND=noninteractive
sed -i -e 's|^URIs:.*|URIs: APT_MIRROR|' -e 's/^Components:.*/Components: main/' /etc/apt/sources.list.d/ubuntu.sources
printf 'Acquire::https::CaInfo "/test/ca-certificates.crt";\nAcquire::https::Timeout "30";\nAcquire::Retries "2";\nAcquire::Languages "none";\n' > /etc/apt/apt.conf.d/99-omen-lifecycle
apt-get -o APT::Update::Error-Mode=any update
apt-get install -y --no-install-recommends /test/current.deb python3 dbus dbus-bin procps desktop-file-utils
'''.replace("APT_MIRROR", args.apt_mirror)
        if args.offline_dependencies:
            setup = '''set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get --no-download install -y --no-install-recommends python3 dbus dbus-bin procps desktop-file-utils
dpkg --install /test/current.deb
apt-get --no-download check
'''
        if args.offline_repository:
            command("cp", str(args.offline_repository.resolve()), container + ":/test/repository.tar")
            setup = """set -eu
export DEBIAN_FRONTEND=noninteractive
mkdir /test/repository
tar -xf /test/repository.tar -C /test/repository
for source in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do
 if [ -f "$source" ]; then mv "$source" "$source.omen-disabled"; fi
done
for suite in resolute resolute-updates resolute-security; do
 echo "deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] file:/test/repository $suite main universe" >> /etc/apt/sources.list.d/omen-offline.list
done
printf 'Acquire::Languages "none";\\n' > /etc/apt/apt.conf.d/99-omen-offline
apt-get -o APT::Update::Error-Mode=any update
apt-get install -y --no-install-recommends /test/current.deb python3 dbus dbus-bin procps desktop-file-utils
"""
        if args.resolve_only:
            setup=setup.replace("apt-get install -y", "apt-get --print-uris --download-only install -y")
        with (EVIDENCE / "package-lifecycle-setup.log").open("w") as log:
            command("exec", "-i", container, "sh", input=setup, text=True, stdout=log, stderr=subprocess.STDOUT)
        if args.resolve_only:
            result.update(result="resolved",application_tests_executed=False)
            return
        with (EVIDENCE / "package-lifecycle-run.log").open("w") as log:
            command("exec", "--env", "OMEN_LIFECYCLE_CONTAINER=1", "--env", "OMEN_GAME_FORMAT_FIXTURES=" + ("1" if args.game_format_fixtures else "0"), container,
                    "python3", "/test/lifecycle.py", text=True, stdout=log, stderr=subprocess.STDOUT)
        result.update(json.loads(output("exec", container, "cat", "/test/result.json")))
        if args.game_format_fixtures:
            formats = json.loads(output("exec", container, "cat", "/test/work/formats/game-formats.json"))
            assert formats["result"] == "passed" and formats["cleanup_complete"]
            assert len(formats["checks_passed"]) == 15 and not formats["upstream_legendary_tested"]
            (EVIDENCE / "installed-game-formats.json").write_text(json.dumps(formats, indent=2) + "\n")
        result["distro_packages"] = output("exec", container, "dpkg-query", "-W", "systemd", "polkitd", "libgtk-4-1", "libadwaita-1-0", "libsqlite3-0")
        result["result"] = "passed"
    finally:
        for name in ["daemon", "dbus", "polkit"]:
            read = subprocess.run([*DOCKER, "exec", container, "cat", f"/test/{name}.log"], capture_output=True, text=True)
            if read.returncode == 0:
                (EVIDENCE / f"package-lifecycle-{name}.log").write_text(read.stdout)
        read = subprocess.run([*DOCKER, "exec", container, "cat", "/test/installed-game-formats.log"], capture_output=True, text=True)
        if read.returncode == 0:
            (EVIDENCE / "installed-game-formats.log").write_text(read.stdout)
        if result["result"] == "passed" or not args.keep_failed:
            command("rm", "--force", container, capture_output=True)
            result["container_removed"] = True
        else:
            result["retained_container_id"] = container
        result["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (EVIDENCE / "package-lifecycle.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
