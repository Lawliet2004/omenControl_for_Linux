#!/usr/bin/env python3
"""Bounded app-only status-overlay captures; no desktop framebuffer or control writes."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = ROOT / 'target' / PROFILE


def main():
    records = []
    with tempfile.TemporaryDirectory(prefix='omen-overlay-capture-', dir=ROOT/'target') as directory:
        temp = Path(directory)
        for name in ['user', 'tmp']:
            (temp/name).mkdir()
        (temp/'user/config.toml').write_text('version = 1\n[general]\noverlay_enabled = true\n')
        env = dict(os.environ, GDK_BACKEND='wayland', GTK_A11Y='none', GSETTINGS_BACKEND='memory',
                   OMEN_REPORT_READY='1', OMEN_CONFIG_DIR=str(temp/'user'), OMEN_SYSTEM_CONFIG_DIR=str(temp/'system'),
                   OMEN_HOME=str(temp/'games'), OMEN_SOCK=str(temp/'missing.sock'), TMPDIR=str(temp/'tmp'),
                   XDG_CONFIG_HOME=str(temp/'config'), XDG_DATA_HOME=str(temp/'data'), XDG_CACHE_HOME=str(temp/'cache'))
        bus_config = temp/'session.conf'
        bus_config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/session.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
        bus = subprocess.Popen(['dbus-daemon', '--config-file='+str(bus_config), '--nofork', '--print-address=1'], stdout=subprocess.PIPE, text=True)
        daemon = None
        try:
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
            assert env['DBUS_SESSION_BUS_ADDRESS']
            with (temp/'daemon.log').open('w') as log:
                daemon = subprocess.Popen([str(BIN/'omen-daemon'), '--mock'], env=env, stdout=subprocess.PIPE, stderr=log, text=True)
                endpoint = daemon.stdout.readline().strip()
                assert Path(endpoint).name == f'omen-mock-{daemon.pid}.sock'
                for fixture, scale in [(True, '1'), (True, '2'), (False, '1')]:
                    prefix = 'overlay-fixture' if fixture else 'overlay-readonly'
                    prefix += '-scale'+scale
                    path = ROOT/'docs/screenshots'/f'{prefix}.png'
                    main_path = ROOT/'docs/screenshots'/f'{prefix}-settings.png'
                    capture_env = dict(env, GDK_SCALE=scale, OMEN_SOCK=endpoint if fixture else str(temp/'missing.sock'),
                                       OMEN_CAPTURE_PATH=str(main_path), OMEN_CAPTURE_OVERLAY_PATH=str(path))
                    completed = subprocess.run([str(BIN/'omen-gui'), '--page', 'settings'], env=capture_env,
                                               capture_output=True, text=True, check=True, timeout=25)
                    assert 'OMEN_UI_BACKEND GdkWaylandDisplay' in completed.stderr
                    assert 'Gtk-WARNING' not in completed.stderr and 'CRITICAL' not in completed.stderr, completed.stderr
                    records.append({'fixture': fixture, 'requested_scale': int(scale), 'gtk_display_type': 'GdkWaylandDisplay',
                                    'images': [{'path': str(p.relative_to(ROOT)), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
                                               for p in [path, main_path]]})
        finally:
            if daemon:
                daemon.terminate(); assert daemon.wait(timeout=8) == 0
            bus.terminate(); assert bus.wait(timeout=5) == 0
    result = {'recorded_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'profile': PROFILE,
              'gui_sha256': hashlib.sha256((BIN/'omen-gui').read_bytes()).hexdigest(),
              'method': 'GTK widget-tree captures of main and overlay windows only; private bus, separate fixture and live read-only sources',
              'hardware_control_writes': 0, 'cleanup_complete': True, 'captures': records}
    (ROOT/'docs/evidence/overlay-captures.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
