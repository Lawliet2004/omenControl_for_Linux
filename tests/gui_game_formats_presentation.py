#!/usr/bin/env python3
"""Bounded app-only captures of expanded launcher setup and location controls."""
import hashlib
import json
import os
from pathlib import Path

from PIL import Image
from game_format_fixtures import legendary, steam
from gui_presentation import measure
from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'release')
OUT = Path(os.environ['OMEN_TEST_EVIDENCE_DIR'])


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'input': 'AT-SPI actions for capture staging; separate XTEST workflows verify keyboard input',
              'gui_sha256': hashlib.file_digest((ROOT / 'target' / PROFILE / 'omen-gui').open('rb'), 'sha256').hexdigest(),
              'cases': []}
    if os.environ.get('OMEN_PRESENTATION_RESUME') == '1' and (OUT / 'presentation.json').exists():
        previous = json.loads((OUT / 'presentation.json').read_text())
        assert previous['gui_sha256'] == result['gui_sha256']
        for case in previous['cases']:
            assert not case['horizontal_clipping'] and not case['contrast_failures'] and not case['collapsed_text_allocations']
            assert hashlib.file_digest((OUT / case['capture']).open('rb'), 'sha256').hexdigest() == case['capture_sha256']
        result['cases'] = previous['cases']
    with Desktop(OUT / 'process') as d:
        settings = d.root / 'config/gtk-4.0'
        settings.mkdir()
        home = d.root / 'home'
        home.mkdir()
        d.env['HOME'] = str(home)
        catalog = d.root / ('Additional Legendary configuration with a long directory name ' + 'x' * 48)
        snap = home / 'snap/steam/common/.local/share/Steam'
        legendary(home / '.config/legendary', d.root / 'game', {'Fixture': 'Offline Legendary fixture'})
        legendary(catalog, d.root / 'game', {'Other': 'Additional configuration'})
        steam(snap, 'Snap shortcut')
        config = d.root / 'user/config.toml'
        config.write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n'
            '[[games.library_locations]]\nsource="legendary"\npath=' + json.dumps(str(catalog)) + '\n'
            '[[games.library_locations]]\nsource="steam"\nsnap=true\npath=' + json.dumps(str(snap)) + '\n')
        service = d.launch([str(ROOT / 'target' / PROFILE / 'omen-daemon'), '--mock-omen'], 'daemon', True)
        d.env['OMEN_SOCK'] = service.stdout.readline().strip()
        try:
            for case, font, width, dark, query in [
                ('legendary-wide-light', 10, 1240, False, 'legendary/Fixture'),
                ('legendary-narrow-large-dark', 20, 600, True, 'legendary/Fixture'),
                ('snap-narrow-large-dark', 20, 600, True, 'steam/snap/shortcut/7/4045620583'),
                ('locations-wide-light', 10, 1240, False, None),
                ('locations-narrow-large-dark', 20, 600, True, None),
            ]:
                if any(item['case'] == case for item in result['cases']):
                    continue
                (settings / 'settings.ini').write_text('[Settings]\ngtk-font-name=Sans ' + str(font) + '\n')
                png, report = OUT / (case + '.png'), OUT / (case + '.json')
                report.unlink(missing_ok=True)
                gui = d.launch([str(ROOT / 'target' / PROFILE / 'omen-gui'), '--new-instance', '--page', 'games'], case,
                    extra={'OMEN_CAPTURE_PATH': str(png.resolve()), 'OMEN_UI_REPORT_PATH': str(report.resolve()),
                           'OMEN_CAPTURE_WIDTH': str(width), 'OMEN_CAPTURE_HEIGHT': '1100','OMEN_CAPTURE_DELAY_SECONDS':'8',
                           'ADW_DEBUG_COLOR_SCHEME': 'prefer-dark' if dark else 'prefer-light'})
                app = d.attach(gui)
                search = d.wait(lambda: next(n for n in d.walk(app) if n.get_role_name() in ['text', 'entry']
                    and n.get_name() == 'Search games'), 'search missing')
                assert search.get_editable_text_iface().set_text_contents(query or 'no-match-for-locations')
                if query:
                    d.wait(lambda: sum(n.get_name() == 'Profile setup' and n.get_role_name() == 'button'
                        for n in d.walk(app)) == 1 and any(query in n.get_name() for n in d.walk(app)
                        if n.get_role_name() == 'label'), 'capture search did not settle on its game')
                else:
                    d.wait(lambda: any(n.get_name().startswith('No games match this search') for n in d.walk(app)),
                        'capture empty search did not settle')
                target = 'Profile setup' if query else 'Library locations'
                node = d.wait(lambda: next(n for n in d.walk(app) if n.get_name() == target
                    and n.get_role_name() != 'table cell'), 'capture disclosure missing')
                action = node.get_action_iface()
                assert action and action.do_action(0)
                required = "Legendary's configuration" if query and query.startswith('legendary/') else (
                    'Automatic wrapping for this sandboxed installation' if query else 'Legendary (Native)')
                d.wait(lambda: any(required in n.get_name() for n in d.walk(app)), 'capture content did not expand')
                if font == 20:
                    bars = [n for n in d.walk(app) if n.get_role_name() == 'scroll bar'
                        and n.get_state_set().contains(d.Atspi.StateType.VERTICAL)
                        and n.get_state_set().contains(d.Atspi.StateType.SHOWING)]
                    if bars:
                        value = bars[0].get_value_iface()
                        destination = value.get_maximum_value()
                        if query is None:
                            label = next(n for n in d.walk(app) if n.get_role_name() == 'label'
                                and n.get_name().startswith('Legendary (Native)'))
                            bounds = label.get_component_iface().get_extents(d.Atspi.CoordType.WINDOW)
                            destination = min(destination, value.get_current_value() + max(0, bounds.y - 140))
                        value.set_current_value(destination)
                        d.drain(.15)
                d.park_pointer()
                gui.wait(timeout=20)
                assert gui.returncode == 0
                data = json.loads(report.read_text())
                assert data['gtk_font_name'] == 'Sans ' + str(font) and data['dark'] == dark
                texts = '\n'.join(w.get('text', '') for w in data['widgets'])
                assert required in texts, (case, 'expanded content missing from captured tree')
                if query and query.startswith('legendary/'):
                    import re
                    view=next(w for w in data['widgets'] if w['kind']=='text_view' and 'game-run' in w['text'])
                    size=float(re.search(r'([0-9.]+)(?:px)?$',view['font']).group(1))
                    points=size*72/96 if view['font'].endswith('px') else size
                    assert abs(points-font)<.02,view['font']
                analysis = measure(data, Image.open(png).convert('RGBA'))
                result['cases'].append({'case': case, 'font': data['gtk_font_name'], 'window_size': data['window_size'],
                    'capture': png.name, 'report': report.name, 'capture_sha256': hashlib.file_digest(png.open('rb'), 'sha256').hexdigest(), **analysis})
                print(case, 'clipping', len(analysis['horizontal_clipping']), 'contrast failures', len(analysis['contrast_failures']), flush=True)
            result['result'] = 'passed' if all(not c['horizontal_clipping'] and not c['contrast_failures']
                and not c['collapsed_text_allocations'] for c in result['cases']) else 'issues-found'
        finally:
            (OUT / 'presentation.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'presentation.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
