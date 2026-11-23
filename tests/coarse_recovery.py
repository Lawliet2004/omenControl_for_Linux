#!/usr/bin/env python3
"""Standalone coarse recovery through real processes; ordinary fixtures only."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = ROOT / 'target' / PROFILE / 'omen-daemon'


def wait_for(predicate, timeout=8):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, 'condition timed out'
        time.sleep(.04)


def main():
    checks = []
    result = {'result': 'failed', 'profile': PROFILE, 'fixture_only': True,
              'physical_writes': 0, 'binary_sha256': hashlib.sha256(BIN.read_bytes()).hexdigest(),
              'checks_passed': checks}
    logs = []
    children = []
    with tempfile.TemporaryDirectory(prefix='omen-coarse-') as directory:
        base = Path(directory)
        (base / 'tmp').mkdir()
        env = dict(os.environ, TMPDIR=str(base / 'tmp'), OMEN_CONFIG_DIR=str(base / 'user'),
                   OMEN_SYSTEM_CONFIG_DIR=str(base / 'system'))
        def start(args, name):
            log = (base / (name + '.log')).open('w')
            logs.append(log)
            child = subprocess.Popen([str(BIN), *args], env=env, stdout=subprocess.PIPE, stderr=log, text=True)
            children.append(child)
            assert select.select([child.stdout], [], [], 8)[0], name
            return child, child.stdout.readline().strip()
        def request(op, args=None):
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(6)
                stream.connect(endpoint)
                operation = {'op': op}
                if args is not None:
                    operation['args'] = args
                stream.sendall(json.dumps({'v': 1, 'id': 3, 'op': operation}).encode() + b'\n')
                return json.loads(stream.makefile('rb').readline(65537))
        def mode(kind):
            reply = request('set_fan_mode', {'mode': {'kind': kind}})
            assert reply['ok'], reply
        def policy():
            return (hp / 'pwm1_enable').read_text().strip()
        def passed(label):
            checks.append(label)
            print('PASS', label, flush=True)
        def helper(name):
            process, path = start(['--coarse-recovery-fixture', str(root), '--instance', 'hwmon0'], name)
            assert path == str(recovery) and recovery.is_socket(), (path, process.poll())
            return process
        def probe(op):
            completed = subprocess.run([str(BIN), '--coarse-probe-fixture', str(recovery), op],
                                       env=env, capture_output=True, text=True, timeout=8)
            assert completed.returncode == 0, completed.stderr
        try:
            daemon, endpoint = start(['--mock', '--mock-recovery', 'auto'], 'controller')
            assert request('get_info')['data']['mock']
            hp = next((base / 'tmp').glob('*/sys/class/hwmon/hwmon0'))
            root = hp.parents[3]
            recovery = root / 'run/omen-fan-recovery/control.sock'
            journal = recovery.parent / 'owned.json'
            gpu = root / 'sys/bus/pci/devices/0000:06:00.0'
            driver = root / 'drivers/amdgpu'
            driver.mkdir(parents=True)
            (gpu / 'driver').symlink_to(driver)
            (hp.parent / 'hwmon2/device').symlink_to(gpu)
            assert request('get_capabilities')['data']['fan_modes']['status'] != 'available'
            writer = helper('writer')
            caps = request('get_capabilities')['data']
            assert caps['fan_modes']['status'] == 'available'
            assert caps['fan_control']['status'] != 'available' and caps['fan_curve']['status'] != 'available'
            passed('coarse control requires its independent writer and never grants manual/curve access')
            mode('max')
            assert policy() == '0' and journal.is_file()
            time.sleep(4.6)
            assert policy() == '0'
            passed('guarded daemon renews standalone writer lease beyond its original deadline')
            second = subprocess.run([str(BIN), '--coarse-recovery-fixture', str(root), '--instance', 'hwmon0'],
                                    env=env, capture_output=True, text=True, timeout=3)
            assert second.returncode and 'another writer' in second.stderr and policy() == '0'
            passed('singleton lock rejects a second writer before startup recovery can touch an active lease')
            with socket.socket(socket.AF_UNIX) as stream:
                stream.connect(str(recovery))
                stream.sendall(b'{"op":"auto","token":null,"seq":1}\n')
                try:
                    stream.recv(4096)
                except ConnectionResetError:
                    pass
            assert policy() == '0'
            passed('same-UID foreign executable cannot command or revoke the trusted controller lease')
            daemon.send_signal(signal.SIGSTOP)
            wait_for(lambda: policy() == '2')
            assert writer.poll() is None and not journal.exists()
            daemon.send_signal(signal.SIGCONT)
            wait_for(lambda: request('export_diagnostics')['data']['fan_policy']['requested'] == {'kind': 'auto'})
            passed('writer restores Automatic while the controller is stopped; resume does not reapply Maximum')
            mode('max')
            daemon.send_signal(signal.SIGSTOP)
            writer.kill()
            writer.wait(timeout=3)
            assert journal.exists() and policy() == '0'
            writer = helper('writer-restart')
            assert policy() == '2' and not journal.exists()
            daemon.send_signal(signal.SIGCONT)
            wait_for(lambda: request('export_diagnostics')['data']['fan_policy']['requested'] == {'kind': 'auto'})
            passed('writer restart restores durable intent before accepting commands while the controller is stopped')
            mode('auto')
            for operation in ['replay', 'manual', 'malformed', 'late']:
                probe(operation)
                wait_for(lambda: policy() == '2')
            passed('replayed, unsupported, malformed and late commands revoke ownership without resurrecting Maximum')
            holding, text = start(['--coarse-probe-fixture', str(recovery), 'hold'], 'owner')
            assert text == 'maximum' and policy() == '0'
            rival = subprocess.run([str(BIN), '--coarse-probe-fixture', str(recovery), 'hold'],
                                   env=env, capture_output=True, text=True, timeout=3)
            assert rival.returncode and policy() == '0'
            holding.kill()
            holding.wait(timeout=3)
            wait_for(lambda: policy() == '2')
            passed('competing trusted processes cannot take ownership; owner death restores Automatic')
            mode('max')
            conflict = root / 'proc/43123'
            conflict.mkdir(parents=True)
            (conflict / 'comm').write_text('omencore-cli\n')
            assert request('get_capabilities')['data']['fan_modes']['status'] != 'available'
            wait_for(lambda: policy() == '2')
            (conflict / 'comm').unlink()
            conflict.rmdir()
            # Interpreted fan controllers do not have a distinctive comm.
            for evidence, contents in [
                ('cgroup', '0::/system.slice/victus-fan.service\n'),
                ('cmdline', '/usr/bin/python3\0/home/user/.local/share/fanpilot/helper.py\0'),
            ]:
                mode('max')
                conflict.mkdir()
                (conflict / 'comm').write_text('python3\n')
                (conflict / evidence).write_text(contents)
                assert request('get_capabilities')['data']['fan_modes']['status'] != 'available'
                wait_for(lambda: policy() == '2')
                (conflict / evidence).unlink()
                (conflict / 'comm').unlink()
                conflict.rmdir()
            passed('interpreted Victus and FanPilot owners revoke the fixture lease')
            mode('max')
            (hp.parent / 'hwmon2/temp1_input').write_text('invalid')
            wait_for(lambda: policy() == '2')
            passed('independent transport preserves capability conflict checks and CPU/GPU loss recovery')
            (hp.parent / 'hwmon2/temp1_input').write_text('45000')
            script = [str(ROOT / 'scripts/coarse-fan-test.py'), '--socket', endpoint, '--instance', 'hwmon0',
                      '--fixture-root', str(root), '--output', str(base / 'trial.json')]
            subprocess.run(['python3', *script], env=env, check=True, stdout=subprocess.DEVNULL, timeout=10)
            assert policy() == '2' and not journal.exists()
            subprocess.run(['python3', *script, '--approve-exact-target', '8C30:F.10:7.0.0-38-generic:hwmon0'],
                           env=env, check=True, stdout=subprocess.DEVNULL, timeout=12)
            trial = json.loads((base / 'trial.json').read_text())
            assert trial['result'] == 'trial_observed' and trial['automatic_restored'] and not trial['physical_writes_requested']
            passed('reviewable trial script stays read-only by default and restores after a five-second approved fixture trial')
            mode('max')
            daemon.kill()
            daemon.wait(timeout=3)
            wait_for(lambda: policy() == '2')
            passed('independent writer survives controller SIGKILL and restores its exact actuator')
            probe('auto')
            ceiling, _ = start(['--coarse-probe-fixture', str(recovery), 'hold'], 'ceiling-owner')
            wait_for(lambda: policy() == '2', timeout=11.5)
            assert ceiling.wait(timeout=3) != 0 and not journal.exists()
            passed('healthy heartbeat cannot extend the independent ten-second physical trial ceiling')
            holding, _ = start(['--coarse-probe-fixture', str(recovery), 'hold'], 'lost-inode-owner')
            (hp / 'pwm1_enable').unlink()
            (hp / 'pwm1_enable').write_text('0')
            (hp / 'pwm1_enable').chmod(0o600)
            holding.kill()
            holding.wait(timeout=3)
            wait_for(journal.exists)
            time.sleep(.25)
            assert policy() == '0'
            writer.kill()
            writer.wait(timeout=3)
            replaced = subprocess.run([str(BIN), '--coarse-recovery-fixture', str(root), '--instance', 'hwmon0'],
                                      env=env, capture_output=True, text=True, timeout=3)
            assert replaced.returncode and 'another actuator' in replaced.stderr and policy() == '0'
            passed('replacement inode is never written; pending intent survives and restart refuses redirection')
            result['result'] = 'passed'
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.send_signal(signal.SIGCONT)
                    child.terminate()
                    try:
                        child.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=3)
                child.stdout.close()
            for log in logs:
                log.close()
            result['recorded_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            result['cleanup_complete'] = True
            evidence = ROOT / 'docs/evidence'
            (evidence / f'coarse-recovery-{PROFILE}.json').write_text(json.dumps(result, indent=2) + '\n')
            (evidence / f'coarse-recovery-{PROFILE}-process.log').write_text('\n'.join(
                str(p.name) + '\n' + p.read_text() for p in base.glob('*.log')))


if __name__ == '__main__':
    main()
