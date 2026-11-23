#!/usr/bin/env python3
"""Private native keyboard workflows for expanded library formats and locations."""
import hashlib
import json
import os
from pathlib import Path
import tomllib

from game_format_fixtures import legendary, steam
from game_formats import call
from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/game-format-ui'))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'host_keyboard_events': False, 'profile': PROFILE, 'checks_passed': [],
              'gui_sha256': hashlib.file_digest((ROOT / 'target' / PROFILE / 'omen-gui').open('rb'), 'sha256').hexdigest()}
    with Desktop(OUT) as d:
        home = d.root / 'home'
        home.mkdir()
        d.env['HOME'] = str(home)
        config = d.root / 'user/config.toml'
        config.write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n'
            '[games.per_game_profiles]\n"legendary/Fixture"="performance"\n"steam/snap/shortcut/7/4045620583"="performance"\n')
        root = home / '.config/legendary'
        snap = home / 'snap/steam/common/.local/share/Steam'
        additional = d.root / 'Additional catalog'
        legendary(root, d.root / 'game', {'Fixture': 'Offline fixture'})
        legendary(additional, d.root / 'game', {'Other': 'Additional fixture'})
        steam(snap, 'Snap shortcut')
        bins = d.root / 'bin'
        bins.mkdir()
        route_log = d.root / 'route.json'
        (bins / 'legendary').write_text("""#!/usr/bin/python3
import json,os,sys
from pathlib import Path
if sys.argv[1:]==['--version']:
 print('legendary version "0.21.1"');sys.exit(0)
Path(os.environ['OMEN_GUI_ROUTE_LOG']).write_text(json.dumps({'args':sys.argv[1:],'configuration':os.environ.get('LEGENDARY_CONFIG_PATH'),'uid':os.getuid()}))
""")
        (bins / 'legendary').chmod(0o700)
        d.env.update(PATH=str(bins) + os.pathsep + os.defpath, OMEN_GUI_ROUTE_LOG=str(route_log))

        def passed(message):
            result['checks_passed'].append(message)
            print('PASS', message, flush=True)

        def named(name):
            return lambda node: node.get_name() == name and node.get_role_name() != 'table cell'

        def showing(fragment):
            return any(fragment in node.get_name() and node.get_state_set().contains(d.Atspi.StateType.SHOWING)
                       for node in d.walk(app))

        def search(query):
            d.focus_window()
            d.key('f', 'Control_L')
            d.wait(lambda: d.describe_focus(app)['name'] == 'Search games', 'game search was not focused')
            d.key('a', 'Control_L')
            d.key('BackSpace')
            d.type_text(query)
            d.wait(lambda: d.Atspi.Text.get_text(d.focused(app).get_text_iface(), 0, -1) == query,
                'actual keyboard search text differs')
            d.drain(.3)

        def locations():
            return tomllib.loads(config.read_text()).get('games', {}).get('library_locations', [])

        def select_location(root, current, expected):
            search('')
            d.tab_to(app, named(current), 'library installation selector unreachable')
            d.key('space')
            # Wait for GTK's popup and its focus transfer before navigating.
            d.wait(lambda: any(n.get_name() == expected and n.get_state_set().contains(d.Atspi.StateType.SHOWING)
                for n in d.walk(app)), 'installation popup did not become visible')
            d.drain(.3)
            trace = []
            d.key('Home')
            # GTK can move focus without selecting a row. Observe the actual
            # selection rather than inferring it from a fixed arrow count.
            for _ in range(10):
                row = d.focused(app)
                selected = row is not None and row.get_state_set().contains(d.Atspi.StateType.SELECTED)
                labels = [n.get_name() for n in d.walk(row) if n.get_role_name() == 'label']
                trace.append({'focus': d.describe_focus(app),
                    'labels': labels, 'selected': selected})
                if selected and expected in labels:
                    break
                d.key('Down')
                d.drain(.1)
            else:
                raise RuntimeError('requested popup item was not selected: ' + str(trace))
            result.setdefault('selector_navigation', []).append(trace)
            d.wait(lambda: (row := d.focused(app)) is not None
                and row.get_state_set().contains(d.Atspi.StateType.SELECTED)
                and any(n.get_name() == expected for n in d.walk(row)), 'requested popup item was not selected')
            d.key('Return')
            d.wait(lambda: d.describe_focus(app)['name'] == expected, 'unexpected installation selected')
            chosen = d.describe_focus(app)
            hint = 'Launch uses this selected configuration.' if expected == 'Legendary · Native' else 'Steam folder containing steamapps'
            d.wait(lambda: showing(hint), 'selected installation folder guidance did not update')
            d.tab_to(app, named('Add library location…'), 'add-location action unreachable')
            d.key('Return')
            d.wait(lambda: any(n.get_name() == 'Add library location' and n.get_role_name() == 'dialog' for n in d.walk(app)), 'folder chooser missing')
            d.focus_window('Add library location')
            d.key('l', 'Control_L')
            d.wait(lambda: d.focused(app) is not None and d.focused(app).get_role_name() in ['text', 'entry'], 'folder location field was not focused')
            d.key('a', 'Control_L')
            d.key('BackSpace')
            d.type_text(str(root), paced=False)
            entry = d.focused(app)
            entered = d.Atspi.Text.get_text(entry.get_text_iface(), 0, -1)
            assert entered.rstrip('/') == str(root), ('folder input differs before acceptance', entered, str(root))
            d.key('Return')
            d.drain(.5)
            result.setdefault('location_selections', []).append({'requested': str(root), 'chosen_control': chosen,
                'saved_after_return': locations(), 'dialog_present': any(n.get_name() == 'Add library location' and n.get_role_name() == 'dialog' for n in d.walk(app))})
            if not any(entry['path'] == str(root) for entry in locations()):
                d.focus_window('Add library location')
                d.tab_to(app, lambda n: n.get_role_name() == 'button' and n.get_name() in ['Select', 'Open'], 'folder confirmation unreachable', limit=40)
                d.key('Return')
            d.wait(lambda: any(entry['path'] == str(root) for entry in locations()), 'selected library location was not saved')
            d.focus_window()

        try:
            service = d.launch([str(ROOT / 'target' / PROFILE / 'omen-daemon'), '--mock-omen'], 'daemon', True)
            endpoint = service.stdout.readline().strip()
            d.env['OMEN_SOCK'] = endpoint
            call(endpoint, 'set_profile', {'profile': 'balanced'})
            gui = d.launch([str(ROOT / 'target' / PROFILE / 'omen-gui'), '--new-instance', '--page', 'games'], 'gui')
            app = d.attach(gui)
            d.focus_window()
            search('legendary/Fixture')
            d.tab_to(app, named('Profile setup'), 'Legendary setup unreachable')
            d.key('space')
            d.wait(lambda: showing("Legendary's configuration"), 'Legendary wrapper instructions absent')
            d.tab_to(app, named('Game wrapper command'), 'Legendary wrapper text unreachable')
            d.key('Tab')
            assert d.describe_focus(app)['name'] != 'Game wrapper command'
            passed('Legendary search, provenance and actual-command setup are keyboard reachable; wrapper text releases Tab')
            d.tab_to(app, named('Launch'), 'Legendary Launch unreachable')
            d.key('Return')
            d.wait(route_log.exists, 'Legendary GUI launch did not reach its executable')
            routed = json.loads(route_log.read_text())
            assert routed == {'args': ['launch', '--', 'Fixture'], 'configuration': str(root), 'uid': os.getuid()}
            d.tab_to(app, named('Restore Balanced'), 'manual restoration unreachable')
            d.key('Return')
            d.wait(lambda: call(endpoint, 'get_profile') == 'balanced', 'manual fixture restoration failed')
            passed('native Legendary Launch preserves its selected configuration and exposes working manual profile restoration')
            search('steam/snap/shortcut/7/4045620583')
            d.tab_to(app, named('Profile setup'), 'Snap shortcut setup unreachable')
            d.key('space')
            d.wait(lambda: showing('Automatic wrapping for this sandboxed installation is not available.'), 'Snap limitation absent')
            toggle = next(n for n in d.walk(app) if n.get_role_name() == 'button' and n.get_name() == 'Enable automatic restore')
            assert not toggle.get_state_set().contains(d.Atspi.StateType.SENSITIVE)
            passed('Snap shortcut setup explains its unsupported automatic wrapping and keeps that action disabled')
            search('')
            d.tab_to(app, named('Library locations'), 'library locations disclosure unreachable')
            d.key('space')
            select_location(additional, 'Steam · Native', 'Legendary · Native')
            saved = next(entry for entry in locations() if entry['path'] == str(additional))
            assert saved['source'] == 'legendary' and not saved.get('flatpak', False) and not saved.get('snap', False)
            search('legendary/Other')
            d.wait(lambda: showing('Additional fixture'), 'adding the Legendary directory did not rescan')
            route_log.unlink()
            d.tab_to(app, named('Launch'), 'added Legendary entry unreachable')
            d.key('Return')
            d.wait(route_log.exists, 'added catalog launch was not submitted')
            assert json.loads(route_log.read_text())['configuration'] == str(additional)
            passed('keyboard folder selection saves a Legendary location, rescans it and launches from that exact configuration')
            select_location(snap, 'Legendary · Native', 'Steam · Snap')
            saved = next(entry for entry in locations() if entry['path'] == str(snap))
            assert saved['source'] == 'steam' and saved['snap'] and not saved.get('flatpak', False)
            passed('the native location chooser persists an explicit Steam Snap route')
            search('')
            d.tab_to(app, named('Remove'), 'location removal unreachable')
            d.key('Return')
            d.wait(lambda: not any(entry['path'] == str(additional) for entry in locations()), 'Legendary location removal was not saved')
            search('legendary/Other')
            d.wait(lambda: showing('No games match this search'), 'removed catalog entry remained visible')
            passed('removing a selected catalog rescans and removes its entry without rewriting the other installation')
            result['result'] = 'passed'
        except Exception as error:
            result['error'] = str(error)
            if 'app' in locals():
                result['focus'] = d.describe_focus(app)
                result['saved_preferences'] = tomllib.loads(config.read_text())
                result['visible_labels'] = [n.get_name()[:350] for n in d.walk(app) if n.get_role_name() == 'label'
                    and n.get_state_set().contains(d.Atspi.StateType.SHOWING)][:70]
                result['visible_controls'] = [{'role': n.get_role_name(), 'name': n.get_name()} for n in d.walk(app)
                    if n.get_state_set().contains(d.Atspi.StateType.SHOWING) and n.get_role_name() in ['button', 'toggle button', 'combo box', 'text', 'entry']]
            raise
        finally:
            result['key_events'] = d.keys
            (OUT / 'gui-game-formats.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'gui-game-formats.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
