#!/usr/bin/env python3
"""Actual distro Lutris/OpenTTD, isolated PPD, and guest-only input. VM only.

Lutris and the game are unmodified signed distro packages. Only their ordinary
user configuration is created. The installed OMEN CLI supplies the production
URI route and command wrapper; the service fixture retains the installed unit
sandbox and production IPC/session code while replacing the physical provider.
"""
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import socket
import string
import subprocess
import sys
import time

RESULT = Path('/var/tmp/omen-vm-result.json')
BASE = Path('/run/omen-real-game-fixture')
HELPER = Path('/usr/local/libexec/omen-managed-fixture')
UNIT = 'omen-linux.service'
OVERRIDE = Path('/run/systemd/system/omen-linux.service.d')
RULE = Path('/etc/polkit-1/rules.d/00-omen-real-game-vm-test.rules')
JOURNAL = Path('/etc/omen-linux/managed-game-sessions.json')


def run(*args, check=True, input=None, timeout=40):
    value = subprocess.run(args, input=input, capture_output=True, text=True, timeout=timeout)
    print('+', ' '.join(args), value.returncode, value.stdout.strip(), value.stderr.strip(), flush=True)
    if check:
        value.check_returncode()
    return value


def wait(predicate, message, timeout=35):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, subprocess.SubprocessError):
            pass
        time.sleep(.15)
    raise RuntimeError(message)


def capture(name):
    print('OMEN_VM_CAPTURE ' + name, flush=True)
    assert sys.stdin.readline().strip() == 'OK'


def main():
    assert os.geteuid() == 0 and os.environ.get('OMEN_REAL_GAME_VM') == '1'
    assert run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    assert HELPER.is_file() and RESULT.is_file()
    assert not BASE.exists() and not OVERRIDE.exists() and not RULE.exists() and not JOURNAL.exists()
    assert not list(Path('/etc/polkit-1/rules.d').glob('*omen*'))
    state = json.loads(RESULT.read_text())
    data = {'result': 'failed', 'checks_passed': [], 'private_ppd': True,
        'physical_hardware_writes': False, 'physical_keyboard_tested': False,
        'input': 'guest-only libevdev/uinput', 'real_game': 'OpenTTD with OpenGFX',
        'launcher': 'native Lutris Linux runner', 'store_account_tested': False,
        'source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'helper_sha256': hashlib.file_digest(HELPER.open('rb'), 'sha256').hexdigest(),
        'authorization': 'guest-only com.omenlinux.control grant; no admin grant',
        'installed_service_sandbox': True}
    account = pwd.getpwnam('ubuntu')
    home = Path(account.pw_dir)
    BASE.mkdir(mode=0o755)
    prefs = BASE / 'user'
    prefs.mkdir(mode=0o700)
    os.chown(prefs, account.pw_uid, account.pw_gid)
    children, owned, logs = [], [], []
    keyboard = None
    user_env = {'HOME': str(home), 'XDG_RUNTIME_DIR': '/run/user/1000',
        'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/run/user/1000/bus',
        'OMEN_CONFIG_DIR': str(prefs), 'PATH': '/usr/local/bin:/usr/bin:/bin:/usr/games',
        'LUTRIS_SKIP_INIT': '1', 'LUTRIS_RUNTIME': '0', 'LUTRIS_HIDE_MEDIA': '1',
        'SDL_VIDEODRIVER': 'x11', 'LANG': 'C.UTF-8'}

    def user_file(path, contents):
        assert not path.exists(), ('refuse existing guest fixture file', str(path))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        path.chmod(0o600)
        os.chown(path, account.pw_uid, account.pw_gid)
        for parent in path.parents:
            if parent == home or parent == BASE:
                break
            if parent.is_relative_to(home):
                os.chown(parent, account.pw_uid, account.pw_gid)
        owned.append(path)

    def user_args(*args):
        return ['runuser', '-u', 'ubuntu', '--', 'env', *[k + '=' + v for k, v in user_env.items()], *args]

    def user_run(*args, check=True):
        return run(*user_args(*args), check=check)

    def request(op):
        script = """import json,socket,sys
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(6);s.connect('/run/omen-linux/ctl.sock')
 s.sendall(json.dumps({'v':1,'id':1,'op':{'op':sys.argv[1]}}).encode()+b'\\n')
 r=json.loads(s.makefile('rb').readline());assert r['ok'],r
 print(json.dumps(r['data']))
"""
        return json.loads(user_run('python3', '-c', script, op).stdout)

    def passed(label):
        data['checks_passed'].append(label)
        state['real_game'] = data
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        print('PASS', label, flush=True)

    def game_process():
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():
                continue
            try:
                if proc.stat().st_uid == account.pw_uid and (proc / 'exe').resolve() == Path('/usr/games/openttd'):
                    return int(proc.name)
            except (OSError, RuntimeError):
                pass

    def xprop(*args):
        return user_run('xprop', *args, check=False).stdout

    def game_window(pid):
        clients = re.findall(r'0x[0-9a-f]+', xprop('-root', '_NET_CLIENT_LIST_STACKING'))
        for xid in clients:
            properties = xprop('-id', xid, '_NET_WM_PID', '_NET_WM_NAME', '_NET_WM_STATE')
            if re.search(r'_NET_WM_PID\(CARDINAL\) = ' + str(pid) + r'\b', properties):
                return {'xid': xid, 'properties': properties}

    def keys(*names):
        import libevdev
        codes = [getattr(libevdev, 'KEY_' + name) for name in names]
        keyboard.send_events([*[libevdev.InputEvent(code, 1) for code in codes], libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.08)
        keyboard.send_events([*[libevdev.InputEvent(code, 0) for code in reversed(codes)], libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.18)

    def focus(window):
        for _ in range(8):
            active = re.findall(r'0x[0-9a-f]+', xprop('-root', '_NET_ACTIVE_WINDOW'))
            if active and int(active[0], 16) == int(window['xid'], 16):
                return
            keys('LEFTALT', 'TAB')
        raise RuntimeError('real game window did not receive desktop focus')

    def live_launch(index):
        assert not game_process()
        before = {s['token'] for s in request('get_game_sessions')['sessions']}
        error_path = BASE / ('launch-' + str(index) + '.log')
        with error_path.open('w') as errors:
            call = subprocess.run(user_args('/usr/bin/omen-cli', 'game-launch', 'lutris/1', '--json'),
                stdout=subprocess.PIPE, stderr=errors, text=True, timeout=25)
        call.check_returncode()
        receipt = json.loads(call.stdout)
        assert receipt['ok'] and receipt['launch']['managed'], receipt
        assert not receipt['launch']['launcher']['game_execution_verified']
        data.setdefault('launch_receipts', []).append(receipt)
        pid = wait(game_process, 'actual OpenTTD process was not launched', timeout=80)
        session = wait(lambda: next((s for s in request('get_game_sessions')['sessions']
            if s['token'] not in before and s['profile_held']), None), 'real game has no verified profile hold')
        assert session['game_id'] == 'lutris/1' and session['requested_profile'] == 'performance'
        assert request('get_profile') == 'performance'
        cgroup = Path('/proc/' + str(pid) + '/cgroup').read_text().strip()
        assert '/omen-game-' + session['token'] + '.scope' in cgroup, cgroup
        window = wait(lambda: game_window(pid), 'OpenTTD did not map a real game window', timeout=50)
        assert 'OpenTTD' in window['properties']
        focus(window)
        data.setdefault('game_processes', []).append({'pid': pid, 'uid': Path('/proc/' + str(pid)).stat().st_uid,
            'exe': str(Path('/proc/' + str(pid) + '/exe').resolve()), 'cgroup': cgroup,
            'argv': Path('/proc/' + str(pid) + '/cmdline').read_bytes().decode().split('\0')[:-1],
            'session': session, 'window': window})
        return pid, session, window

    def quit_game(pid, window):
        focus(window)
        # The game's documented console command exits normally, without a kill
        # signal, launcher stop request, or synthetic executable replacing it.
        keys('GRAVE')
        for name in 'QUIT':
            keys(name)
        keys('ENTER')
        wait(lambda: not Path('/proc/' + str(pid)).exists(), 'OpenTTD did not exit through its own console', timeout=35)
        # The Lutris run URI asks its application to quit when this game stops.
        wait(lambda: not any(p.name.isdigit() and p.stat().st_uid == account.pw_uid and
            b'/usr/games/lutris\0' in (p / 'cmdline').read_bytes()
            for p in Path('/proc').iterdir() if (p / 'cmdline').exists()), 'Lutris did not finish its game run', timeout=40)

    try:
        run('systemctl', 'stop', 'sddm.service')
        user_file(prefs / 'config.toml', 'version=1\n[general]\nonboarding_dismissed=true\n'
            '[games]\nmanaged_game_ids=["lutris/1"]\n[games.per_game_profiles]\n"lutris/1"="performance"\n')
        user_file(home / '.config/plasma-workspace/env/omen-real-game.sh',
            'export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 GTK_A11Y=atspi LIBGL_ALWAYS_SOFTWARE=1 QT_QUICK_BACKEND=software\n')
        user_file(home / '.config/autostart/omen-real-game.desktop', '[Desktop Entry]\nType=Application\nName=OMEN real game validation\n'
            'Exec=/usr/bin/env OMEN_CONFIG_DIR=' + str(prefs) + ' GTK_A11Y=atspi /usr/bin/omen-gui --new-instance --page games\n')
        user_file(home / '.config/lutris/lutris.conf', '[lutris]\nhide_media=True\n')
        user_file(home / '.config/openttd/openttd.cfg', '[game_creation]\nmap_x=6\nmap_y=6\n'
            'generation_seed=12345\n[network]\nserver_game_type=local\n[gui]\nthreaded_load=false\n')
        sessions = [p for p in Path('/usr/share/wayland-sessions').glob('*.desktop') if 'startplasma-wayland' in p.read_text()]
        assert len(sessions) == 1
        conf = Path('/etc/sddm.conf.d/10-omen-real-game.conf')
        assert not conf.exists()
        conf.parent.mkdir(exist_ok=True)
        conf.write_text('[Autologin]\nUser=ubuntu\nSession=' + sessions[0].name + '\nRelogin=false\n'
            '[Theme]\nCurrent=breeze\n[X11]\nServerPath=/usr/lib/xorg/Xorg\n')
        owned.append(conf)
        run('modprobe', 'uinput')
        import libevdev
        device = libevdev.Device()
        device.name = 'omen-private-vm-real-game-keyboard'
        for name in [*string.ascii_uppercase, 'LEFTCTRL', 'LEFTALT', 'TAB', 'ENTER', 'ESC', 'GRAVE']:
            device.enable(getattr(libevdev, 'KEY_' + name))
        keyboard = device.create_uinput_device()
        run('systemctl', 'start', 'sddm.service')

        def local_session():
            value = run('loginctl', 'show-user', 'ubuntu', '-p', 'Display', '--value', check=False).stdout.strip()
            if value:
                info = run('loginctl', 'show-session', value, '-p', 'Type', '-p', 'Active', '-p', 'Remote', check=False).stdout
                if 'Type=wayland' in info and 'Active=yes' in info and 'Remote=no' in info:
                    return value
        data['desktop_session'] = wait(local_session, 'no active local Plasma Wayland session', timeout=100)
        def desktop_environment():
            env_text = user_run('systemctl', '--user', 'show-environment').stdout
            exported = dict(line.split('=', 1) for line in env_text.splitlines() if '=' in line)
            return exported if exported.get('DISPLAY') and exported.get('XAUTHORITY') else None
        # logind marks a session active before startplasma/kwin finish exporting
        # the desktop's Xwayland environment to the user manager.
        exported = wait(desktop_environment, 'desktop did not export its Xwayland environment', timeout=70)
        for key in ['DISPLAY', 'XAUTHORITY', 'WAYLAND_DISPLAY', 'XDG_SESSION_TYPE']:
            if key in exported:
                user_env[key] = exported[key]
        assert user_env.get('DISPLAY') and user_env.get('XAUTHORITY')
        verified = run('dpkg', '--verify', 'lutris', 'openttd', 'openttd-data', 'openttd-opengfx')
        assert not verified.stdout.strip(), 'installed launcher or game files differ from the signed distro packages'
        data['installed_files'] = {path: hashlib.file_digest(Path(path).open('rb'), 'sha256').hexdigest()
            for path in ['/usr/games/openttd', '/usr/games/lutris',
                '/usr/lib/python3/dist-packages/lutris/runner_interpreter.py',
                '/usr/lib/python3/dist-packages/lutris/runners/linux.py']}
        data['versions'] = run('dpkg-query', '-W', '-f=${Package}\t${Version}\t${Installed-Size}\n',
            'openttd', 'openttd-data', 'openttd-opengfx', 'lutris', 'plasma-workspace').stdout
        data['installed_game_kib'] = sum(int(line.split('\t')[-1]) for line in data['versions'].splitlines()
            if line.split('\t')[0] in ['openttd', 'openttd-data', 'openttd-opengfx'])
        assert data['installed_game_kib'] * 1024 < 2_000_000_000
        setup = '''from lutris.startup import init_lutris
from lutris.config import write_game_config
from lutris.database import games
init_lutris()
config=write_game_config('openttd-omen-validation', {
 'game': {'exe':'/usr/games/openttd','args':'-g -r 960x600 -s null -m null -x -c /home/ubuntu/.config/openttd/openttd.cfg'},
 'system': {'disable_runtime':True,'gamemode':False,'prefix_command':'/usr/bin/omen-cli game-run lutris/1 --'}})
game_id=games.add_game(name='OpenTTD',slug='openttd-omen-validation',runner='linux',platform='Linux',
 installed=1,configpath=config,executable='/usr/games/openttd',directory='/usr/games')
assert game_id==1,game_id
print(config)
'''
        user_run('python3', '-c', setup)
        inventory = json.loads(user_run('/usr/bin/omen-cli', 'games', '--json').stdout)
        assert any(g['id'] == 'lutris/1' and g['name'] == 'OpenTTD' for g in inventory), inventory
        data['inventory'] = inventory
        passed('installed OMEN discovers actual native Lutris database and the signed OpenTTD/OpenGFX game is below 2 GB')
        bus_log = (BASE / 'bus.log').open('w')
        ppd_log = (BASE / 'ppd.log').open('w')
        logs.extend([bus_log, ppd_log])
        bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1', '--address=unix:path=' + str(BASE / 'bus')],
            stdout=subprocess.PIPE, stderr=bus_log, text=True)
        children.append(bus)
        address = bus.stdout.readline().strip()
        assert address.startswith('unix:')
        ppd = subprocess.Popen([str(HELPER), 'ppd-service', address], stdout=subprocess.PIPE, stderr=ppd_log, text=True)
        children.append(ppd)
        assert ppd.stdout.readline().strip() == 'READY'
        run('systemctl', 'stop', UNIT)
        OVERRIDE.mkdir()
        (OVERRIDE / 'fixture.conf').write_text('[Service]\nExecStart=\nExecStart=' + str(HELPER) +
            ' fixture-service /run/omen-linux/ctl.sock ' + str(JOURNAL) + ' ' + address + ' authorized\n')
        RULE.write_text('polkit.addRule(function(action, subject) { if (subject.user == "ubuntu" && action.id == "com.omenlinux.control") return polkit.Result.YES; });\n')
        run('systemctl', 'daemon-reload')
        run('systemctl', 'start', UNIT)
        wait(lambda: Path('/run/omen-linux/ctl.sock').is_socket(), 'private provider service missing')
        assert request('get_profile') == 'balanced'
        assert run('systemctl', 'show', UNIT, '-p', 'ProtectSystem', '--value').stdout.strip() == 'strict'
        assert run('systemctl', 'show', UNIT, '-p', 'ProtectControlGroups', '--value').stdout.strip() == 'yes'

        pid, session, window = live_launch(1)
        time.sleep(3)
        capture('openttd-lutris-windowed')
        passed('production launch URI reaches unmodified Lutris command prefix, actual OpenTTD window and verified user scope with a held Performance profile')
        keys('LEFTALT', 'ENTER')
        fullscreen = wait(lambda: '_NET_WM_STATE_FULLSCREEN' in xprop('-id', window['xid'], '_NET_WM_STATE'), 'game fullscreen transition missing')
        assert fullscreen and request('get_profile') == 'performance' and game_process() == pid
        capture('openttd-lutris-fullscreen')
        keys('LEFTALT', 'ENTER')
        wait(lambda: '_NET_WM_STATE_FULLSCREEN' not in xprop('-id', window['xid'], '_NET_WM_STATE'), 'game did not leave fullscreen')
        passed('guest keyboard enters and exits actual game fullscreen on Plasma Wayland/Xwayland without losing the profile hold')
        quit_game(pid, window)
        wait(lambda: any(s['token'] == session['token'] and s['phase'] == 'ended' and not s['profile_held']
            for s in request('get_game_sessions')['sessions']), 'normal game exit did not end tracking')
        wait(lambda: request('get_profile') == 'balanced', 'normal game exit retained the private hold')
        assert json.loads(user_run('/usr/bin/omen-cli', 'game-launch-issues', '--json').stdout) == []
        capture('openttd-normal-exit')
        passed('game console quit ends the real process and scope, releases the hold and leaves no saved launch issue')

        pid, session, window = live_launch(2)
        user_run('/usr/bin/omen-cli', 'game-session-release', session['token'])
        assert request('get_profile') == 'balanced' and game_process() == pid
        assert not any(s['token'] == session['token'] and s['profile_held'] for s in request('get_game_sessions')['sessions'])
        quit_game(pid, window)
        passed('explicit profile release keeps the real game running and normal game exit does not reapply a hold')

        pid, session, window = live_launch(3)
        run('systemctl', 'restart', UNIT)
        wait(lambda: request('get_profile') == 'balanced', 'restart retained the private provider hold')
        assert game_process() == pid
        assert any(s['token'] == session['token'] and s['phase'] == 'interrupted' and not s['profile_held']
            for s in request('get_game_sessions')['sessions'])
        quit_game(pid, window)
        assert request('get_profile') == 'balanced'
        passed('service restart releases the hold, records Interrupted and preserves actual game execution until normal quit')
        data['final_sessions'] = request('get_game_sessions')
        data['result'] = 'passed'
    except BaseException as error:
        data['error'] = type(error).__name__ + ': ' + str(error)
        for path in BASE.glob('launch-*.log'):
            print('Launcher diagnostic', path.name, path.read_text(errors='replace')[-20000:], flush=True)
        try:
            capture('real-game-failure')
        except (AssertionError, OSError):
            pass
        raise
    finally:
        run('systemctl', 'stop', 'sddm.service', check=False)
        run('systemctl', 'stop', UNIT, check=False)
        if keyboard is not None:
            del keyboard
        for stream in logs:
            stream.flush()
        artifacts = {'logs': {p.name: p.read_text(errors='replace')[-100000:] for p in BASE.glob('*.log')},
            'lutris_log': (home / '.cache/lutris/lutris.log').read_text(errors='replace')[-100000:]
                if (home / '.cache/lutris/lutris.log').is_file() else None,
            'game_config': (home / '.config/openttd/openttd.cfg').read_text()
                if (home / '.config/openttd/openttd.cfg').exists() else None}
        Path('/var/tmp/omen-real-game-artifacts.json').write_text(json.dumps(artifacts, indent=2) + '\n')
        shutil.rmtree(OVERRIDE, ignore_errors=True)
        RULE.unlink(missing_ok=True)
        JOURNAL.unlink(missing_ok=True)
        for path in reversed(owned):
            path.unlink(missing_ok=True)
        run('systemctl', 'daemon-reload', check=False)
        run('systemctl', 'start', UNIT, check=False)
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=10)
        for stream in logs:
            stream.close()
        shutil.rmtree(BASE)
        HELPER.unlink(missing_ok=True)
        data['fixture_cleanup_complete'] = True
        state['real_game'] = data
        if data['result'] == 'passed':
            state['checks_passed'].extend(data['checks_passed'])
        RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
