#!/usr/bin/env python3
"""New launcher discovery/routing and optional upstream offline execution.

All store records, accounts, game executables, buses and PPD state are private
fixtures. No launcher installation, login, game download or physical write.
OMEN_LEGENDARY_SOURCE selects an unmodified pinned upstream checkout; its Python
dependencies must already be available, optionally via OMEN_LEGENDARY_DEPENDENCIES.
"""
import hashlib
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import subprocess
import tempfile
import time

from game_format_fixtures import legendary, put, steam

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = Path(os.environ.get('OMEN_TEST_BIN_DIR', ROOT / 'target' / PROFILE))
TEMP_ROOT = Path(os.environ.get('OMEN_TEST_TEMP_DIR', ROOT / 'target'))
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/game-formats'))


def call(endpoint, operation, args=None):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(6)
        stream.connect(str(endpoint))
        op = {'op': operation}
        if args is not None:
            op['args'] = args
        stream.sendall(json.dumps({'v': 1, 'id': 1, 'op': op}).encode() + b'\n')
        response = json.loads(stream.makefile('rb').readline())
        assert response['ok'], response
        return response['data']


def wait(predicate, message, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.04)
    raise RuntimeError(message)


def main():
    assert os.geteuid() != 0
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'system_power_bus_used': False, 'profile': PROFILE, 'checks_passed': [],
              'cli_sha256': hashlib.file_digest((BIN / 'omen-cli').open('rb'), 'sha256').hexdigest(),
              'daemon_sha256': hashlib.file_digest((BIN / 'omen-daemon').open('rb'), 'sha256').hexdigest(),
              'upstream_legendary_tested': False}
    with tempfile.TemporaryDirectory(prefix='omen-formats-', dir=TEMP_ROOT) as directory:
        temp = Path(directory)
        home, user, system, bins = [temp / name for name in ['home', 'user', 'system', 'bin']]
        for path in [home, user, system, bins, temp / 'tmp']:
            path.mkdir(mode=0o700)
        native = home / '.steam/steam'
        flatpak = home / '.var/app/com.valvesoftware.Steam/.local/share/Steam'
        snap = home / 'snap/steam/common/.local/share/Steam'
        for root, title in [(native, 'Native shortcut'), (flatpak, 'Flatpak shortcut'), (snap, 'Snap shortcut')]:
            steam(root, title, store=root != flatpak)
        game = temp / 'game'
        game.mkdir()
        default = home / '.config/legendary'
        selected = temp / 'Selected Legendary'
        decoy = temp / 'Inherited decoy'
        legendary(default, game, {'Fixture': 'Offline fixture', '--literal': 'Literal fixture'})
        legendary(selected, game, {'Other': 'Selected configuration'})
        legendary(decoy, game, {'Wrong': 'Inherited configuration must be ignored'})
        for root in [home / '.config/heroic', home / '.var/app/com.heroicgameslauncher.hgl/config/heroic']:
            put(root / 'sideload_apps/library.json', json.dumps({'games': [
                {'runner': 'sideload', 'app_name': 'local-game', 'title': 'Sideload fixture', 'is_installed': True,
                 'install': {'executable': 'untrusted; metadata must never be executed'}},
                {'runner': 'sideload', 'app_name': 'removed', 'title': 'Removed', 'is_installed': False},
                {'runner': 'sideload', 'app_name': 'unknown', 'title': 'Unknown installed state'},
                {'runner': 'sideload', 'app_name': 'invalid/id', 'title': 'Invalid', 'is_installed': True},
            ]}))
        ids = ['steam/620', 'steam/shortcut/7/4045620583', 'steam/flatpak/shortcut/7/4045620583',
               'steam/snap/620', 'steam/snap/shortcut/7/4045620583',
               'legendary/Fixture', 'legendary/--literal', 'legendary/Other',
               'heroic/sideload/local-game', 'heroic/flatpak/sideload/local-game']
        config = user / 'config.toml'
        preferences = 'version=1\n[games.per_game_profiles]\n' + ''.join(json.dumps(i) + '="performance"\n' for i in ids)
        preferences += '[[games.library_locations]]\nsource="legendary"\npath=' + json.dumps(str(selected)) + '\n'
        config.write_text(preferences)
        route_log = temp / 'routes.jsonl'
        helper_text = """#!/usr/bin/python3
import json,os,sys
if sys.argv[1:]==['--version']:
 print('legendary version "'+os.environ.get('OMEN_FIXTURE_LEGENDARY_VERSION','0.21.1')+'"');sys.exit(0)
with open(os.environ['OMEN_ROUTE_LOG'],'a') as f:
 f.write(json.dumps({'program':sys.argv[0],'args':sys.argv[1:],'uid':os.getuid(),'configuration':os.environ.get('LEGENDARY_CONFIG_PATH')})+'\\n')
sys.exit(37 if os.environ.get('OMEN_FAIL_LAUNCH')=='1' else 0)
"""
        for binary in ['steam', 'snap', 'flatpak', 'legendary', 'heroic']:
            put(bins / binary, helper_text)
            (bins / binary).chmod(0o700)
        env = dict(os.environ, HOME=str(home), OMEN_HOME=str(home), OMEN_CONFIG_DIR=str(user),
                   OMEN_SYSTEM_CONFIG_DIR=str(system), TMPDIR=str(temp / 'tmp'), PATH=str(bins),
                   XDG_CONFIG_HOME=str(decoy), LEGENDARY_CONFIG_PATH=str(decoy),
                   XDG_DATA_HOME=str(temp / 'data'), XDG_CACHE_HOME=str(temp / 'cache'),
                   DBUS_SYSTEM_BUS_ADDRESS='unix:path=' + str(temp / 'no-system-bus'), OMEN_ROUTE_LOG=str(route_log))
        children, logs = [], []

        def start(command, name, child_env=None):
            log = (temp / (name + '.log')).open('w')
            logs.append(log)
            child = subprocess.Popen(command, env=child_env or env, stdout=subprocess.PIPE,
                                     stderr=log, text=True, start_new_session=True)
            children.append(child)
            return child

        def passed(description):
            result['checks_passed'].append(description)
            print('PASS', description, flush=True)

        def inventory(child_env=None):
            run = subprocess.run([str(BIN / 'omen-cli'), '--socket', str(temp / 'missing'), 'games', '--json'],
                                 env=child_env or env, text=True, capture_output=True, timeout=10)
            assert run.returncode == 0, run.stderr
            return json.loads(run.stdout)

        def launch(identifier, endpoint, child_env=None, success=True):
            run = subprocess.run([str(BIN / 'omen-cli'), '--socket', str(endpoint), 'game-launch', identifier, '--json'],
                                 env=child_env or env, text=True, capture_output=True, timeout=20)
            assert (run.returncode == 0) == success, (run.stdout, run.stderr)
            return json.loads(run.stdout)

        try:
            listed = inventory()
            assert {g['id'] for g in listed} == set(ids)
            assert next(g for g in listed if g['id'] == 'legendary/Other')['library_root'] == str(selected)
            assert next(g for g in listed if g['id'].startswith('steam/shortcut/'))['library_root'] == str(native)
            assert 'library_root' not in next(g for g in listed if g['id'] == 'steam/620')
            passed('desktop-only JSON inventory finds all routes, keeps provenance and ignores inherited paths under OMEN_HOME')
            overridden = dict(env)
            overridden.pop('OMEN_HOME')
            overridden['LEGENDARY_CONFIG_PATH'] = str(decoy)
            found = {g['id'] for g in inventory(overridden)}
            assert 'legendary/Wrong' in found and 'legendary/Fixture' not in found
            passed('the real-home Legendary override is honored without merging a different default configuration')
            daemon = start([str(BIN / 'omen-daemon'), '--mock-omen'], 'mock-daemon')
            endpoint = daemon.stdout.readline().strip()
            assert call(endpoint, 'get_info')['mock']
            env['OMEN_SOCK'] = endpoint
            encoded = '17375808096043008000'
            for identifier, binary, args, root in [
                ('steam/shortcut/7/4045620583', 'steam', ['steam://rungameid/' + encoded], None),
                ('steam/flatpak/shortcut/7/4045620583', 'flatpak', ['run', 'com.valvesoftware.Steam', 'steam://rungameid/' + encoded], None),
                ('steam/snap/620', 'snap', ['run', 'steam', 'steam://rungameid/620'], None),
                ('steam/snap/shortcut/7/4045620583', 'snap', ['run', 'steam', 'steam://rungameid/' + encoded], None),
                ('legendary/--literal', 'legendary', ['launch', '--', '--literal'], default),
                ('legendary/Other', 'legendary', ['launch', '--', 'Other'], selected),
                ('heroic/sideload/local-game', 'heroic', ['heroic://launch/sideload/local-game'], None),
                ('heroic/flatpak/sideload/local-game', 'flatpak', ['run', 'com.heroicgameslauncher.hgl', 'heroic://launch/sideload/local-game'], None),
            ]:
                call(endpoint, 'set_profile', {'profile': 'balanced'})
                receipt = launch(identifier, endpoint)
                assert receipt['launch']['profile_was_applied'] and not receipt['launch']['launcher']['game_execution_verified']
                routed = json.loads(route_log.read_text().splitlines()[-1])
                assert routed['program'] == str(bins / binary) and routed['args'] == args and routed['uid'] == os.getuid()
                if root is not None:
                    assert routed['configuration'] == str(root)
                assert call(endpoint, 'get_profile') == 'performance'
                passed(identifier + ': exact route, literal arguments and desktop UID verified')
            call(endpoint, 'set_profile', {'profile': 'balanced'})
            before = route_log.read_bytes()
            (bins / 'snap').rename(bins / 'snap-disabled')
            failure = launch('steam/snap/620', endpoint, success=False)
            assert failure['error']['stage'] == 'preparation' and call(endpoint, 'get_profile') == 'balanced'
            assert route_log.read_bytes() == before
            (bins / 'snap-disabled').rename(bins / 'snap')
            passed('a missing Snap executable fails before profile mutation or command execution')
            steam(native, 'Other account', account=8)
            failure = launch('steam/shortcut/7/4045620583', endpoint, success=False)
            assert failure['error']['stage'] == 'preparation' and call(endpoint, 'get_profile') == 'balanced'
            assert route_log.read_bytes() == before
            steam(native, 'Native shortcut')
            passed('a changed recent Steam account invalidates the old shortcut before profile mutation')
            failure = launch('steam/snap/620', endpoint, dict(env, OMEN_FAIL_LAUNCH='1'), success=False)
            assert failure['error']['stage'] == 'launcher' and call(endpoint, 'get_profile') == 'balanced'
            passed('early Snap submission failure restores only the owned fixture profile change')
            malformed = native / 'userdata/7/config/shortcuts.vdf'
            malformed.write_bytes(malformed.read_bytes()[:-1])
            found = {g['id'] for g in inventory()}
            assert 'steam/620' in found and 'steam/shortcut/7/4045620583' not in found and 'legendary/Other' in found
            passed('a malformed binary shortcut file does not hide independent store or launcher inventories')
            before = route_log.read_bytes()
            failure = launch('legendary/Other', endpoint, dict(env, OMEN_FIXTURE_LEGENDARY_VERSION='0.20.33'), success=False)
            assert failure['error']['stage']=='preparation' and call(endpoint,'get_profile')=='balanced'
            assert route_log.read_bytes()==before
            passed('an older Legendary that ignores configuration routing is rejected before profile mutation')
            source = os.environ.get('OMEN_LEGENDARY_SOURCE')
            if source:
                source = Path(source).resolve()
                dependencies = Path(os.environ['OMEN_LEGENDARY_DEPENDENCIES']).resolve()
                from game_formats_upstream import verify_upstream
                verify_upstream(temp, env, bins, config, default, selected, game, source, dependencies,
                                endpoint, launch, call, start, passed, result, BIN)
            result['result'] = 'passed'
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGTERM)
                    try:
                        child.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
            for log in logs:
                log.close()
            for path in temp.glob('*.log'):
                (OUT / path.name).write_bytes(path.read_bytes())
            (OUT / 'game-formats.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'game-formats.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
