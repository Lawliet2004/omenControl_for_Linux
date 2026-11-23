#!/usr/bin/env python3
"""Private-PPD fixture in the installed sandbox; disposable QEMU guest only."""
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import socket
import subprocess
import time

RESULT = Path('/var/tmp/omen-vm-result.json')
UNIT = 'omen-linux.service'
BASE = Path('/run/omen-managed-fixture')
HELPER = Path('/usr/local/libexec/omen-managed-fixture')
SOCKET = Path('/run/omen-linux/ctl.sock')
JOURNAL = Path('/etc/omen-linux/managed-game-sessions.json')
OVERRIDE = Path('/run/systemd/system/omen-linux.service.d')
RULE = Path('/etc/polkit-1/rules.d/00-omen-managed-vm-test.rules')


def run(*args, check=True):
    value = subprocess.run(args, capture_output=True, text=True, timeout=35)
    print('+', args, value.returncode, value.stdout.strip(), value.stderr.strip(), flush=True)
    if check:
        value.check_returncode()
    return value


def wait(predicate, message, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.SubprocessError):
            pass
        time.sleep(.1)
    raise RuntimeError(message)


CLIENT = '''import json,socket,sys
with socket.socket(socket.AF_UNIX) as stream:
 stream.settimeout(6);stream.connect('/run/omen-linux/ctl.sock')
 stream.sendall(json.dumps({'v':1,'id':1,'op':{'op':sys.argv[1]}}).encode()+b'\\n')
 reply=json.loads(stream.makefile('rb').readline());assert reply['ok'],reply
 print(json.dumps(reply['data']))
'''


def request(op):
    return json.loads(run('runuser', '-u', 'ubuntu', '--', 'python3', '-c', CLIENT, op).stdout)


def main():
    assert os.geteuid() == 0
    assert run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    assert HELPER.is_file() and not BASE.exists() and not OVERRIDE.exists() and not RULE.exists() and not JOURNAL.exists()
    state = json.loads(RESULT.read_text())
    def passed(label):
        state['checks_passed'].append(label)
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        print('PASS', label, flush=True)
    state['managed_session_fixture'] = {'helper_sha256': hashlib.file_digest(HELPER.open('rb'), 'sha256').hexdigest(),
        'private_ppd': True, 'physical_hardware_writes': False, 'production_polkit': True,
        'installed_service_sandbox': True}
    BASE.mkdir(mode=0o755)
    user = pwd.getpwnam('ubuntu')
    preferences = BASE / 'user'
    preferences.mkdir(mode=0o700)
    os.chown(preferences, user.pw_uid, user.pw_gid)
    config = preferences / 'config.toml'
    config.write_text('version = 1\n[games]\nmanaged_game_ids = ["custom/vm"]\n[games.per_game_profiles]\n"custom/vm" = "performance"\n')
    os.chown(config, user.pw_uid, user.pw_gid)
    bus_log = (BASE / 'bus.log').open('w')
    ppd_log = (BASE / 'ppd.log').open('w')
    children = []
    def game(seconds):
        child = subprocess.Popen(['runuser', '-u', 'ubuntu', '--', 'env',
            'XDG_RUNTIME_DIR=/run/user/' + str(user.pw_uid),
            'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/' + str(user.pw_uid) + '/bus',
            'OMEN_CONFIG_DIR=' + str(preferences), '/usr/bin/omen-cli',
            'game-run', 'custom/vm', '--', '/bin/sleep', str(seconds)],
            stdout=subprocess.DEVNULL, stderr=ppd_log)
        children.append(child)
        return child
    try:
        run('loginctl', 'enable-linger', 'ubuntu')
        run('systemctl', 'start', 'user@' + str(user.pw_uid) + '.service')
        bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1',
                                '--address=unix:path=' + str(BASE / 'bus')],
                               stdout=subprocess.PIPE, stderr=bus_log, text=True)
        children.append(bus)
        address = bus.stdout.readline().strip()
        assert address.startswith('unix:')
        ppd = subprocess.Popen([str(HELPER), 'ppd-service', address], stdout=subprocess.PIPE,
                               stderr=ppd_log, text=True)
        children.append(ppd)
        assert ppd.stdout.readline().strip() == 'READY'
        run('systemctl', 'stop', UNIT)
        OVERRIDE.mkdir()
        (OVERRIDE / 'fixture.conf').write_text('[Service]\nExecStart=\nExecStart=' + str(HELPER) +
            ' fixture-service ' + str(SOCKET) + ' ' + str(JOURNAL) + ' ' + address + ' authorized\n')
        # Only this guest's desktop test user receives the normal control action.
        RULE.write_text('polkit.addRule(function(action, subject) { if (subject.user == "ubuntu" && action.id == "com.omenlinux.control") return polkit.Result.YES; });\n')
        run('systemctl', 'daemon-reload')
        run('systemctl', 'start', UNIT)
        wait(lambda: SOCKET.is_socket(), 'fixture service socket absent')
        assert request('get_profile') == 'balanced'
        first = game(4)
        wait(lambda: any(s['profile_held'] for s in request('get_game_sessions')['sessions']), 'sandbox rejected live user scope')
        assert request('get_profile') == 'performance'
        token = request('get_game_sessions')['sessions'][0]['token']
        pid = int(run('systemctl', 'show', UNIT, '-p', 'MainPID', '--value').stdout.strip())
        status = Path('/proc/' + str(pid) + '/status').read_text()
        assert 'NoNewPrivs:\t1' in status
        assert run('systemctl', 'show', UNIT, '-p', 'ProtectControlGroups', '--value').stdout.strip() == 'yes'
        assert run('systemctl', 'show', UNIT, '-p', 'ProtectSystem', '--value').stdout.strip() == 'strict'
        passed('installed service sandbox admits production-polkit-authorized desktop wrapper through peer pidfd and real cgroup v2')
        first.wait(timeout=15)
        assert first.returncode == 0
        wait(lambda: request('get_profile') == 'balanced', 'sandbox could not observe final process exit')
        assert any(s['token'] == token and s['phase'] == 'ended' and not s['profile_held']
                   for s in request('get_game_sessions')['sessions'])
        assert JOURNAL.stat().st_uid == 0 and JOURNAL.stat().st_mode & 0o777 == 0o600
        assert b'cookie' not in JOURNAL.read_bytes().lower()
        passed('read-only cgroup sandbox observes exit and releases private PPD hold, persisting a root-owned private journal')
        second = game(9)
        wait(lambda: any(s['profile_held'] for s in request('get_game_sessions')['sessions']), 'second sandbox launch failed')
        run('systemctl', 'restart', UNIT)
        wait(lambda: request('get_profile') == 'balanced', 'service restart retained provider hold')
        assert second.poll() is None
        assert any(s['phase'] == 'interrupted' and not s['profile_held'] for s in request('get_game_sessions')['sessions'])
        second.wait(timeout=15)
        assert second.returncode == 0 and request('get_profile') == 'balanced'
        passed('installed sandbox restart preserves game execution and recovers Interrupted history without reapplying authority')
    finally:
        run('systemctl', 'stop', UNIT, check=False)
        shutil.rmtree(OVERRIDE, ignore_errors=True)
        RULE.unlink(missing_ok=True)
        JOURNAL.unlink(missing_ok=True)
        run('systemctl', 'daemon-reload', check=False)
        run('systemctl', 'start', UNIT, check=False)
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=10)
        bus_log.close()
        ppd_log.close()
        shutil.rmtree(BASE)
        HELPER.unlink()
        state['managed_session_fixture']['cleanup_complete'] = True
        RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
