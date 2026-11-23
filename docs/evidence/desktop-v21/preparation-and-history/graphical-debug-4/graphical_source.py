#!/usr/bin/env python3
"""Real KDE/PAM authorization through the installed GUI. Disposable VM only.

No authorization-grant rule is installed. A temporary local VM password is kept
in memory, supplied to chpasswd on stdin and typed through guest-only uinput.
Input characters/password values are never included in logs or result records.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import secrets
import socket
import string
import subprocess
import sys
import time

RESULT = Path('/var/tmp/omen-vm-result.json')
CONFIG = Path('/etc/omen-linux/config.toml')
ACTION = 'com.omenlinux.admin'


def run(*args, check=True, input=None):
    value = subprocess.run(args, input=input, capture_output=True, text=True, timeout=90)
    if input is None:
        print('+', ' '.join(args), value.returncode, value.stdout.strip(), value.stderr.strip(), flush=True)
    else:
        print('+', args[0], '(private stdin)', value.returncode, flush=True)
    if check:
        value.check_returncode()
    return value


def wait(predicate, message, timeout=45):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (FileNotFoundError, ProcessLookupError, StopIteration, ConnectionRefusedError):
            pass
        time.sleep(.12)
    raise RuntimeError(message)


def request(op, args=None):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(8)
        connection.connect('/run/omen-linux/ctl.sock')
        operation = {'op': op}
        if args is not None:
            operation['args'] = args
        connection.sendall(json.dumps({'v': 1, 'id': 1, 'op': operation}).encode() + b'\n')
        reply = json.loads(connection.makefile('rb').readline())
        assert reply['ok'], reply
        return reply['data']


def capture(name):
    print('OMEN_VM_CAPTURE ' + name, flush=True)
    assert sys.stdin.readline().strip() == 'OK', 'private VM capture was not acknowledged'


def worker(account, session, password, keyboard, files, output):
    # Polkit deliberately permits temporary-authorization inspection only from
    # the subject's own session. Move this guest-only observer into the verified
    # local SDDM scope before dropping privileges. The GUI already belongs to
    # the real desktop; its authorization is still exercised through KDE/PAM.
    scope = run('loginctl', 'show-session', session, '-p', 'Scope', '--value').stdout.strip()
    assert scope == 'session-' + session + '.scope'
    cgroup = run('systemctl', 'show', scope, '-p', 'ControlGroup', '--value').stdout.strip()
    assert re.fullmatch('/user.slice/user-' + str(account.pw_uid) + r'\.slice/session-[0-9]+\.scope', cgroup)
    Path('/sys/fs/cgroup' + cgroup + '/cgroup.procs').write_text(str(os.getpid()) + '\n')
    assert Path('/proc/self/cgroup').read_text().strip() == '0::' + cgroup
    os.setgroups([])
    os.setgid(account.pw_gid)
    os.setuid(account.pw_uid)
    os.environ.update(HOME=account.pw_dir, XDG_RUNTIME_DIR='/run/user/' + str(account.pw_uid),
        DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/' + str(account.pw_uid) + '/bus',
        XDG_SESSION_TYPE='wayland', XDG_SESSION_ID=session, QT_LINUX_ACCESSIBILITY_ALWAYS_ON='1')
    os.environ.pop('AT_SPI_BUS_ADDRESS', None)
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi, Gio, GLib
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    address = bus.call_sync('org.a11y.Bus', '/org/a11y/bus', 'org.a11y.Bus', 'GetAddress',
        None, None, Gio.DBusCallFlags.NONE, 10000, None).unpack()[0]
    os.environ['AT_SPI_BUS_ADDRESS'] = address
    Atspi.init()
    Atspi.set_timeout(2500, 5000)
    context = GLib.MainContext.default()
    system = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    state = {'result': 'failed', 'checks_passed': [], 'physical_keyboard_tested': False,
        'input': 'libevdev/uinput in the private guest; no host input devices',
        'password_recorded': False, 'authorization_grant_rule_installed': False, 'session': session,
        'cache_inspector_cgroup': cgroup, 'cache_inspector_uid': os.geteuid()}

    def drain(seconds=.12):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            while context.pending():
                context.iteration(False)
            time.sleep(.01)

    def wait(predicate, message, timeout=45):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            while context.pending():
                context.iteration(False)
            try:
                value = predicate()
                if value:
                    return value
            except (GLib.Error, FileNotFoundError, ProcessLookupError, StopIteration, ConnectionRefusedError):
                pass
            time.sleep(.12)
        raise RuntimeError(message)

    def walk(node):
        if node is None:
            return
        try:
            if node.get_state_set().contains(Atspi.StateType.DEFUNCT):
                return
            yield node
            for index in range(node.get_child_count()):
                yield from walk(node.get_child_at_index(index))
        except GLib.Error:
            return

    def applications():
        drain(.03)
        root = Atspi.get_desktop(0)
        return [root.get_child_at_index(i) for i in range(root.get_child_count())]

    def pid_for(name):
        for path in Path('/proc').iterdir():
            if not path.name.isdigit():
                continue
            try:
                if path.stat().st_uid != account.pw_uid:
                    continue
                executable = (path / 'cmdline').read_bytes().split(b'\0')[0].decode()
                if Path(executable).name == name:
                    return int(path.name)
            except (PermissionError, ProcessLookupError, FileNotFoundError):
                pass

    observed_pids = set()

    def application(name):
        pid = pid_for(name)
        node = next((node for node in applications() if node is not None and node.get_process_id() == pid), None)
        if node is not None and pid not in observed_pids:
            node.set_cache_mask(Atspi.Cache.NONE)
            node.clear_cache()
            observed_pids.add(pid)
        return node

    def showing(node):
        return node.get_state_set().contains(Atspi.StateType.SHOWING)

    gui = wait(lambda: application('omen-gui'), 'installed GUI did not join the desktop accessibility bus')
    gui.set_cache_mask(Atspi.Cache.NONE)
    gui_pid = gui.get_process_id()
    assert Path('/proc/' + str(gui_pid)).stat().st_uid == account.pw_uid
    state['gui_pid'] = gui_pid

    def labels(app=gui):
        return [node.get_name() for node in walk(app) if node.get_role_name() == 'label']

    def find(app, role, name):
        roles = {'button': Atspi.Role.PUSH_BUTTON, 'label': Atspi.Role.LABEL}
        return next((node for node in walk(app) if node.get_role() == roles[role] and node.get_name() == name), None)

    def click(app, name):
        def ready_button():
            node = find(app, 'button', name)
            return node if node and node.get_state_set().contains(Atspi.StateType.SENSITIVE) else None
        node = wait(ready_button, 'native enabled button absent: ' + name)
        component = node.get_component_iface()
        if component is not None:
            try:
                component.scroll_to(Atspi.ScrollType.ANYWHERE)
            except GLib.Error:
                # Qt's bridge need not implement scrolling to an already
                # visible dialog button. The actual action must still succeed.
                pass
        action = node.get_action_iface()
        names = [action.get_action_name(i) for i in range(action.get_n_actions())]
        indices = [i for i, value in enumerate(names) if value.casefold() in ['click', 'press', 'activate']]
        assert indices, 'unsupported native button actions for ' + name + ': ' + repr(names)
        index = indices[0]
        assert action.do_action(index)
        drain()

    def key(name, *modifiers):
        import libevdev
        codes = [getattr(libevdev, 'KEY_' + value.upper()) for value in [*modifiers, name]]
        events = []
        for code in codes:
            events.extend([libevdev.InputEvent(code, 1), libevdev.InputEvent(libevdev.SYN_REPORT, 0)])
        for code in reversed(codes):
            events.extend([libevdev.InputEvent(code, 0), libevdev.InputEvent(libevdev.SYN_REPORT, 0)])
        keyboard.send_events(events)
        drain(.08)

    def type_text(text):
        names = {'/': 'slash', '.': 'dot', '-': 'minus', ' ': 'space'}
        for char in text:
            assert char.isascii() and (char.islower() or char.isdigit() or char in names), 'unsupported fixture input'
            key(names.get(char, char))

    def agent_dialog():
        agent = application('polkit-kde-authentication-agent-1')
        return agent if agent and any(node.get_role() in [Atspi.Role.DIALOG, Atspi.Role.FRAME, Atspi.Role.WINDOW] and showing(node)
            for node in walk(agent)) else None

    def password_field(agent):
        return next((node for node in walk(agent) if node.get_role() == Atspi.Role.PASSWORD_TEXT and showing(node)), None)

    def agent_ready():
        agent = agent_dialog()
        return agent if agent and password_field(agent) is not None else None

    def import_preview(path):
        click(gui, 'Import service policies…')
        picker = wait(lambda: application('xdg-desktop-portal-kde'), 'KDE file portal absent')
        wait(lambda: any(node.get_role() in [Atspi.Role.DIALOG, Atspi.Role.FILE_CHOOSER] and showing(node) for node in walk(picker)), 'KDE file chooser absent')
        # KDE's URL navigator only navigates on Enter. Use the actual Name
        # field (its native Alt+N mnemonic), then explicitly activate Open.
        key('n', 'leftalt')
        drain(.25)
        entry = wait(lambda: next((node for node in walk(picker) if node.get_state_set().contains(Atspi.StateType.FOCUSED)
            and node.get_text_iface() is not None and node.get_role() != Atspi.Role.PASSWORD_TEXT), None), 'portal location input not focused')
        key('a', 'leftctrl')
        type_text(str(path))
        entered = Atspi.Text.get_text(entry.get_text_iface(), 0, -1)
        assert entered == str(path), 'portal input differs from selected fixture path'
        click(picker, 'Open')
        wait(lambda: find(gui, 'button', 'Apply policies'), 'service policy preview absent')
        assert any('Battery automation: disabled' in value for value in labels()), 'fixture must keep automation disabled'

    def submit(path):
        import_preview(path)
        click(gui, 'Apply policies')

    def authority(method):
        subject = ('unix-session', {'session-id': GLib.Variant('s', session)})
        return system.call_sync('org.freedesktop.PolicyKit1', '/org/freedesktop/PolicyKit1/Authority',
            'org.freedesktop.PolicyKit1.Authority', method, GLib.Variant('((sa{sv}))', (subject,)),
            None, Gio.DBusCallFlags.NONE, 10000, None).unpack()

    def cache():
        return [entry for entry in authority('EnumerateTemporaryAuthorizations')[0] if entry[1] == ACTION]

    def passed(text):
        state['checks_passed'].append(text)
        print('PASS', text, flush=True)

    def denied():
        return any('Import service policies:' in value and ('denied' in value.lower() or 'permission' in value.lower()) for value in labels())

    try:
        authority('RevokeTemporaryAuthorizations')
        assert not cache()
        assert not request('get_info')['mock']
        assert request('get_service_settings')['power']['battery_aware'] is False
        before = request('get_service_settings')
        import_preview(files[0])
        click(gui, 'Cancel')
        wait(lambda: 'Service policy import cancelled' in labels(), 'preview cancellation feedback absent')
        assert request('get_service_settings') == before and not agent_dialog()
        passed('installed Wayland GUI uses the real KDE file portal and canceling its disabled-policy preview performs no service write')

        submit(files[0])
        agent = wait(agent_ready, 'real KDE authentication prompt absent')
        capture('polkit-approval-prompt')
        assert request('get_service_settings') == before
        click(agent, 'Cancel')
        wait(denied, 'canceled authorization did not report denial in the GUI')
        assert request('get_service_settings') == before and not cache()
        passed('canceling the real polkit dialog leaves service policies unchanged and returns GUI error feedback')

        submit(files[0])
        agent = wait(agent_ready, 'authentication prompt absent for rejection case')
        field = password_field(agent)
        assert field.get_component_iface().grab_focus()
        type_text('incorrectfixturepassword')
        key('enter')
        drain(3)
        assert request('get_service_settings') == before
        if agent_dialog():
            capture('polkit-rejected-password')
            click(agent, 'Cancel')
        wait(denied, 'rejected authentication did not finish with GUI denial')
        assert not cache()
        passed('an incorrect VM-only password cannot authorize policy import; subsequent cancellation preserves configuration')

        submit(files[0])
        agent = wait(agent_ready, 'authentication prompt absent for approval case')
        field = password_field(agent)
        assert field.get_component_iface().grab_focus()
        type_text(password)
        key('enter')
        wait(lambda: request('get_service_settings')['power']['on_ac_profile'] == 'unchanged', 'approved GUI import was not applied')
        retained = wait(cache, 'active desktop approval was not retained by the packaged policy')
        assert not agent_dialog()
        assert request('get_service_settings')['power']['battery_aware'] is False
        state['retained_authorization'] = {'action': retained[0][1], 'obtained': retained[0][3], 'expires': retained[0][4],
            'id_sha256': hashlib.sha256(retained[0][0].encode()).hexdigest()}
        passed('the real KDE/PAM prompt authorizes the installed GUI, saves disabled automation and creates a temporary authorization')

        submit(files[1])
        wait(lambda: request('get_service_settings')['power']['on_ac_profile'] == 'balanced', 'retained authorization did not admit the second GUI import')
        assert not agent_dialog() and cache()
        capture('polkit-cached-policy-import')
        passed('a second GUI import uses the observed active-session cache without a new authentication dialog')

        deadline = time.monotonic() + 360
        while cache():
            assert time.monotonic() < deadline, 'temporary authorization exceeded the observed expiry window'
            print('Authorization cache still present; waiting for natural expiry', flush=True)
            drain(10)
        state['cache_expired_at'] = time.time()
        submit(files[0])
        agent = wait(agent_ready, 'expired authorization did not require a fresh real prompt')
        capture('polkit-expired-authorization')
        assert request('get_service_settings')['power']['on_ac_profile'] == 'balanced'
        click(agent, 'Cancel')
        wait(denied, 'post-expiry cancellation feedback absent')
        assert request('get_service_settings')['power']['on_ac_profile'] == 'balanced' and not cache()
        passed('natural cache expiry requires a fresh real dialog and canceling it cannot mutate service policy')
        state['result'] = 'passed'
    except BaseException as error:
        state['error'] = type(error).__name__ + ': ' + str(error)
        state['gui_labels'] = labels()[:50]
        raise
    finally:
        output.write_text(json.dumps(state, indent=2) + '\n')


def main():
    assert os.geteuid() == 0 and os.environ.get('OMEN_GRAPHICAL_VM') == '1'
    assert run('systemd-detect-virt', '--vm').stdout.strip() in ['kvm', 'qemu']
    assert RESULT.is_file() and not list(Path('/etc/polkit-1/rules.d').glob('*omen*'))
    state = json.loads(RESULT.read_text())
    state.pop('graphical_authorization', None)
    state['graphical_authorization_source_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    account = pwd.getpwnam('ubuntu')
    home = Path(account.pw_dir)
    original = CONFIG.read_bytes()
    old_hash = next(line.split(':')[1] for line in Path('/etc/shadow').read_text().splitlines() if line.startswith('ubuntu:'))
    password = ''.join(secrets.choice(string.ascii_lowercase + string.digits) for _ in range(20))
    run('chpasswd', input='ubuntu:' + password + '\n')
    owned = []
    keyboard = None

    def user_file(path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(0o600)
        os.chown(path, account.pw_uid, account.pw_gid)
        for parent in path.parents:
            if parent == home:
                break
            if str(parent).startswith(str(home) + '/'):
                os.chown(parent, account.pw_uid, account.pw_gid)
        owned.append(path)

    try:
        run('systemctl', 'stop', 'sddm.service')
        settings = request('get_service_settings')
        settings['power'].update(battery_aware=False, on_ac_profile='balanced')
        request('set_service_settings', {'settings': settings})
        files = [home / 'policies/safe-a.json', home / 'policies/safe-b.json']
        for path, profile in zip(files, ['unchanged', 'balanced']):
            data = copy.deepcopy(settings)
            data['power']['on_ac_profile'] = profile
            user_file(path, json.dumps({'scope': 'service', 'settings': data}))
        user_file(home / '.config/omen-linux/config.toml', 'version=1\n[general]\nonboarding_dismissed=true\n')
        user_file(home / '.config/plasma-workspace/env/omen-test.sh',
            'export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 GTK_A11Y=atspi GTK_USE_PORTAL=1 LIBGL_ALWAYS_SOFTWARE=1 QT_QUICK_BACKEND=software\n')
        user_file(home / '.config/autostart/omen-test.desktop', '[Desktop Entry]\nType=Application\nName=OMEN validation\n'
            'Exec=/usr/bin/env OMEN_REPORT_READY=1 GTK_A11Y=atspi GTK_USE_PORTAL=1 /usr/bin/omen-gui --new-instance --page settings\n')
        sessions = [p for p in Path('/usr/share/wayland-sessions').glob('*.desktop') if 'startplasma-wayland' in p.read_text()]
        assert len(sessions) == 1
        conf = Path('/etc/sddm.conf.d/10-omen-test.conf')
        conf.parent.mkdir(exist_ok=True)
        conf.write_text('[Autologin]\nUser=ubuntu\nSession=' + sessions[0].name + '\nRelogin=false\n'
            '[Theme]\nCurrent=breeze\n[X11]\nServerPath=/usr/lib/xorg/Xorg\n')
        owned.append(conf)
        run('modprobe', 'uinput')
        import libevdev
        device = libevdev.Device()
        device.name = 'omen-private-vm-keyboard'
        for name in [*string.ascii_uppercase, *string.digits, 'LEFTCTRL', 'LEFTSHIFT', 'LEFTALT', 'TAB', 'ENTER',
                'ESC', 'BACKSPACE', 'SPACE', 'SLASH', 'DOT', 'MINUS', 'HOME', 'END', 'UP', 'DOWN', 'F6']:
            device.enable(getattr(libevdev, 'KEY_' + name))
        keyboard = device.create_uinput_device()
        run('systemctl', 'start', 'sddm.service')
        def local_session():
            value = run('loginctl', 'show-user', 'ubuntu', '-p', 'Display', '--value', check=False).stdout.strip()
            if value:
                info = run('loginctl', 'show-session', value, '-p', 'Type', '-p', 'Active', '-p', 'Remote', check=False).stdout
                if 'Type=wayland' in info and 'Active=yes' in info and 'Remote=no' in info:
                    return value
        session = wait(local_session, 'SDDM did not create an active local Plasma Wayland session', timeout=100)
        capture('plasma-session-started')
        output = home / 'policies/graphical-result.json'
        output.unlink(missing_ok=True)
        owned.append(output)
        child = os.fork()
        if child == 0:
            try:
                worker(account, session, password, keyboard, files, output)
            except BaseException:
                import traceback
                traceback.print_exc()
                os._exit(1)
            os._exit(0)
        _, status = os.waitpid(child, 0)
        data = json.loads(output.read_text()) if output.exists() else {'result': 'failed', 'error': 'worker produced no result'}
        state['graphical_authorization'] = data
        RESULT.write_text(json.dumps(state, indent=2) + '\n')
        assert os.waitstatus_to_exitcode(status) == 0 and data['result'] == 'passed', 'graphical authorization did not pass'
        state['checks_passed'].extend(data['checks_passed'])
    except BaseException as error:
        state.setdefault('graphical_authorization', {}).setdefault('error', type(error).__name__ + ': ' + str(error))
        try:
            capture('graphical-failure')
        except (AssertionError, OSError):
            pass
        raise
    finally:
        run('systemctl', 'stop', 'sddm.service', check=False)
        if keyboard is not None:
            del keyboard
        for path in reversed(owned):
            path.unlink(missing_ok=True)
        run('chpasswd', '-e', input='ubuntu:' + old_hash + '\n')
        CONFIG.write_bytes(original)
        CONFIG.chmod(0o600)
        run('systemctl', 'restart', 'omen-linux.service', check=False)
        state.setdefault('graphical_authorization', {})['fixture_cleanup_complete'] = True
        RESULT.write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
