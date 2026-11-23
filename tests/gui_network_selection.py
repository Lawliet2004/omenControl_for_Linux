#!/usr/bin/env python3
"""Measure the selected network-reading state with real private keyboard input."""
import hashlib
import json
import os
from pathlib import Path

from PIL import Image
from gui_presentation import measure
from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/network-selection-presentation'))


def measure_selected(report, image):
    analysis = measure(report, image)
    # Keep geometry checks for selected text, but its native highlight uses a
    # different foreground from Label.color(). Do not infer its contrast.
    for key in ['contrast', 'contrast_failures']:
        analysis[key] = [item for item in analysis[key] if not item['text'].startswith('Receive: ')]
    return analysis


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
              'host_keyboard_events': False, 'profile': PROFILE, 'cases': [],
              'gui_sha256': hashlib.file_digest((ROOT / 'target' / PROFILE / 'omen-gui').open('rb'), 'sha256').hexdigest()}
    with Desktop(OUT / 'process') as d:
        (d.root / 'user/config.toml').write_text('version = 1\n[general]\nonboarding_dismissed = true\npoll_interval_ms = 500\n')
        settings = d.root / 'config/gtk-4.0'
        settings.mkdir()
        service = d.launch([str(ROOT / 'target' / PROFILE / 'omen-daemon'), '--mock-omen'], 'daemon', True)
        d.env['OMEN_SOCK'] = service.stdout.readline().strip()
        fixture_root = next(path.parent.parent for path in (d.root / 'tmp').glob('*/sys/class')
                            if (path / 'net/fixture0').is_dir())
        counters = fixture_root / 'proc/net/dev'
        original = counters.read_text()
        try:
            for name, font, width in [('wide-light', 10, 1240), ('narrow-large-light', 20, 600)]:
                counters.write_text(original)
                (settings / 'settings.ini').write_text('[Settings]\ngtk-font-name=Sans ' + str(font) + '\n')
                report_path = OUT / (name + '.json')
                capture = OUT / (name + '.png')
                report_path.unlink(missing_ok=True)
                gui = d.launch([str(ROOT / 'target' / PROFILE / 'omen-gui'), '--new-instance', '--page', 'vitals'], name,
                    extra={'OMEN_CAPTURE_PATH':str(capture.resolve()), 'OMEN_UI_REPORT_PATH':str(report_path.resolve()),
                           'OMEN_CAPTURE_WIDTH':str(width), 'OMEN_CAPTURE_HEIGHT':'1100', 'ADW_DEBUG_COLOR_SCHEME':'prefer-light'})
                app = d.attach(gui)
                d.focus_window()
                d.key('2', 'Control_L')
                network = d.tab_to(app, lambda n: n.get_role_name() == 'label' and n.get_name().startswith('Receive: '),
                                   'network reading unreachable before capture', limit=15)
                d.key('a', 'Control_L')
                counters.write_text(original.replace('1048576 100 0 0', '2097152 100 2 3'))
                d.wait(lambda: any(n.get_name().startswith('Reading held for selection.')
                       and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app)),
                       'selected-reading explanation missing', timeout=3)
                assert network.get_text_iface().get_n_selections() == 1
                gui.wait(timeout=12)
                assert gui.returncode == 0
                report = json.loads(report_path.read_text())
                assert report['gtk_font_name'] == 'Sans ' + str(font)
                assert any(w.get('text', '').startswith('Reading held for selection.') for w in report['widgets'])
                analysis = measure_selected(report, Image.open(capture).convert('RGBA'))
                assert not any(analysis[key] for key in ['horizontal_clipping','collapsed_text_allocations','contrast_failures'])
                result['cases'].append({'case':name, 'font':report['gtk_font_name'], 'capture':capture.name,
                    'selected_reading_contrast_measured':False,
                    'excluded_sample_reason':'native selection foreground is not exposed by Label.color()',
                    'capture_sha256':hashlib.file_digest(capture.open('rb'), 'sha256').hexdigest(), **analysis})
                print('PASS', name, flush=True)
            result['result'] = 'passed'
        finally:
            result['key_events'] = d.keys
            (OUT / 'network-selection.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'network-selection.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
