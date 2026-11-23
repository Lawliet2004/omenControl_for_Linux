#!/usr/bin/env python3
"""Check actual GUI icon resolution with missing and installed desktop themes."""
import hashlib
import json
import os
from pathlib import Path

from PIL import Image
from gui_presentation import measure
from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'release')
OUT = Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()
NAVIGATION = {'view-grid-symbolic', 'applications-system-symbolic', 'speedometer-symbolic',
    'view-refresh-symbolic', 'weather-clear-night-symbolic', 'video-display-symbolic',
    'battery-symbolic', 'applications-games-symbolic', 'preferences-system-symbolic'}


def main():
    OUT.mkdir(exist_ok=False, parents=True)
    result = {'result': 'failed', 'profile': PROFILE, 'fixture_only': True,
        'physical_hardware_writes': False, 'host_keyboard_input': False,
        'gui_sha256': hashlib.file_digest((ROOT/'target'/PROFILE/'omen-gui').open('rb'), 'sha256').hexdigest(),
        'cases': []}
    matrix = [('missing-light-wide', 'OmenMissingThemeFixture', False, 1240, 10),
        ('missing-dark-narrow-large', 'OmenMissingThemeFixture', True, 600, 20),
        ('adwaita-light-wide', 'Adwaita', False, 1240, 10),
        ('breeze-dark-wide', 'breeze', True, 1240, 10)]
    try:
        with Desktop(OUT/'process') as desktop:
            settings = desktop.root/'config/gtk-4.0'
            settings.mkdir()
            (desktop.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\n')
            daemon = desktop.launch([str(ROOT/'target'/PROFILE/'omen-daemon'), '--mock-omen'], 'daemon', True)
            desktop.env['OMEN_SOCK'] = daemon.stdout.readline().strip()
            for name, theme, dark, width, font in matrix:
                (settings/'settings.ini').write_text('[Settings]\ngtk-icon-theme-name='+theme+'\ngtk-font-name=Sans '+str(font)+'\n')
                capture, report_path = OUT/(name+'.png'), OUT/(name+'.json')
                gui = desktop.launch([str(ROOT/'target'/PROFILE/'omen-gui'), '--new-instance', '--page', 'settings'], name,
                    extra={'OMEN_CAPTURE_PATH':str(capture), 'OMEN_UI_REPORT_PATH':str(report_path),
                        'OMEN_CAPTURE_WIDTH':str(width), 'OMEN_CAPTURE_HEIGHT':'1100',
                        'ADW_DEBUG_COLOR_SCHEME':'prefer-dark' if dark else 'prefer-light'})
                desktop.attach(gui)
                desktop.focus_window()
                gui.wait(timeout=25)
                assert gui.returncode == 0, name
                report = json.loads(report_path.read_text())
                assert report['icon_theme'] == theme and report['dark'] == dark, name
                assert report['gtk_font_name'] == 'Sans '+str(font), name
                icons = report['icons']
                assert icons, name
                assert all(icon['resolved_name'] != 'image-missing' and icon['file'] for icon in icons), name
                fallback = [icon for icon in icons if icon['file'].startswith('resource:///com/omenlinux/Gui/icons/')]
                if width >= 850:
                    assert NAVIGATION <= {icon['requested_name'] for icon in icons}, name
                    assert all(icon['symbolic'] for icon in icons if icon['requested_name'] in NAVIGATION), name
                if theme == 'OmenMissingThemeFixture' and width >= 850:
                    assert len(fallback) >= 7, 'missing theme did not use the bundled navigation fallback'
                if theme in ['Adwaita', 'breeze']:
                    assert any('/icons/'+theme+'/' in icon['file'] for icon in icons), 'installed theme lost priority'
                analysis = measure(report, Image.open(capture).convert('RGBA'))
                assert not analysis['horizontal_clipping'] and not analysis['collapsed_text_allocations'] and not analysis['contrast_failures'], name
                result['cases'].append({'name':name, 'theme':theme, 'dark':dark, 'font':report['gtk_font_name'],
                    'window_size':report['window_size'], 'icons':icons, 'bundled_fallbacks':len(fallback),
                    'capture':capture.name, 'capture_sha256':hashlib.file_digest(capture.open('rb'), 'sha256').hexdigest(),
                    'report':report_path.name, **analysis})
                print('PASS', name, len(icons), 'resolved icons;', len(fallback), 'bundled fallbacks', flush=True)
        result['cleanup_complete'] = True
        result['result'] = 'passed'
    finally:
        (OUT/'icon-themes.json').write_text(json.dumps(result, indent=2)+'\n')


if __name__ == '__main__':
    main()
