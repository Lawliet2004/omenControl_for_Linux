"""Private, synthetic installed records; never read store credentials."""
import json
from pathlib import Path
import struct


def put(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def steam(root, title, appid=0xF1234567, account=7, store=False):
    put(root / 'config/loginusers.vdf', '"users"{"%d"{"MostRecent""1"}}' % (76561197960265728 + account))
    path = root / f'userdata/{account}/config/shortcuts.vdf'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b'\x00shortcuts\0\x000\0\x02appid\0' + struct.pack('<I', appid)
                    + b'\x01AppName\0' + title.encode() + b'\0\x01exe\0/untrusted; not executed\0\x08\x08\x08')
    if store:
        put(root / 'steamapps/appmanifest_620.acf', '"AppState"{"appid""620""name""Store fixture""StateFlags""4"}')


def legendary(root, game, names):
    """Minimal records also accepted by the pinned upstream CLI in offline mode."""
    root.mkdir(parents=True, exist_ok=True)
    installed = {}
    for app, title in names.items():
        installed[app] = {'app_name': app, 'title': title, 'version': '1', 'install_path': str(game),
                          'executable': 'game.py', 'platform': 'Linux', 'can_run_offline': True}
        put(root / 'metadata' / (app + '.json'), json.dumps({'app_name': app, 'app_title': title,
            'asset_infos': {}, 'metadata': {'namespace': 'fixture', 'customAttributes': {}}}))
    put(root / 'installed.json', json.dumps(installed))
    put(root / 'user.json', json.dumps({'account_id': 'fixture-account', 'displayName': 'Offline fixture'}))
    put(root / 'config.ini', '[Legendary]\ndisable_update_check = true\ndisable_update_notice = true\n'
        + ''.join('[' + app + ']\noffline = true\nno_wine = true\n' for app in names))
