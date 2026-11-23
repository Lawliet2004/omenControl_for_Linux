#!/usr/bin/env python3
"""Extract Ubuntu QEMU packages locally; never install packages on the host."""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import shlex
import subprocess
import urllib.parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, default=Path(__file__).resolve().parents[1] / "target/vm-runtime")
    args = parser.parse_args()
    root = args.destination.resolve()
    debs, prefix = root / "debs", root / "root"
    debs.mkdir(parents=True, exist_ok=True)
    simulation = subprocess.check_output(["apt-get", "--simulate", "--no-install-recommends", "install", "qemu-system-x86", "qemu-utils"], text=True, stderr=subprocess.STDOUT)
    (root / "resolution.log").write_text(simulation)
    packages = [(match[1], match[2]) for line in simulation.splitlines()
                if (match := re.match(r"Inst ([^ ]+) \(([^ ]+)", line))]
    if not packages:
        raise RuntimeError("No packages resolved; use the installed QEMU tools instead")

    def fetch(item):
        package, version = item
        metadata = subprocess.check_output(["apt-get", "--print-uris", "download", package + "=" + version], text=True)
        line = next(line for line in metadata.splitlines() if line.startswith("'"))
        url, filename, size, digest = shlex.split(line)
        parsed = urllib.parse.urlsplit(url)
        assert parsed.hostname.endswith(".ubuntu.com"), "expected official Ubuntu archive"
        url = urllib.parse.urlunsplit(parsed._replace(scheme="https"))
        algorithm, expected = digest.split(":", 1)
        assert algorithm in ["SHA256", "SHA512"]
        dest = debs / filename
        def valid():
            return dest.exists() and dest.stat().st_size == int(size) and hashlib.file_digest(dest.open("rb"), algorithm.lower()).hexdigest() == expected
        if not valid():
            subprocess.run(["curl", "--fail", "--location", "--retry", "2", "--retry-all-errors", "--connect-timeout", "20", "--max-time", "240",
                            "--silent", "--show-error", "--output", str(dest), url], check=True)
        assert valid(), f"archive checksum mismatch for {package}"
        return {"package": package, "version": version, "url": url, "apt_digest": digest,
                "sha256": hashlib.file_digest(dest.open("rb"), "sha256").hexdigest(), "filename": filename}

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(fetch, packages))
    for result in results:
        subprocess.run(["dpkg-deb", "--extract", str(debs / result["filename"]), str(prefix)], check=True)
    (root / "packages.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"Extracted {len(results)} distribution packages into {prefix}; no host packages installed")


if __name__ == "__main__":
    main()
