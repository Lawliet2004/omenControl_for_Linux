#!/usr/bin/env python3
"""Read-only preflight by default; explicitly approved five-second Auto/Max trial."""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import time

TARGET = '8C30:F.10:7.0.0-38-generic'


def call(endpoint, operation, args=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(65)
        stream.connect(endpoint)
        op = {'op': operation}
        if args is not None:
            op['args'] = args
        stream.sendall(json.dumps({'v': 1, 'id': 7, 'op': op}).encode() + b'\n')
        value = json.loads(stream.makefile('rb').readline(65537))
        if not value.get('ok'):
            raise RuntimeError(str(value.get('error')))
        return value['data']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--socket', default='/run/omen-linux/ctl.sock')
    parser.add_argument('--instance', default='hwmon7')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--approve-exact-target', help='Authorize only BOARD:BIOS:KERNEL:hwmonN for this invocation')
    parser.add_argument('--fixture-root', type=Path, help='Require mock identity and an ordinary private fixture root')
    args = parser.parse_args()
    assert args.instance.startswith('hwmon') and args.instance[5:].isdigit()
    root = args.fixture_root.resolve() if args.fixture_root else Path('/')
    if args.fixture_root:
        assert root != Path('/') and not str(root).startswith(('/sys/', '/proc/', '/dev/'))
        hp = root / 'sys/class/hwmon' / args.instance
    else:
        hp = Path('/sys/devices/platform/hp-wmi/hwmon') / args.instance
    report = {'fixture_only': bool(args.fixture_root), 'physical_writes_requested': False,
              'result': 'preflight_failed', 'observations': [], 'automatic_restored': None}
    attempted = False
    def observation():
        return {'unix_ms': round(time.time() * 1000),
                'mode': (hp / 'pwm1_enable').read_text().strip(),
                'cpu_rpm': int((hp / 'fan1_input').read_text()),
                'gpu_rpm': int((hp / 'fan2_input').read_text())}
    def interrupt(_sig, _frame):
        raise RuntimeError('trial interrupted; restoring Automatic')
    signal.signal(signal.SIGINT, interrupt)
    signal.signal(signal.SIGTERM, interrupt)
    try:
        dmi = root / 'sys/class/dmi/id'
        actual = ':'.join([(dmi / 'board_name').read_text().strip(), (dmi / 'bios_version').read_text().strip(),
                           (root / 'proc/sys/kernel/osrelease').read_text().strip()])
        exact = actual + ':' + args.instance
        assert actual == TARGET and hp.resolve() == hp, 'unverified target path or machine'
        assert (hp / 'name').read_text().strip() == 'hp'
        assert not (hp / 'pwm1').exists() and not (hp / 'pwm2').exists()
        report['exact_target'] = exact
        report['observations'].append(observation())
        info = call(args.socket, 'get_info')
        assert info['mock'] is bool(args.fixture_root), 'fixture/live daemon mismatch'
        machine = info['machine']
        assert ':'.join([machine['board_name'], machine['bios_version'], machine['kernel_version']]) == TARGET
        report['capabilities'] = call(args.socket, 'get_capabilities')['fan_modes']
        report['result'] = 'read_only_preflight'
        if not args.approve_exact_target:
            return
        assert args.approve_exact_target == exact, 'approval does not match exact target'
        assert report['capabilities']['status'] == 'available', 'independent writer unavailable'
        assert report['observations'][0]['mode'] == '2', 'first trial requires Automatic baseline'
        report['physical_writes_requested'] = not bool(args.fixture_root)
        attempted = True
        call(args.socket, 'set_fan_mode', {'mode': {'kind': 'max'}})
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            report['observations'].append(observation())
            assert report['observations'][-1]['mode'] == '0', 'Maximum ended before observation completed'
            policy = call(args.socket, 'export_diagnostics')['fan_policy']
            assert policy['guard'] and policy['owns_override'], 'thermal coverage or ownership lost'
            report['observations'][-1]['thermal_guard'] = policy['guard']
            time.sleep(.5)
        report['result'] = 'trial_observed'
    except BaseException as error:
        report['error'] = str(error)
        report['result'] = 'failed'
    finally:
        if attempted:
            try:
                call(args.socket, 'set_fan_mode', {'mode': {'kind': 'auto'}})
            except BaseException as error:
                report['automatic_request_error'] = str(error)
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                try:
                    final = observation()
                    if final['mode'] == '2':
                        report['automatic_restored'] = True
                        report['final_observation'] = final
                        break
                except OSError:
                    pass
                time.sleep(.1)
            if report['automatic_restored'] is not True:
                report['automatic_restored'] = False
                report['result'] = 'failed'
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with open(args.output, 'w', opener=lambda path, flags: os.open(path, flags, 0o600)) as output:
            json.dump(report, output, indent=2)
            output.write('\n')
        print(json.dumps(report, indent=2))
    if report['result'] == 'failed':
        raise SystemExit(1)


if __name__ == '__main__':
    main()
