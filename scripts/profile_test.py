#!/usr/bin/env python3
"""Explicit, supervised Balanced→original profile test through production IPC."""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time


def call(path, name, args=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(65)
        stream.connect(path)
        op = {"op": name}
        if args is not None:
            op["args"] = args
        stream.sendall(json.dumps({"v": 1, "id": 1, "op": op}).encode() + b"\n")
        response = json.loads(stream.makefile("rb").readline(1_048_577))
        if not response.get("ok"):
            raise RuntimeError(f"{name}: {response.get('error')}")
        return response["data"]


def independent_profile():
    output = subprocess.check_output([
        "/usr/bin/busctl", "--system", "--timeout=2", "--json=short", "get-property",
        "org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles", "ActiveProfile",
    ], timeout=4)
    return {"power-saver": "quiet", "balanced": "balanced", "performance": "performance"}[json.loads(output)["data"]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--approve-balanced-restore", action="store_true", required=True,
                        help="Explicitly authorize this invocation's two profile operations")
    parser.add_argument("--fixture-test", action="store_true", help="Require mock daemon; no host PPD access")
    args = parser.parse_args()
    result = {"fixture_only": args.fixture_test, "started_unix": time.time(), "events": []}
    info = call(args.socket, "get_info")
    if bool(info["mock"]) != args.fixture_test:
        raise RuntimeError("mock/live identity mismatch; refusing the test")
    machine = info["machine"]
    if not args.fixture_test and (machine["board_name"], machine["bios_version"], machine["kernel_version"]) != ("8C30", "F.10", "7.0.0-38-generic"):
        raise RuntimeError("identity differs from the approved plan")
    caps = call(args.socket, "get_capabilities")
    if caps["performance_profiles"]["status"] != "available":
        raise RuntimeError("daemon has not explicitly enabled profile control")
    original = call(args.socket, "get_profile")
    if not args.fixture_test:
        if not call(args.socket, "get_ac")["online"]:
            raise RuntimeError("AC must be connected for this first test")
        if independent_profile() != original:
            raise RuntimeError("independent state differs before the test")
    result.update(machine=machine, original=original)
    result["daemon_mock"] = info["mock"]
    result["capability"] = caps["performance_profiles"]
    result["client_uid"] = os.getuid()
    if not args.fixture_test:
        result["independent_original"] = independent_profile()
        result["ac"] = call(args.socket, "get_ac")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Record the previous state before any write, for manual recovery even if
    # the supervisor itself is killed. SIGKILL recovery is not guaranteed.
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    attempted = False
    def interrupted(_signal, _frame):
        raise KeyboardInterrupt("test interrupted; attempting profile restoration")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        attempted = True
        call(args.socket, "set_profile", {"profile": "balanced"})
        observed = call(args.socket, "get_profile")
        independent = observed if args.fixture_test else independent_profile()
        result["balanced_readback"] = {"daemon": observed, "independent": independent,
                                        "unix": time.time()}
        if observed != "balanced" or independent != "balanced":
            raise RuntimeError("Balanced did not read back consistently")
        result["events"].append("Balanced verified")
    finally:
        try:
            if attempted:
                call(args.socket, "set_profile", {"profile": original})
                observed = call(args.socket, "get_profile")
                independent = observed if args.fixture_test else independent_profile()
                result["restored_readback"] = {"daemon": observed, "independent": independent,
                                               "unix": time.time()}
                restored = observed == original and independent == original
                result["restored"] = restored
                if not restored:
                    raise RuntimeError("original profile restoration could not be verified")
        except BaseException as error:
            result["recovery_error"] = str(error)
            raise
        finally:
            result["finished_unix"] = time.time()
            args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
