#!/usr/bin/env python3
"""Actual XTEST keyboard workflows and Orca narration on a private X display.

No AT-SPI action/selection/edit APIs drive the app. AT-SPI only observes it.
X focus selection stands in for the absent WM on the nested test display.
Speech generation is logged; no speech server or audible output is used.
"""
import hashlib
import csv
import json
import os
from pathlib import Path
import subprocess
import tomllib

from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = ROOT / 'target' / PROFILE
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/a11y-keyboard'))


def main():
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'host_keyboard_events': False, 'input': 'XTEST on a new authenticated rootful Xwayland display',
              'screen_reader': 'installed Orca; generated speech log, no audible synthesis',
              'profile': PROFILE, 'gui_sha256': hashlib.file_digest((BIN / 'omen-gui').open('rb'), 'sha256').hexdigest(),
              'checks_passed': []}
    with Desktop(OUT) as d:
        config = d.root / 'user/config.toml'
        marker = d.root / 'game-executed'
        config.write_text('version = 1\n[general]\nonboarding_dismissed = true\npoll_interval_ms = 500\n'
            '[games.per_game_profiles]\n"custom/keyboard" = "performance"\n'
            '[[games.custom_games]]\nid = "custom/keyboard"\nname = "Keyboard fixture"\n'
            'source = "custom"\nlaunch_cmd = "/usr/bin/touch ' + str(marker) + '"\n')
        issues = config.parent / 'launch-issues'
        issues.mkdir(mode=0o700)
        issue_path = issues / 'issues.json'
        issue_path.write_text(json.dumps({'version': 1, 'uid': os.getuid(), 'issues': [{
            'id': '1234567890abcdef1234567890abcdef', 'game_id': 'custom/keyboard', 'scope_token': None,
            'stage': 'scope', 'reason': {'kind': 'scope_setup'}, 'occurred_unix_ms': 1}]}))
        issue_path.chmod(0o600)

        def passed(description):
            result['checks_passed'].append(description)
            print('PASS', description, flush=True)

        def parents(node):
            while node is not None:
                yield node
                node = node.get_parent()

        def focus_name(name):
            return lambda node: node.get_name() == name and node.get_role_name() != 'table cell'

        def text(node):
            return d.Atspi.Text.get_text(node.get_text_iface(), 0, -1)

        try:
            assert not subprocess.run(['pgrep', '-u', str(os.getuid()), '-x', 'orca'], capture_output=True).stdout, 'an existing user Orca instance is running; do not replace it'
            service = d.launch([str(BIN / 'omen-daemon'), '--mock-omen'], 'daemon', True)
            d.env['OMEN_SOCK'] = service.stdout.readline().strip()
            orca = d.launch(['/usr/bin/orca', '--debug-file', str(d.root / 'orca-debug.log')], 'orca')
            d.drain(2)
            assert orca.poll() is None
            gui = d.launch([str(BIN / 'omen-gui'), '--new-instance'], 'gui')
            app = d.attach(gui)
            d.drain(.5)
            d.focus_window()

            def page(name, number):
                d.key(str(number), 'Control_L')
                d.wait(lambda: (d.root / 'gui.log').read_text().split('OMEN_UI_PAGE ')[-1].splitlines()[0] == name,
                       'page shortcut did not select ' + name)

            page('settings', 9)
            d.tab_to(app, focus_name('Compatibility evidence'), 'compatibility disclosure unreachable')
            d.key('Tab')
            assert d.describe_focus(app)['name'] != 'Compatibility evidence', 'collapsed disclosure trapped Tab'
            passed('Tab escapes collapsed selectable disclosure content without entering an unrooted widget')
            d.key('F6')
            assert d.describe_focus(app)['name'] == 'Show or hide navigation'
            d.key('F6')
            assert any(n.get_name() == 'Pages' for n in parents(d.focused(app)))
            d.key('Up')
            d.key('Tab')
            assert not any(n.get_name() == 'Pages' for n in parents(d.focused(app)))
            passed('F6 cycles panes, arrows choose a destination, and Tab enters the selected page')
            for number, name in enumerate(['dashboard', 'vitals', 'performance', 'fan', 'lighting', 'graphics', 'battery', 'games', 'settings'], 1):
                page(name, number)
            passed('Ctrl+1 through Ctrl+9 reach every page using window-local actions')
            page('vitals', 2)
            d.key('f', 'Control_L')
            d.wait(lambda: d.describe_focus(app)['name'] == 'Search sensors', 'sensor search shortcut did not focus its entry')
            d.type_text('no-sensor-matches')
            d.wait(lambda: any(n.get_name() == 'No sensors match this search.' for n in d.walk(app)), 'keyboard search did not filter sensors')
            d.key('a', 'Control_L')
            d.key('BackSpace')
            passed('Ctrl+F names and focuses sensor search; actual typed input filters and clears it')
            network = d.tab_to(app, lambda node: node.get_role_name() == 'label'
                and node.get_name().startswith('Receive: '), 'network readings unreachable by keyboard')
            d.key('a', 'Control_L')
            assert network.get_text_iface().get_n_selections() == 1
            selected_reading = text(network)
            fixture_root = next(path.parent.parent for path in (d.root / 'tmp').glob('*/sys/class')
                                if (path / 'net/fixture0').is_dir())
            counters = fixture_root / 'proc/net/dev'
            original = counters.read_text()
            counters.write_text(original.replace('1048576 100 0 0', '2097152 100 2 3')
                .replace('524288 50 0 0', '1572864 50 4 5'))
            d.wait(lambda: any(n.get_name().startswith('Reading held for selection.')
                and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app)), 'selected reading did not explain its held state')
            d.drain(.8)
            assert text(network) == selected_reading, 'the selected snapshot changed during copying'
            assert d.focused(app) == network, 'holding network readings lost keyboard focus'
            assert network.get_text_iface().get_n_selections() == 1, 'holding network readings cleared text selection'
            selection = d.Atspi.Text.get_selection(network.get_text_iface(), 0)
            assert selection.start_offset == 0 and selection.end_offset == len(text(network))
            d.key('Home', 'Control_L')
            for _ in range(3):
                d.key('Right', 'Shift_L')
            counters.write_text(original.replace('1048576 100 0 0', '3145728 100 6 7')
                .replace('524288 50 0 0', '2621440 50 8 9'))
            d.drain(1)
            selection = d.Atspi.Text.get_selection(network.get_text_iface(), 0)
            assert selection.start_offset == 0 and selection.end_offset == 3
            assert d.focused(app) == network and text(network) == selected_reading
            d.key('Right')
            d.wait(lambda: 'Errors RX/TX: 6 / 8' in network.get_name(), 'clearing selection did not resume current network readings')
            assert not any(n.get_name().startswith('Reading held for selection.')
                and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app))
            d.key('a', 'Control_L')
            d.key('Tab')
            assert d.focused(app) != network, 'network readings trapped Tab'
            d.tab_to(app, lambda node: node == network, 'network reading could not be focused again')
            d.key('a', 'Control_L')
            counters.unlink()
            d.wait(lambda: any(n.get_name().startswith('Network monitoring unavailable:') for n in d.walk(app)), 'missing counters did not clear a held snapshot')
            assert not any(n.get_role_name() == 'label' and n.get_name().startswith('Receive: ') for n in d.walk(app))
            counters.write_text(original)
            d.wait(lambda: any(n.get_role_name() == 'label' and n.get_name().startswith('Receive: ') for n in d.walk(app)), 'network readings did not recover')
            passed('network copy selections keep a named stable snapshot, resume when cleared and release Tab; missing data removes held rows')
            page('dashboard', 1)
            d.tab_to(app, focus_name('Choose metrics'), 'metric chooser unreachable')
            d.key('space')
            d.wait(lambda: any(n.get_role_name() == 'check box' and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app)), 'metrics menu did not open with the keyboard')
            d.key('Escape')
            d.wait(lambda: d.describe_focus(app)['name'] == 'Choose metrics', 'metrics menu did not return focus')
            passed('the native metric chooser opens with Space and Escape returns keyboard focus')
            d.tab_to(app, lambda node: node.get_role_name() == 'toggle button' and any(p.get_role_name() == 'combo box' for p in parents(node)), 'history range unreachable')
            d.key('space')
            d.key('Down')
            d.key('Return')
            d.wait(lambda: any(n.get_role_name() == 'toggle button' and n.get_name() == 'Last minute'
                and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app)), 'range selection did not change to Last minute')
            d.tab_to(app, focus_name('Export CSV…'), 'chart export unreachable')
            d.key('Return')
            d.wait(lambda: any(n.get_role_name() == 'file chooser' or n.get_role_name() == 'dialog' and n.get_name() == 'Export chart history to a new file' for n in d.walk(app)), 'native CSV chooser missing')
            d.focus_window('Export chart history to a new file')
            d.key('Escape')
            d.wait(lambda: not any(n.get_role_name() == 'dialog' and n.get_name() == 'Export chart history to a new file' for n in d.walk(app)), 'CSV chooser did not cancel')
            d.focus_window()
            assert not list(d.root.rglob('*.csv'))
            passed('history range selection and native CSV picker cancellation work from the keyboard')
            d.tab_to(app, focus_name('Export CSV…'), 'CSV export focus was not recovered')
            d.key('Return')
            d.wait(lambda: any(n.get_role_name() == 'dialog' and n.get_name() == 'Export chart history to a new file' for n in d.walk(app)), 'second CSV chooser missing')
            d.focus_window('Export chart history to a new file')
            d.tab_to(app, lambda node: node.get_role_name() in ['text', 'entry'] and node.get_name() in ['Name', 'Name:'], 'CSV name field unreachable')
            destination = d.root / 'keyboard-history.csv'
            d.key('a', 'Control_L')
            d.type_text(str(destination))
            d.key('Return')
            d.wait(destination.exists, 'keyboard CSV export did not write a file')
            with destination.open(newline='') as stream:
                rows = list(csv.reader(stream))
            assert rows[0] == ['series', 'timestamp_unix_ms', 'value', 'source', 'state'] and len(rows) > 1
            assert destination.stat().st_mode & 0o777 == 0o600
            d.focus_window()
            passed('keyboard CSV saving writes actual retained acquisitions with the expected schema and private permissions')
            d.tab_to(app, focus_name('Read history data'), 'history disclosure unreachable')
            d.key('space')
            history = d.tab_to(app, focus_name('History readings'), 'readable history unreachable')
            snapshot = text(history)
            assert 'CPU' in snapshot and 'Acquisition times' in snapshot
            d.drain(1)
            assert text(history) == snapshot
            d.key('a', 'Control_L')
            assert history.get_text_iface().get_n_selections() == 1
            d.key('Tab')
            assert d.focused(app) != history, 'read-only history trapped Tab'
            passed('readable history is keyboard-selectable, stays stable while polling and releases Tab focus')
            page('games', 8)
            d.key('f', 'Control_L')
            d.wait(lambda: d.describe_focus(app)['name'] == 'Search games', 'game search shortcut did not focus its entry')
            d.type_text('custom/keyboard')
            d.tab_to(app, focus_name('Profile setup'), 'profile setup unreachable')
            d.key('space')
            d.tab_to(app, focus_name('Enable automatic restore'), 'mode control unreachable')
            d.key('Return')
            d.wait(lambda: 'custom/keyboard' in tomllib.loads(config.read_text())['games'].get('managed_game_ids', []), 'automatic mode not saved')
            d.key('Return')
            d.wait(lambda: not tomllib.loads(config.read_text())['games'].get('managed_game_ids', []), 'manual mode not restored')
            passed('game search and inline setup can save and reverse the automatic-restore preference using keys')
            d.key('f', 'Control_L')
            d.wait(lambda: d.describe_focus(app)['name'] == 'Search games', 'game header did not regain focus')
            before_clear = config.read_bytes()
            d.tab_to(app, focus_name('Clear launch issues'), 'desktop recovery action unreachable')
            d.key('Return')
            d.wait(lambda: not json.loads(issue_path.read_text())['issues'], 'keyboard reset did not clear saved launch issues')
            assert config.read_bytes() == before_clear and not marker.exists()
            d.wait(lambda: 'Desktop launch issues cleared' in (d.root / 'orca-debug.log').read_text(), 'launch reset not announced')
            passed('keyboard recovery clears only desktop launch reports and Orca announces the result')
            d.resize(600, 1000)
            page('settings', 9)
            d.key('F6')
            d.key('F6')
            d.wait(lambda: any(n.get_name() == 'Pages' for n in parents(d.focused(app))), 'F6 did not focus collapsed navigation')
            d.key('Up')
            assert any(n.get_name() == 'Pages' for n in parents(d.focused(app))), 'arrow navigation unexpectedly closed the sidebar'
            d.key('Down')
            d.key('Return')
            d.wait(lambda: not any(n.get_name() == 'Pages' for n in parents(d.focused(app))), 'Enter did not enter page content')
            passed('narrow navigation opens with F6, keeps arrow selection reachable and enters content with Enter')
            page('settings', 9)
            d.tab_to(app, focus_name('Save notification preferences'), 'notification save unreachable')
            d.key('Return')
            d.wait(lambda: 'Notification preferences saved' in (d.root / 'orca-debug.log').read_text(), 'action feedback not announced by Orca')
            passed('Orca receives and generates the asynchronous action-feedback announcement without moving focus')
            speech = [line.split('SPEECH OUTPUT:', 1)[1].strip() for line in (d.root / 'orca-debug.log').read_text().splitlines() if 'SPEECH OUTPUT:' in line]
            for expected in ['Pages', 'Temperature history', 'History readings', 'Search sensors', 'Search games']:
                assert any(expected in line for line in speech), ('missing narration', expected)
            result['speech_output'] = speech
            assert orca.poll() is None
            result['result'] = 'passed'
        except Exception as error:
            result['error'] = str(error)
            if 'app' in locals():
                result['focus'] = d.describe_focus(app)
                result['visible_controls'] = [{'role': n.get_role_name(), 'name': n.get_name()}
                    for n in d.walk(app) if n.get_state_set().contains(d.Atspi.StateType.SHOWING)
                    and n.get_role_name() in ['button', 'toggle button', 'combo box', 'entry', 'text', 'check box']]
            raise
        finally:
            result['key_events'] = d.keys
            result['private_display'] = d.env.get('DISPLAY')
            (OUT / 'keyboard-orca.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'keyboard-orca.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
