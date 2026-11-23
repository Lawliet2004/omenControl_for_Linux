#!/usr/bin/env python3
"""Trust/freshness/thermal tests for DynamicUser reader; QEMU fixtures only."""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time

RESULT = Path('/var/tmp/omen-vm-result.json')
BASE = Path('/run/omen-nv-fixture')
SERVICE = 'omen-nvidia-reader.service'
SOCKET = 'omen-nvidia-reader.socket'
BIN = '/usr/bin/omen-daemon'


def run(*args, check=True):
    c = subprocess.run(args, capture_output=True, text=True, timeout=30)
    print('+', args, c.returncode, c.stdout.strip(), c.stderr.strip(), flush=True)
    if check:
        c.check_returncode()
    return c


def wait_for(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, 'reader fixture timed out'
        time.sleep(.08)


def main():
    assert os.geteuid() == 0 and run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    state = json.loads(RESULT.read_text())
    def passed(label):
        state['checks_passed'].append(label)
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        print('PASS', label, flush=True)
    assert not BASE.exists()
    BASE.mkdir(mode=0o755)
    (BASE / 'tmp').mkdir()
    source = BASE / 'temperature.json'
    sample = {'id': 'nvidia/temp', 'label': 'NVIDIA fixture temperature', 'kind': 'temperature',
              'source': 'nvidia_smi', 'value': 45.0, 'device_id': '0000:01:00.0',
              'measured_unix_ms': 0, 'text': None}
    def payload(timestamp=0):
        source.write_text(json.dumps(dict(sample, measured_unix_ms=timestamp)))
        source.chmod(0o444)
    payload()
    override = Path('/run/systemd/system/omen-nvidia-reader.service.d')
    assert not override.exists()
    override.mkdir()
    (override / 'fixture.conf').write_text(f'[Service]\nExecStart=\nExecStart={BIN} --nvidia-reader-fixture {source}\n')
    children = []
    logs = []
    def start(args, name):
        log = (BASE / (name + '.log')).open('w')
        logs.append(log)
        child = subprocess.Popen([BIN, *args], stdout=subprocess.PIPE, stderr=log, text=True,
                                 env=dict(os.environ, TMPDIR=str(BASE / 'tmp'), OMEN_CONFIG_DIR=str(BASE / 'user'),
                                          OMEN_SYSTEM_CONFIG_DIR=str(BASE / 'system')))
        children.append(child)
        return child, child.stdout.readline().strip()
    def prop(unit, field):
        return run('systemctl', 'show', unit, '-p', field, '--value').stdout.strip()
    def request(op, args=None):
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(8)
            stream.connect(endpoint)
            operation = {'op': op}
            if args is not None:
                operation['args'] = args
            stream.sendall(json.dumps({'v': 1, 'id': 8, 'op': operation}).encode() + b'\n')
            value = json.loads(stream.makefile('rb').readline(65537))
            assert value['ok'], value
            return value['data']
    def mode(value):
        request('set_fan_mode', {'mode': {'kind': value}})
    try:
        run('systemctl', 'daemon-reload')
        assert prop(SOCKET, 'UnitFileState') == 'static' and prop(SERVICE, 'UnitFileState') == 'static'
        run('systemctl', 'start', SOCKET)
        read = json.loads(run(BIN, '--nvidia-reader-probe-fixture').stdout)
        assert read[0]['value'] == 45.0 and read[0]['device_id'] == '0000:01:00.0'
        pid = int(prop(SERVICE, 'MainPID'))
        status = Path(f'/proc/{pid}/status').read_text()
        assert int(next(line.split()[1] for line in status.splitlines() if line.startswith('Uid:'))) != 0
        assert int(next(line.split()[1] for line in status.splitlines() if line.startswith('CapEff:')), 16) == 0
        assert Path('/run/omen-nvidia-reader/control.sock').stat().st_uid == 0
        passed('static socket activates a capability-free DynamicUser reader behind a root-private PID-1 endpoint')
        assert run(BIN, '--nvidia-reader-probe', check=False).returncode != 0
        assert run('runuser', '-u', 'ubuntu', '--', BIN, '--nvidia-reader-probe-fixture', check=False).returncode != 0
        passed('live client rejects fixture frames and desktop users cannot impersonate or access the control telemetry endpoint')
        daemon, endpoint = start(['--mock', '--mock-recovery', 'auto', '--mock-nvidia-reader'], 'controller')
        assert request('get_info')['mock']
        hp = next((BASE / 'tmp').glob('*/sys/class/hwmon/hwmon0'))
        root = hp.parents[3]
        for pci, driver_name in [('0000:01:00.0', 'nvidia'), ('0000:06:00.0', 'amdgpu')]:
            gpu = root / 'sys/bus/pci/devices' / pci
            driver = root / 'drivers' / driver_name
            driver.mkdir(parents=True)
            (gpu / 'driver').symlink_to(driver)
            (gpu / 'power/runtime_status').write_text('active')
        (hp.parent / 'hwmon2/device').symlink_to(root / 'sys/bus/pci/devices/0000:06:00.0')
        helper, recovery = start(['--coarse-recovery-fixture', str(root), '--instance', 'hwmon0'], 'writer')
        assert Path(recovery).is_socket()
        def policy():
            return (hp / 'pwm1_enable').read_text().strip()
        mode('max')
        guard = request('export_diagnostics')['fan_policy']['guard']
        assert 'nvidia/temp' in guard['frame']['sensor_ids'] and policy() == '0'
        payload(1)
        wait_for(lambda: policy() == '2')
        payload(int(time.time() * 1000) + 60000)
        assert run(BIN, '--nvidia-reader-probe-fixture', check=False).returncode != 0
        payload()
        time.sleep(1.1)
        assert policy() == '2'
        passed('identified active NVIDIA temperature passes fixture preflight; stale/future data restores Auto and cannot rearm itself')
        mode('max')
        run('systemctl', 'stop', SOCKET)
        wait_for(lambda: policy() == '2')
        assert prop(SERVICE, 'ActiveState') == 'inactive'
        passed('telemetry service loss restores Automatic without widening the controller device sandbox or using a user cache')
    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
            child.stdout.close()
        for log in logs:
            log.close()
        run('journalctl', '--no-pager', '-u', SERVICE, '-u', SOCKET, check=False)
        for path in BASE.glob('*.log'):
            print(path.name, path.read_text(), flush=True)
        run('systemctl', 'stop', SOCKET, SERVICE, check=False)
        shutil.rmtree(override)
        run('systemctl', 'daemon-reload')
        run('systemctl', 'reset-failed', SOCKET, SERVICE, check=False)
        shutil.rmtree(BASE)
    state['isolated_nvidia_fixture_only'] = True
    state['isolated_nvidia_cleanup_complete'] = True
    RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
