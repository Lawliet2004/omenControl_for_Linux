#!/usr/bin/env python3
"""Read-only measurements of release executables; no profile/control writes."""
import json
import datetime
import hashlib
import os
from pathlib import Path
import selectors
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/release"
HZ = os.sysconf("SC_CLK_TCK")


def resources(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    ticks = sum(int(stat[i]) for i in [11, 12])
    child_ticks = sum(int(stat[i]) for i in [13, 14])
    rss = next(l for l in Path(f"/proc/{pid}/status").read_text().splitlines() if l.startswith("VmRSS:"))
    return ticks, child_ticks, int(rss.split()[1])


def sample(pid, duration=5):
    start = time.monotonic()
    before = resources(pid)
    time.sleep(duration)
    after = resources(pid)
    elapsed = time.monotonic() - start
    return {"seconds": elapsed, "rss_kib": after[2],
            "cpu_pct_one_core": 100 * (after[0] - before[0]) / HZ / elapsed,
            "reaped_children_cpu_pct_one_core": 100 * (after[1] - before[1]) / HZ / elapsed}


def run():
    result = {"method": "release binaries; 5-second windows; CPU percent of one logical core; page-cache-warm launches; live read-only monitoring", "cpu_count": os.cpu_count()}
    with tempfile.TemporaryDirectory(prefix="omen-benchmark-") as temp:
        env = dict(os.environ, OMEN_CONFIG_DIR=temp, OMEN_SYSTEM_CONFIG_DIR=temp, OMEN_SOCK=f"{temp}/control.sock")
        durations = []
        for _ in range(5):
            start = time.monotonic()
            subprocess.run([str(BIN / "omen-cli"), "detect"], env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            durations.append(time.monotonic() - start)
        result["cli_detect"] = {"runs_seconds": durations, "median_seconds": statistics.median(durations)}
        start = time.monotonic()
        daemon = subprocess.Popen([str(BIN / "omen-daemon")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            while not Path(env["OMEN_SOCK"]).exists():
                if daemon.poll() is not None or time.monotonic() - start > 5:
                    raise RuntimeError("daemon did not become ready")
                time.sleep(0.01)
            result["daemon_socket_ready_seconds"] = time.monotonic() - start
            result["daemon_idle_no_clients"] = sample(daemon.pid)
        finally:
            daemon.terminate()
            daemon.wait(timeout=5)
        env["OMEN_REPORT_READY"] = "1"
        for overlay in [False, True]:
            (Path(temp)/"config.toml").write_text(f"version = 1\n[general]\noverlay_enabled = {str(overlay).lower()}\n")
            measurement = {"overlay_enabled": overlay, "main_window_visible": True}
            start = time.monotonic()
            gui = subprocess.Popen([str(BIN / "omen-gui"), "--new-instance"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                selector = selectors.DefaultSelector()
                selector.register(gui.stderr, selectors.EVENT_READ)
                startup_output = bytearray()
                while time.monotonic() - start < 10:
                    if selector.select(timeout=0.2):
                        data = os.read(gui.stderr.fileno(), 8192)
                        if not data:
                            raise RuntimeError("GUI exited before mapping")
                        startup_output.extend(data)
                        if b"OMEN_UI_MAPPED" in startup_output:
                            measurement["window_map_seconds"] = time.monotonic() - start
                            measurement["gtk_display_type"] = next((line.split(" ",1)[1] for line in startup_output.decode(errors="replace").splitlines()
                                                               if line.startswith("OMEN_UI_BACKEND ")), "not reported")
                            break
                        startup_output = startup_output[-16384:]
                else:
                    raise RuntimeError("GUI did not map a window")
                time.sleep(2)
                measurement["resources"] = sample(gui.pid)
            finally:
                gui.terminate()
                gui.wait(timeout=5)
            result["gui_with_overlay" if overlay else "gui_without_overlay"] = measurement
        result["gui_window_map_seconds"] = result["gui_without_overlay"]["window_map_seconds"]
        result["gtk_display_type"] = result["gui_without_overlay"]["gtk_display_type"]
        result["gui_monitoring_dashboard"] = result["gui_without_overlay"]["resources"]
    result["binary_sha256"] = {name: hashlib.sha256((BIN / name).read_bytes()).hexdigest()
                               for name in ["omen-cli", "omen-daemon", "omen-gui"]}
    result["cleanup_complete"] = True
    result["kernel"] = os.uname().release
    result["recorded_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (ROOT / "docs/evidence/performance.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    run()
