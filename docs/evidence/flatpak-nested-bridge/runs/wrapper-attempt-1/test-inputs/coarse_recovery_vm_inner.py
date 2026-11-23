#!/usr/bin/env python3
"""Independent cgroup/restart checks; disposable QEMU guest, ordinary files only."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

RESULT = Path('/var/tmp/omen-vm-result.json')
ROOT = Path('/run/omen-coarse-fixture')
WRITER = 'omen-fan-recovery@hwmon0.service'
OWNER = 'omen-coarse-controller-fixture.service'
BINARY = '/usr/bin/omen-daemon'


def run(*args, check=True):
    c = subprocess.run(args, capture_output=True, text=True, timeout=30)
    print('+', args, c.returncode, c.stdout.strip(), c.stderr.strip(), flush=True)
    if check:
        c.check_returncode()
    return c


def wait_for(predicate, timeout=12):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, 'fixture recovery timed out'
        time.sleep(.05)


def main():
    assert os.geteuid() == 0 and Path('/proc/1/comm').read_text().strip() == 'systemd'
    assert run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    assert RESULT.exists(), 'baseline VM test state missing'
    state = json.loads(RESULT.read_text())
    def passed(label):
        state['checks_passed'].append(label)
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        print('PASS', label, flush=True)
    def write(path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value + '\n')
        path.chmod(0o600)
    hp = ROOT / 'sys/class/hwmon/hwmon0'
    area = ROOT / 'run/omen-fan-recovery'
    socket = area / 'control.sock'
    journal = area / 'owned.json'
    dropin = Path('/run/systemd/system') / (WRITER + '.d')
    owner_unit = Path('/run/systemd/system') / OWNER
    assert not ROOT.exists() and not dropin.exists() and not owner_unit.exists()
    ROOT.mkdir(mode=0o700)
    area.mkdir(mode=0o700, parents=True)
    for name, value in [('name', 'hp'), ('pwm1_enable', '2')]:
        write(hp / name, value)
    for name, value in [('sys_vendor', 'HP'), ('product_name', 'Victus'), ('board_name', '8C30'), ('bios_version', 'F.10')]:
        write(ROOT / 'sys/class/dmi/id' / name, value)
    write(ROOT / 'proc/sys/kernel/osrelease', '7.0.0-38-generic')
    dropin.mkdir()
    (dropin / 'fixture.conf').write_text(f'''[Service]
ExecStart=
ExecStart={BINARY} --coarse-recovery-fixture {ROOT} --instance hwmon0
BindPaths=
BindPaths={hp}/pwm1_enable
ReadOnlyPaths={ROOT}
ReadWritePaths={area}
''')
    owner_unit.write_text(f'''[Unit]
Description=Ordinary-file coarse controller cgroup test
[Service]
ExecStart={BINARY} --coarse-probe-fixture {socket} hold
Restart=no
CapabilityBoundingSet=CAP_DAC_OVERRIDE
ProtectSystem=strict
ProtectHome=yes
NoNewPrivileges=yes
PrivateTmp=yes
''')
    def policy():
        return (hp / 'pwm1_enable').read_text().strip()
    def start_owner():
        run('systemctl', 'reset-failed', OWNER, check=False)
        run('systemctl', 'start', OWNER)
        wait_for(lambda: policy() == '0')
        assert journal.exists()
    def prop(unit, key):
        return run('systemctl', 'show', unit, '-p', key, '--value').stdout.strip()
    try:
        run('systemctl', 'daemon-reload')
        assert prop(WRITER, 'UnitFileState') == 'static'
        passed('packaged experimental writer is static and never enabled by installation')
        run('systemctl', 'start', WRITER)
        wait_for(socket.is_socket)
        assert policy() == '2' and not journal.exists()
        passed('packaged writer sandbox starts without an unowned fixture write')
        writer_pid = prop(WRITER, 'MainPID')
        assert int(writer_pid) > 1
        test = "from pathlib import Path; p=Path('" + str(hp / 'name') + "');\ntry: p.write_text('bad')\nexcept OSError as e: assert e.errno==30\nelse: raise RuntimeError('name mount writable')"
        run('nsenter', '-t', writer_pid, '-m', '--', 'python3', '-c', test)
        assert (hp / 'name').read_text().strip() == 'hp'
        start_owner()
        assert prop(WRITER, 'ControlGroup') != prop(OWNER, 'ControlGroup')
        time.sleep(4.6)
        assert policy() == '0'
        passed('writer has a separate cgroup and only the exact fixture mode attribute is writable')
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGKILL', OWNER)
        wait_for(lambda: policy() == '2')
        assert prop(WRITER, 'ActiveState') == 'active' and not journal.exists()
        passed('killing the controller entire service cgroup leaves independent Automatic recovery alive')
        start_owner()
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGSTOP', OWNER)
        old_pid = prop(WRITER, 'MainPID')
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGKILL', WRITER)
        wait_for(lambda: policy() == '2' and socket.is_socket() and not journal.exists())
        wait_for(lambda: prop(WRITER, 'MainPID') not in ['0', old_pid])
        assert prop(WRITER, 'ActiveState') == 'active'
        passed('systemd restarts a killed writer and its journal restores Automatic while controller remains stopped')
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGCONT', OWNER)
        wait_for(lambda: prop(OWNER, 'ActiveState') in ['failed', 'inactive'])
        time.sleep(.6)
        assert policy() == '2'
        passed('old controller lease cannot reapply Maximum after writer restart')
        start_owner()
        run('systemctl', 'stop', OWNER)
        wait_for(lambda: policy() == '2')
        assert not journal.exists()
        passed('normal controller service stop restores Automatic without a broad hwmon sweep')
        start_owner()
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGSTOP', OWNER)
        wait_for(lambda: policy() == '2')
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGCONT', OWNER)
        run('systemctl', 'stop', OWNER)
        passed('independently scheduled writer expires a stopped controller lease in the real systemd guest')
    finally:
        run('journalctl', '--no-pager', '-u', WRITER, '-u', OWNER, check=False)
        run('systemctl', 'kill', '--kill-whom=all', '--signal=SIGCONT', OWNER, check=False)
        run('systemctl', 'stop', OWNER, WRITER, check=False)
        assert not journal.exists(), 'do not remove unresolved recovery intent'
        shutil.rmtree(dropin)
        owner_unit.unlink()
        run('systemctl', 'daemon-reload')
        run('systemctl', 'reset-failed', OWNER, WRITER, check=False)
        shutil.rmtree(ROOT)
    pending = Path('/run/omen-fan-recovery/owned.json')
    assert not pending.exists()
    pending.write_text('{"fixture_removal_guard": true}')
    pending.chmod(0o600)
    try:
        refusal = run('/var/lib/dpkg/info/omen-linux.prerm', 'remove', check=False)
        assert refusal.returncode != 0 and 'pending fan recovery intent' in refusal.stderr
        assert Path(BINARY).is_file() and pending.exists()
    finally:
        pending.unlink()
        run('systemctl', 'start', 'omen-linux.service')
    passed('package removal hook preserves its executable and refuses to discard pending recovery intent')
    state['coarse_recovery_fixture_only'] = True
    state['coarse_recovery_cleanup_complete'] = True
    RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
