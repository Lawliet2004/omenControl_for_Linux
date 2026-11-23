#!/usr/bin/env python3
"""Verify the package without installing it or starting its systemd service."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))


def run(*args, **kwargs):
    return subprocess.run(list(args), check=True, capture_output=True, text=True, **kwargs)


def archive_entries(package, control=False):
    flag = "--ctrl-tarfile" if control else "--fsys-tarfile"
    archive = subprocess.check_output(["dpkg-deb", flag, str(package)])
    result = {}
    with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
        for member in contents.getmembers():
            name = member.name.removeprefix("./")
            digest = hashlib.sha256(contents.extractfile(member).read()).hexdigest() if member.isfile() else None
            result[name] = (member.type, member.mode, member.uid, member.gid, member.linkname, digest)
    return result


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--lifecycle-reference", type=Path, help="Exact successfully lifecycle-tested .deb; permit final documentation-only changes")
    parser.add_argument("--vm-reference", type=Path, help="Exact successfully systemd-VM-tested .deb; permit documentation-only changes")
    args = parser.parse_args()
    package = args.package.resolve()
    result = {"package": str(package.relative_to(ROOT)), "bytes": package.stat().st_size,
              "sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
              "version": run("dpkg-deb", "-f", str(package), "Version").stdout.strip(),
              "installed_by_verifier": False, "checks": {}, "binaries": {}}
    for label, selected, evidence_file in [
        ("lifecycle", args.lifecycle_reference, "package-lifecycle.json"),
        ("vm", args.vm_reference, "vm-systemd.json"),
    ]:
        if selected is None:
            continue
        reference = selected.resolve()
        tested = json.loads((EVIDENCE / evidence_file).read_text())
        assert tested["result"] == "passed"
        if label == "lifecycle":
            assert tested["container_removed"]
        else:
            assert tested["vm_exited"] and tested["temporary_files_removed"]
        assert hashlib.sha256(reference.read_bytes()).hexdigest() == tested["package_sha256"]
        before, after = archive_entries(reference), archive_entries(package)
        changed = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
        assert all(name.startswith("usr/share/doc/omen-linux/") for name in changed), changed
        before_control, after_control = archive_entries(reference, True), archive_entries(package, True)
        before_control.pop("md5sums", None)
        after_control.pop("md5sums", None)
        assert before_control == after_control, "maintainer scripts or control metadata changed"
        result[label + "_equivalence"] = {"reference_package": str(reference.relative_to(ROOT)),
            "reference_sha256": tested["package_sha256"], "runtime_payload_identical": True,
            "maintainer_scripts_and_control_identical": True, "documentation_changes": changed}
    archive = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", str(package)])
    with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
        assert all(p.uid == p.gid == 0 for p in contents.getmembers())
        assert all(p.mode & 0o777 == 0o755 for p in contents.getmembers() if p.isdir())
    result["checks"]["root_ownership_and_directory_modes"] = True
    with tempfile.TemporaryDirectory(prefix="omen-package-verify-") as temporary:
        stage = Path(temporary)
        run("dpkg-deb", "-R", str(package), str(stage))
        checksums = run("md5sum", "-c", "DEBIAN/md5sums", cwd=stage)
        (EVIDENCE / "package-checksums.log").write_text(checksums.stdout)
        result["checks"]["payload_md5sums"] = True
        for name in ["omen-cli", "omen-daemon", "omen-gui"]:
            path = stage / "usr/bin" / name
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            assert os.access(path, os.X_OK)
            assert digest == hashlib.sha256((ROOT / "target/release" / name).read_bytes()).hexdigest()
            result["binaries"][name] = {"sha256": digest, "matches_release": True}
        wrapper = stage / "usr/libexec/omen-linux/omen-flatpak-game"
        manifest = json.loads((stage / "usr/share/doc/omen-linux/flatpak/build-manifest.json").read_text())
        assert hashlib.sha256(wrapper.read_bytes()).hexdigest() == manifest["binary_sha256"]
        assert hashlib.sha256((ROOT / "packaging/omen-flatpak-game.c").read_bytes()).hexdigest() == manifest["source_sha256"]
        headers = run("readelf", "-lW", str(wrapper)).stdout
        dynamic = run("readelf", "-dW", str(wrapper)).stdout
        assert "INTERP" not in headers and "(NEEDED)" not in dynamic
        assert any("GNU_STACK" in line and " RW " in line and "RWE" not in line for line in headers.splitlines())
        flatpak_docs = stage / "usr/share/doc/omen-linux/flatpak"
        assert (flatpak_docs / "omen-flatpak-game.c").read_bytes() == (ROOT / "packaging/omen-flatpak-game.c").read_bytes()
        assert "MIT" in (flatpak_docs / "musl-copyright").read_text()
        assert (stage / "usr/share/doc/omen-linux/FLATPAK_SETUP.md").is_file()
        result["checks"]["static_flatpak_helper_source_manifest_and_musl_notice"] = True
        desktop = run("desktop-file-validate", str(stage / "usr/share/applications/com.omenlinux.Gui.desktop"))
        (EVIDENCE / "desktop-file-validation.log").write_text(desktop.stdout + desktop.stderr)
        result["checks"]["desktop_file_validate"] = True
        policy = stage / "usr/share/polkit-1/actions/com.omenlinux.policy"
        ET.parse(policy)
        assert policy.read_bytes() == (ROOT / "packaging/polkit/com.omenlinux.policy").read_bytes()
        result["checks"]["polkit_xml_matches_source"] = True
        assets = stage / "usr/share/doc/omen-linux/icon-fallback"
        provenance = json.loads((assets / "PROVENANCE.json").read_text())
        for name in ["ADWAITA_NOTICE", "COPYING.LGPL-3", "PROVENANCE.json", "omen.gresource.xml"]:
            assert (assets / name).read_bytes() == (ROOT / "gui/assets" / name).read_bytes(), name
        for icon in provenance["icons"]:
            relative = Path(icon["resource_path"])
            assert not relative.is_absolute() and ".." not in relative.parts
            data = (assets / relative).read_bytes()
            assert hashlib.sha256(data).hexdigest() == icon["sha256"]
            assert data == (ROOT / "gui/assets" / relative).read_bytes()
        result["checks"]["bundled_icon_sources_and_notices"] = len(provenance["icons"])
        notices = gzip.decompress((stage / "usr/share/doc/omen-linux/DEPENDENCY_NOTICES.md.gz").read_bytes()).decode()
        result["dependency_notices"] = sum(line.startswith("## ") for line in notices.splitlines())
        assert result["dependency_notices"] > 0
        linkage = run("ldd", str(stage / "usr/bin/omen-gui"))
        assert "not found" not in linkage.stdout
        (EVIDENCE / "package-runtime-linkage.log").write_text(linkage.stdout)
        result["checks"]["gui_linkage_current_environment"] = True
        diagnostic = json.loads(run(str(stage / "usr/bin/omen-cli"), "diagnostics").stdout)
        assert diagnostic["mock"] is False
        (EVIDENCE / "package-cli-diagnostics.json").write_text(json.dumps(diagnostic, indent=2) + "\n")
        result["checks"]["extracted_cli_readonly_discovery"] = True
        verified = run("systemd-analyze", "--root=" + str(stage), "--recursive-errors=no", "--man=no",
                       "--generators=no", "verify", "/usr/lib/systemd/system/omen-linux.service")
        (EVIDENCE / "systemd-payload-validation.log").write_text(verified.stdout + verified.stderr)
        result["systemd_validation"] = {"exit_code": verified.returncode,
                                        "method": "unmodified unit in extracted package root; no unit started"}
    (EVIDENCE / "package-validation.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
