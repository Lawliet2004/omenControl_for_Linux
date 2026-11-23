#!/usr/bin/env python3
"""Capture only OMEN GTK trees, using private fixture hardware/configuration."""
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BIN = ROOT / "target" / PROFILE
OUT = ROOT / "docs/screenshots/audit-v15"


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    records = []
    with tempfile.TemporaryDirectory(prefix="omen-audit-captures-") as directory:
        temp = Path(directory)
        config = temp / "user"
        config.mkdir()
        (config / "config.toml").write_text(
            "version = 1\n[general]\npoll_interval_ms = 500\nonboarding_dismissed = true\n")
        env = dict(os.environ, OMEN_CONFIG_DIR=str(config),
                   OMEN_SYSTEM_CONFIG_DIR=str(temp / "system"), OMEN_HOME=str(temp / "home"),
                   GDK_BACKEND="x11", GSETTINGS_BACKEND="memory", GTK_A11Y="none")
        with (temp / "daemon.log").open("w") as log:
            daemon = subprocess.Popen([str(BIN / "omen-daemon"), "--mock-omen"],
                                      env=env, stdout=subprocess.PIPE, stderr=log, text=True)
            try:
                endpoint = daemon.stdout.readline().strip()
                assert Path(endpoint).is_socket(), "fixture endpoint missing"
                env["OMEN_SOCK"] = endpoint
                for name, page, width, height, theme, scale, contrast in [
                    ("dashboard-light", "dashboard", 1240, 800, "prefer-light", "1", "0"),
                    ("dashboard-dark-narrow", "dashboard", 600, 900, "prefer-dark", "1", "0"),
                    ("vitals-large-text", "vitals", 900, 1000, "prefer-light", "1.5", "0"),
                    ("settings-high-contrast", "settings", 1240, 900, "prefer-light", "1", "1"),
                    ("fan-narrow", "fan", 600, 900, "prefer-light", "1", "0"),
                    ("battery-narrow", "battery", 600, 900, "prefer-dark", "1", "0"),
                ]:
                    path = OUT / (name + ".png")
                    capture_env = dict(env, OMEN_CAPTURE_PATH=str(path),
                        OMEN_CAPTURE_WIDTH=str(width), OMEN_CAPTURE_HEIGHT=str(height),
                        ADW_DEBUG_COLOR_SCHEME=theme, ADW_DEBUG_HIGH_CONTRAST=contrast,
                        GDK_DPI_SCALE=scale)
                    subprocess.run([str(BIN / "omen-gui"), "--page", page], env=capture_env,
                                   check=True, timeout=30, stdout=subprocess.DEVNULL)
                    data = path.read_bytes()
                    actual = struct.unpack(">II", data[16:24])
                    records.append({"path": str(path.relative_to(ROOT)), "page": page,
                        "requested_size": [width, height], "actual_size": actual,
                        "color_scheme": theme, "requested_text_scale": scale,
                        "requested_high_contrast": contrast == "1",
                        "sha256": hashlib.sha256(data).hexdigest()})
                    print(name, actual, flush=True)
            finally:
                daemon.terminate()
                daemon.wait(timeout=8)
    result = {"fixture_only": True, "hardware_writes": False,
              "capture": "application-owned GTK widget trees; not the desktop framebuffer",
              "backend": "x11", "profile": PROFILE,
              "binary_sha256": hashlib.sha256((BIN / "omen-gui").read_bytes()).hexdigest(),
              "captures": records}
    evidence = ROOT / "target/audit-2026-10-27"
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "visual-captures.json").write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
