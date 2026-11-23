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
FLATPAK = os.environ.get('OMEN_REAL_FLATPAK_VM') == '1'
GAME_ID = 'lutris/flatpak/1' if FLATPAK else 'lutris/1'
APP = 'net.lutris.Lutris'


def run(*args, check=True, input=None, timeout=40):
    args = tuple(map(str, args))
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
    assert os.geteuid() == 0 and (os.environ.get('OMEN_REAL_GAME_VM') == '1' or FLATPAK)
    assert run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    assert HELPER.is_file() and RESULT.is_file()
    assert not BASE.exists() and not OVERRIDE.exists() and not RULE.exists() and not JOURNAL.exists()
    assert not list(Path('/etc/polkit-1/rules.d').glob('*omen*'))
    state = json.loads(RESULT.read_text())
    data = {'result': 'failed', 'checks_passed': [], 'private_ppd': True,
        'physical_hardware_writes': False, 'physical_keyboard_tested': False,
        'input': 'guest-only XTEST for the game; libevdev/uinput for desktop authorization' if FLATPAK else 'guest-only libevdev/uinput', 'real_game': 'OpenTTD with OpenGFX',
        'launcher': 'signed Flathub Lutris Linux runner' if FLATPAK else 'native Lutris Linux runner', 'store_account_tested': False,
        'source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'helper_sha256': hashlib.file_digest(HELPER.open('rb'), 'sha256').hexdigest(),
        'authorization': 'guest-only com.omenlinux.control grant; no admin grant',
        'installed_service_sandbox': True}
    account = pwd.getpwnam('ubuntu')
    home = Path(account.pw_dir)
    BASE.mkdir(mode=0o755)
    prefs = home / '.config/omen-linux' if FLATPAK else BASE / 'user'
    prefs.mkdir(mode=0o700, parents=True)
    os.chown(prefs, account.pw_uid, account.pw_gid)
    children, owned, logs = [], [], []
    keyboard = None
    mouse = None
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
        state['real_flatpak_game' if FLATPAK else 'real_game'] = data
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        print('PASS', label, flush=True)

    def game_process():
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():
                continue
            try:
                if proc.stat().st_uid != account.pw_uid:
                    continue
                executable = (proc / 'exe').resolve()
                if FLATPAK:
                    # The unmodified distro game uses its copied distro loader.
                    # Linux reports that loader as /proc/PID/exe; require both
                    # its exact private path and the actual game argument.
                    directory = home / '.var/app' / APP / 'data/omen-openttd'
                    argv = (proc / 'cmdline').read_bytes().split(b'\0')
                    if executable.parent == directory / 'lib' and executable.name.startswith('ld-linux-') and str(directory / 'openttd').encode() in argv:
                        return int(proc.name)
                elif executable == Path('/usr/games/openttd'):
                    return int(proc.name)
            except (OSError, RuntimeError):
                pass

    def xprop(*args):
        return user_run('xprop', *args, check=False).stdout

    def game_window(pid):
        clients = re.findall(r'0x[0-9a-f]+', xprop('-root', '_NET_CLIENT_LIST_STACKING'))
        for xid in clients:
            properties = xprop('-id', xid, '_NET_WM_PID', '_NET_WM_NAME', '_NET_WM_STATE')
            namespace_pid = pid
            if FLATPAK:
                match = re.search(r'^NSpid:\s+(.+)$', Path('/proc/' + str(pid) + '/status').read_text(), re.M)
                assert match, 'sandbox process has no namespace identity'
                namespace_pid = int(match[1].split()[-1])
            if re.search(r'_NET_WM_PID\(CARDINAL\) = ' + str(namespace_pid) + r'\b', properties):
                return {'xid': xid, 'properties': properties, 'host_pid': pid, 'namespace_pid': namespace_pid}

    def keys(*names):
        import libevdev
        codes = [getattr(libevdev, 'KEY_' + name) for name in names]
        keyboard.send_events([*[libevdev.InputEvent(code, 1) for code in codes], libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.08)
        keyboard.send_events([*[libevdev.InputEvent(code, 0) for code in reversed(codes)], libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.18)

    def mouse_click(x, y):
        import libevdev
        def move(dx, dy):
            while dx or dy:
                step_x, step_y = max(-128, min(128, dx)), max(-128, min(128, dy))
                mouse.send_events([libevdev.InputEvent(libevdev.EV_REL.REL_X, step_x),
                    libevdev.InputEvent(libevdev.EV_REL.REL_Y, step_y),
                    libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
                dx, dy = dx - step_x, dy - step_y
            time.sleep(.05)
        move(-3000, -3000)
        move(int(x), int(y))
        mouse.send_events([libevdev.InputEvent(libevdev.EV_KEY.BTN_LEFT, 1),
            libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.08)
        mouse.send_events([libevdev.InputEvent(libevdev.EV_KEY.BTN_LEFT, 0),
            libevdev.InputEvent(libevdev.EV_SYN.SYN_REPORT, 0)])
        time.sleep(.18)

    def game_keys(*names):
        # Use the same guest uinput path as the proven native run for both
        # launchers. The Flatpak XTEST path sent ESC+Ctrl+S 12 times in
        # real-flatpak-debug-7..9 and produced no screenshot, while the native
        # uinput path produced one even with the video-settings modal still
        # visible (docs/evidence/real-game-v21). focus() already verifies the
        # game window is active/focused before these keys are sent.
        return keys(*names)

    def focus(window):
        for _ in range(8):
            active = re.findall(r'0x[0-9a-f]+', xprop('-root', '_NET_ACTIVE_WINDOW'))
            # On a mixed Wayland/Xwayland desktop the X root can retain the last
            # X client while a native Wayland window has actual keyboard focus.
            # KWin clears the client's FOCUSED state in that case.
            focused = '_NET_WM_STATE_FOCUSED' in xprop('-id', window['xid'], '_NET_WM_STATE')
            if focused and active and int(active[0], 16) == int(window['xid'], 16):
                return
            keys('LEFTALT', 'TAB')
        raise RuntimeError('real game window did not receive desktop focus')

    def live_launch(index):
        assert not game_process()
        before = {s['token'] for s in request('get_game_sessions')['sessions']}
        error_path = BASE / ('launch-' + str(index) + '.log')
        with error_path.open('w') as errors:
            call = subprocess.run(user_args('/usr/bin/omen-cli', 'game-launch', GAME_ID, '--json'),
                stdout=subprocess.PIPE, stderr=errors, text=True, timeout=25)
        call.check_returncode()
        receipt = json.loads(call.stdout)
        assert receipt['ok'] and receipt['launch']['managed'], receipt
        assert not receipt['launch']['launcher']['game_execution_verified']
        data.setdefault('launch_receipts', []).append(receipt)
        pid = wait(game_process, 'actual OpenTTD process was not launched', timeout=80)
        session = wait(lambda: next((s for s in request('get_game_sessions')['sessions']
            if s['token'] not in before and s['profile_held']), None), 'real game has no verified profile hold')
        assert session['game_id'] == GAME_ID and session['requested_profile'] == 'performance'
        assert request('get_profile') == 'performance'
        cgroup = Path('/proc/' + str(pid) + '/cgroup').read_text().strip()
        assert '/omen-game-' + session['token'] + '.scope' in cgroup, cgroup
        window = wait(lambda: game_window(pid), 'OpenTTD did not map a real game window', timeout=50)
        assert 'OpenTTD' in window['properties']
        focus(window)
        # A mapped SDL window can precede world generation and input readiness.
        # Require the actual game's screenshot hotkey to create a native file
        # before later console commands or recovery actions are sent.
        def game_images():
            roots=[home/'.var/app'/APP,home/'.local/share/openttd',home/'.openttd',home/'.config/openttd'] if FLATPAK else [home]
            return {p for root in roots if root.is_dir() for p in root.rglob('*.png') if any(part.lower().lstrip('.') in ['openttd','omen-openttd'] for part in p.parts)}
        previous_images = game_images()
        native_image = None
        for _ in range(12):
            focus(window)
            game_keys('ESC' if FLATPAK else 'SPACE')  # Dismiss the game's software-rendering notice.
            game_keys('LEFTCTRL', 'S')
            time.sleep(1)
            current_images = game_images() - previous_images
            if current_images:
                native_image = sorted(current_images)[-1]
                break
        assert native_image is not None, 'actual game did not respond with a native screenshot'
        data.setdefault('native_game_screenshots', []).append({'path': str(native_image.relative_to(home)),
            'sha256': hashlib.file_digest(native_image.open('rb'), 'sha256').hexdigest(),
            'bytes': native_image.stat().st_size})
        data.setdefault('game_processes', []).append({'pid': pid, 'uid': Path('/proc/' + str(pid)).stat().st_uid,
            'exe': str(Path('/proc/' + str(pid) + '/exe').resolve()), 'cgroup': cgroup,
            'argv': Path('/proc/' + str(pid) + '/cmdline').read_bytes().decode().split('\0')[:-1],
            'session': session, 'window': window})
        return pid, session, window

    def quit_game(pid, window):
        focus(window)
        # The game's documented console command exits normally, without a kill
        # signal, launcher stop request, or synthetic executable replacing it.
        game_keys('GRAVE')
        for name in 'QUIT':
            game_keys(name)
        game_keys('ENTER')
        wait(lambda: not Path('/proc/' + str(pid)).exists(), 'OpenTTD did not exit through its own console', timeout=35)
        # The native URI exits its launcher too. A Flatpak launcher may keep
        # its application scope alive; only the nested game group must end.
        if not FLATPAK:
            wait(lambda: not any(p.name.isdigit() and p.stat().st_uid == account.pw_uid and
                b'/usr/games/lutris\0' in (p / 'cmdline').read_bytes()
                for p in Path('/proc').iterdir() if (p / 'cmdline').exists()), 'Lutris did not finish its game run', timeout=40)

    try:
        run('systemctl', 'stop', 'sddm.service')
        user_file(prefs / 'config.toml', 'version=1\n[general]\nonboarding_dismissed=true\n'
            '[games]\nmanaged_game_ids=['+json.dumps(GAME_ID)+']\n[games.per_game_profiles]\n'+json.dumps(GAME_ID)+'="performance"\n')
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
        for name in [*string.ascii_uppercase, 'LEFTCTRL', 'LEFTALT', 'TAB', 'ENTER', 'ESC', 'GRAVE', 'SPACE']:
            device.enable(getattr(libevdev, 'KEY_' + name))
        keyboard = device.create_uinput_device()
        mdevice = libevdev.Device()
        mdevice.name = 'omen-private-vm-real-game-mouse'
        for code in [libevdev.EV_REL.REL_X, libevdev.EV_REL.REL_Y, libevdev.EV_KEY.BTN_LEFT]:
            mdevice.enable(code)
        mouse = mdevice.create_uinput_device()
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
        setup = '''import sys
# The installed entry point uses an absolute /usr path for asset discovery.
# Supply that same module search prefix only while creating user configuration.
sys.path[0]='/usr/games'
import gi
gi.require_version('Gtk','3.0')
gi.require_version('Gdk','3.0')
gi.require_version('GdkPixbuf','2.0')
from lutris.startup import init_lutris
from lutris.config import write_game_config
from lutris.database import games
init_lutris()
config=write_game_config('openttd-omen-validation', {
 'game': {'exe':'/usr/games/openttd','args':'-g -r 960x600 -s null -m null -x -c /home/ubuntu/.config/openttd/openttd.cfg'},
 'system': {'disable_runtime':True,'gamemode':False,'prefix_command':'/usr/bin/omen-cli game-run lutris/1 --'}})
game_id=games.add_game(name='OpenTTD',slug='openttd-omen-validation',runner='linux',platform='Linux',
 installed=1,configpath=config,executable='/usr/games/openttd',directory='/usr/games')
assert str(game_id)=='1',game_id
print(config)
'''
        if FLATPAK:
            game_dir = home / '.var/app' / APP / 'data/omen-openttd'
            user_run('mkdir','-p','-m','700',game_dir)
            library_dir = game_dir / 'lib'; library_dir.mkdir()
            shutil.copy2('/usr/games/openttd', game_dir / 'openttd')
            libraries=set()
            for line in run('ldd','/usr/games/openttd').stdout.splitlines():
                match=re.search(r'(/[^ ()]+)',line)
                if match:libraries.add(Path(match[1]))
            for source in libraries:shutil.copy2(source,library_dir/source.name)
            # The game and loader are unmodified distro binaries. Bundle their
            # exact libraries because a Flatpak runtime has a different ABI set.
            loader=next(p for p in libraries if p.name.startswith('ld-linux-'))
            launch=game_dir/'run-game'
            launch.write_text('#!/bin/sh\nexec "'+str(library_dir/loader.name)+'" --library-path "'+str(library_dir)+'" "'+str(game_dir/'openttd')+'" "$@"\n')
            launch.chmod(0o700)
            assets=home/'.var/app'/APP/'data/openttd/baseset';assets.mkdir(parents=True)
            asset_source=Path('/usr/share/games/openttd/baseset')
            dangling=[p for p in asset_source.rglob('*') if p.is_symlink() and not p.exists()]
            # Minimal cloud images can omit package documentation targeted by
            # a shipped changelog symlink. No missing game asset is accepted.
            assert all(p.name=='changelog.txt.gz' for p in dangling),dangling
            data['omitted_dangling_package_documentation']=[str(p) for p in dangling]
            shutil.copytree(asset_source,assets,dirs_exist_ok=True,ignore_dangling_symlinks=True)
            shutil.copytree('/usr/share/games/openttd/lang',game_dir/'lang')
            shutil.copytree('/usr/share/games/openttd/lang',assets.parent/'lang')
            game_cfg=game_dir/'openttd.cfg';game_cfg.write_bytes((home/'.config/openttd/openttd.cfg').read_bytes())
            run('chown','-R','1000:1000',home/'.var/app'/APP)
            setup=setup.replace("sys.path[0]='/usr/games'", "sys.path[0]='/app/lib/lutris';sys.argv[0]='/app/bin/lutris'")
            setup=setup.replace('/usr/games/openttd',str(launch)).replace('/home/ubuntu/.config/openttd/openttd.cfg',str(game_cfg))
            setup=setup.replace("directory='/usr/games'","directory="+repr(str(game_dir)))
            prefix=json.loads(user_run('/usr/bin/omen-cli','game-flatpak-setup',GAME_ID,'--install','--json').stdout)
            assert prefix['ready'],prefix
            setup=setup.replace("'prefix_command':'/usr/bin/omen-cli game-run lutris/1 --'", "'prefix_command':"+repr(prefix['wrapper_command']))
            user_run('flatpak','run','--command=python3',APP,'-c',setup)
            data['flatpak_info']=user_run('flatpak','info','--user',APP).stdout
            data['flatpak_commit']=user_run('flatpak','info','--user','--show-commit',APP).stdout.strip()
            data['bundled_game_inputs']={str(p.relative_to(game_dir)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in game_dir.rglob('*') if p.is_file()}
        else:
            user_run('python3', '-c', setup)
        inventory = json.loads(user_run('/usr/bin/omen-cli', 'games', '--json').stdout)
        assert any(g['id'] == GAME_ID and g['name'] == 'OpenTTD' for g in inventory), inventory
        data['inventory'] = inventory
        passed(('installed OMEN discovers actual Flatpak Lutris database' if FLATPAK else 'installed OMEN discovers actual native Lutris database') + ' and the signed OpenTTD/OpenGFX game is below 2 GB')
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
        focus(window)
        capture('openttd-lutris-windowed')
        passed('production launch URI reaches unmodified Lutris command prefix, actual OpenTTD window and verified user scope with a held Performance profile')
        game_keys('LEFTALT', 'ENTER')
        fullscreen = wait(lambda: '_NET_WM_STATE_FULLSCREEN' in xprop('-id', window['xid'], '_NET_WM_STATE'), 'game fullscreen transition missing')
        assert fullscreen and request('get_profile') == 'performance' and game_process() == pid
        capture('openttd-lutris-fullscreen')
        game_keys('LEFTALT', 'ENTER')
        wait(lambda: '_NET_WM_STATE_FULLSCREEN' not in xprop('-id', window['xid'], '_NET_WM_STATE'), 'game did not leave fullscreen')
        passed('guest keyboard enters and exits actual game fullscreen on Plasma Wayland/Xwayland without losing the profile hold')
        quit_game(pid, window)
        wait(lambda: any(s['token'] == session['token'] and s['phase'] == 'ended' and not s['profile_held']
            for s in request('get_game_sessions')['sessions']), 'normal game exit did not end tracking')
        wait(lambda: request('get_profile') == 'balanced', 'normal game exit retained the private hold')
        assert json.loads(user_run('/usr/bin/omen-cli', 'game-launch-issues', '--json').stdout) == []
        time.sleep(3)  # Allow the separate GUI session-list poll to finish.
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
        if FLATPAK:
            RULE.write_text('polkit.addRule(function(action, subject) { if (subject.user == "ubuntu" && action.id == "com.omenlinux.control") return polkit.Result.AUTH_ADMIN; });\n')
            data['cancellation_policy']='guest-only AUTH_ADMIN challenge for com.omenlinux.control'
            time.sleep(1)  # Allow polkitd to reload the AUTH_ADMIN rule.
            # The CLI blocks on the polkit prompt, so launch it in the
            # background: the harness must probe/cancel the dialog while
            # the CLI is still waiting. A synchronous call here timed out
            # in debug-10 and could never reach the ESC below.
            launch = subprocess.Popen(user_args('/usr/bin/omen-cli','game-launch',GAME_ID,'--json'),
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            children.append(launch)
            probe="""import os,gi,json
gi.require_version('Atspi','2.0')
from gi.repository import Atspi,Gio,GLib
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
os.environ['AT_SPI_BUS_ADDRESS']=bus.call_sync('org.a11y.Bus','/org/a11y/bus','org.a11y.Bus','GetAddress',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
Atspi.init();Atspi.set_timeout(1000,2000)
def walk(n):
 yield n
 for i in range(n.get_child_count()):
  c=n.get_child_at_index(i)
  if c is not None:yield from walk(c)
d=Atspi.get_desktop(0)
print(json.dumps(any(n.get_role()==Atspi.Role.PASSWORD_TEXT and n.get_state_set().contains(Atspi.StateType.SHOWING)
 for i in range(d.get_child_count()) if (a:=d.get_child_at_index(i)) is not None and a.get_name()=='polkit-kde-authentication-agent-1' for n in walk(a))))
"""
            wait(lambda:json.loads(user_run('python3','-c',probe).stdout),'Flatpak profile approval dialog did not appear',timeout=60)
            capture('flatpak-profile-approval-cancel')
            # ESC (debug-11) and the AT-SPI action-click (debug-12) leave
            # this Qt dialog open with the password field focused. Tab to
            # the Cancel button instead, verifying focus at each step so
            # Space cannot activate Details/OK by mistake.
            focused="""import os,gi,json
gi.require_version('Atspi','2.0')
from gi.repository import Atspi,Gio,GLib
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
os.environ['AT_SPI_BUS_ADDRESS']=bus.call_sync('org.a11y.Bus','/org/a11y/bus','org.a11y.Bus','GetAddress',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
Atspi.init();Atspi.set_timeout(1000,2000)
def walk(n):
 yield n
 for i in range(n.get_child_count()):
  c=n.get_child_at_index(i)
  if c is not None:yield from walk(c)
d=Atspi.get_desktop(0)
agent=next(a for i in range(d.get_child_count()) if (a:=d.get_child_at_index(i)) is not None and a.get_name()=='polkit-kde-authentication-agent-1')
node=next((n for n in walk(agent) if n.get_state_set().contains(Atspi.StateType.FOCUSED)),None)
print(json.dumps([node.get_name() if node else '',node.get_role_name() if node else '']))
"""
            for _ in range(8):
                keys('TAB')
                name, role = json.loads(user_run('python3','-c',focused).stdout)
                # get_role_name() returns 'button' for these push buttons
                # (debug-13 reached ['Cancel','button'] but 'push button'
                # never matched); the name alone identifies Cancel here.
                if name == 'Cancel' and role in ('button', 'push button'):
                    break
            else:
                raise RuntimeError('Tab navigation did not focus the polkit Cancel button')
            keys('ENTER')
            # Space/Enter/action-click do not activate this Qt agent
            # (debug-12..15); click its centre with the guest pointer.
            extents="""import os,gi,json
gi.require_version('Atspi','2.0')
from gi.repository import Atspi,Gio,GLib
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
os.environ['AT_SPI_BUS_ADDRESS']=bus.call_sync('org.a11y.Bus','/org/a11y/bus','org.a11y.Bus','GetAddress',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
Atspi.init();Atspi.set_timeout(1000,2000)
def walk(n):
 yield n
 for i in range(n.get_child_count()):
  c=n.get_child_at_index(i)
  if c is not None:yield from walk(c)
d=Atspi.get_desktop(0)
agent=next(a for i in range(d.get_child_count()) if (a:=d.get_child_at_index(i)) is not None and a.get_name()=='polkit-kde-authentication-agent-1')
button=next(n for n in walk(agent) if n.get_name()=='Cancel' and n.get_role_name() in ('button','push button'))
rect=button.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
print(json.dumps([rect.x+rect.width//2,rect.y+rect.height//2]))
"""
            center = json.loads(user_run('python3','-c',extents).stdout)
            mouse_click(center[0], center[1])
            wait(lambda:not json.loads(user_run('python3','-c',probe).stdout),'polkit dialog did not disappear after pointer click on Cancel',timeout=30)
            stdout, stderr = launch.communicate(timeout=40)
            assert launch.returncode == 1, (launch.returncode, stdout, stderr)
            receipt = json.loads(stdout)
            assert receipt['ok'] is False and receipt['error']['stage'] == 'profile_apply', receipt
            assert not game_process()
            data['canceled_launch_receipt'] = receipt
            def canceled():
                reports=json.loads(user_run('/usr/bin/omen-cli','game-launch-issues','--json').stdout)
                return any(r['game_id']==GAME_ID and r['stage']=='registration' for r in reports)
            wait(canceled,'canceled Flatpak authorization was not recorded',timeout=30)
            assert not game_process() and request('get_profile')=='balanced'
            assert not any(s['profile_held'] for s in request('get_game_sessions')['sessions'])
            passed('canceling the real KDE profile prompt prevents the Flatpak game command, records the registration failure and leaves no hold')
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
        if mouse is not None:
            del mouse
        for stream in logs:
            stream.flush()
        artifacts = {'logs': {p.name: p.read_text(errors='replace')[-100000:] for p in BASE.glob('*.log')},
            'lutris_log': (home / ('.var/app/'+APP+'/cache/lutris/lutris.log' if FLATPAK else '.cache/lutris/lutris.log')).read_text(errors='replace')[-100000:]
                if (home / ('.var/app/'+APP+'/cache/lutris/lutris.log' if FLATPAK else '.cache/lutris/lutris.log')).is_file() else None,
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
        state['real_flatpak_game' if FLATPAK else 'real_game'] = data
        if data['result'] == 'passed':
            state['checks_passed'].extend(data['checks_passed'])
        RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
