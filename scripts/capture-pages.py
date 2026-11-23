#!/usr/bin/env python3
"""Capture the application's own GTK tree; never capture desktop content."""
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
binary = root / "target" / os.environ.get("OMEN_TEST_PROFILE", "debug") / "omen-gui"
for page in ["dashboard", "vitals", "performance", "fan", "lighting", "graphics", "games", "battery", "settings"]:
    output = root / "docs/screenshots" / f"{page}.png"
    env = dict(os.environ, OMEN_CAPTURE_PATH=str(output))
    subprocess.run([str(binary), "--page", page], env=env, check=True, timeout=25)
    print(output)
