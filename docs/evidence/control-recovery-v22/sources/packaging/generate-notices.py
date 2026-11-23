#!/usr/bin/env python3
"""Retain resolved Rust dependency notices in the distributable package."""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", host,
    ], cwd=root))
    sections = ["# Rust dependency notices\n\n"
                "Generated from Cargo.lock and the resolved crate distributions. Includes\n"
                "build/test dependencies; inclusion does not imply runtime use. Application\n"
                "source is GPL-3.0-or-later; dependencies retain their own terms.\n"]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        files = {p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(
            ("LICENSE", "LICENCE", "COPYING", "UNLICENSE", "NOTICE", "COPYRIGHT"))}
        if package.get("license_file"):
            files.add(directory / package["license_file"])
        if not files:
            raise RuntimeError(f"Missing license files: {package['name']} {package['version']}")
        sections.append(f"\n## {package['name']} {package['version']}\n\n"
                        f"License expression: {package.get('license') or 'see license file'}\n\n"
                        f"Upstream: {package.get('repository') or package['source']}\n")
        for path in sorted(files):
            sections.append(f"\n### {path.name}\n\n````text\n{path.read_text()}\n````\n")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("".join(sections))


if __name__ == "__main__":
    main()
