#!/usr/bin/env python3
"""Large native library benchmark and interaction regression on private fixtures."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import signal
import shutil
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = ROOT / 'target' / PROFILE
GUI = Path(os.environ.get('OMEN_GUI_BIN', BIN / 'omen-gui'))
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/library-ui'))
COUNT = int(os.environ.get('OMEN_LIBRARY_COUNT', '10000'))
BENCH = os.environ.get('OMEN_LIBRARY_BENCH_ONLY') == '1'


def main():
    assert os.geteuid() != 0 and 1 <= COUNT <= 10000
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'count': COUNT, 'profile': PROFILE, 'benchmark_only': BENCH,
              'gui_sha256': hashlib.file_digest(GUI.open('rb'), 'sha256').hexdigest(),
              'checks_passed': [], 'requested_backend': os.environ.get('GDK_BACKEND', 'auto')}
    with tempfile.TemporaryDirectory(prefix='omen-library-ui-', dir=ROOT / 'target') as directory:
        temp = Path(directory)
        for name in ['user', 'system', 'tmp']:
            (temp / name).mkdir()
        config = temp / 'user/config.toml'
        def preferences(count):
            header = 'version = 1\n[general]\nonboarding_dismissed = true\npoll_interval_ms = 500\n'
            games = ''.join('[[games.custom_games]]\nid = "custom/%05d"\nname = "Library fixture %05d"\nsource = "custom"\nlaunch_cmd = %s\n' %
                            (i, i, json.dumps('/usr/bin/touch ' + str(temp / ('launched-%05d' % i)))) for i in range(count))
            return header + ('\n[games.per_game_profiles]\n"custom/%05d" = "performance"\n' % (count - 1) if not BENCH else '') + games
        config.write_text(preferences(COUNT))
        children, logs = [], []
        env = dict(os.environ, OMEN_CONFIG_DIR=str(config.parent), OMEN_SYSTEM_CONFIG_DIR=str(temp / 'system'),
                   OMEN_HOME=str(temp / 'home'), OMEN_REPORT_READY='1', TMPDIR=str(temp / 'tmp'),
                   GSETTINGS_BACKEND='memory', GTK_A11Y='atspi', GDK_DEBUG='no-portals', LC_ALL='C.UTF-8',
                   XDG_CONFIG_HOME=str(temp / 'xdg-config'), XDG_DATA_HOME=str(temp / 'xdg-data'), XDG_CACHE_HOME=str(temp / 'cache'))
        def launch(command, name, pipe=False):
            log = (temp / (name + '.log')).open('w');logs.append(log)
            child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE if pipe else log, stderr=log, text=True, start_new_session=True)
            children.append(child)
            return child
        gui = None
        try:
            bus_config = temp / 'session.conf'
            bus_config.write_text('<busconfig><type>session</type><listen>unix:path=' + str(temp / 'session') + '</listen><auth>EXTERNAL</auth><policy context="default"><allow user="' + str(os.getuid()) + '"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
            bus = launch(['dbus-daemon', '--config-file=' + str(bus_config), '--nofork', '--print-address=1'], 'session', True)
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
            env['AT_SPI_BUS_ADDRESS'] = 'unix:path=' + str(temp / 'a11y')
            a11y = launch(['dbus-daemon', '--config-file=/usr/share/defaults/at-spi2/accessibility.conf', '--nofork', '--print-address=1', '--address=' + env['AT_SPI_BUS_ADDRESS']], 'a11y', True)
            assert a11y.stdout.readline().strip()
            os.environ.update({k: env[k] for k in ['DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS']})
            daemon = launch([str(BIN / 'omen-daemon'), '--mock-omen'], 'daemon', True)
            env['OMEN_SOCK'] = daemon.stdout.readline().strip()
            import gi
            gi.require_version('Atspi', '2.0')
            from gi.repository import Atspi, GLib
            Atspi.init();Atspi.set_timeout(500, 1500)
            context = GLib.MainContext.default()
            peak_rss = 0
            def rss():
                nonlocal peak_rss
                if gui is not None and gui.poll() is None:
                    status = Path('/proc/%d/status' % gui.pid).read_text()
                    value = int(next(line.split()[1] for line in status.splitlines() if line.startswith('VmRSS:')))
                    peak_rss = max(peak_rss, value)
                    if value > 1_500_000:
                        raise RuntimeError('own GUI exceeded 1.5 GB benchmark limit')
            def wait(predicate, message, timeout=60):
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    rss()
                    while context.pending():context.iteration(False)
                    try:
                        value = predicate()
                        if value:return value
                    except (GLib.Error, StopIteration):pass
                    time.sleep(.05)
                raise RuntimeError(message)
            started = time.monotonic()
            gui = launch([str(GUI), '--new-instance', '--page', 'games'], 'gui')
            def own():
                desktop = Atspi.get_desktop(0)
                return next(n for i in range(desktop.get_child_count()) if (n := desktop.get_child_at_index(i)) is not None and n.get_process_id() == gui.pid)
            app = wait(own, 'own application unavailable')
            app.set_cache_mask(Atspi.Cache.NONE)
            def walk(node, depth=0):
                if node is None or depth > 45:return
                try:
                    if node.get_state_set().contains(Atspi.StateType.DEFUNCT):return
                    yield node
                    for i in range(node.get_child_count()):yield from walk(node.get_child_at_index(i), depth + 1)
                except GLib.Error:return
            def text(fragment):
                return any(n.get_role_name() == 'label' and fragment in n.get_name() for n in walk(app))
            def exact_text(value):
                return any(n.get_role_name() == 'label' and n.get_name() == value for n in walk(app))
            wait(lambda: text('Found %d game' % COUNT), 'large library did not finish scanning')
            result['scan_and_first_accessible_response_seconds'] = time.monotonic() - started
            rss();result['loaded_rss_kib'] = peak_rss
            print('MEASURE', COUNT, result['scan_and_first_accessible_response_seconds'], peak_rss, flush=True)
            def passed(label):result['checks_passed'].append(label);print('PASS', label, flush=True)
            def click(name):
                def find():
                    return next(n for n in walk(app) if n.get_role_name() == 'button' and n.get_name() == name and n.get_state_set().contains(Atspi.StateType.SENSITIVE))
                button = wait(find, 'missing button ' + name)
                showing = button.get_state_set().contains(Atspi.StateType.SHOWING)
                if not showing:
                    bars = [n for n in walk(app) if n.get_role_name() == 'scroll bar'
                            and n.get_state_set().contains(Atspi.StateType.VERTICAL)
                            and n.get_state_set().contains(Atspi.StateType.SHOWING)]
                    for bar in bars:
                        value = bar.get_value_iface()
                        if value is not None:
                            value.set_current_value(value.get_minimum_value() if name == 'Rescan library' else value.get_maximum_value())
                    button = wait(lambda: next(n for n in walk(app) if n.get_role_name() == 'button' and n.get_name() == name
                                               and n.get_state_set().contains(Atspi.StateType.SENSITIVE)
                                               and n.get_state_set().contains(Atspi.StateType.SHOWING)), 'button did not scroll into view ' + name)
                    result.setdefault('scrolled_for_actions', []).append(name)
                iface = button.get_action_iface()
                index = next(i for i in range(iface.get_n_actions()) if iface.get_action_name(i) in ['click', 'press', 'activate'])
                assert iface.do_action(index), 'native action refused ' + name
            if not BENCH:
                search = wait(lambda: next(n for n in walk(app) if n.get_role_name() in ['entry', 'text'] and n.get_editable_text_iface() is not None), 'search unavailable')
                target = 'custom/%05d' % (COUNT - 1)
                filtered_at = time.monotonic()
                assert search.get_editable_text_iface().set_text_contents(target)
                wait(lambda: text('Library fixture %05d' % (COUNT - 1)), 'final game cannot be found', timeout=15)
                result['search_final_entry_seconds'] = time.monotonic() - filtered_at
                before_policy = config.read_bytes()
                daemon.send_signal(signal.SIGSTOP)
                try:
                    wait(lambda: text('Performance (unavailable here)'), 'profile picker did not age its choices', timeout=20)
                    # Recycle native rows while snapshots are stopped. Updater
                    # cleanup must not depend on the next daemon dispatch.
                    for n in range(24):
                        query = 'custom/%05d' % (0 if n % 2 == 0 else COUNT - 1)
                        assert search.get_editable_text_iface().set_text_contents(query)
                        wait(lambda: text('Library fixture %05d' % (0 if n % 2 == 0 else COUNT - 1)),
                             'row recycling stalled during an outage')
                finally:
                    daemon.send_signal(signal.SIGCONT)
                wait(lambda: exact_text('Performance') and exact_text('Fixture'), 'profile picker did not recover current choices with a fresh report', timeout=25)
                assert config.read_bytes() == before_policy
                passed('profile availability follows current capabilities without rewriting the saved binding')
                click('Launch')
                wait(lambda: (temp / ('launched-%05d' % (COUNT - 1))).exists(), 'wrong or missing launch', timeout=15)
                assert len(list(temp.glob('launched-*'))) == 1
                passed('search across the complete library launches the final matching game with its own identity')
                click('Profile setup')
                wait(lambda: text('Custom commands are wrapped automatically'), 'setup missing')
                assert search.get_editable_text_iface().set_text_contents('no-such-library-game')
                wait(lambda: text('No games match this search'), 'empty search not explained', timeout=15)
                assert search.get_editable_text_iface().set_text_contents(target)
                wait(lambda: text('Custom commands are wrapped automatically'), 'expanded setup state was lost on recycling', timeout=15)
                passed('empty search retains controls and returning to a game preserves expanded setup')
                click('Rescan library')
                assert not search.get_state_set().contains(Atspi.StateType.DEFUNCT)
                assert Atspi.Text.get_text(search.get_text_iface(), 0, -1) == target
                wait(lambda: text('Found %d game' % COUNT), 'rescan did not complete', timeout=15)
                wait(lambda: text('Custom commands are wrapped automatically'), 'rescan lost expanded setup', timeout=15)
                passed('rescan preserves the current search and expanded game setup')
                config.write_text(preferences(COUNT - 1))
                click('Rescan library')
                wait(lambda: text('Found %d game' % (COUNT - 1)), 'changed library did not rescan', timeout=15)
                wait(lambda: text('No games match this search'), 'removed game remained visible', timeout=15)
                passed('a removed game disappears after rescan and its old action is not retained')
            result['result'] = 'passed'
        except Exception as error:
            result['error'] = str(error)
            if 'app' in locals():
                try:
                    import itertools
                    result['failure_labels'] = [n.get_name() for n in itertools.islice((n for n in walk(app) if n.get_role_name() == 'label'), 80)]
                except Exception:pass
            raise
        finally:
            result['peak_observed_rss_kib'] = locals().get('peak_rss')
            for child in reversed(children):
                try:os.killpg(child.pid, signal.SIGTERM)
                except ProcessLookupError:pass
                if child.poll() is None:
                    child.terminate()
                    try:child.wait(timeout=8)
                    except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
            for log in logs:log.close()
            for path in temp.glob('*.log'):(OUT / path.name).write_bytes(path.read_bytes())
            for attempt in range(10):
                try:shutil.rmtree(temp);break
                except FileNotFoundError:break
                except OSError:
                    if attempt == 9:raise
                    time.sleep(.1)
            result['cleanup_complete'] = not temp.exists()
            (OUT / 'library-ui.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':main()
