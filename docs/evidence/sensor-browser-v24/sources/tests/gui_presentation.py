#!/usr/bin/env python3
"""Measure real GTK text geometry and rendered label contrast on private fixtures.

Pixels are read from the application's own PNG, never a desktop screenshot.
Contrast uses the modal solid background within each text allocation and GTK's
computed foreground/effective opacity. Native inactive controls are reported
separately. Hover, pressed and every desktop/theme combination remain outside
this matrix. This is a measurement, not a claim of universal WCAG conformance.
"""
from collections import Counter
import hashlib
import json
import math
import os
from pathlib import Path
import re

from PIL import Image
from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
OUT = Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR', ROOT / 'target/a11y-presentation'))
PAGES = ['dashboard', 'vitals', 'performance', 'fan', 'lighting', 'graphics', 'battery', 'games', 'settings']


def intersect(a, b):
    left, top = max(a[0], b[0]), max(a[1], b[1])
    right, bottom = min(a[0] + a[2], b[0] + b[2]), min(a[1] + a[3], b[1] + b[3])
    return [left, top, right - left, bottom - top]


def luminance(rgb):
    linear = [x / 12.92 if x <= .04045 else ((x + .055) / 1.055) ** 2.4 for x in rgb]
    return sum(x * y for x, y in zip(linear, [.2126, .7152, .0722]))


def ratio(a, b):
    lo, hi = sorted([luminance(a), luminance(b)])
    return (hi + .05) / (lo + .05)


def measure(report, image):
    result = {'horizontal_clipping': [], 'collapsed_text_allocations': [], 'contrast': [], 'inactive_labels': [], 'charts': []}
    origin = report['render_bounds']
    scale_x = image.width / origin[2]
    scale_y = image.height / origin[3]
    for widget in report['widgets']:
        if widget['kind'] == 'chart':
            result['charts'].append(widget)
            continue
        if not widget['text'].strip() or widget['bounds'] is None or widget['opacity'] < .01:
            continue
        x, y, width, height = widget['bounds']
        if width <= 0 or height <= 0:
            result['collapsed_text_allocations'].append({'text':widget['text'], 'bounds':widget['bounds'], 'path':widget['path']})
            continue
        clip = [0, 0, *report['window_size']]
        for bounds in widget['clips']:
            clip = intersect(clip, bounds)
        ox, oy = widget['layout_offset']
        lx, ly, lw, lh = widget['logical']
        item = {'text': widget['text'], 'path': widget['path'], 'font': widget['font'], 'sensitive': widget['sensitive']}
        if not widget['ellipsized'] and (x + ox + lx < clip[0] - 3 or x + ox + lx + lw > clip[0] + clip[2] + 3):
            result['horizontal_clipping'].append(dict(item, bounds=widget['bounds'], logical=widget['logical'], clip=clip))
        visible = intersect(clip, widget['bounds'])
        if visible[2] <= 0 or visible[3] <= 0:
            continue
        # A partially scrolled line is not a reliable background sample.
        if visible[3] < height - 1 or visible[2] < width - 1:
            continue
        left = max(0, math.ceil((x - origin[0]) * scale_x))
        top = max(0, math.ceil((y - origin[1]) * scale_y))
        right = min(image.width, math.floor((x + width - origin[0]) * scale_x))
        bottom = min(image.height, math.floor((y + height - origin[1]) * scale_y))
        if right <= left or bottom <= top:
            continue
        colors = Counter(image.crop((left, top, right, bottom)).getdata())
        foreground = widget['foreground']
        candidates = [(rgb, count) for rgb, count in colors.most_common(5) if rgb[3] == 255
                      and sum(abs(rgb[i] / 255 - foreground[i]) for i in range(3)) > .06]
        if not candidates:
            continue
        background, count = candidates[0]
        if count < (right - left) * (bottom - top) * .25:
            continue
        bg = [v / 255 for v in background[:3]]
        alpha = foreground[3] * widget['opacity']
        fg = [foreground[i] * alpha + bg[i] * (1 - alpha) for i in range(3)]
        size = float(re.search(r'([\d.]+)(?:px)?$', widget['font']).group(1))
        pixels = size if widget['font'].endswith('px') else size * 96 / 72
        bold = widget.get('font_weight', 400) >= 700
        large = pixels >= 24 - .01 or pixels >= 14 * 96 / 72 - .01 and bold
        threshold = 3.0 if large else 4.5
        measured = dict(item, ratio=ratio(fg, bg), required=threshold, foreground=fg, background=bg,
                        background_fraction=count / ((right - left) * (bottom - top)))
        result['contrast' if widget['sensitive'] else 'inactive_labels'].append(measured)
    result['contrast_failures'] = [item for item in result['contrast'] if item['ratio'] + .02 < item['required']]
    return result


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    matrix = [('light', 10), ('dark', 10), ('high-contrast', 10), ('large-light', 20)]
    if os.environ.get('OMEN_PRESENTATION_CASES'):
        selected = os.environ['OMEN_PRESENTATION_CASES'].split(',')
        matrix = [case for case in matrix if case[0] in selected]
    pages = os.environ.get('OMEN_PRESENTATION_PAGES', ','.join(PAGES)).split(',')
    result = {'result': 'failed', 'profile': PROFILE, 'fixture_only': True, 'physical_hardware_writes': False,
              'gui_sha256': hashlib.file_digest((ROOT / 'target' / PROFILE / 'omen-gui').open('rb'), 'sha256').hexdigest(),
              'measurement': __doc__, 'cases': []}
    with Desktop(OUT / 'process') as d:
        settings = d.root / 'config/gtk-4.0'
        settings.mkdir()
        config = d.root / 'user/config.toml'
        config.write_text('version = 1\n[general]\nonboarding_dismissed = true\npoll_interval_ms = 500\n'
            '[[games.custom_games]]\nid = "custom/presentation"\nname = "Presentation fixture"\n'
            'source = "custom"\nlaunch_cmd = "/bin/true"\n')
        service = d.launch([str(ROOT / 'target' / PROFILE / 'omen-daemon'), '--mock-omen'], 'daemon', True)
        d.env['OMEN_SOCK'] = service.stdout.readline().strip()
        d.env.pop('ADW_DEBUG_HIGH_CONTRAST', None)
        try:
            for case, font in matrix:
                (settings / 'settings.ini').write_text('[Settings]\ngtk-font-name=Sans ' + str(font) + '\n')
                for page in pages:
                    name = case + '-' + page
                    report_path = OUT / (name + '.json')
                    png_path = OUT / (name + '.png')
                    report_path.unlink(missing_ok=True)
                    extra = {'OMEN_CAPTURE_PATH': str(png_path.resolve()), 'OMEN_UI_REPORT_PATH': str(report_path.resolve()),
                             'OMEN_CAPTURE_WIDTH': '600' if font == 20 else '1240', 'OMEN_CAPTURE_HEIGHT': '1100',
                             'ADW_DEBUG_COLOR_SCHEME': 'prefer-dark' if case == 'dark' else 'prefer-light'}
                    if case == 'high-contrast':
                        extra['ADW_DEBUG_HIGH_CONTRAST'] = '1'
                    gui = d.launch([str(ROOT / 'target' / PROFILE / 'omen-gui'), '--new-instance', '--page', page], name, extra=extra)
                    app = d.attach(gui)
                    d.focus_window()
                    gui.wait(timeout=15)
                    assert gui.returncode == 0
                    report = json.loads(report_path.read_text())
                    assert report['gtk_font_name'] == 'Sans ' + str(font)
                    assert report['dark'] == (case == 'dark')
                    assert report['high_contrast'] == (case == 'high-contrast')
                    analysis = measure(report, Image.open(png_path).convert('RGBA'))
                    item = {'case': case, 'page': page, 'font': report['gtk_font_name'], 'window_size': report['window_size'],
                            'report': report_path.name, 'capture': png_path.name, 'capture_sha256': hashlib.file_digest(png_path.open('rb'), 'sha256').hexdigest(),
                            **analysis}
                    result['cases'].append(item)
                    print(name, 'clipping', len(analysis['horizontal_clipping']), 'contrast failures', len(analysis['contrast_failures']),
                          'labels', len(analysis['contrast']), flush=True)
            result['result'] = 'passed' if all(not c['horizontal_clipping'] and not c['contrast_failures'] and not c['collapsed_text_allocations'] for c in result['cases']) else 'issues-found'
        finally:
            (OUT / 'presentation.json').write_text(json.dumps(result, indent=2) + '\n')
    result['cleanup_complete'] = True
    (OUT / 'presentation.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
