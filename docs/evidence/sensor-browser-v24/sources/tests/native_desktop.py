"""Private native desktop fixtures. Keyboard events never target the host display.

Requires an existing Wayland session, rootful Xwayland, xauth and libXtst.
Own D-Bus, accessibility bus, configurations and X authorization are temporary.
"""
import ctypes as C
import ast
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


class Desktop:
    def __init__(self, out):
        self.out = Path(out)
        self.out.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(prefix='omen-native-', dir=ROOT / 'target')
        self.root = Path(self.directory.name)
        self.children, self.logs, self.keys = [], [], []
        for name in ['user', 'system', 'tmp', 'config', 'data', 'cache', 'runtime']:
            (self.root / name).mkdir(mode=0o700)
        self.env = dict(os.environ, OMEN_CONFIG_DIR=str(self.root / 'user'),
                        OMEN_SYSTEM_CONFIG_DIR=str(self.root / 'system'), OMEN_HOME=str(self.root / 'home'),
                        TMPDIR=str(self.root / 'tmp'), XDG_CONFIG_HOME=str(self.root / 'config'),
                        XDG_DATA_HOME=str(self.root / 'data'), XDG_CACHE_HOME=str(self.root / 'cache'),
                        XDG_RUNTIME_DIR=str(self.root / 'runtime'),
                        GSETTINGS_BACKEND='memory', GDK_DEBUG='no-portals', GTK_A11Y='atspi',
                        OMEN_REPORT_READY='1', LC_ALL='C.UTF-8',
                        DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(self.root / 'no-system-bus'),
                        SPEECHD_ADDRESS='unix_socket:' + str(self.root / 'no-speech-server'), SPEECHD_CMD='/bin/false')
        self.display = None
        self.previous_authority = os.environ.get('XAUTHORITY')

    def launch(self, command, name, pipe=False, extra=None):
        log = (self.root / (name + '.log')).open('w')
        self.logs.append(log)
        child = subprocess.Popen(command, env=dict(self.env, **(extra or {})), text=True,
                                 stdout=subprocess.PIPE if pipe else log, stderr=log, start_new_session=True)
        self.children.append(child)
        return child

    def __enter__(self):
        try:
            self._start()
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def _start(self):
        assert os.geteuid() != 0 and os.environ.get('WAYLAND_DISPLAY')
        bus_config = self.root / 'session.conf'
        bus_config.write_text('<busconfig><type>session</type><listen>unix:path=' + str(self.root / 'session')
            + '</listen><auth>EXTERNAL</auth><policy context="default"><allow user="' + str(os.getuid())
            + '"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/>'
            + '<allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/>'
            + '</policy></busconfig>')
        bus = self.launch(['dbus-daemon', '--config-file=' + str(bus_config), '--nofork', '--print-address=1'], 'session', True)
        self.env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
        assert self.env['DBUS_SESSION_BUS_ADDRESS'].startswith('unix:')
        self.env.pop('AT_SPI_BUS_ADDRESS', None)
        number = next(n for n in range(220 + os.getpid() % 5000, 20000)
                      if not Path(f'/tmp/.X{n}-lock').exists() and not Path(f'/tmp/.X11-unix/X{n}').exists())
        self.env['DISPLAY'] = f':{number}'
        authority = self.root / 'authority'
        authority.touch(mode=0o600)
        self.env['XAUTHORITY'] = str(authority)
        os.environ['XAUTHORITY'] = str(authority)
        subprocess.run(['xauth', '-f', str(authority)], input=f'add :{number} . {secrets.token_hex(16)}\n',
                       env=self.env, capture_output=True, text=True, check=True)
        self.xserver = self.launch(['Xwayland', f':{number}', '-geometry', '1400x1100', '-decorate',
            '-nolisten', 'tcp', '-noreset', '-shm', '-auth', str(authority)], 'xwayland',
            extra={'GDK_BACKEND': 'wayland', 'LIBGL_ALWAYS_SOFTWARE': '1', 'NO_AT_BRIDGE': '1',
                   'XDG_RUNTIME_DIR': os.environ['XDG_RUNTIME_DIR']})
        self.x = C.CDLL('libX11.so.6')
        self.xt = C.CDLL('libXtst.so.6')
        self.x.XOpenDisplay.argtypes = [C.c_char_p]
        self.x.XOpenDisplay.restype = C.c_void_p
        self.x.XCloseDisplay.argtypes = [C.c_void_p]
        self.x.XDefaultRootWindow.argtypes = [C.c_void_p]
        self.x.XDefaultRootWindow.restype = C.c_ulong
        self.x.XQueryTree.argtypes = [C.c_void_p, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_ulong),
                                      C.POINTER(C.POINTER(C.c_ulong)), C.POINTER(C.c_uint)]
        self.x.XFetchName.argtypes = [C.c_void_p, C.c_ulong, C.POINTER(C.c_void_p)]
        self.x.XFree.argtypes = [C.c_void_p]
        self.x.XSetInputFocus.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_ulong]
        self.x.XGetInputFocus.argtypes = [C.c_void_p, C.POINTER(C.c_ulong), C.POINTER(C.c_int)]
        self.x.XSync.argtypes = [C.c_void_p, C.c_int]
        self.x.XResizeWindow.argtypes = [C.c_void_p, C.c_ulong, C.c_uint, C.c_uint]
        self.x.XStringToKeysym.argtypes = [C.c_char_p]
        self.x.XStringToKeysym.restype = C.c_ulong
        self.x.XKeysymToKeycode.argtypes = [C.c_void_p, C.c_ulong]
        self.x.XKeysymToKeycode.restype = C.c_uint
        self.x.XFlush.argtypes = [C.c_void_p]
        self.xt.XTestFakeKeyEvent.argtypes = [C.c_void_p, C.c_uint, C.c_int, C.c_ulong]
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.display = self.x.XOpenDisplay(self.env['DISPLAY'].encode())
            if self.display:
                break
            assert self.xserver.poll() is None, 'private Xwayland stopped'
            time.sleep(.05)
        assert self.display, 'private X display did not become ready'
        self.env.update(GDK_BACKEND='x11', XDG_SESSION_TYPE='x11', WAYLAND_DISPLAY='')
        self.launch(['/usr/libexec/at-spi-bus-launcher', '--launch-immediately', '--a11y=1', '--screen-reader=1'], 'a11y')
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            address = subprocess.run(['gdbus', 'call', '--session', '--dest', 'org.a11y.Bus', '--object-path',
                '/org/a11y/bus', '--method', 'org.a11y.Bus.GetAddress'], env=self.env, text=True, capture_output=True)
            if address.returncode == 0:
                self.env['AT_SPI_BUS_ADDRESS'] = ast.literal_eval(address.stdout)[0]
                break
            time.sleep(.05)
        assert self.env.get('AT_SPI_BUS_ADDRESS', '').startswith('unix:'), 'private accessibility bus missing'
        os.environ['AT_SPI_BUS_ADDRESS'] = self.env['AT_SPI_BUS_ADDRESS']
        import gi
        gi.require_version('Atspi', '2.0')
        from gi.repository import Atspi, GLib
        self.Atspi, self.GLib = Atspi, GLib
        Atspi.init()
        Atspi.set_timeout(2000, 5000)
        self.context = GLib.MainContext.default()

    def drain(self, seconds=.08):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            while self.context.pending():
                self.context.iteration(False)
            time.sleep(.01)

    def wait(self, predicate, message, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.drain(.03)
            try:
                result = predicate()
                if result:
                    return result
            except (self.GLib.Error, StopIteration):
                pass
        raise RuntimeError(message)

    def attach(self, process):
        def find():
            desktop = self.Atspi.get_desktop(0)
            return next(n for i in range(desktop.get_child_count())
                        if (n := desktop.get_child_at_index(i)) is not None and n.get_process_id() == process.pid)
        app = self.wait(find, 'own GUI missing from accessibility bus')
        app.set_cache_mask(self.Atspi.Cache.NONE)
        app.clear_cache()
        return app

    def walk(self, node):
        if node is None:
            return
        try:
            if node.get_state_set().contains(self.Atspi.StateType.DEFUNCT):
                return
            yield node
            for i in range(node.get_child_count()):
                yield from self.walk(node.get_child_at_index(i))
        except self.GLib.Error:
            return

    def focus_window(self, title='OMEN Control'):
        root = self.x.XDefaultRootWindow(self.display)
        root_ret, parent, children, count = C.c_ulong(), C.c_ulong(), C.POINTER(C.c_ulong)(), C.c_uint()
        assert self.x.XQueryTree(self.display, root, C.byref(root_ret), C.byref(parent), C.byref(children), C.byref(count))
        matches = []
        for n in range(count.value):
            name = C.c_void_p()
            if self.x.XFetchName(self.display, children[n], C.byref(name)) and name:
                if C.string_at(name).decode(errors='replace') == title:
                    matches.append(children[n])
                self.x.XFree(name)
        self.x.XFree(children)
        assert len(matches) == 1, (title, matches)
        self.window = matches[0]
        self.x.XSetInputFocus(self.display, matches[0], 2, 0)
        self.x.XSync(self.display, 0)
        self.drain(.2)
        focus, revert = C.c_ulong(), C.c_int()
        self.x.XGetInputFocus(self.display, C.byref(focus), C.byref(revert))
        assert focus.value == matches[0], ('private X input focus differs', focus.value, matches[0])

    def resize(self, width, height):
        assert self.display and self.xserver.poll() is None
        self.x.XResizeWindow(self.display, self.window, width, height)
        self.x.XFlush(self.display)
        self.drain(.5)

    def key(self, name, *modifiers, drain=True):
        # This handle is exclusively the new authenticated Xwayland server.
        assert self.display and self.xserver.poll() is None
        codes = [self.x.XKeysymToKeycode(self.display, self.x.XStringToKeysym(k.encode())) for k in (*modifiers, name)]
        assert all(codes), (name, modifiers)
        for code in codes:
            assert self.xt.XTestFakeKeyEvent(self.display, code, 1, 0)
        for code in reversed(codes):
            assert self.xt.XTestFakeKeyEvent(self.display, code, 0, 0)
        self.x.XFlush(self.display)
        self.keys.append('+'.join((*modifiers, name)))
        if drain:
            self.drain()

    def type_text(self, text, paced=True):
        symbols = {'/': 'slash', '.': 'period', '-': 'minus', '_': 'underscore', ' ': 'space', ':': 'colon'}
        for char in text:
            assert char.isascii() and (char.isalnum() or char in symbols), char
            name = symbols.get(char, char)
            self.key(name, *(['Shift_L'] if char.isupper() or char in '_:' else []), drain=paced)
        if not paced:
            self.drain(.2)

    def tab_to(self, app, predicate, message, limit=100):
        trail = []
        for _ in range(limit):
            node = self.focused(app)
            if node is not None and predicate(node):
                return node
            trail.append(self.describe_focus(app))
            self.key('Tab')
        raise RuntimeError(message + ': ' + str(trail))

    def focused(self, app):
        return next((n for n in self.walk(app) if n.get_state_set().contains(self.Atspi.StateType.FOCUSED)), None)

    def describe_focus(self, app):
        node = self.focused(app)
        return None if node is None else {'role': node.get_role_name(), 'name': node.get_name()}

    def __exit__(self, *_args):
        if self.display:
            self.x.XCloseDisplay(self.display)
            self.display = None
        for child in reversed(self.children):
            if child.poll() is not None:
                continue
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            if child.poll() is None:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)
        for log in self.logs:
            log.close()
        for path in self.root.glob('*.log'):
            shutil_path = self.out / path.name
            shutil_path.write_bytes(path.read_bytes())
        self.directory.cleanup()
        if self.previous_authority is None:
            os.environ.pop('XAUTHORITY', None)
        else:
            os.environ['XAUTHORITY'] = self.previous_authority
